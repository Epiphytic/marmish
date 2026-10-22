//! Bounded encrypted prefixes. Caller holds the I/O guard through metadata commit.
use crate::cipher::{self, Context, Journal, StorageKeys};
use crate::fault::{fault_point, FaultPoint};
use crate::launcher::{IoGuard, StateRoot};
use crate::model::*;
use crate::store::OutputCommit;
use rusqlite::{params, Connection, TransactionBehavior};
use std::path::{Path, PathBuf};

const CHUNK: u64 = 16 * 1024;
const PAGE: u64 = 4096;
const MAX_CAP: u64 = 64 * 1024 * 1024;

pub(crate) struct Spool {
    root: StateRoot,
    job: JobId,
    stream: Stream,
    path: Option<PathBuf>,
    db: Option<Connection>,
    cap: u64,
    commit: OutputCommit,
    failed: bool,
}

pub(crate) fn add_gap(
    gaps: &mut Vec<MissingRange>,
    start: u64,
    end: Option<u64>,
    reason: MissingReason,
) {
    if end == Some(start) {
        return;
    }
    if let Some(previous) = gaps.last_mut() {
        if previous.reason == reason && previous.end == Some(start) {
            previous.end = end;
            return;
        }
    }
    if gaps.len() < 4 {
        gaps.push(MissingRange { start, end, reason });
    } else if let Some(last) = gaps.last_mut() {
        last.end = end;
    }
}

fn stream_id(stream: Stream) -> i64 {
    match stream {
        Stream::Stdout => 0,
        Stream::Stderr => 1,
    }
}
fn max_pages(cap: u64) -> u64 {
    (2 * cap).div_ceil(PAGE) + 64
}

fn bounds(path: &Path, cap: u64) -> Result<()> {
    let db_bound = max_pages(cap) * PAGE;
    let size = std::fs::symlink_metadata(path)
        .map_err(|_| EngineError::StorageFailure)?
        .len();
    if size > db_bound {
        return Err(EngineError::StorageFailure);
    }
    let journal = PathBuf::from(format!("{}-journal", path.display()));
    match std::fs::symlink_metadata(journal) {
        Ok(meta) if meta.len() > 2 * db_bound + 65536 => Err(EngineError::StorageFailure),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(EngineError::StorageFailure),
    }
}

fn configure_bound(db: &Connection, cap: u64) -> Result<()> {
    let count: i64 = db
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(cipher::db_error)?;
    if count < 0 || count as u64 > max_pages(cap) {
        return Err(EngineError::StorageFailure);
    }
    db.pragma_update(None, "max_page_count", max_pages(cap) as i64)
        .map_err(cipher::db_error)?;
    Ok(())
}

// Cap is authenticated alongside the format/domain so readers can enforce the
// original stream's physical bound without accepting a caller-supplied budget.
fn metadata(db: &Connection, job: JobId, stream: Stream) -> Result<u64> {
    let (version, stored_job, stored_stream, cap): (i64, Vec<u8>, i64, i64) = db
        .query_row(
            "SELECT version,job,stream,cap FROM spool_meta WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(cipher::db_error)?;
    if version != 1
        || stored_job != job.0
        || stored_stream != stream_id(stream)
        || cap <= 0
        || cap > MAX_CAP as i64
    {
        return Err(EngineError::StorageFailure);
    }
    Ok(cap as u64)
}

fn initialize(db: &mut Connection, job: JobId, stream: Stream, cap: u64) -> Result<()> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(cipher::db_error)?;
    tx.execute_batch("CREATE TABLE spool_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,job BLOB NOT NULL,stream INTEGER NOT NULL,cap INTEGER NOT NULL);
        CREATE TABLE chunks(start INTEGER PRIMARY KEY CHECK(start>=0),end INTEGER NOT NULL,data BLOB NOT NULL CHECK(typeof(data)='blob' AND length(data)>0 AND length(data)<=16384 AND end-start=length(data)));
        CREATE INDEX chunks_end ON chunks(end);").map_err(cipher::db_error)?;
    tx.execute(
        "INSERT INTO spool_meta VALUES(1,1,?1,?2,?3)",
        params![&job.0[..], stream_id(stream), cap as i64],
    )
    .map_err(cipher::db_error)?;
    tx.commit().map_err(cipher::db_error)
}

// Inspect only lengths and offsets: never materialize an unbounded payload.
fn unsigned(row: &rusqlite::Row<'_>, index: usize) -> Result<u64> {
    let value: i64 = row.get(index).map_err(cipher::db_error)?;
    value.try_into().map_err(|_| EngineError::StorageFailure)
}

fn prefix(db: &Connection, cap: u64) -> Result<u64> {
    let mut query = db
        .prepare("SELECT start,end,length(data),typeof(data) FROM chunks ORDER BY start")
        .map_err(cipher::db_error)?;
    let mut rows = query.query([]).map_err(cipher::db_error)?;
    let mut end = 0;
    let mut previous_len = CHUNK;
    let mut count = 0;
    while let Some(row) = rows.next().map_err(cipher::db_error)? {
        let start = unsigned(row, 0)?;
        let next = unsigned(row, 1)?;
        let len = unsigned(row, 2)?;
        let kind: String = row.get(3).map_err(cipher::db_error)?;
        count += 1;
        if start != end
            || previous_len != CHUNK
            || len == 0
            || len > CHUNK
            || next.checked_sub(start) != Some(len)
            || next > cap
            || kind != "blob"
            || count > cap.div_ceil(CHUNK)
        {
            return Err(EngineError::StorageFailure);
        }
        end = next;
        previous_len = len;
    }
    Ok(end)
}

fn trim(db: &mut Connection, retained: u64) -> Result<()> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(cipher::db_error)?;
    tx.execute(
        "UPDATE chunks SET data=substr(data,1,?1-start),end=?1 WHERE start<?1 AND end>?1",
        [retained as i64],
    )
    .map_err(cipher::db_error)?;
    tx.execute("DELETE FROM chunks WHERE start>=?1", [retained as i64])
        .map_err(cipher::db_error)?;
    tx.commit().map_err(cipher::db_error)
}

impl Spool {
    pub fn open(
        root: &StateRoot,
        keys: &StorageKeys,
        job: JobId,
        stream: Stream,
        committed: &OutputStatus,
        cap: u64,
        guard: &IoGuard,
    ) -> Result<Self> {
        root.check_io(job, guard)?;
        if cap == 0
            || cap > MAX_CAP
            || committed.retained > cap
            || committed.produced < committed.retained
            || committed.gaps.len() > 4
        {
            return Err(EngineError::StorageFailure);
        }
        let mut commit = OutputCommit {
            retained: committed.retained,
            produced: committed.produced,
            closed: committed.closed,
            gaps: committed.gaps.clone(),
        };
        let mut failed = commit
            .gaps
            .iter()
            .any(|gap| gap.reason == MissingReason::StorageFailure);
        let opened = (|| {
            let path = root.prepare_spool_db(
                job,
                stream,
                !committed.closed && committed.retained == 0,
                guard,
            )?;
            bounds(&path, cap)?;
            let mut db = cipher::open(&path, keys, Context::Spool(job, stream), Journal::Delete)?;
            root.verify_spool_files(job, stream, guard)?;
            bounds(&path, cap)?;
            configure_bound(&db, cap)?;
            let tables: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'", [], |row| row.get(0)).map_err(cipher::db_error)?;
            if tables == 0 && !committed.closed && committed.retained == 0 {
                initialize(&mut db, job, stream, cap)?;
            }
            if metadata(&db, job, stream)? != cap {
                return Err(EngineError::StorageFailure);
            }
            let actual = prefix(&db, cap)?;
            if actual > commit.retained {
                trim(&mut db, commit.retained)?;
            }
            root.verify_spool_files(job, stream, guard)?;
            bounds(&path, cap)?;
            Ok((path, db, actual.min(commit.retained)))
        })();
        let (path, db) = match opened {
            Ok((path, db, actual)) => {
                if actual < commit.retained {
                    commit.retained = actual;
                    failed = true;
                    add_gap(
                        &mut commit.gaps,
                        actual,
                        Some(commit.produced),
                        MissingReason::StorageFailure,
                    );
                }
                (Some(path), Some(db))
            }
            Err(EngineError::Busy) => return Err(EngineError::Busy),
            Err(EngineError::UnsafePath) => return Err(EngineError::UnsafePath),
            Err(_) => {
                commit.retained = 0;
                failed = true;
                add_gap(
                    &mut commit.gaps,
                    0,
                    Some(commit.produced),
                    MissingReason::StorageFailure,
                );
                (None, None)
            }
        };
        Ok(Self {
            root: root.try_clone()?,
            job,
            stream,
            path,
            db,
            cap,
            commit,
            failed,
        })
    }

    fn persist(&mut self, bytes: &[u8], guard: &IoGuard) -> Result<()> {
        self.root.verify_spool_files(self.job, self.stream, guard)?;
        let path = self.path.as_ref().ok_or(EngineError::StorageFailure)?;
        bounds(path, self.cap)?;
        let db = self.db.as_mut().ok_or(EngineError::StorageFailure)?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(cipher::db_error)?;
        let mut offset = self.commit.retained;
        let mut remaining = bytes;
        if !offset.is_multiple_of(CHUNK) {
            let len = (CHUNK - offset % CHUNK).min(remaining.len() as u64) as usize;
            let changed = tx.execute("UPDATE chunks SET data=CAST(data || ?1 AS BLOB),end=?2 WHERE start=?3 AND end=?4", params![&remaining[..len], (offset + len as u64) as i64, (offset - offset % CHUNK) as i64, offset as i64]).map_err(cipher::db_error)?;
            if changed != 1 {
                return Err(EngineError::StorageFailure);
            }
            offset += len as u64;
            remaining = &remaining[len..];
        }
        for chunk in remaining.chunks(CHUNK as usize) {
            let end = offset + chunk.len() as u64;
            tx.execute(
                "INSERT INTO chunks VALUES(?1,?2,?3)",
                params![offset as i64, end as i64, chunk],
            )
            .map_err(cipher::db_error)?;
            offset = end;
        }
        bounds(path, self.cap)?;
        self.root.verify_spool_files(self.job, self.stream, guard)?;
        fault_point(&self.root, self.job, FaultPoint::BeforeSpoolSync);
        tx.commit().map_err(cipher::db_error)?;
        fault_point(&self.root, self.job, FaultPoint::AfterSpoolSync);
        self.root.verify_spool_files(self.job, self.stream, guard)?;
        bounds(path, self.cap)
    }

    pub fn append(&mut self, bytes: &[u8], guard: &IoGuard) -> Result<OutputCommit> {
        self.root.check_io(self.job, guard)?;
        if self.commit.closed {
            return Err(EngineError::StorageFailure);
        }
        let previous = self.commit.produced;
        let produced = previous
            .checked_add(bytes.len() as u64)
            .ok_or(EngineError::StorageFailure)?;
        let keep = self.cap.saturating_sub(previous).min(bytes.len() as u64) as usize;
        if keep > 0 && !self.failed {
            match self.persist(&bytes[..keep], guard) {
                Ok(()) => self.commit.retained += keep as u64,
                Err(EngineError::Busy) => return Err(EngineError::Busy),
                Err(EngineError::UnsafePath) => return Err(EngineError::UnsafePath),
                Err(_) => {
                    self.failed = true;
                    self.db.take();
                }
            }
        }
        self.commit.produced = produced;
        if self.failed {
            add_gap(
                &mut self.commit.gaps,
                previous.max(self.commit.retained),
                Some(produced),
                MissingReason::StorageFailure,
            );
        } else if produced > self.cap {
            add_gap(
                &mut self.commit.gaps,
                previous.max(self.cap),
                Some(produced),
                MissingReason::Limit,
            );
        }
        Ok(self.commit.clone())
    }

    pub fn close(&mut self, guard: &IoGuard) -> Result<OutputCommit> {
        self.root.check_io(self.job, guard)?;
        self.db.take();
        self.commit.closed = true;
        Ok(self.commit.clone())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn read(
        root: &StateRoot,
        keys: &StorageKeys,
        job: JobId,
        stream: Stream,
        status: &OutputStatus,
        cursor: u64,
        limit: usize,
        guard: &IoGuard,
    ) -> Result<OutputChunk> {
        root.check_io(job, guard)?;
        if limit == 0
            || limit > marmish_core::Limits::RAW_BYTES
            || status.retained > status.produced
            || status.gaps.len() > 4
        {
            return Err(EngineError::InvalidRequest);
        }
        let mut status = status.clone();
        let mut bytes = Vec::new();
        if cursor < status.retained {
            let loaded = load_prefix(
                root,
                keys,
                job,
                stream,
                status.retained,
                cursor,
                limit,
                guard,
            );
            let actual = match loaded {
                Ok((actual, output)) => {
                    bytes = output;
                    actual
                }
                Err(EngineError::Busy) => return Err(EngineError::Busy),
                Err(EngineError::UnsafePath) => return Err(EngineError::UnsafePath),
                Err(_) => 0,
            };
            if actual < status.retained {
                status.retained = actual;
                add_gap(
                    &mut status.gaps,
                    actual,
                    Some(status.produced),
                    MissingReason::StorageFailure,
                );
                status.gaps.sort_by_key(|gap| gap.start);
            }
        }
        let mut next_cursor = cursor
            .checked_add(bytes.len() as u64)
            .ok_or(EngineError::InvalidRequest)?;
        if bytes.is_empty() {
            if let Some(gap) = status
                .gaps
                .iter()
                .find(|gap| gap.start <= cursor && gap.end.is_none_or(|end| cursor < end))
            {
                next_cursor = gap.end.unwrap_or(gap.start);
            }
        }
        Ok(OutputChunk {
            bytes,
            next_cursor,
            status,
        })
    }
}

// Authenticate and validate the complete stream before returning a bounded slice.
#[allow(clippy::too_many_arguments)]
fn load_prefix(
    root: &StateRoot,
    keys: &StorageKeys,
    job: JobId,
    stream: Stream,
    retained: u64,
    cursor: u64,
    limit: usize,
    guard: &IoGuard,
) -> Result<(u64, Vec<u8>)> {
    let path = root.prepare_spool_db(job, stream, false, guard)?;
    // Bound even recovery work before SQLite can inspect a hot journal.
    bounds(&path, MAX_CAP)?;
    let db = cipher::open(&path, keys, Context::Spool(job, stream), Journal::Delete)?;
    root.verify_spool_files(job, stream, guard)?;
    let cap = metadata(&db, job, stream)?;
    bounds(&path, cap)?;
    configure_bound(&db, cap)?;
    let actual = prefix(&db, cap)?.min(retained);
    let end = actual.min(cursor.saturating_add(limit as u64));
    let mut output = Vec::new();
    if cursor < end {
        let mut query = db.prepare("SELECT substr(data,max(1,?1-start+1),min(end,?2)-max(start,?1)) FROM chunks WHERE end>?1 AND start<?2 ORDER BY start").map_err(cipher::db_error)?;
        let mut rows = query
            .query(params![cursor as i64, end as i64])
            .map_err(cipher::db_error)?;
        while let Some(row) = rows.next().map_err(cipher::db_error)? {
            let part: Vec<u8> = row.get(0).map_err(cipher::db_error)?;
            output.extend_from_slice(&part);
        }
        if output.len() as u64 != end - cursor {
            return Err(EngineError::StorageFailure);
        }
    }
    root.verify_spool_files(job, stream, guard)?;
    bounds(&path, cap)?;
    Ok((actual, output))
}

#[cfg(test)]
mod tests;
