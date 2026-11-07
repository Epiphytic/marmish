//! SQL-only local Get receiver leases and bounded replay-safe collection grants.
use super::*;
use crate::copy_local_gc::{
    self as local, Claim, Disposition, Entry, GcOutcome, LeaseId, Location, Operation,
    ReceiverBase, Request, Response,
};
use marmish_stream::{
    copy_protocol::{self as cp, AttemptMode, CopyError, Direction, Launch},
    wire::Binding,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
const ROW_CAP: i64 = 1024;
const REPLAY_CAP: i64 = 1024;
const RESERVE_TTL: u64 = 900;
#[derive(Clone, Serialize, Deserialize)]
struct Prior {
    state: String,
    location: Option<Location>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Gc {
    grant: Claim,
    attempts: u8,
    retry: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Row {
    launch: Launch,
    binding: Binding,
    group: Vec<u8>,
    base: ReceiverBase,
    revision: u64,
    state: String,
    location: Option<Location>,
    prior: Option<Prior>,
    retention: u64,
    expires: u64,
    updated: u64,
    retry: u64,
    gc: Option<Gc>,
    #[serde(default)]
    released: Option<Disposition>,
}
fn current_client_room(conn: &Connection, row: &Row, client: [u8; 32]) -> Result<bool> {
    if row.binding.client != client || row.launch.client != client {
        return Ok(false);
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM command_rooms WHERE group_id=?1 AND session=?2 AND peer=?3 AND direction=0 AND revoked=0)",
        params![row.group, row.binding.session.as_slice(), row.binding.server.as_slice()],
        |r| r.get(0),
    ).map_err(db)
}
fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(v).map_err(|_| Error::Storage("local_copy_encode"))
}
fn decode<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T> {
    serde_json::from_slice(b).map_err(|_| Error::Storage("local_copy_decode"))
}
fn failure(code: CopyError) -> Error {
    Error::Copy(code)
}
fn clock(id: &[u8; 16]) -> u64 {
    u64::from_be_bytes(id[..8].try_into().expect("fixed request identifier"))
}
fn get(conn: &Connection, key: &str) -> Result<Option<Row>> {
    let bytes: Option<Vec<u8>> = conn
        .query_row(
            "SELECT record FROM copy_local_receivers WHERE k=?1",
            [key],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    bytes.map(|b| decode(&b)).transpose()
}
fn save(conn: &Connection, key: &str, row: &Row) -> Result<()> {
    conn.execute("INSERT INTO copy_local_receivers(k,record) VALUES(?1,?2) ON CONFLICT(k) DO UPDATE SET record=excluded.record",params![key,encode(row)?]).map_err(db)?;
    Ok(())
}
fn remove(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM copy_local_receivers WHERE k=?1", [key])
        .map_err(db)?;
    Ok(())
}
fn rows(conn: &Connection) -> Result<Vec<(String, Row)>> {
    let mut statement = conn
        .prepare("SELECT k,record FROM copy_local_receivers ORDER BY k")
        .map_err(db)?;
    let values = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })
        .map_err(db)?;
    let mut out = Vec::new();
    for value in values {
        let (k, b) = value.map_err(db)?;
        out.push((k, decode(&b)?));
    }
    Ok(out)
}
fn immutable(a: &Launch, b: &Launch) -> bool {
    let mut a = a.clone();
    a.mode = b.mode;
    a == *b
}
fn lease_matches(row: &Row, lease: &LeaseId) -> bool {
    row.revision == lease.revision && row.binding.stream == lease.stream
}
fn normalize_retention(retention: u64) -> Result<u64> {
    let value = retention.max(cp::MIN_RETENTION_SECS as u64);
    if value > cp::MAX_RETENTION_SECS {
        return Err(failure(CopyError::Protocol));
    }
    Ok(value)
}
fn rollback(conn: &Connection, key: &str, mut row: Row) -> Result<()> {
    match row.prior.take() {
        None if row.location.is_none() => remove(conn, key),
        Some(prior) => {
            row.state = if prior.state == "closed" {
                "closed".into()
            } else {
                "unknown".into()
            };
            row.location = prior.location;
            save(conn, key, &row)
        }
        _ => {
            row.state = "unknown".into();
            save(conn, key, &row)
        }
    }
}
// Fence old timestamps before removing replay results, including internally
// generated prune records whose timestamps can predate a new incoming request.
fn trim_replays(conn: &Connection) -> Result<()> {
    let count: i64 = conn
        .query_row("SELECT count(*) FROM copy_local_replays", [], |r| r.get(0))
        .map_err(db)?;
    if count > REPLAY_CAP {
        let oldest: i64 = conn
            .query_row(
                "SELECT created FROM copy_local_replays ORDER BY created LIMIT 1 OFFSET ?1",
                [count - REPLAY_CAP - 1],
                |r| r.get(0),
            )
            .map_err(db)?;
        conn.execute("UPDATE copy_local_floor SET floor=max(floor,?1)", [oldest])
            .map_err(db)?;
        conn.execute("DELETE FROM copy_local_replays WHERE created<=?1", [oldest])
            .map_err(db)?;
    }
    Ok(())
}
fn record_prune(conn: &Connection, key: &str, row: &Row) -> Result<()> {
    let mut hash = Sha256::new();
    hash.update(b"marmish-local-receiver-prune-v1");
    hash.update(key.as_bytes());
    hash.update(row.revision.to_be_bytes());
    hash.update(row.updated.to_be_bytes());
    let digest = hash.finalize();
    let mut id = [0u8; 16];
    id[..8].copy_from_slice(&row.updated.to_be_bytes());
    id[8..].copy_from_slice(&digest[..8]);
    conn.execute(
        "INSERT OR IGNORE INTO copy_local_replays VALUES(?1,?2,?3,?4)",
        params![
            id.as_slice(),
            i64::try_from(row.updated).map_err(|_| failure(CopyError::Protocol))?,
            digest.as_slice(),
            encode(&Response::Ok)?
        ],
    )
    .map_err(db)?;
    Ok(())
}
fn expire(conn: &Connection, now: u64, restart: bool) -> Result<()> {
    for (key, mut row) in rows(conn)? {
        if row.state == "reserved" && (restart || now.saturating_sub(row.updated) >= RESERVE_TTL) {
            record_prune(conn, &key, &row)?;
            rollback(conn, &key, row)?;
        } else if restart && row.state == "active" {
            row.state = "unknown".into();
            save(conn, &key, &row)?;
        }
    }
    trim_replays(conn)
}
impl Journal {
    pub(crate) fn copy_local_init(&mut self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS copy_local_receivers(k TEXT PRIMARY KEY,record BLOB NOT NULL); CREATE TABLE IF NOT EXISTS copy_local_replays(id BLOB PRIMARY KEY,created INTEGER NOT NULL,digest BLOB NOT NULL,response BLOB NOT NULL); CREATE TABLE IF NOT EXISTS copy_local_floor(singleton INTEGER PRIMARY KEY CHECK(singleton=1),floor INTEGER NOT NULL); INSERT OR IGNORE INTO copy_local_floor VALUES(1,0);").map_err(db)?;
        let now = u64::try_from(crate::copy_worker::now().map_err(|_| Error::Invalid("clock"))?)
            .map_err(|_| Error::Invalid("clock"))?;
        let tx = self.conn.unchecked_transaction().map_err(db)?;
        expire(&tx, now, true)?;
        tx.commit().map_err(db)
    }
    pub(crate) fn copy_local_request(
        &mut self,
        request: Request,
        client: [u8; 32],
        retention: u64,
        now: u64,
    ) -> Result<Response> {
        let timestamp = clock(&request.id);
        let horizon = crate::foundation::RETRY_HORIZON;
        if timestamp > now.saturating_add(60) || now.saturating_sub(timestamp) > horizon {
            return Err(failure(CopyError::Conflict));
        }
        let bytes = encode(&request)?;
        if bytes.len() > 32 * 1024 {
            return Err(failure(CopyError::Protocol));
        }
        let digest: Vec<u8> = Sha256::digest(&bytes).to_vec();
        let tx = self.conn.unchecked_transaction().map_err(db)?;
        let floor: i64 = tx
            .query_row(
                "SELECT floor FROM copy_local_floor WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .map_err(db)?;
        if timestamp <= u64::try_from(floor).map_err(|_| Error::Storage("local_copy_floor"))? {
            return Err(failure(CopyError::Conflict));
        }
        let old: Option<(Vec<u8>, Vec<u8>)> = tx
            .query_row(
                "SELECT digest,response FROM copy_local_replays WHERE id=?1",
                [request.id.as_slice()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        if let Some((old, response)) = old {
            if old != digest {
                return Err(failure(CopyError::Conflict));
            }
            // Cached activation is permission to start a receiver only while
            // that exact lease remains active. Release, supersession or restart
            // must not let a delayed retry recreate unindexed physical state.
            if let Operation::Activate { lease } = &request.operation {
                let current = get(&tx, &lease.namespace)?;
                let row = current.ok_or_else(|| failure(CopyError::Conflict))?;
                if !lease_matches(&row, lease)
                    || row.state != "active"
                    || !current_client_room(&tx, &row, client)?
                {
                    return Err(failure(CopyError::Conflict));
                }
            }
            let cached: Response = decode(&response)?;
            if let Response::Granted { claim: Some(grant) } = &cached {
                let current = get(&tx, &grant.namespace)?;
                let still_owned = match current {
                    Some(row) if row.state == "closed" && row.revision == grant.revision => {
                        match row.gc {
                            Some(gc) => encode(&gc.grant)? == encode(grant)?,
                            None => false,
                        }
                    }
                    _ => false,
                };
                if !still_owned {
                    return Err(failure(CopyError::Conflict));
                }
            }
            return Ok(cached);
        }
        expire(&tx, now, false)?;
        let response = match apply(&tx, &request, client, normalize_retention(retention)?, now) {
            Ok(response) => response,
            Err(Error::Copy(code)) => Response::Rejected { code },
            Err(error) => return Err(error),
        };
        tx.execute(
            "INSERT INTO copy_local_replays VALUES(?1,?2,?3,?4)",
            params![
                request.id.as_slice(),
                i64::try_from(timestamp).map_err(|_| failure(CopyError::Protocol))?,
                digest,
                encode(&response)?
            ],
        )
        .map_err(db)?;
        trim_replays(&tx)?;
        tx.commit().map_err(db)?;
        Ok(response)
    }
}
fn apply(
    conn: &Connection,
    request: &Request,
    client: [u8; 32],
    retention: u64,
    now: u64,
) -> Result<Response> {
    match &request.operation {
        Operation::Reserve {
            launch,
            binding,
            base,
        } => reserve(conn, launch, binding, base, client, retention, now),
        Operation::Activate { lease } => {
            let mut row =
                get(conn, &lease.namespace)?.ok_or_else(|| failure(CopyError::Conflict))?;
            if !lease_matches(&row, lease)
                || row.state != "reserved"
                || !current_client_room(conn, &row, client)?
            {
                return Err(failure(CopyError::Conflict));
            }
            let key = crate::stream_service::key(&row.group, &row.binding.stream);
            let bytes: Option<Vec<u8>> = conn
                .query_row("SELECT record FROM live_streams WHERE k=?1", [key], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(db)?;
            let record: crate::stream_service::StreamRecord =
                decode(&bytes.ok_or_else(|| failure(CopyError::Conflict))?)?;
            if record.server_side
                || record.binding != row.binding
                || record.group != row.group
                || !matches!(&record.launch,crate::stream_service::StreamLaunch::NativeCopy{launch} if immutable(launch,&row.launch))
                || record.open_digest
                    != Some(
                        row.launch
                            .digest(record.codec)
                            .map_err(|_| failure(CopyError::Protocol))?,
                    )
            {
                return Err(failure(CopyError::Conflict));
            }
            row.state = "active".into();
            row.updated = now;
            save(conn, &lease.namespace, &row)?;
            Ok(Response::Ok)
        }
        Operation::Release { lease, disposition } => release(conn, lease, disposition, now),
        Operation::Claim { exclude } => claim(conn, request.id, exclude, now),
        Operation::Complete {
            claim,
            revision,
            outcome,
        } => complete(conn, *claim, *revision, outcome, now),
        Operation::Forget {
            namespace,
            revision,
            acknowledge_retained_state,
        } => {
            let row = get(conn, namespace)?.ok_or_else(|| failure(CopyError::NotFound))?;
            if !acknowledge_retained_state
                || row.revision != *revision
                || row.state != "unknown"
                || row.gc.is_some()
            {
                return Err(failure(CopyError::Busy));
            }
            remove(conn, namespace)?;
            Ok(Response::Ok)
        }
        Operation::Inspect => Ok(Response::Entries {
            entries: rows(conn)?
                .into_iter()
                .map(|(namespace, r)| Entry {
                    namespace,
                    state: r.state,
                    revision: r.revision,
                })
                .collect(),
        }),
    }
}
fn reserve(
    conn: &Connection,
    launch: &Launch,
    binding: &Binding,
    base: &ReceiverBase,
    client: [u8; 32],
    retention: u64,
    now: u64,
) -> Result<Response> {
    launch
        .validate()
        .map_err(|_| failure(CopyError::Protocol))?;
    if launch.direction != Direction::Get
        || launch.client != client
        || binding.client != client
        || binding.server != launch.server
        || base.path.is_empty()
        || base.path.len() > cp::MAX_PATH
        || base.path[0] != b'/'
        || base.path.contains(&0)
    {
        return Err(failure(CopyError::Protocol));
    }
    let group: Vec<u8> = conn
        .query_row(
            "SELECT group_id FROM command_rooms WHERE session=?1 AND peer=?2 AND direction=0 AND revoked=0",
            params![binding.session.as_slice(), binding.server.as_slice()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?
        .ok_or_else(|| failure(CopyError::Conflict))?;
    let key = local::namespace(launch);
    let old = get(conn, &key)?;
    let (revision, location, prior, expires) = match old {
        Some(mut old) => {
            if !immutable(&old.launch, launch) || old.base != *base {
                return Err(failure(CopyError::Conflict));
            }
            if let Some(gc) = old.gc.as_mut() {
                if now >= gc.retry {
                    gc.attempts = gc.attempts.saturating_add(1);
                    gc.retry = now.saturating_add(60);
                    if gc.attempts >= 5 {
                        old.gc = None;
                        old.state = "unknown".into();
                    }
                    save(conn, &key, &old)?;
                }
                return Err(failure(CopyError::Busy));
            }
            if launch.mode == AttemptMode::Create {
                return Err(failure(if old.state == "closed" {
                    CopyError::Conflict
                } else {
                    CopyError::Busy
                }));
            }
            let prior = if old.state == "reserved" {
                old.prior
            } else {
                Some(Prior {
                    state: old.state,
                    location: old.location.clone(),
                })
            };
            (
                old.revision
                    .checked_add(1)
                    .ok_or_else(|| failure(CopyError::Quota))?,
                old.location,
                prior,
                old.expires,
            )
        }
        None => {
            let count: i64 = conn
                .query_row("SELECT count(*) FROM copy_local_receivers", [], |r| {
                    r.get(0)
                })
                .map_err(db)?;
            if count >= ROW_CAP {
                return Err(failure(CopyError::Quota));
            }
            (1, None, None, 0)
        }
    };
    let row = Row {
        launch: launch.clone(),
        binding: binding.clone(),
        group,
        base: base.clone(),
        revision,
        state: "reserved".into(),
        location,
        prior,
        retention,
        expires: expires.max(
            now.checked_add(retention)
                .ok_or_else(|| failure(CopyError::Protocol))?,
        ),
        updated: now,
        retry: 0,
        gc: None,
        released: None,
    };
    save(conn, &key, &row)?;
    Ok(Response::Reserved {
        lease: LeaseId {
            namespace: key,
            stream: binding.stream,
            revision,
        },
        retention,
    })
}
fn release(
    conn: &Connection,
    lease: &LeaseId,
    disposition: &Disposition,
    now: u64,
) -> Result<Response> {
    let mut row = get(conn, &lease.namespace)?.ok_or_else(|| failure(CopyError::Conflict))?;
    if !lease_matches(&row, lease) {
        return Err(failure(CopyError::Conflict));
    }
    if let Some(previous) = &row.released {
        return if previous == disposition {
            Ok(Response::Ok)
        } else {
            Err(failure(CopyError::Conflict))
        };
    }
    match disposition {
        Disposition::NeverSubmitted => {
            if !matches!(row.state.as_str(), "reserved" | "active") {
                return Err(failure(CopyError::Conflict));
            }
            row.released = Some(disposition.clone());
            rollback(conn, &lease.namespace, row)?;
        }
        Disposition::Attempted => {
            if !matches!(row.state.as_str(), "active" | "unknown") {
                return Err(failure(CopyError::Conflict));
            }
            row.released = Some(disposition.clone());
            row.state = "unknown".into();
            row.prior = None;
            save(conn, &lease.namespace, &row)?;
        }
        Disposition::Established { location } => {
            if location.base != row.base || location.stream != lease.stream || row.state != "active"
            {
                return Err(failure(CopyError::Conflict));
            }
            row.released = Some(disposition.clone());
            row.location = Some(location.clone());
            row.prior = None;
            row.state = "closed".into();
            row.updated = now;
            row.expires = row.expires.max(
                now.checked_add(row.retention)
                    .ok_or_else(|| failure(CopyError::Protocol))?,
            );
            save(conn, &lease.namespace, &row)?;
        }
    }
    Ok(Response::Ok)
}
fn claim(conn: &Connection, id: [u8; 16], exclude: &str, now: u64) -> Result<Response> {
    let mut all = rows(conn)?;
    all.sort_by_key(|(k, r)| (r.retry, r.expires, k.clone()));
    // Exhaustion is evaluated even without completion: abandoned executors cannot
    // pin every claim slot forever. Physical state remains unknown and retained.
    for (k, r) in &mut all {
        if let Some(gc) = &r.gc
            && gc.attempts >= 5
            && now >= gc.retry
        {
            r.gc = None;
            r.state = "unknown".into();
            save(conn, k, r)?;
        }
    }
    let count = all.iter().filter(|(_, r)| r.gc.is_some()).count();
    for (key, mut row) in all {
        if key == exclude || row.retry > now {
            continue;
        }
        if let Some(mut gc) = row.gc.take() {
            if gc.retry > now {
                continue;
            }
            gc.attempts += 1;
            gc.retry = now.saturating_add(60u64 << gc.attempts.saturating_sub(1).min(4));
            row.retry = gc.retry;
            let grant = gc.grant.clone();
            row.gc = Some(gc);
            save(conn, &key, &row)?;
            return Ok(Response::Granted {
                claim: Some(Box::new(grant)),
            });
        }
        if count >= 8 || row.state != "closed" || row.expires > now {
            continue;
        }
        let Some(location) = row.location.clone() else {
            continue;
        };
        let grant = Claim {
            id,
            namespace: key.clone(),
            revision: row.revision,
            launch: row.launch.clone(),
            location,
            timestamp: now,
        };
        row.gc = Some(Gc {
            grant: grant.clone(),
            attempts: 1,
            retry: now.saturating_add(60),
        });
        row.retry = now.saturating_add(60);
        save(conn, &key, &row)?;
        return Ok(Response::Granted {
            claim: Some(Box::new(grant)),
        });
    }
    Ok(Response::Granted { claim: None })
}
fn complete(
    conn: &Connection,
    id: [u8; 16],
    revision: u64,
    outcome: &GcOutcome,
    now: u64,
) -> Result<Response> {
    let (key, mut row) = rows(conn)?
        .into_iter()
        .find(|(_, r)| {
            r.gc.as_ref()
                .is_some_and(|g| g.grant.id == id && g.grant.revision == revision)
        })
        .ok_or_else(|| failure(CopyError::Conflict))?;
    match outcome {
        GcOutcome::Cleaned => remove(conn, &key)?,
        GcOutcome::Busy => {
            row.gc = None;
            row.retry = now.saturating_add(60);
            save(conn, &key, &row)?;
        }
        GcOutcome::NotExpired { expires } => {
            row.gc = None;
            row.expires = row.expires.max(*expires);
            row.retry = row.expires;
            save(conn, &key, &row)?;
        }
        GcOutcome::Uncertain => {
            row.gc = None;
            row.state = "unknown".into();
            save(conn, &key, &row)?;
        }
        GcOutcome::Transient => {
            if row.gc.as_ref().is_some_and(|g| g.attempts >= 5) {
                row.gc = None;
                row.state = "unknown".into();
            }
            save(conn, &key, &row)?;
        }
    }
    Ok(Response::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: u64 = 1_000_000;
    fn journal() -> (tempfile::TempDir, Journal) {
        let dir = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let journal = Journal::open(
            &dir.path().join("journal"),
            &[91; 32],
            &hex::encode([1; 32]),
        )
        .unwrap();
        journal
            .conn
            .execute(
                "INSERT INTO command_rooms(group_id,transport,session,peer,direction,authority) VALUES(?1,?2,?3,?4,0,?5)",
                params![vec![5u8], vec![5u8], [4u8;16].as_slice(), [2u8;32].as_slice(), [8u8;32].as_slice()],
            )
            .unwrap();
        (dir, journal)
    }
    fn launch() -> Launch {
        Launch {
            version: cp::VERSION,
            mode: AttemptMode::Create,
            transfer: [3; 32],
            client: [1; 32],
            server: [2; 32],
            direction: Direction::Get,
            source: b"source".to_vec(),
            destination: b"destination".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
        }
    }
    fn binding() -> Binding {
        Binding {
            client: [1; 32],
            server: [2; 32],
            session: [4; 16],
            stream: [6; 16],
        }
    }
    fn base() -> ReceiverBase {
        ReceiverBase {
            path: b"/private/base".to_vec(),
            identity: local::Identity {
                device: 7,
                inode: 8,
            },
        }
    }
    fn request(n: u64, operation: Operation) -> Request {
        let mut id = [0; 16];
        id[..8].copy_from_slice(&NOW.to_be_bytes());
        id[8..].copy_from_slice(&n.to_be_bytes());
        Request { id, operation }
    }
    fn reserve(j: &mut Journal, n: u64, l: Launch, b: Binding) -> LeaseId {
        match j
            .copy_local_request(
                request(
                    n,
                    Operation::Reserve {
                        launch: l,
                        binding: b,
                        base: base(),
                    },
                ),
                [1; 32],
                cp::MIN_RETENTION_SECS as u64,
                NOW,
            )
            .unwrap()
        {
            Response::Reserved { lease, .. } => lease,
            _ => panic!("expected reservation"),
        }
    }
    fn activate_for_test(j: &Journal, lease: &LeaseId) {
        let mut row = get(&j.conn, &lease.namespace).unwrap().unwrap();
        row.state = "active".into();
        save(&j.conn, &lease.namespace, &row).unwrap();
    }
    fn close(j: &mut Journal, lease: &LeaseId, n: u64) {
        activate_for_test(j, lease);
        let location = Location {
            base: base(),
            parent: local::Identity {
                device: 9,
                inode: 10,
            },
            stream: lease.stream,
        };
        assert!(matches!(
            j.copy_local_request(
                request(
                    n,
                    Operation::Release {
                        lease: lease.clone(),
                        disposition: Disposition::Established { location }
                    }
                ),
                [1; 32],
                cp::MIN_RETENTION_SECS as u64,
                NOW
            )
            .unwrap(),
            Response::Ok
        ));
    }
    #[test]
    fn reserve_retry_is_exact_and_changed_request_is_conflict() {
        let (_dir, mut j) = journal();
        let operation = Operation::Reserve {
            launch: launch(),
            binding: binding(),
            base: base(),
        };
        let req = request(1, operation);
        let first = j.copy_local_request(req.clone(), [1; 32], 1, NOW).unwrap();
        let second = j.copy_local_request(req.clone(), [1; 32], 1, NOW).unwrap();
        assert_eq!(encode(&first).unwrap(), encode(&second).unwrap());
        let mut changed = req;
        changed.operation = Operation::Inspect;
        assert!(matches!(
            j.copy_local_request(changed, [1; 32], 1, NOW),
            Err(Error::Copy(CopyError::Conflict))
        ));
    }
    #[test]
    fn never_submitted_removes_fresh_intent_and_restores_closed_predecessor() {
        let (_dir, mut j) = journal();
        let lease = reserve(&mut j, 1, launch(), binding());
        assert!(matches!(
            j.copy_local_request(
                request(
                    2,
                    Operation::Release {
                        lease: lease.clone(),
                        disposition: Disposition::NeverSubmitted
                    }
                ),
                [1; 32],
                1,
                NOW
            )
            .unwrap(),
            Response::Ok
        ));
        assert!(get(&j.conn, &lease.namespace).unwrap().is_none());
        let lease = reserve(&mut j, 3, launch(), binding());
        close(&mut j, &lease, 4);
        let old = get(&j.conn, &lease.namespace).unwrap().unwrap();
        let mut l = launch();
        l.mode = AttemptMode::Resume;
        let mut b = binding();
        b.stream = [7; 16];
        let resumed = reserve(&mut j, 5, l, b);
        j.copy_local_request(
            request(
                6,
                Operation::Release {
                    lease: resumed,
                    disposition: Disposition::NeverSubmitted,
                },
            ),
            [1; 32],
            1,
            NOW,
        )
        .unwrap();
        let restored = get(&j.conn, &lease.namespace).unwrap().unwrap();
        assert_eq!(restored.state, "closed");
        assert!(restored.location == old.location);
    }
    #[test]
    fn cached_activation_never_reauthorizes_a_released_or_superseded_lease() {
        let (_dir, mut j) = journal();
        let lease = reserve(&mut j, 1, launch(), binding());
        activate_for_test(&j, &lease);
        let activation = request(
            2,
            Operation::Activate {
                lease: lease.clone(),
            },
        );
        // Model the durable response left by an already admitted activation.
        let digest = Sha256::digest(encode(&activation).unwrap()).to_vec();
        j.conn
            .execute(
                "INSERT INTO copy_local_replays VALUES(?1,?2,?3,?4)",
                params![
                    activation.id.as_slice(),
                    i64::try_from(NOW).unwrap(),
                    digest,
                    encode(&Response::Ok).unwrap()
                ],
            )
            .unwrap();
        assert!(matches!(
            j.copy_local_request(activation.clone(), [1; 32], 1, NOW)
                .unwrap(),
            Response::Ok
        ));
        j.copy_local_request(
            request(
                3,
                Operation::Release {
                    lease,
                    disposition: Disposition::NeverSubmitted,
                },
            ),
            [1; 32],
            1,
            NOW,
        )
        .unwrap();
        assert!(matches!(
            j.copy_local_request(activation, [1; 32], 1, NOW),
            Err(Error::Copy(CopyError::Conflict))
        ));
    }
    #[test]
    fn chained_unactivated_reservations_remove_fresh_intent_and_prune_is_replayed() {
        let (_dir, mut j) = journal();
        let first = reserve(&mut j, 1, launch(), binding());
        let mut l = launch();
        l.mode = AttemptMode::Resume;
        let mut b = binding();
        b.stream = [7; 16];
        let second = reserve(&mut j, 2, l.clone(), b.clone());
        b.stream = [8; 16];
        let third = reserve(&mut j, 3, l, b);
        assert!(third.revision > second.revision);
        j.copy_local_request(
            request(
                4,
                Operation::Release {
                    lease: third,
                    disposition: Disposition::NeverSubmitted,
                },
            ),
            [1; 32],
            1,
            NOW,
        )
        .unwrap();
        assert!(get(&j.conn, &first.namespace).unwrap().is_none());
        let fresh = reserve(&mut j, 5, launch(), binding());
        let before: i64 = j
            .conn
            .query_row("SELECT count(*) FROM copy_local_replays", [], |r| r.get(0))
            .unwrap();
        expire(&j.conn, NOW + RESERVE_TTL, false).unwrap();
        assert!(get(&j.conn, &fresh.namespace).unwrap().is_none());
        let after: i64 = j
            .conn
            .query_row("SELECT count(*) FROM copy_local_replays", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after, before + 1);
        expire(&j.conn, NOW + RESERVE_TTL, false).unwrap();
        let repeated: i64 = j
            .conn
            .query_row("SELECT count(*) FROM copy_local_replays", [], |r| r.get(0))
            .unwrap();
        assert_eq!(repeated, after);
    }
    #[test]
    fn resume_supersedes_unknown_and_stale_release_cannot_close_new_revision() {
        let (_dir, mut j) = journal();
        let old = reserve(&mut j, 1, launch(), binding());
        activate_for_test(&j, &old);
        let mut l = launch();
        l.mode = AttemptMode::Resume;
        let mut b = binding();
        b.stream = [7; 16];
        let new = reserve(&mut j, 2, l, b);
        assert!(new.revision > old.revision);
        assert!(matches!(
            j.copy_local_request(
                request(
                    3,
                    Operation::Release {
                        lease: old,
                        disposition: Disposition::NeverSubmitted
                    }
                ),
                [1; 32],
                1,
                NOW
            )
            .unwrap(),
            Response::Rejected {
                code: CopyError::Conflict
            }
        ));
    }
    #[test]
    fn restart_prunes_unactivated_and_retains_activated_unknown() {
        let (_dir, mut j) = journal();
        let first = reserve(&mut j, 1, launch(), binding());
        let mut l = launch();
        l.transfer = [9; 32];
        let active = reserve(&mut j, 2, l, binding());
        activate_for_test(&j, &active);
        j.copy_local_init().unwrap();
        assert!(get(&j.conn, &first.namespace).unwrap().is_none());
        assert_eq!(
            get(&j.conn, &active.namespace).unwrap().unwrap().state,
            "unknown"
        );
        assert!(matches!(
            j.copy_local_request(
                request(3, Operation::Activate { lease: first }),
                [1; 32],
                1,
                NOW
            )
            .unwrap(),
            Response::Rejected {
                code: CopyError::Conflict
            }
        ));
    }
    #[test]
    fn wrong_base_and_unpersisted_open_activation_are_refused() {
        let (_dir, mut j) = journal();
        let lease = reserve(&mut j, 1, launch(), binding());
        assert!(matches!(
            j.copy_local_request(request(2, Operation::Activate { lease }), [1; 32], 1, NOW)
                .unwrap(),
            Response::Rejected {
                code: CopyError::Conflict
            }
        ));
        let mut l = launch();
        l.mode = AttemptMode::Resume;
        let mut changed = base();
        changed.identity.inode += 1;
        assert!(matches!(
            j.copy_local_request(
                request(
                    3,
                    Operation::Reserve {
                        launch: l,
                        binding: binding(),
                        base: changed
                    }
                ),
                [1; 32],
                1,
                NOW
            )
            .unwrap(),
            Response::Rejected {
                code: CopyError::Conflict
            }
        ));
    }
    #[test]
    fn pruning_many_distinct_timestamps_restores_exact_replay_cap() {
        let (_dir, j) = journal();
        let tx = j.conn.unchecked_transaction().unwrap();
        for n in 0..REPLAY_CAP + 7 {
            tx.execute(
                "INSERT INTO copy_local_replays VALUES(?1,?2,?3,?4)",
                params![
                    n.to_be_bytes().as_slice(),
                    i64::try_from(NOW).unwrap() + n,
                    vec![0u8; 32],
                    encode(&Response::Ok).unwrap()
                ],
            )
            .unwrap();
        }
        trim_replays(&tx).unwrap();
        let count: i64 = tx
            .query_row("SELECT count(*) FROM copy_local_replays", [], |r| r.get(0))
            .unwrap();
        let floor: i64 = tx
            .query_row("SELECT floor FROM copy_local_floor", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, REPLAY_CAP);
        assert_eq!(floor, i64::try_from(NOW).unwrap() + 6);
        tx.commit().unwrap();
    }
    #[test]
    fn replay_cap_eviction_never_blocks_release_gc_or_reserve() {
        let (_dir, mut j) = journal();
        let lease = reserve(&mut j, 1, launch(), binding());
        activate_for_test(&j, &lease);
        let tx = j.conn.unchecked_transaction().unwrap();
        for n in 0..REPLAY_CAP {
            let id = (n + 1).to_be_bytes();
            tx.execute(
                "INSERT INTO copy_local_replays VALUES(?1,?2,?3,?4)",
                params![
                    id.as_slice(),
                    i64::try_from(NOW - 100).unwrap(),
                    vec![0u8; 32],
                    encode(&Response::Ok).unwrap()
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        close(&mut j, &lease, 2);
        assert_eq!(
            get(&j.conn, &lease.namespace).unwrap().unwrap().state,
            "closed"
        );
        let mut row = get(&j.conn, &lease.namespace).unwrap().unwrap();
        row.expires = NOW - 1;
        save(&j.conn, &lease.namespace, &row).unwrap();
        let Response::Granted { claim: Some(grant) } = j
            .copy_local_request(
                request(
                    3,
                    Operation::Claim {
                        exclude: String::new(),
                    },
                ),
                [1; 32],
                1,
                NOW,
            )
            .unwrap()
        else {
            panic!("expected claim")
        };
        j.copy_local_request(
            request(
                4,
                Operation::Complete {
                    claim: grant.id,
                    revision: grant.revision,
                    outcome: GcOutcome::Cleaned,
                },
            ),
            [1; 32],
            1,
            NOW,
        )
        .unwrap();
        reserve(&mut j, 5, launch(), binding());
        let mut stale = request(6, Operation::Inspect);
        stale.id[..8].copy_from_slice(&(NOW - 100).to_be_bytes());
        assert!(matches!(
            j.copy_local_request(stale, [1; 32], 1, NOW),
            Err(Error::Copy(CopyError::Conflict))
        ));
    }
    #[test]
    fn final_release_disposition_is_idempotent_and_altered_release_is_refused() {
        let (_dir, mut j) = journal();
        let lease = reserve(&mut j, 1, launch(), binding());
        activate_for_test(&j, &lease);
        let attempted = request(
            2,
            Operation::Release {
                lease: lease.clone(),
                disposition: Disposition::Attempted,
            },
        );
        assert!(matches!(
            j.copy_local_request(attempted, [1; 32], 1, NOW).unwrap(),
            Response::Ok
        ));
        let before = encode(&get(&j.conn, &lease.namespace).unwrap().unwrap()).unwrap();
        for (n, disposition) in [
            (3, Disposition::NeverSubmitted),
            (
                4,
                Disposition::Established {
                    location: Location {
                        base: base(),
                        parent: local::Identity {
                            device: 9,
                            inode: 10,
                        },
                        stream: lease.stream,
                    },
                },
            ),
        ] {
            assert!(matches!(
                j.copy_local_request(
                    request(
                        n,
                        Operation::Release {
                            lease: lease.clone(),
                            disposition
                        }
                    ),
                    [1; 32],
                    1,
                    NOW
                )
                .unwrap(),
                Response::Rejected {
                    code: CopyError::Conflict
                }
            ));
            assert_eq!(
                encode(&get(&j.conn, &lease.namespace).unwrap().unwrap()).unwrap(),
                before
            );
        }
        assert!(matches!(
            j.copy_local_request(
                request(
                    5,
                    Operation::Release {
                        lease: lease.clone(),
                        disposition: Disposition::Attempted
                    }
                ),
                [1; 32],
                1,
                NOW
            )
            .unwrap(),
            Response::Ok
        ));
        let mut l = launch();
        l.mode = AttemptMode::Resume;
        let mut b = binding();
        b.stream = [7; 16];
        let next = reserve(&mut j, 6, l, b);
        close(&mut j, &next, 7);
        let closed = encode(&get(&j.conn, &next.namespace).unwrap().unwrap()).unwrap();
        for (n, disposition) in [
            (8, Disposition::Attempted),
            (9, Disposition::NeverSubmitted),
        ] {
            assert!(matches!(
                j.copy_local_request(
                    request(
                        n,
                        Operation::Release {
                            lease: next.clone(),
                            disposition
                        }
                    ),
                    [1; 32],
                    1,
                    NOW
                )
                .unwrap(),
                Response::Rejected {
                    code: CopyError::Conflict
                }
            ));
            assert_eq!(
                encode(&get(&j.conn, &next.namespace).unwrap().unwrap()).unwrap(),
                closed
            );
        }
    }
    #[test]
    fn gc_retry_backoff_matches_each_grant_number() {
        let (_dir, mut j) = journal();
        let lease = reserve(&mut j, 1, launch(), binding());
        close(&mut j, &lease, 2);
        let mut row = get(&j.conn, &lease.namespace).unwrap().unwrap();
        row.expires = NOW - 1;
        save(&j.conn, &lease.namespace, &row).unwrap();
        let mut time = NOW;
        for (index, delay) in [60u64, 120, 240, 480, 960].into_iter().enumerate() {
            let response = claim(&j.conn, [index as u8; 16], "", time).unwrap();
            assert!(matches!(response, Response::Granted { claim: Some(_) }));
            let row = get(&j.conn, &lease.namespace).unwrap().unwrap();
            let gc = row.gc.unwrap();
            assert_eq!(gc.retry, time + delay);
            time = gc.retry;
        }
    }
    #[test]
    fn cached_claim_cannot_reauthorize_gc_after_complete_resume_or_namespace_reuse() {
        for outcome in [GcOutcome::Busy, GcOutcome::Cleaned, GcOutcome::Uncertain] {
            let (_dir, mut j) = journal();
            let lease = reserve(&mut j, 1, launch(), binding());
            close(&mut j, &lease, 2);
            let mut row = get(&j.conn, &lease.namespace).unwrap().unwrap();
            row.expires = NOW - 1;
            save(&j.conn, &lease.namespace, &row).unwrap();
            let original = request(
                3,
                Operation::Claim {
                    exclude: String::new(),
                },
            );
            let Response::Granted { claim: Some(grant) } = j
                .copy_local_request(original.clone(), [1; 32], 1, NOW)
                .unwrap()
            else {
                panic!("claim")
            };
            assert!(matches!(
                j.copy_local_request(original.clone(), [1; 32], 1, NOW)
                    .unwrap(),
                Response::Granted { claim: Some(_) }
            ));
            let cleaned = matches!(outcome, GcOutcome::Cleaned);
            let unknown = matches!(outcome, GcOutcome::Uncertain);
            j.copy_local_request(
                request(
                    4,
                    Operation::Complete {
                        claim: grant.id,
                        revision: grant.revision,
                        outcome,
                    },
                ),
                [1; 32],
                1,
                NOW,
            )
            .unwrap();
            if unknown {
                j.copy_local_request(
                    request(
                        5,
                        Operation::Forget {
                            namespace: lease.namespace.clone(),
                            revision: lease.revision,
                            acknowledge_retained_state: true,
                        },
                    ),
                    [1; 32],
                    1,
                    NOW,
                )
                .unwrap();
            }
            let mut l = launch();
            if !cleaned && !unknown {
                l.mode = AttemptMode::Resume;
            }
            let next = reserve(&mut j, 6, l, binding());
            assert!(get(&j.conn, &next.namespace).unwrap().is_some());
            assert!(matches!(
                j.copy_local_request(original, [1; 32], 1, NOW),
                Err(Error::Copy(CopyError::Conflict))
            ));
        }
    }
    #[test]
    fn abandoned_claims_exhaust_into_retained_unknown_and_release_slots() {
        let (_dir, mut j) = journal();
        let lease = reserve(&mut j, 1, launch(), binding());
        close(&mut j, &lease, 2);
        let mut row = get(&j.conn, &lease.namespace).unwrap().unwrap();
        row.expires = NOW - 1;
        save(&j.conn, &lease.namespace, &row).unwrap();
        let mut now = NOW;
        for n in 3..8 {
            let req = request(
                n,
                Operation::Claim {
                    exclude: String::new(),
                },
            );
            let Response::Granted { claim: Some(_) } =
                j.copy_local_request(req, [1; 32], 1, now).unwrap()
            else {
                panic!("expected bounded retry")
            };
            now += 2000;
        }
        let response = j
            .copy_local_request(
                request(
                    8,
                    Operation::Claim {
                        exclude: String::new(),
                    },
                ),
                [1; 32],
                1,
                now,
            )
            .unwrap();
        assert!(matches!(response, Response::Granted { claim: None }));
        let row = get(&j.conn, &lease.namespace).unwrap().unwrap();
        assert_eq!(row.state, "unknown");
        assert!(row.gc.is_none());
    }
}
