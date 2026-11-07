//! SQL-only native-copy admission and replay fences. Filesystem ownership is
//! deliberately separate: an expired index never proves a child or receipt gone.
use super::*;
use crate::stream_service::{StreamLaunch, StreamRecord};
use marmish_stream::{copy_protocol as cp, wire as sw};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CopyAttempt {
    pub launch: cp::Launch,
    pub binding: sw::Binding,
    pub group: Vec<u8>,
    pub digest: [u8; 32],
    pub revision: u64,
    pub state: String,
    pub cleanup_confirmed: bool,
    pub created: u64,
    pub updated: u64,
    pub completed: Option<u64>,
    pub expires: u64,
    pub retention_seconds: u64,
}
fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(v).map_err(|_| Error::Storage("copy_encode"))
}
fn decode<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T> {
    serde_json::from_slice(b).map_err(|_| Error::Storage("copy_decode"))
}
fn attempt_key(l: &cp::Launch) -> String {
    format!(
        "{}:{}:{}:{}",
        hex::encode(l.client),
        hex::encode(l.server),
        hex::encode(l.transfer),
        l.direction as u8
    )
}
fn immutable_equal(a: &cp::Launch, b: &cp::Launch) -> bool {
    let mut a = a.clone();
    a.mode = b.mode;
    a == *b
}
fn stream_time(b: &sw::Binding) -> u64 {
    u64::from_be_bytes(b.stream[..8].try_into().expect("fixed stream identifier"))
}
pub(super) fn copy_sync_record(conn: &Connection, record: &StreamRecord) -> Result<()> {
    let StreamLaunch::NativeCopy { launch } = &record.launch else {
        return Ok(());
    };
    if !record.server_side {
        return Ok(());
    }
    let key = attempt_key(launch);
    let bytes: Option<Vec<u8>> = conn
        .query_row("SELECT record FROM copy_attempts WHERE k=?1", [&key], |r| {
            r.get(0)
        })
        .optional()
        .map_err(db)?;
    let mut attempt: CopyAttempt =
        decode(&bytes.ok_or(Error::Storage("copy_counterpart_missing"))?)?;
    if attempt.binding != record.binding && !record.active {
        return Ok(());
    }
    if attempt.binding != record.binding
        || attempt.group != record.group
        || attempt.digest
            != record
                .open_digest
                .ok_or(Error::Storage("copy_digest_missing"))?
        || attempt.revision > record.revision
    {
        return Err(Error::Storage("copy_counterpart_conflict"));
    }
    let cleanup_uncertain = attempt.state == "cleanup_uncertain";
    attempt.revision = record.revision;
    if !cleanup_uncertain {
        attempt.state = record.state.clone();
    }
    attempt.updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Invalid("clock"))?
        .as_secs();
    // stream_finish is invoked only after owned cleanup or definitive no-spawn.
    // Explicit administrative reconciliation cannot manufacture this proof.
    let was_cleanup_confirmed = attempt.cleanup_confirmed;
    attempt.cleanup_confirmed = !cleanup_uncertain
        && !record.active
        && record.terminal.is_some()
        && !matches!(
            record.state.as_str(),
            "lost"
                | "unknown_launch"
                | "cleanup_uncertain"
                | "transport_uncertain"
                | "reconciled_possible_live"
        );
    attempt.completed = record.completed_at;
    if attempt.cleanup_confirmed && !was_cleanup_confirmed {
        attempt.expires = attempt.expires.max(
            attempt
                .updated
                .checked_add(attempt.retention_seconds)
                .ok_or(Error::Invalid("copy_retention"))?,
        );
    }
    conn.execute(
        "UPDATE copy_attempts SET record=?2,expires=?3,cleanup_confirmed=?4 WHERE k=?1",
        params![
            key,
            encode(&attempt)?,
            i64::try_from(attempt.expires).map_err(|_| Error::Invalid("copy_retention"))?,
            attempt.cleanup_confirmed
        ],
    )
    .map_err(db)?;
    Ok(())
}
fn defer_gc(conn: &Connection, key: &str, timestamp: u64) -> Result<()> {
    let last: i64 = conn
        .query_row(
            "SELECT coalesce(max(turn),0) FROM copy_gc_schedule",
            [],
            |r| r.get(0),
        )
        .map_err(db)?;
    let turn = last.checked_add(1).ok_or(Error::Storage("copy_gc_turn"))?;
    let retry_after = i64::try_from(timestamp)
        .ok()
        .and_then(|now| now.checked_add(60))
        .ok_or(Error::Invalid("copy_gc_clock"))?;
    conn.execute("INSERT INTO copy_gc_schedule(k,turn,retry_after) VALUES(?1,?2,?3) ON CONFLICT(k) DO UPDATE SET turn=excluded.turn,retry_after=excluded.retry_after",params![key,turn,retry_after]).map_err(db)?;
    Ok(())
}
impl Journal {
    /// Format upgrade and copy-only replay floor are one atomic transaction.
    /// Old argv records become Exec, preserving their exact stored Open digest.
    pub(crate) fn copy_init(&mut self, old_format: i64) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS copy_attempts(k TEXT PRIMARY KEY,principal BLOB NOT NULL,record BLOB NOT NULL,expires INTEGER NOT NULL,cleanup_confirmed INTEGER NOT NULL,stream_key TEXT NOT NULL); CREATE TABLE IF NOT EXISTS copy_open_receipts(k TEXT PRIMARY KEY,principal BLOB NOT NULL,digest BLOB NOT NULL,request BLOB NOT NULL,response BLOB NOT NULL,expires INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS copy_gc_claims(k TEXT PRIMARY KEY,revision INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS copy_refusals(k TEXT PRIMARY KEY,principal BLOB NOT NULL,digest BLOB NOT NULL,request BLOB NOT NULL,response BLOB NOT NULL,expires INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS copy_replay_state(id INTEGER PRIMARY KEY CHECK(id=1),floor INTEGER NOT NULL); INSERT OR IGNORE INTO copy_replay_state VALUES(1,0);").map_err(db)?;
        tx.execute_batch("CREATE INDEX IF NOT EXISTS copy_attempt_gc ON copy_attempts(cleanup_confirmed,expires,k); CREATE INDEX IF NOT EXISTS copy_attempt_principal ON copy_attempts(principal); CREATE INDEX IF NOT EXISTS copy_receipt_expiry ON copy_open_receipts(expires); CREATE INDEX IF NOT EXISTS copy_refusal_expiry ON copy_refusals(expires); CREATE INDEX IF NOT EXISTS copy_refusal_principal ON copy_refusals(principal);").map_err(db)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS copy_gc_schedule(k TEXT PRIMARY KEY,turn INTEGER NOT NULL,retry_after INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS copy_gc_turn ON copy_gc_schedule(turn); DELETE FROM copy_gc_schedule WHERE k NOT IN (SELECT k FROM copy_attempts);").map_err(db)?;
        if old_format < 7 {
            let rows: Vec<(String, Vec<u8>)> = {
                let mut q = tx
                    .prepare("SELECT k,record FROM live_streams")
                    .map_err(db)?;
                q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(db)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(db)?
            };
            for (key, bytes) in rows {
                let mut value: serde_json::Value = decode(&bytes)?;
                let map = value
                    .as_object_mut()
                    .ok_or(Error::Storage("stream_migration"))?;
                if let Some(argv) = map.remove("argv") {
                    if map.contains_key("launch") {
                        return Err(Error::Storage("stream_migration"));
                    }
                    map.insert(
                        "launch".into(),
                        serde_json::json!({"kind":"exec","argv":argv}),
                    );
                }
                // Validate the complete migrated representation before writing.
                let record: StreamRecord = serde_json::from_value(value)
                    .map_err(|_| Error::Storage("stream_migration"))?;
                if matches!(record.launch, StreamLaunch::NativeCopy { .. }) {
                    return Err(Error::Storage("copy_migration"));
                }
                tx.execute(
                    "UPDATE live_streams SET record=?2 WHERE k=?1",
                    params![key, encode(&record)?],
                )
                .map_err(db)?;
            }
        }
        let claims: Vec<String> = {
            let mut q = tx
                .prepare("SELECT k FROM copy_gc_claims LIMIT 16385")
                .map_err(db)?;
            q.query_map([], |r| r.get(0))
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?
        };
        for key in claims {
            let bytes: Vec<u8> = tx
                .query_row("SELECT record FROM copy_attempts WHERE k=?1", [&key], |r| {
                    r.get(0)
                })
                .map_err(db)?;
            let mut attempt: CopyAttempt = decode(&bytes)?;
            attempt.state = "cleanup_uncertain".into();
            attempt.cleanup_confirmed = false;
            tx.execute(
                "UPDATE copy_attempts SET record=?2,cleanup_confirmed=0 WHERE k=?1",
                params![key, encode(&attempt)?],
            )
            .map_err(db)?;
        }
        tx.pragma_update(None, "user_version", JOURNAL_FORMAT)
            .map_err(db)?;
        tx.commit().map_err(db)
    }
    pub(crate) fn copy_attempt_matches(&self, record: &StreamRecord) -> Result<bool> {
        let StreamLaunch::NativeCopy { launch } = &record.launch else {
            return Ok(true);
        };
        let bytes: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT record FROM copy_attempts WHERE k=?1",
                [attempt_key(launch)],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        let Some(bytes) = bytes else { return Ok(false) };
        let a: CopyAttempt = decode(&bytes)?;
        Ok(a.binding == record.binding
            && a.group == record.group
            && Some(a.digest) == record.open_digest
            && a.revision == record.revision)
    }
    /// This returns an already durable response. It never performs file I/O or
    /// starts execution; the stream/index revision is the later preparation fence.
    pub(crate) fn admit_copy_open(
        &mut self,
        record: &StreamRecord,
        request: &sw::Request,
        request_bytes: &[u8],
        retention: u64,
        timestamp: u64,
    ) -> Result<Vec<u8>> {
        let sw::RequestBody::OpenCopy { launch, codec } = &request.body else {
            return Err(Error::Invalid("copy_open"));
        };
        let launch_digest = launch
            .digest(*codec)
            .map_err(|_| Error::Invalid("copy_launch"))?;
        let k = record.key();
        let digest = sw::request_digest(request_bytes);
        let tx = self.conn.transaction().map_err(db)?;
        // Known slots/refusals always precede the unknown-Open floor.
        let opened: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = tx
            .query_row(
                "SELECT digest,request,response FROM copy_open_receipts WHERE k=?1",
                [&k],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db)?;
        if let Some((d, q, r)) = opened {
            if d == digest && q == request_bytes {
                return Ok(r);
            }
            return sw::encode_response(&sw::Response {
                binding: request.binding.clone(),
                sequence: request.sequence,
                request_sha256: digest,
                body: sw::ResponseBody::CopyRefused {
                    code: sw::CopyAdmissionError::Conflict,
                    prior_binding: None,
                },
            })
            .map_err(|_| Error::Invalid("copy_response"));
        }
        let known: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = tx
            .query_row(
                "SELECT digest,request,response FROM copy_refusals WHERE k=?1",
                [&k],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db)?;
        if let Some((d, q, r)) = known {
            if d == digest && q == request_bytes {
                return Ok(r);
            }
            return sw::encode_response(&sw::Response {
                binding: request.binding.clone(),
                sequence: request.sequence,
                request_sha256: digest,
                body: sw::ResponseBody::CopyRefused {
                    code: sw::CopyAdmissionError::Conflict,
                    prior_binding: None,
                },
            })
            .map_err(|_| Error::Invalid("copy_response"));
        }
        let existing: Option<Vec<u8>> = tx
            .query_row("SELECT record FROM live_streams WHERE k=?1", [&k], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db)?;
        let existing: Option<StreamRecord> = existing.map(|b| decode(&b)).transpose()?;
        if let Some(old) = &existing
            && let Some(old_digest) = old.open_digest
            && old_digest == launch_digest
            && old.binding == request.binding
        {
            let response: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT response FROM stream_slots WHERE k=?1 AND digest=?2",
                    params![&k, digest.as_slice()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db)?
                .flatten();
            if let Some(response) = response {
                return Ok(response);
            }
            return Err(Error::Storage("copy_open_receipt_missing"));
        }
        let floor: i64 = tx
            .query_row("SELECT floor FROM copy_replay_state WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(db)?;
        if stream_time(&request.binding)
            <= u64::try_from(floor).map_err(|_| Error::Storage("copy_floor"))?
        {
            return sw::encode_response(&sw::Response {
                binding: request.binding.clone(),
                sequence: request.sequence,
                request_sha256: digest,
                body: sw::ResponseBody::Error {
                    code: sw::WireErrorCode::Expired,
                },
            })
            .map_err(|_| Error::Invalid("copy_response"));
        }
        // Age/skew must be checked before a refusal can advance the global floor.
        request
            .validate_at(timestamp)
            .map_err(|_| Error::Invalid("stream_expired"))?;
        let mut refusal = None;
        if existing
            .as_ref()
            .is_some_and(|r| r.binding != request.binding || r.open_digest.is_some())
        {
            refusal = Some((sw::CopyAdmissionError::Conflict, None));
        } else if existing
            .as_ref()
            .is_some_and(|r| r.cancel || r.terminal.is_some())
        {
            // Cancel is an irreversible fence even when it arrived before
            // capability negotiation/Open; a later Hello cannot revive the ID.
            refusal = Some((sw::CopyAdmissionError::Denied, None));
        } else if !existing.as_ref().is_some_and(|r| r.copy_hello) {
            refusal = Some((sw::CopyAdmissionError::Unsupported, None));
        }
        let index_key = attempt_key(launch);
        let old: Option<Vec<u8>> = tx
            .query_row(
                "SELECT record FROM copy_attempts WHERE k=?1",
                [&index_key],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        let old: Option<CopyAttempt> = old.map(|b| decode(&b)).transpose()?;
        let gc_pending: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM copy_gc_claims WHERE k=?1)",
                [&index_key],
                |r| r.get(0),
            )
            .map_err(db)?;
        if refusal.is_none() && gc_pending {
            refusal = Some((
                if old.as_ref().is_some_and(|a| a.state == "cleanup_uncertain") {
                    sw::CopyAdmissionError::RecoveryRequired
                } else {
                    sw::CopyAdmissionError::Busy
                },
                old.as_ref().map(|a| a.binding.clone()),
            ));
        }
        if refusal.is_none()
            && let Some(prior) = &old
        {
            if !immutable_equal(&prior.launch, launch) {
                refusal = Some((sw::CopyAdmissionError::Conflict, None));
            } else {
                let prior_key = crate::stream_service::key(&prior.group, &prior.binding.stream);
                let counterpart: Option<Vec<u8>> = tx
                    .query_row(
                        "SELECT record FROM live_streams WHERE k=?1",
                        [prior_key],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(db)?;
                let counterpart: Option<StreamRecord> =
                    counterpart.map(|b| decode(&b)).transpose()?;
                if counterpart.as_ref().is_none_or(|r| {
                    r.binding != prior.binding
                        || r.open_digest != Some(prior.digest)
                        || r.revision != prior.revision
                }) || matches!(
                    prior.state.as_str(),
                    "lost"
                        | "unknown_launch"
                        | "cleanup_uncertain"
                        | "transport_uncertain"
                        | "reconciled_possible_live"
                ) {
                    refusal = Some((
                        sw::CopyAdmissionError::RecoveryRequired,
                        Some(prior.binding.clone()),
                    ));
                } else if !prior.cleanup_confirmed {
                    refusal = Some((sw::CopyAdmissionError::Busy, Some(prior.binding.clone())));
                } else if launch.mode == cp::AttemptMode::Create {
                    refusal = Some((sw::CopyAdmissionError::Conflict, None));
                }
            }
        } else if refusal.is_none() && launch.mode == cp::AttemptMode::Resume {
            refusal = Some((sw::CopyAdmissionError::RecoveryRequired, None));
        }
        if refusal.is_none() {
            match super::command_store::stream_check_admission(
                &tx,
                &request.binding.client,
                true,
                existing.is_none(),
            ) {
                Ok(()) => {}
                Err(Error::Invalid(
                    "stream_retained_capacity"
                    | "stream_active_capacity"
                    | "command_admission_closed",
                )) => {
                    refusal = Some((sw::CopyAdmissionError::Capacity, None));
                }
                Err(error) => return Err(error),
            }
        }
        // Preserve ordinary stream control headroom even though the immutable
        // copy admission receipt also has its own bounded durable table.
        if refusal.is_none() {
            let global: i64 = tx
                .query_row("SELECT count(*) FROM stream_slots", [], |r| r.get(0))
                .map_err(db)?;
            let peer: i64 = tx.query_row("SELECT count(*) FROM stream_slots WHERE k IN (SELECT k FROM live_streams WHERE principal=?1)", [request.binding.client.as_slice()], |r| r.get(0)).map_err(db)?;
            let own: i64 = tx
                .query_row("SELECT count(*) FROM stream_slots WHERE k=?1", [&k], |r| {
                    r.get(0)
                })
                .map_err(db)?;
            if global >= 992 || peer >= 124 || own >= 4 {
                refusal = Some((sw::CopyAdmissionError::Capacity, None));
            }
        }
        let count: i64 = tx
            .query_row("SELECT count(*) FROM copy_attempts", [], |r| r.get(0))
            .map_err(db)?;
        let peer: i64 = tx
            .query_row(
                "SELECT count(*) FROM copy_attempts WHERE principal=?1",
                [request.binding.client.as_slice()],
                |r| r.get(0),
            )
            .map_err(db)?;
        if refusal.is_none() && old.is_none() && (count >= 16384 || peer >= 1024) {
            refusal = Some((sw::CopyAdmissionError::Capacity, None));
        }
        let body = if let Some((code, prior_binding)) = refusal {
            sw::ResponseBody::CopyRefused {
                code,
                prior_binding,
            }
        } else {
            sw::ResponseBody::Opened {
                codec: *codec,
                disposition: sw::OpenDisposition::Created,
                open_sha256: launch_digest,
            }
        };
        let response = sw::encode_response(&sw::Response {
            binding: request.binding.clone(),
            sequence: request.sequence,
            request_sha256: digest,
            body: body.clone(),
        })
        .map_err(|_| Error::Invalid("copy_response"))?;
        let expires = timestamp
            .checked_add(retention)
            .ok_or(Error::Invalid("copy_retention"))?;
        if matches!(body, sw::ResponseBody::CopyRefused { .. }) {
            let all: i64 = tx
                .query_row("SELECT count(*) FROM copy_refusals", [], |r| r.get(0))
                .map_err(db)?;
            let peer: i64 = tx
                .query_row(
                    "SELECT count(*) FROM copy_refusals WHERE principal=?1",
                    [request.binding.client.as_slice()],
                    |r| r.get(0),
                )
                .map_err(db)?;
            if all >= 16384 || peer >= 1024 {
                tx.execute(
                    "UPDATE copy_replay_state SET floor=max(floor,?1) WHERE id=1",
                    [i64::try_from(stream_time(&request.binding))
                        .map_err(|_| Error::Invalid("copy_timestamp"))?],
                )
                .map_err(db)?;
            } else {
                tx.execute("INSERT INTO copy_refusals(k,principal,digest,request,response,expires) VALUES(?1,?2,?3,?4,?5,?6)",params![k,request.binding.client.as_slice(),digest.as_slice(),request_bytes,&response,i64::try_from(expires).map_err(|_|Error::Invalid("copy_retention"))?]).map_err(db)?;
            }
        } else {
            let mut r = record.clone();
            r.launch = StreamLaunch::NativeCopy {
                launch: launch.clone(),
            };
            r.open_digest = Some(launch_digest);
            r.codec = *codec;
            r.state = "preparing".into();
            r.active = true;
            r.revision = r
                .revision
                .checked_add(1)
                .ok_or(Error::Invalid("copy_revision"))?;
            let a = CopyAttempt {
                launch: launch.clone(),
                binding: r.binding.clone(),
                group: r.group.clone(),
                digest: launch_digest,
                revision: r.revision,
                state: r.state.clone(),
                cleanup_confirmed: false,
                created: old.as_ref().map_or(timestamp, |a| a.created),
                updated: timestamp,
                completed: None,
                expires: old.as_ref().map_or(expires, |a| a.expires.max(expires)),
                retention_seconds: retention,
            };
            // A fresh admitted revision must not inherit an older attempt's retry delay.
            tx.execute("DELETE FROM copy_gc_schedule WHERE k=?1", [&index_key])
                .map_err(db)?;
            tx.execute("INSERT INTO copy_attempts(k,principal,record,expires,cleanup_confirmed,stream_key) VALUES(?1,?2,?3,?4,0,?5) ON CONFLICT(k) DO UPDATE SET record=excluded.record,expires=excluded.expires,cleanup_confirmed=0,stream_key=excluded.stream_key",params![index_key,r.binding.client.as_slice(),encode(&a)?,i64::try_from(a.expires).map_err(|_|Error::Invalid("copy_retention"))?,r.key()]).map_err(db)?;
            tx.execute("INSERT INTO live_streams(k,principal,server_side,active,record) VALUES(?1,?2,1,1,?3) ON CONFLICT(k) DO UPDATE SET active=1,record=excluded.record",params![k,r.binding.client.as_slice(),encode(&r)?]).map_err(db)?;
            tx.execute("INSERT INTO copy_open_receipts(k,principal,digest,request,response,expires) VALUES(?1,?2,?3,?4,?5,?6)",params![k,r.binding.client.as_slice(),digest.as_slice(),request_bytes,&response,i64::try_from(expires).map_err(|_|Error::Invalid("copy_retention"))?]).map_err(db)?;
            tx.execute("INSERT INTO stream_slots(k,digest,sequence,request,response) VALUES(?1,?2,0,?3,?4) ON CONFLICT(k,digest) DO UPDATE SET response=excluded.response",params![k,digest.as_slice(),request_bytes,&response]).map_err(db)?;
            tx.execute(
                "UPDATE command_replay_state SET admitted=admitted+1 WHERE id=1",
                [],
            )
            .map_err(db)?;
        }
        tx.commit().map_err(db)?;
        Ok(response)
    }
}

impl Journal {
    /// One durable claim at a time. A claim excludes new attempts until the
    /// matching worker completion; orphan claims from a previous daemon are
    /// conservative uncertainty, not permission to repeat deletion.
    pub(crate) fn copy_gc_candidate(
        &mut self,
        timestamp: u64,
    ) -> Result<Option<(String, CopyAttempt)>> {
        let tx = self.conn.transaction().map_err(db)?;
        let rows: Vec<(String, Vec<u8>)> = {
            let mut q=tx.prepare("SELECT a.k,a.record FROM copy_attempts a LEFT JOIN copy_gc_schedule s ON s.k=a.k WHERE a.cleanup_confirmed=1 AND a.expires<?1 AND a.k NOT IN (SELECT k FROM copy_gc_claims) AND coalesce(s.retry_after,0)<=?1 ORDER BY coalesce(s.turn,0),a.expires,a.k LIMIT 32").map_err(db)?;
            q.query_map(
                [i64::try_from(timestamp).map_err(|_| Error::Invalid("clock"))?],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?
            .collect::<rusqlite::Result<_>>()
            .map_err(db)?
        };
        for (key, bytes) in rows {
            let attempt: CopyAttempt = decode(&bytes)?;
            if !attempt.cleanup_confirmed || timestamp <= attempt.expires {
                continue;
            }
            let stream_key = crate::stream_service::key(&attempt.group, &attempt.binding.stream);
            let record: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT record FROM live_streams WHERE k=?1",
                    [&stream_key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db)?;
            let Some(record) = record else {
                defer_gc(&tx, &key, timestamp)?;
                continue;
            };
            let record: StreamRecord = decode(&record)?;
            if record.active
                || record.revision != attempt.revision
                || record.open_digest != Some(attempt.digest)
            {
                defer_gc(&tx, &key, timestamp)?;
                continue;
            }
            let publications:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM stream_publications WHERE k=?1 AND state NOT IN ('acked','retired_failed'))",[&stream_key],|r|r.get(0)).map_err(db)?;
            if publications {
                defer_gc(&tx, &key, timestamp)?;
                continue;
            }
            defer_gc(&tx, &key, timestamp)?;
            tx.execute(
                "INSERT INTO copy_gc_claims(k,revision) VALUES(?1,?2)",
                params![
                    &key,
                    i64::try_from(attempt.revision).map_err(|_| Error::Invalid("copy_revision"))?
                ],
            )
            .map_err(db)?;
            tx.commit().map_err(db)?;
            return Ok(Some((key, attempt)));
        }
        // Old attempt streams no longer own the filesystem namespace. Their
        // immutable admission receipts remain until their own replay fence, then
        // can be removed without touching the current receiver's files.
        let superseded: Vec<(String, Vec<u8>)> = {
            let mut q=tx.prepare("SELECT k,request FROM copy_open_receipts WHERE expires<?1 AND k NOT IN (SELECT stream_key FROM copy_attempts) LIMIT 32").map_err(db)?;
            q.query_map(
                [i64::try_from(timestamp).map_err(|_| Error::Invalid("clock"))?],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?
            .collect::<rusqlite::Result<_>>()
            .map_err(db)?
        };
        for (key, bytes) in superseded {
            let request =
                sw::decode_request(&bytes).map_err(|_| Error::Storage("copy_open_receipt"))?;
            let active: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM live_streams WHERE k=?1 AND active=1)",
                    [&key],
                    |r| r.get(0),
                )
                .map_err(db)?;
            let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM stream_publications WHERE k=?1 AND state NOT IN ('acked','retired_failed'))",[&key],|r|r.get(0)).map_err(db)?;
            if active || pending {
                continue;
            }
            tx.execute(
                "UPDATE copy_replay_state SET floor=max(floor,?1) WHERE id=1",
                [i64::try_from(stream_time(&request.binding))
                    .map_err(|_| Error::Invalid("clock"))?],
            )
            .map_err(db)?;
            for table in [
                "copy_open_receipts",
                "stream_slots",
                "stream_attempts",
                "stream_publications",
                "live_streams",
            ] {
                tx.execute(&format!("DELETE FROM {table} WHERE k=?1"), [&key])
                    .map_err(db)?;
            }
        }
        // Expired refusal receipts may be removed only after advancing the
        // copy-only unknown-open floor in this same transaction.
        let expired: Vec<(String, Vec<u8>)> = {
            let mut q = tx
                .prepare("SELECT k,request FROM copy_refusals WHERE expires<?1 LIMIT 32")
                .map_err(db)?;
            q.query_map(
                [i64::try_from(timestamp).map_err(|_| Error::Invalid("clock"))?],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?
            .collect::<rusqlite::Result<_>>()
            .map_err(db)?
        };
        for (key, bytes) in expired {
            let request = sw::decode_request(&bytes).map_err(|_| Error::Storage("copy_refusal"))?;
            tx.execute(
                "UPDATE copy_replay_state SET floor=max(floor,?1) WHERE id=1",
                [i64::try_from(stream_time(&request.binding))
                    .map_err(|_| Error::Invalid("clock"))?],
            )
            .map_err(db)?;
            tx.execute("DELETE FROM copy_refusals WHERE k=?1", [key])
                .map_err(db)?;
        }
        tx.commit().map_err(db)?;
        Ok(None)
    }
    pub(crate) fn copy_gc_complete(
        &mut self,
        key: &str,
        revision: u64,
        cleaned: bool,
    ) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        let claim: Option<i64> = tx
            .query_row(
                "SELECT revision FROM copy_gc_claims WHERE k=?1",
                [key],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if claim != Some(i64::try_from(revision).map_err(|_| Error::Invalid("copy_revision"))?) {
            return Err(Error::Storage("copy_gc_claim"));
        }
        if cleaned {
            let bytes: Vec<u8> = tx
                .query_row("SELECT record FROM copy_attempts WHERE k=?1", [key], |r| {
                    r.get(0)
                })
                .map_err(db)?;
            let attempt: CopyAttempt = decode(&bytes)?;
            if !attempt.cleanup_confirmed || attempt.revision != revision {
                return Err(Error::Storage("copy_gc_revision"));
            }
            let stream_key = crate::stream_service::key(&attempt.group, &attempt.binding.stream);
            tx.execute(
                "UPDATE copy_replay_state SET floor=max(floor,?1) WHERE id=1",
                [i64::try_from(stream_time(&attempt.binding))
                    .map_err(|_| Error::Invalid("clock"))?],
            )
            .map_err(db)?;
            tx.execute("DELETE FROM copy_open_receipts WHERE k=?1", [&stream_key])
                .map_err(db)?;
            tx.execute("DELETE FROM stream_slots WHERE k=?1", [&stream_key])
                .map_err(db)?;
            tx.execute("DELETE FROM stream_attempts WHERE k=?1", [&stream_key])
                .map_err(db)?;
            tx.execute("DELETE FROM stream_publications WHERE k=?1", [&stream_key])
                .map_err(db)?;
            tx.execute(
                "DELETE FROM live_streams WHERE k=?1 AND active=0",
                [&stream_key],
            )
            .map_err(db)?;
            tx.execute("DELETE FROM copy_gc_schedule WHERE k=?1", [key])
                .map_err(db)?;
            tx.execute("DELETE FROM copy_attempts WHERE k=?1", [key])
                .map_err(db)?;
        }
        tx.execute("DELETE FROM copy_gc_claims WHERE k=?1", [key])
            .map_err(db)?;
        tx.commit().map_err(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marmish_stream::codec::Codec;
    fn setup() -> (tempfile::TempDir, Journal) {
        let d = super::super::private_test_directory();
        let journal = Journal::open(
            &d.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        (d, journal)
    }
    fn open(id: u8, transfer: u8, mode: cp::AttemptMode) -> (StreamRecord, sw::Request, Vec<u8>) {
        let mut stream = [id; 16];
        stream[..8].copy_from_slice(&1_000_000u64.to_be_bytes());
        let binding = sw::Binding {
            stream,
            session: [9; 16],
            client: [1; 32],
            server: [2; 32],
        };
        let launch = cp::Launch {
            version: 1,
            mode,
            transfer: [transfer; 32],
            client: binding.client,
            server: binding.server,
            direction: cp::Direction::Put,
            source: b"literal' $*\xff".to_vec(),
            destination: b"out".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
        };
        let request = sw::Request {
            binding: binding.clone(),
            sequence: 0,
            body: sw::RequestBody::OpenCopy {
                launch,
                codec: Codec::Zlib,
            },
        };
        let bytes = sw::encode_request(&request).unwrap();
        let record = StreamRecord {
            binding,
            group: vec![5],
            server_side: true,
            hello: false,
            copy_hello: true,
            open_digest: None,
            launch: StreamLaunch::default(),
            codec: Codec::None,
            state: "hello".into(),
            cancel: false,
            active: false,
            terminal: None,
            identity: None,
            child_disposition: None,
            revision: 0,
            sequence: 0,
            stdin_accepted: 0,
            stdin_delivered: 0,
            stdout_eof: None,
            stderr_eof: None,
            completed_at: None,
            last_exchange_digest: None,
            payload_drained: false,
            payload_expired: false,
            projection_invalidated: false,
        };
        (record, request, bytes)
    }
    fn admit(j: &mut Journal, r: &StreamRecord, q: &sw::Request, b: &[u8]) -> sw::ResponseBody {
        sw::decode_response(&j.admit_copy_open(r, q, b, 604860, 1_000_000).unwrap())
            .unwrap()
            .body
    }
    #[test]
    fn admission_is_atomic_idempotent_and_busy_receipt_is_immutable() {
        let (_d, mut j) = setup();
        let (r, q, b) = open(1, 3, cp::AttemptMode::Create);
        j.stream_insert(&r).unwrap();
        assert!(matches!(
            admit(&mut j, &r, &q, &b),
            sw::ResponseBody::Opened {
                disposition: sw::OpenDisposition::Created,
                ..
            }
        ));
        let saved = j.stream_get(&r.key()).unwrap().unwrap();
        assert!(saved.active);
        assert!(j.copy_attempt_matches(&saved).unwrap());
        assert!(matches!(
            admit(&mut j, &r, &q, &b),
            sw::ResponseBody::Opened {
                disposition: sw::OpenDisposition::Created,
                ..
            }
        ));
        let (r2, q2, b2) = open(2, 3, cp::AttemptMode::Resume);
        j.stream_insert(&r2).unwrap();
        assert!(matches!(
            admit(&mut j, &r2, &q2, &b2),
            sw::ResponseBody::CopyRefused {
                code: sw::CopyAdmissionError::Busy,
                prior_binding: Some(_)
            }
        ));
        let mut done = saved;
        done.terminal = Some(sw::Terminal::Exited { code: 0 });
        done.state = "terminal".into();
        j.stream_finish(&mut done).unwrap();
        // Same Busy request stays Busy; a fresh stream observes completion.
        assert!(matches!(
            admit(&mut j, &r2, &q2, &b2),
            sw::ResponseBody::CopyRefused {
                code: sw::CopyAdmissionError::Busy,
                ..
            }
        ));
        let (r3, q3, b3) = open(3, 3, cp::AttemptMode::Resume);
        j.stream_insert(&r3).unwrap();
        assert!(matches!(
            admit(&mut j, &r3, &q3, &b3),
            sw::ResponseBody::Opened { .. }
        ));
    }
    #[test]
    fn uncertain_and_reconciled_attempts_never_authorize_resume() {
        for state in ["lost", "unknown_launch", "reconciled_possible_live"] {
            let (_d, mut j) = setup();
            let (r, q, b) = open(1, 3, cp::AttemptMode::Create);
            j.stream_insert(&r).unwrap();
            admit(&mut j, &r, &q, &b);
            let mut saved = j.stream_get(&r.key()).unwrap().unwrap();
            saved.state = state.into();
            saved.terminal = Some(sw::Terminal::Failed {
                code: sw::WireErrorCode::Uncertain,
            });
            if state == "reconciled_possible_live" {
                j.stream_finish(&mut saved).unwrap()
            } else {
                j.stream_save(&saved).unwrap()
            }
            let (r2, q2, b2) = open(2, 3, cp::AttemptMode::Resume);
            j.stream_insert(&r2).unwrap();
            assert!(matches!(
                admit(&mut j, &r2, &q2, &b2),
                sw::ResponseBody::CopyRefused {
                    code: sw::CopyAdmissionError::RecoveryRequired,
                    ..
                }
            ));
        }
    }
    #[test]
    fn unknown_open_floor_does_not_affect_known_receipt_or_exec() {
        let (_d, mut j) = setup();
        let (r, q, b) = open(1, 3, cp::AttemptMode::Create);
        j.stream_insert(&r).unwrap();
        admit(&mut j, &r, &q, &b);
        j.conn
            .execute("UPDATE copy_replay_state SET floor=1000000", [])
            .unwrap();
        assert!(matches!(
            admit(&mut j, &r, &q, &b),
            sw::ResponseBody::Opened { .. }
        ));
        let (r2, q2, b2) = open(2, 4, cp::AttemptMode::Create);
        j.stream_insert(&r2).unwrap();
        assert!(matches!(
            admit(&mut j, &r2, &q2, &b2),
            sw::ResponseBody::Error {
                code: sw::WireErrorCode::Expired
            }
        ));
        let mut exec = r2;
        exec.active = true;
        j.stream_admit(&mut exec).unwrap();
        assert!(j.stream_get(&exec.key()).unwrap().unwrap().active);
    }
    #[test]
    fn transaction_failure_never_leaves_launch_or_index_fence() {
        let (_d, mut j) = setup();
        let (r, q, b) = open(1, 3, cp::AttemptMode::Create);
        j.stream_insert(&r).unwrap();
        j.conn.execute_batch("CREATE TRIGGER reject_copy_slot BEFORE INSERT ON stream_slots BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(j.admit_copy_open(&r, &q, &b, 604860, 1_000_000).is_err());
        assert!(!j.stream_get(&r.key()).unwrap().unwrap().active);
        assert_eq!(
            j.conn
                .query_row("SELECT count(*) FROM copy_attempts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            j.conn
                .query_row(
                    "SELECT admitted FROM command_replay_state WHERE id=1",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    #[test]
    fn garbage_collection_requires_confirmed_cleanup_and_fences_deleted_receipt() {
        let (_d, mut j) = setup();
        let (r, q, b) = open(1, 3, cp::AttemptMode::Create);
        j.stream_insert(&r).unwrap();
        admit(&mut j, &r, &q, &b);
        assert!(j.copy_gc_candidate(u64::MAX / 4).unwrap().is_none());
        let mut done = j.stream_get(&r.key()).unwrap().unwrap();
        done.terminal = Some(sw::Terminal::Exited { code: 0 });
        done.state = "terminal".into();
        j.stream_finish(&mut done).unwrap();
        let (key, candidate) = j.copy_gc_candidate(u64::MAX / 4).unwrap().unwrap();
        let (r2, q2, b2) = open(2, 3, cp::AttemptMode::Resume);
        j.stream_insert(&r2).unwrap();
        assert!(matches!(
            admit(&mut j, &r2, &q2, &b2),
            sw::ResponseBody::CopyRefused {
                code: sw::CopyAdmissionError::Busy,
                ..
            }
        ));
        // A failed physical reclaim never releases the index or receipt.
        j.copy_gc_complete(&key, candidate.revision, false).unwrap();
        assert!(matches!(
            admit(&mut j, &r, &q, &b),
            sw::ResponseBody::Opened { .. }
        ));
        assert!(j.copy_gc_candidate(u64::MAX / 4).unwrap().is_none());
        let (key, candidate) = j.copy_gc_candidate(u64::MAX / 4 + 61).unwrap().unwrap();
        j.copy_gc_complete(&key, candidate.revision, true).unwrap();
        assert_eq!(
            j.conn
                .query_row("SELECT count(*) FROM copy_gc_schedule", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(j.stream_get(&r.key()).unwrap().is_none());
        assert!(matches!(
            admit(&mut j, &r, &q, &b),
            sw::ResponseBody::Error {
                code: sw::WireErrorCode::Expired
            }
        ));
    }
    #[test]
    fn interrupted_gc_claim_cannot_be_cleared_by_status_projection() {
        let (_d, mut j) = setup();
        let (r, q, b) = open(1, 3, cp::AttemptMode::Create);
        j.stream_insert(&r).unwrap();
        admit(&mut j, &r, &q, &b);
        let mut done = j.stream_get(&r.key()).unwrap().unwrap();
        done.terminal = Some(sw::Terminal::Exited { code: 0 });
        done.state = "terminal".into();
        j.stream_finish(&mut done).unwrap();
        j.copy_gc_candidate(u64::MAX / 4).unwrap().unwrap();
        j.copy_init(7).unwrap();
        j.stream_save(&done).unwrap();
        assert!(j.copy_gc_candidate(u64::MAX / 4).unwrap().is_none());
        let (r2, q2, b2) = open(2, 3, cp::AttemptMode::Resume);
        j.stream_insert(&r2).unwrap();
        assert!(matches!(
            admit(&mut j, &r2, &q2, &b2),
            sw::ResponseBody::CopyRefused {
                code: sw::CopyAdmissionError::RecoveryRequired,
                ..
            }
        ));
    }
    #[test]
    fn copy_admission_preserves_reserved_stream_control_slots() {
        let (_d, mut j) = setup();
        let (r, q, b) = open(1, 3, cp::AttemptMode::Create);
        j.stream_insert(&r).unwrap();
        for sequence in 0u64..4 {
            j.conn
                .execute(
                    "INSERT INTO stream_slots(k,digest,sequence,request) VALUES(?1,?2,?3,X'')",
                    params![r.key(), sequence.to_be_bytes().as_slice(), sequence as i64],
                )
                .unwrap();
        }
        assert!(matches!(
            admit(&mut j, &r, &q, &b),
            sw::ResponseBody::CopyRefused {
                code: sw::CopyAdmissionError::Capacity,
                ..
            }
        ));
        assert!(!j.stream_get(&r.key()).unwrap().unwrap().active);
    }
    #[test]
    fn cancellation_before_copy_open_is_an_immutable_admission_fence() {
        let (_d, mut j) = setup();
        let (mut r, q, b) = open(1, 3, cp::AttemptMode::Create);
        r.cancel = true;
        r.state = "cancelled".into();
        r.terminal = Some(sw::Terminal::Cancelled);
        j.stream_insert(&r).unwrap();
        assert!(matches!(
            admit(&mut j, &r, &q, &b),
            sw::ResponseBody::CopyRefused {
                code: sw::CopyAdmissionError::Denied,
                ..
            }
        ));
        assert!(!j.stream_get(&r.key()).unwrap().unwrap().active);
        // Even an inconsistent later capability projection cannot turn this
        // exact durable refusal into an executable admission.
        r.cancel = false;
        r.terminal = None;
        j.stream_save(&r).unwrap();
        assert!(matches!(
            admit(&mut j, &r, &q, &b),
            sw::ResponseBody::CopyRefused {
                code: sw::CopyAdmissionError::Denied,
                ..
            }
        ));
        let count: i64 = j
            .conn
            .query_row("SELECT count(*) FROM copy_attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
    fn completed(j: &mut Journal, id: u8) -> StreamRecord {
        let (r, q, b) = open(id, id, cp::AttemptMode::Create);
        j.stream_insert(&r).unwrap();
        assert!(matches!(
            admit(j, &r, &q, &b),
            sw::ResponseBody::Opened { .. }
        ));
        let mut done = j.stream_get(&r.key()).unwrap().unwrap();
        done.terminal = Some(sw::Terminal::Exited { code: 0 });
        done.state = "terminal".into();
        j.stream_finish(&mut done).unwrap();
        done
    }
    #[test]
    fn failed_gc_rotates_behind_other_eligible_attempts() {
        let (_d, mut j) = setup();
        let first = completed(&mut j, 1);
        let second = completed(&mut j, 2);
        let now = u64::MAX / 4;
        let (k, a) = j.copy_gc_candidate(now).unwrap().unwrap();
        assert!(a.binding == first.binding);
        j.copy_gc_complete(&k, a.revision, false).unwrap();
        let (_, next) = j.copy_gc_candidate(now + 61).unwrap().unwrap();
        assert!(next.binding == second.binding);
    }
    #[test]
    fn bounded_scan_rotates_more_than_thirty_two_blocked_attempts() {
        let (_d, mut j) = setup();
        for id in 1..=33 {
            let r = completed(&mut j, id);
            j.conn
                .execute("DELETE FROM live_streams WHERE k=?1", [r.key()])
                .unwrap();
        }
        let good = completed(&mut j, 34);
        let now = u64::MAX / 4;
        assert!(j.copy_gc_candidate(now).unwrap().is_none());
        let (_, candidate) = j.copy_gc_candidate(now).unwrap().unwrap();
        assert!(candidate.binding == good.binding);
    }
    #[test]
    fn gc_schedule_checks_overflow_and_cleans_orphans_on_startup() {
        let (_d, mut j) = setup();
        j.conn
            .execute(
                "INSERT INTO copy_gc_schedule(k,turn,retry_after) VALUES('orphan',?1,0)",
                [i64::MAX],
            )
            .unwrap();
        assert!(matches!(
            defer_gc(&j.conn, "another", 1),
            Err(Error::Storage("copy_gc_turn"))
        ));
        j.copy_init(7).unwrap();
        assert!(matches!(
            defer_gc(&j.conn, "another", i64::MAX as u64),
            Err(Error::Invalid("copy_gc_clock"))
        ));
        assert_eq!(
            j.conn
                .query_row("SELECT count(*) FROM copy_gc_schedule", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn format_six_exec_migration_preserves_open_digest() {
        let (d, mut j) = setup();
        let (mut r, _, _) = open(1, 3, cp::AttemptMode::Create);
        r.copy_hello = false;
        r.open_digest = Some([8; 32]);
        r.launch = StreamLaunch::Exec {
            argv: vec![b"literal$'".to_vec()],
        };
        j.stream_insert(&r).unwrap();
        let mut v = serde_json::to_value(&r).unwrap();
        let m = v.as_object_mut().unwrap();
        let launch = m.remove("launch").unwrap();
        m.insert("argv".into(), launch["argv"].clone());
        j.conn
            .execute(
                "UPDATE live_streams SET record=?1",
                [serde_json::to_vec(&v).unwrap()],
            )
            .unwrap();
        j.conn.pragma_update(None, "user_version", 6).unwrap();
        drop(j);
        let j = Journal::open(
            &d.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        let migrated = j.stream_get(&r.key()).unwrap().unwrap();
        assert_eq!(migrated.open_digest, Some([8; 32]));
        assert!(
            matches!(migrated.launch,StreamLaunch::Exec{argv}if argv==vec![b"literal$'".to_vec()])
        );
        assert_eq!(
            j.conn
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
    }
}
