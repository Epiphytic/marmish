//! Offline SQLCipher cursor state. Socket attribution, clock qualification and authority are caller obligations.

use std::path::Path;
use std::time::Duration;

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};
use storage_sqlite::{SqlCipherHardening, SqlCipherKey, open_hardened_sqlcipher};

#[cfg(test)]
#[path = "peer_installed_cursor.rs"]
pub(crate) mod installed;

#[path = "peer_cursor_pages.rs"]
mod pages;
#[path = "peer_cursor_retry.rs"]
mod retry;

#[path = "peer_consent_checkpoint.rs"]
pub(crate) mod consent_checkpoint;

pub(crate) use pages::{Page, ScanStart};
#[cfg(test)]
pub(crate) use pages::{ServiceTurn, Turn};
#[cfg(test)]
pub(crate) use retry::RetryFinding;
pub(crate) use retry::{Auth, Metadata, TrustedOutcome};

pub(crate) const EVENT_BYTES: usize = 64 * 1024;
const VISIBLE: usize = 256;
const DEFERRED_BYTES: usize = VISIBLE * EVENT_BYTES;
const MAX_ORIGINS: usize = 16;

pub(crate) type Origin = [u8; 32];
pub(crate) type EventId = [u8; 32];

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Storage,
    Binding,
    Range,
    ClockRegressed,
    Stale,
    Page,
    RelayContract,
    Backlog,
    Conflict,
    Classification,
}

impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}

pub(crate) struct HardenedPeerConnection(Connection);

impl HardenedPeerConnection {
    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn into_founder_ledger(self) -> Connection {
        self.0
    }

    pub(crate) fn open(path: &Path, key: [u8; 32]) -> Result<Self> {
        let conn = open_keyed(path, key)?;
        let mode: String = conn.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))?;
        if mode != "delete" {
            return Err(Error::Storage);
        }
        check_hardening(&conn)?;
        Ok(Self(conn))
    }
}

fn open_keyed(path: &Path, key: [u8; 32]) -> Result<Connection> {
    marmish_engine::initialize_storage().map_err(|_| Error::Storage)?;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    open_hardened_sqlcipher(
        &conn,
        &SqlCipherKey::raw(key),
        SqlCipherHardening::live_cache(),
    )
    .map_err(|_| Error::Storage)?;
    conn.busy_timeout(Duration::from_secs(2))?;
    conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

fn check_hardening(conn: &Connection) -> Result<()> {
    let cipher: String = conn.query_row("PRAGMA cipher_version", [], |row| row.get(0))?;
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    if cipher.is_empty() || mode != "delete" {
        return Err(Error::Storage);
    }
    for (pragma, expected) in [
        ("PRAGMA synchronous", 2),
        ("PRAGMA foreign_keys", 1),
        ("PRAGMA secure_delete", 1),
        ("PRAGMA temp_store", 2),
        ("PRAGMA busy_timeout", 2_000),
    ] {
        let actual: i64 = conn.query_row(pragma, [], |row| row.get(0))?;
        if actual != expected {
            return Err(Error::Storage);
        }
    }
    conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(())
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS peer_cursor_version (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    version INTEGER NOT NULL CHECK (version = 1),
    account BLOB NOT NULL CHECK (length(account) = 32)
) STRICT;
CREATE TABLE IF NOT EXISTS peer_cursor_origin (
    origin BLOB PRIMARY KEY CHECK (length(origin) = 32),
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    retry_revision INTEGER NOT NULL DEFAULT 0 CHECK (retry_revision >= 0),
    wall INTEGER NOT NULL DEFAULT 0 CHECK (wall >= 0),
    anchor INTEGER CHECK (anchor >= 0),
    lower INTEGER CHECK (lower >= 0),
    upper INTEGER CHECK (upper >= 0),
    boundary INTEGER CHECK (boundary >= 0),
    ties BLOB NOT NULL DEFAULT X'' CHECK (length(ties) <= 96 AND length(ties) % 32 = 0),
    visible INTEGER NOT NULL DEFAULT 0 CHECK (visible BETWEEN 0 AND 256),
    completed_anchor INTEGER CHECK (completed_anchor >= 0),
    completed_wall INTEGER CHECK (completed_wall >= 0),
    quarantine_until INTEGER CHECK (quarantine_until >= 0)
) STRICT;
CREATE TABLE IF NOT EXISTS peer_cursor_deferred (
    origin BLOB NOT NULL REFERENCES peer_cursor_origin(origin),
    id BLOB NOT NULL CHECK (length(id) = 32),
    stamp INTEGER NOT NULL CHECK (stamp >= 0),
    frame BLOB NOT NULL CHECK (length(frame) BETWEEN 1 AND 65536),
    auth INTEGER NOT NULL CHECK (auth BETWEEN 0 AND 2),
    expires INTEGER NOT NULL CHECK (expires >= 0),
    prerequisite BLOB CHECK (prerequisite IS NULL OR (auth = 2 AND length(prerequisite) = 32)),
    retry_at INTEGER NOT NULL CHECK (retry_at >= 0),
    step INTEGER NOT NULL CHECK (step BETWEEN 0 AND 6),
    revision INTEGER NOT NULL CHECK (revision > 0),
    PRIMARY KEY (origin, id)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS peer_cursor_due ON peer_cursor_deferred (origin, retry_at, id);
";

pub(crate) struct CursorStore {
    conn: Connection,
    origins: Vec<Origin>,
}

impl CursorStore {
    pub(crate) fn attach(
        HardenedPeerConnection(mut conn): HardenedPeerConnection,
        account: [u8; 32],
        configured_origins: &[Origin],
    ) -> Result<Self> {
        if configured_origins.is_empty() || configured_origins.len() > MAX_ORIGINS {
            return Err(Error::Binding);
        }
        let mut origins = configured_origins.to_vec();
        origins.sort_unstable();
        if origins.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::Binding);
        }
        check_hardening(&conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        let current = tx
            .query_row(
                "SELECT version, account FROM peer_cursor_version WHERE singleton=1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, [u8; 32]>(1)?)),
            )
            .optional()?;
        let mut stored = stored_origins(&tx)?;
        match current {
            None if stored.is_empty() => {
                tx.execute(
                    "INSERT INTO peer_cursor_version VALUES (1, 1, ?1)",
                    [account],
                )?;
                for origin in &origins {
                    tx.execute(
                        "INSERT INTO peer_cursor_origin (origin) VALUES (?1)",
                        [origin],
                    )?;
                }
                stored.clone_from(&origins);
            }
            Some((1, expected)) if expected == account => {}
            _ => return Err(Error::Binding),
        }
        if stored != origins {
            return Err(Error::Binding);
        }
        tx.commit()?;
        Ok(Self { conn, origins })
    }

    fn begin(&mut self, origin: Origin) -> Result<Transaction<'_>> {
        if self.origins.binary_search(&origin).is_err() {
            return Err(Error::Binding);
        }
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    fn with_wall(&mut self, origin: Origin, wall: u64) -> Result<Transaction<'_>> {
        let wall = seconds(wall)?;
        let tx = self.begin(origin)?;
        if tx.execute(
            "UPDATE peer_cursor_origin SET wall=?2 WHERE origin=?1 AND wall<=?2",
            params![origin, wall],
        )? != 1
        {
            return Err(Error::ClockRegressed);
        }
        Ok(tx)
    }
}

fn stored_origins(tx: &Transaction<'_>) -> Result<Vec<Origin>> {
    let mut statement =
        tx.prepare("SELECT origin FROM peer_cursor_origin ORDER BY origin LIMIT 17")?;
    let rows = statement.query_map([], |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn seconds(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Range)
}

fn read_seconds(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    row_seconds(column, row.get(column)?)
}

fn read_optional_seconds(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(column)?
        .map(|value| row_seconds(column, value))
        .transpose()
}

fn row_seconds(column: usize, value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}

fn after(wall: u64, delta: u64) -> Result<i64> {
    seconds(wall.checked_add(delta).ok_or(Error::Range)?)
}

#[derive(Clone, Copy)]
pub(crate) struct RawRecord<'a> {
    pub(crate) id: EventId,
    pub(crate) stamp: u64,
    pub(crate) bytes: &'a [u8],
}

impl RawRecord<'_> {
    fn check(self) -> Result<()> {
        if self.bytes.is_empty() || self.bytes.len() > EVENT_BYTES {
            return Err(Error::Range);
        }
        seconds(self.stamp)?;
        Ok(())
    }

    fn digest(self) -> [u8; 32] {
        Sha256::digest(self.bytes).into()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReceiptKind {
    Invalid,
    Deferred,
    Accepted,
}

pub(crate) struct Receipt {
    origin: Origin,
    id: EventId,
    stamp: u64,
    digest: [u8; 32],
    kind: ReceiptKind,
}

impl Receipt {
    fn new(origin: Origin, record: RawRecord<'_>, kind: ReceiptKind) -> Self {
        Self {
            origin,
            id: record.id,
            stamp: record.stamp,
            digest: record.digest(),
            kind,
        }
    }

    fn matches(&self, origin: Origin, record: RawRecord<'_>, digest: [u8; 32]) -> bool {
        self.origin == origin
            && self.id == record.id
            && self.stamp == record.stamp
            && self.digest == digest
    }
}

#[cfg(test)]
#[path = "peer_cursor_tests.rs"]
mod tests;
