//! SQLCipher launch fences, reply slots and native publication ownership.
use super::*;
use crate::command_evidence::PublishedEvidence;
use crate::stream_service::StreamRecord;

#[derive(Clone)]
pub(crate) struct StreamPublication {
    pub group: Vec<u8>,
    pub event_id: String,
    pub stream_key: String,
    pub payload: Vec<u8>,
    pub state: String,
    pub native_id: Option<Vec<u8>>,
    pub evidence: Option<Vec<u8>>,
    pub intent: Option<Vec<u8>>,
}
fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_secs())
        .map_err(|_| Error::Invalid("clock"))
}
type StreamSlot = (Vec<u8>, Option<Vec<u8>>);

fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| Error::Storage("stream_encode"))
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| Error::Storage("stream_decode"))
}
impl Journal {
    pub(crate) fn stream_init(&mut self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS stream_attempts(k TEXT NOT NULL,digest BLOB NOT NULL,last_ms INTEGER NOT NULL,attempt INTEGER NOT NULL,created INTEGER NOT NULL,PRIMARY KEY(k,digest)); CREATE TABLE IF NOT EXISTS live_streams(k TEXT PRIMARY KEY,principal BLOB NOT NULL,server_side INTEGER NOT NULL,active INTEGER NOT NULL,record BLOB NOT NULL); CREATE TABLE IF NOT EXISTS stream_slots(k TEXT NOT NULL,digest BLOB NOT NULL,sequence INTEGER NOT NULL,request BLOB NOT NULL,response BLOB,PRIMARY KEY(k,digest)); CREATE TABLE IF NOT EXISTS stream_publications(group_id BLOB NOT NULL,event_id TEXT NOT NULL,k TEXT NOT NULL,payload BLOB NOT NULL,state TEXT NOT NULL,native_id BLOB,evidence BLOB,intent BLOB,PRIMARY KEY(group_id,event_id));").map_err(db)?;
        for mut record in self.stream_records()? {
            if record.active
                || record.state == "preparing"
                || record.state == "launching"
                || (!record.server_side
                    && record.open_digest.is_some()
                    && record.terminal.is_none())
            {
                if record.server_side {
                    use crate::stream_service::ChildDisposition;
                    use marmish_engine::{LiveProcessRecord, RecordedProcessDisposition};
                    record.child_disposition = Some(
                        record
                            .identity
                            .as_deref()
                            .and_then(|bytes| {
                                serde_json::from_slice::<LiveProcessRecord>(bytes).ok()
                            })
                            .map(|identity| {
                                match marmish_engine::probe_recorded_process(&identity) {
                                    RecordedProcessDisposition::Absent => ChildDisposition::Absent,
                                    RecordedProcessDisposition::PresentUnowned => {
                                        ChildDisposition::PresentUnowned
                                    }
                                    RecordedProcessDisposition::Unknown => {
                                        ChildDisposition::Unknown
                                    }
                                }
                            })
                            .unwrap_or(ChildDisposition::Unknown),
                    );
                }
                record.state = if matches!(record.state.as_str(), "launching" | "unknown_launch")
                    && record.identity.is_none()
                {
                    "unknown_launch"
                } else {
                    "lost"
                }
                .into();
                record.terminal = Some(marmish_stream::wire::Terminal::Failed {
                    code: marmish_stream::wire::WireErrorCode::Uncertain,
                });
                self.stream_save(&record)?;
            }
        }
        Ok(())
    }
    pub(crate) fn stream_records(&self) -> Result<Vec<StreamRecord>> {
        let mut q = self
            .conn
            .prepare("SELECT record FROM live_streams ORDER BY k LIMIT 10001")
            .map_err(db)?;
        q.query_map([], |r| r.get::<_, Vec<u8>>(0))
            .map_err(db)?
            .map(|r| decode(&r.map_err(db)?))
            .collect()
    }
    /// Shared finite/live admission caps active server rows at 32. Include
    /// Lost/UnknownLaunch holds; callers must apply their state-specific gate.
    pub(crate) fn stream_active_server_records(&self) -> Result<Vec<StreamRecord>> {
        let mut query = self.conn.prepare("SELECT record FROM live_streams WHERE server_side=1 AND active=1 ORDER BY k LIMIT 33").map_err(db)?;
        let records: Vec<StreamRecord> = query
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(db)?
            .map(|row| decode(&row.map_err(db)?))
            .collect::<Result<_>>()?;
        if records.len() > 32 {
            return Err(Error::Invalid("stream_active_capacity"));
        }
        Ok(records)
    }
    pub(crate) fn stream_get(&self, key: &str) -> Result<Option<StreamRecord>> {
        self.conn
            .query_row("SELECT record FROM live_streams WHERE k=?1", [key], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .optional()
            .map_err(db)?
            .map(|r| decode(&r))
            .transpose()
    }
    pub(crate) fn stream_save(&mut self, r: &StreamRecord) -> Result<()> {
        self.conn
            .execute(
                "UPDATE live_streams SET active=?2,record=?3 WHERE k=?1",
                params![r.key(), r.active, encode(r)?],
            )
            .map_err(db)?;
        Ok(())
    }
    pub(crate) fn stream_insert(&mut self, r: &StreamRecord) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        super::command_store::stream_check_admission(
            &tx,
            &r.binding.client,
            r.server_side && r.active,
            true,
        )?;
        tx.execute("INSERT INTO live_streams(k,principal,server_side,active,record) VALUES(?1,?2,?3,?4,?5)",params![r.key(),r.binding.client.as_slice(),r.server_side,r.active,encode(r)?]).map_err(db)?;
        if r.server_side && r.active {
            tx.execute(
                "UPDATE command_replay_state SET admitted=admitted+1 WHERE id=1",
                [],
            )
            .map_err(db)?;
        }
        tx.commit().map_err(db)
    }
    pub(crate) fn stream_finish(&mut self, r: &mut StreamRecord) -> Result<()> {
        r.active = false;
        r.completed_at.get_or_insert(now()?);
        if r.identity.is_none() {
            r.payload_drained = true;
        }
        let tx = self.conn.transaction().map_err(db)?;
        let was_active: bool = tx
            .query_row(
                "SELECT active FROM live_streams WHERE k=?1",
                [r.key()],
                |row| row.get(0),
            )
            .map_err(db)?;
        tx.execute(
            "UPDATE live_streams SET active=0,record=?2 WHERE k=?1",
            params![r.key(), encode(r)?],
        )
        .map_err(db)?;
        if was_active && r.server_side {
            tx.execute(
                "UPDATE command_replay_state SET finished=finished+1 WHERE id=1",
                [],
            )
            .map_err(db)?;
        }
        tx.commit().map_err(db)
    }
    pub(crate) fn stream_slot(&self, key: &str, digest: &[u8; 32]) -> Result<Option<StreamSlot>> {
        self.conn
            .query_row(
                "SELECT request,response FROM stream_slots WHERE k=?1 AND digest=?2",
                params![key, digest.as_slice()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)
    }
    pub(crate) fn stream_request_slot(
        &mut self,
        key: &str,
        digest: &[u8; 32],
        seq: u64,
        request: &[u8],
    ) -> Result<()> {
        let sequence = i64::try_from(seq).map_err(|_| Error::Invalid("stream_sequence"))?;
        let cancel = marmish_stream::wire::decode_request(request)
            .is_ok_and(|r| matches!(r.body, marmish_stream::wire::RequestBody::Cancel));
        let tx = self.conn.transaction().map_err(db)?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM stream_slots WHERE k=?1 AND digest=?2)",
                params![key, digest.as_slice()],
                |r| r.get(0),
            )
            .map_err(db)?;
        if exists {
            return Ok(());
        }
        if sequence > 1 {
            tx.execute(
                "DELETE FROM stream_slots WHERE k=?1 AND sequence>0 AND sequence<?2",
                params![key, sequence - 1],
            )
            .map_err(db)?;
        }
        if sequence == 0 {
            tx.execute("DELETE FROM stream_slots WHERE rowid IN (SELECT rowid FROM stream_slots WHERE k=?1 AND sequence=0 ORDER BY rowid DESC LIMIT -1 OFFSET 1)",[key]).map_err(db)?;
        }
        let n: i64 = tx
            .query_row("SELECT count(*) FROM stream_slots", [], |r| r.get(0))
            .map_err(db)?;
        let peer:i64=tx.query_row("SELECT count(*) FROM stream_slots WHERE k IN (SELECT k FROM live_streams WHERE principal=(SELECT principal FROM live_streams WHERE k=?1))",[key],|r|r.get(0)).map_err(db)?;
        let own: i64 = tx
            .query_row("SELECT count(*) FROM stream_slots WHERE k=?1", [key], |r| {
                r.get(0)
            })
            .map_err(db)?;
        let (global_limit, peer_limit, own_limit) = if cancel {
            (1024, 128, 5)
        } else {
            (992, 124, 4)
        };
        if n >= global_limit || peer >= peer_limit || own >= own_limit {
            return Err(Error::Invalid("stream_slot_capacity"));
        }
        tx.execute(
            "INSERT INTO stream_slots(k,digest,sequence,request) VALUES(?1,?2,?3,?4)",
            params![key, digest.as_slice(), sequence, request],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
    pub(crate) fn stream_requests(&self, key: &str) -> Result<Vec<Vec<u8>>> {
        let mut query = self
            .conn
            .prepare("SELECT request FROM stream_slots WHERE k=?1 ORDER BY rowid LIMIT 5")
            .map_err(db)?;
        query
            .query_map([key], |row| row.get(0))
            .map_err(db)?
            .map(|row| row.map_err(db))
            .collect()
    }
    pub(crate) fn stream_response_slot(
        &mut self,
        key: &str,
        digest: &[u8; 32],
        response: &[u8],
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE stream_slots SET response=?3 WHERE k=?1 AND digest=?2",
                params![key, digest.as_slice(), response],
            )
            .map_err(db)?;
        Ok(())
    }
    pub(crate) fn stream_queue_publication(&mut self, p: &StreamPublication) -> Result<()> {
        if self.stream_publication_owned(&p.group, &p.event_id)? {
            return Ok(());
        }
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM stream_publications", [], |r| r.get(0))
            .map_err(db)?;
        if n >= 992 {
            self.conn.execute("DELETE FROM stream_publications WHERE rowid IN (SELECT rowid FROM stream_publications WHERE state IN ('acked','retired_failed') ORDER BY rowid LIMIT ?1)", [128i64.max(n - 864)]).map_err(db)?;
        }
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM stream_publications", [], |r| r.get(0))
            .map_err(db)?;
        let peer:i64=self.conn.query_row("SELECT count(*) FROM stream_publications WHERE state NOT IN ('acked','retired_failed') AND k IN (SELECT k FROM live_streams WHERE principal=(SELECT principal FROM live_streams WHERE k=?1))",[&p.stream_key],|r|r.get(0)).map_err(db)?;
        let cancel = self.stream_publication_is_cancel(p)?;
        let (global_limit, peer_limit) = if cancel { (1024, 128) } else { (992, 124) };
        if n >= global_limit || peer >= peer_limit {
            return Err(Error::Invalid("stream_publication_capacity"));
        }
        self.conn.execute("INSERT OR IGNORE INTO stream_publications(group_id,event_id,k,payload,state) VALUES(?1,?2,?3,?4,'unsubmitted')",params![p.group,p.event_id,p.stream_key,p.payload]).map_err(db)?;
        Ok(())
    }
    pub(crate) fn stream_publication_is_cancel(&self, p: &StreamPublication) -> Result<bool> {
        Ok(
            if let Ok(app) = cgka_traits::app_event::MarmotAppEvent::decode(&p.payload) {
                if let Ok(request) = marmish_stream::wire::decode_request(app.content.as_bytes()) {
                    matches!(request.body, marmish_stream::wire::RequestBody::Cancel)
                } else if let Ok(response) =
                    marmish_stream::wire::decode_response(app.content.as_bytes())
                {
                    // Cancel has a canonical sequence-zero digest. Reconstruct
                    // it so terminal payload reclamation cannot erase priority.
                    let cancel = marmish_stream::wire::Request {
                        binding: response.binding,
                        sequence: 0,
                        body: marmish_stream::wire::RequestBody::Cancel,
                    };
                    marmish_stream::wire::encode_request(&cancel).is_ok_and(|bytes| {
                        marmish_stream::wire::request_digest(&bytes) == response.request_sha256
                    })
                } else {
                    false
                }
            } else {
                false
            },
        )
    }
    pub(crate) fn stream_publications(&self) -> Result<Vec<StreamPublication>> {
        let mut q=self.conn.prepare("SELECT group_id,event_id,k,payload,state,native_id,evidence,intent FROM stream_publications ORDER BY CASE WHEN state IN ('acked','retired_failed') THEN 1 ELSE 0 END,rowid LIMIT 1024").map_err(db)?;
        q.query_map([], |r| {
            Ok(StreamPublication {
                group: r.get(0)?,
                event_id: r.get(1)?,
                stream_key: r.get(2)?,
                payload: r.get(3)?,
                state: r.get(4)?,
                native_id: r.get(5)?,
                evidence: r.get(6)?,
                intent: r.get(7)?,
            })
        })
        .map_err(db)?
        .map(|r| r.map_err(db))
        .collect()
    }
    pub(crate) fn stream_publication(
        &self,
        group: &[u8],
        event: &str,
    ) -> Result<Option<StreamPublication>> {
        self.conn.query_row("SELECT group_id,event_id,k,payload,state,native_id,evidence,intent FROM stream_publications WHERE group_id=?1 AND event_id=?2",params![group,event],|r| Ok(StreamPublication {group:r.get(0)?,event_id:r.get(1)?,stream_key:r.get(2)?,payload:r.get(3)?,state:r.get(4)?,native_id:r.get(5)?,evidence:r.get(6)?,intent:r.get(7)?})).optional().map_err(db)
    }
    pub(crate) fn stream_publication_owned(&self, group: &[u8], event: &str) -> Result<bool> {
        self.conn.query_row("SELECT EXISTS(SELECT 1 FROM stream_publications WHERE group_id=?1 AND event_id=?2)",params![group,event],|r|r.get(0)).map_err(db)
    }
    pub(crate) fn stream_publication_state(
        &mut self,
        group: &[u8],
        event: &str,
        state: &str,
    ) -> Result<()> {
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Invalid("clock"))?
            .as_millis()
            .try_into()
            .map_err(|_| Error::Invalid("clock"))?;
        self.stream_publication_state_at(group, event, state, timestamp_ms)
    }
    fn stream_publication_state_at(
        &mut self,
        group: &[u8],
        event: &str,
        state: &str,
        timestamp_ms: u64,
    ) -> Result<()> {
        if matches!(state, "acked" | "retired_failed") {
            let previous: Option<(String,Vec<u8>,String)> = self.conn.query_row("SELECT k,payload,state FROM stream_publications WHERE group_id=?1 AND event_id=?2",params![group,event],|row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(db)?;
            if let Some((key, payload, previous_state)) = previous
                && !matches!(previous_state.as_str(), "acked" | "retired_failed")
                && (previous_state != "uncertain" || state == "acked")
                && let Ok(app) = cgka_traits::app_event::MarmotAppEvent::decode(&payload)
            {
                let digest = marmish_stream::wire::request_digest(app.content.as_bytes());
                self.conn
                    .execute(
                        "UPDATE stream_attempts SET last_ms=?3 WHERE k=?1 AND digest=?2",
                        params![key, digest.as_slice(), timestamp_ms as i64],
                    )
                    .map_err(db)?;
            }
            self.conn.execute("UPDATE stream_publications SET state=?3,payload=X'',native_id=NULL,evidence=NULL,intent=NULL WHERE group_id=?1 AND event_id=?2 AND state NOT IN ('acked','retired_failed') AND (state!='uncertain' OR ?3='acked')",params![group,event,state]).map_err(db)?;
        } else {
            self.conn
                .execute(
                    "UPDATE stream_publications SET state=?3 WHERE group_id=?1 AND event_id=?2 AND state NOT IN ('acked','retired_failed') AND (state!='uncertain' OR ?3='uncertain')",
                    params![group, event, state],
                )
                .map_err(db)?;
        }
        Ok(())
    }
    pub(crate) fn stream_publication_queued(
        &mut self,
        group: &[u8],
        event: &str,
        intent: &[u8],
    ) -> Result<()> {
        self.conn.execute("UPDATE stream_publications SET state='queued',intent=?3 WHERE group_id=?1 AND event_id=?2 AND state IN ('prepared','queued')",params![group,event,intent]).map_err(db)?;
        Ok(())
    }
    pub(crate) fn stream_publication_published(
        &mut self,
        p: &marmot_session::PublishedApplicationMessage,
    ) -> Result<()> {
        self.conn.execute("UPDATE stream_publications SET state=CASE WHEN state IN ('failed','retired_failed') THEN 'failed' WHEN state='uncertain' THEN state ELSE 'published' END,native_id=?3,evidence=?4 WHERE group_id=?1 AND event_id=?2 AND state NOT IN ('acked','retired_failed')",params![p.group_id.as_slice(),p.app_event_id,p.message_id.as_slice(),PublishedEvidence::from_native(p).encode()?]).map_err(db)?;
        Ok(())
    }
    pub(crate) fn stream_publication_refused(&mut self, group: &[u8], intent: &[u8]) -> Result<()> {
        self.conn
            .execute(
                "UPDATE stream_publications SET state='failed' WHERE group_id=?1 AND intent=?2 AND state NOT IN ('uncertain','acked','retired_failed')",
                params![group, intent],
            )
            .map_err(db)?;
        Ok(())
    }
    pub(crate) fn stream_publication_invalidated(
        &mut self,
        group: &[u8],
        native: &[u8],
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE stream_publications SET state='failed' WHERE group_id=?1 AND native_id=?2 AND state NOT IN ('uncertain','acked','retired_failed')",
                params![group, native],
            )
            .map_err(db)?;
        Ok(())
    }
    pub(crate) fn streams_idle(&self) -> Result<bool> {
        self.conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM live_streams WHERE active=1) AND NOT EXISTS(SELECT 1 FROM stream_publications WHERE state NOT IN ('acked','retired_failed'))",[],|r|r.get(0)).map_err(db)
    }
}
impl Journal {
    /// Payload lifetime is independent from immutable replay/launch tombstones.
    pub(crate) fn stream_maintain(&mut self, timestamp: u64) -> Result<()> {
        let retention = super::command_store::stream_payload_retention(&self.conn)?;
        for mut record in self.stream_records()? {
            if record.active {
                continue;
            }
            let expired = record
                .completed_at
                .is_some_and(|end| timestamp.saturating_sub(end) >= retention);
            let born = u64::from_be_bytes(
                record.binding.stream[..8]
                    .try_into()
                    .map_err(|_| Error::Invalid("stream_identifier"))?,
            );
            let native_pending: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM stream_publications WHERE k=?1 AND state NOT IN ('acked','retired_failed'))", [record.key()], |row| row.get(0)).map_err(db)?;
            if timestamp > born.saturating_add(7 * 86400 + 60)
                && !native_pending
                && !matches!(
                    record.state.as_str(),
                    "lost" | "unknown_launch" | "launching" | "transport_uncertain"
                )
                && (record.server_side || record.open_digest.is_none() || record.terminal.is_some())
            {
                self.stream_clear_slots(&record.key())?;
                self.conn
                    .execute("DELETE FROM stream_attempts WHERE k=?1", [record.key()])
                    .map_err(db)?;
                self.conn
                    .execute("DELETE FROM stream_publications WHERE k=?1", [record.key()])
                    .map_err(db)?;
                self.conn
                    .execute(
                        "DELETE FROM live_streams WHERE k=?1 AND active=0",
                        [record.key()],
                    )
                    .map_err(db)?;
                continue;
            }
            if record.terminal.is_none() {
                continue;
            }
            let has_slots: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM stream_slots WHERE k=?1)",
                    [record.key()],
                    |row| row.get(0),
                )
                .map_err(db)?;
            if (record.payload_drained || expired)
                && (has_slots
                    || !record.argv.is_empty()
                    || (expired && !record.payload_drained && !record.payload_expired))
            {
                record.argv.clear();
                record.payload_expired |= expired && !record.payload_drained;
                self.stream_clear_slots(&record.key())?;
                self.stream_save(&record)?;
            }
        }
        Ok(())
    }
    pub(crate) fn stream_clear_slots(&mut self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM stream_slots WHERE k=?1", [key])
            .map_err(db)?;
        Ok(())
    }
    pub(crate) fn stream_enqueue_attempt(
        &mut self,
        key: &str,
        bytes: &[u8],
        timestamp_ms: u64,
        publication: impl FnOnce(u64) -> Result<StreamPublication>,
    ) -> Result<()> {
        self.conn
            .execute_batch("SAVEPOINT stream_enqueue")
            .map_err(db)?;
        let result = (|| {
            if let Some(created) = self.stream_attempt(key, bytes, timestamp_ms)? {
                self.stream_queue_publication(&publication(created)?)?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.conn
                .execute_batch("ROLLBACK TO stream_enqueue; RELEASE stream_enqueue")
                .map_err(db)?;
        } else {
            self.conn
                .execute_batch("RELEASE stream_enqueue")
                .map_err(db)?;
        }
        result
    }
    /// One outstanding native attempt per logical frame. Retry only definitive
    /// completion, using durable capped backoff and collision-free event dates.
    pub(crate) fn stream_attempt(
        &mut self,
        key: &str,
        bytes: &[u8],
        timestamp_ms: u64,
    ) -> Result<Option<u64>> {
        let digest = marmish_stream::wire::request_digest(bytes);
        let publications = self.stream_publications()?;
        for publication in publications.iter().filter(|p| p.stream_key == key) {
            if let Ok(app) = cgka_traits::app_event::MarmotAppEvent::decode(&publication.payload)
                && app.content.as_bytes() == bytes
                && !matches!(publication.state.as_str(), "acked" | "retired_failed")
            {
                return Ok(None);
            }
        }
        let previous: Option<(u64, u64, u64)> = self
            .conn
            .query_row(
                "SELECT last_ms,attempt,created FROM stream_attempts WHERE k=?1 AND digest=?2",
                params![key, digest.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, i64>(1)? as u64,
                        row.get::<_, i64>(2)? as u64,
                    ))
                },
            )
            .optional()
            .map_err(db)?;
        let now_seconds = timestamp_ms / 1000;
        let (attempt, created) = if let Some((last_ms, attempt, created)) = previous {
            let delay = 250u64 << attempt.saturating_sub(1).min(3);
            if timestamp_ms.saturating_sub(last_ms) < delay {
                return Ok(None);
            }
            (
                attempt.saturating_add(1).min(5),
                now_seconds.max(created.saturating_add(1)),
            )
        } else {
            (1, now_seconds)
        };
        if created > now_seconds.saturating_add(60) {
            return Ok(None);
        }
        // Keep retry state for every still-owned native attempt. Updating an
        // old row must refresh its eviction priority, not its immutable rowid.
        let protected: std::collections::BTreeSet<_> = publications
            .iter()
            .filter(|p| {
                p.stream_key == key && !matches!(p.state.as_str(), "acked" | "retired_failed")
            })
            .filter_map(|p| cgka_traits::app_event::MarmotAppEvent::decode(&p.payload).ok())
            .map(|app| marmish_stream::wire::request_digest(app.content.as_bytes()))
            .collect();
        let candidates: Vec<Vec<u8>> = {
            let mut query = self
                .conn
                .prepare(
                    "SELECT digest FROM stream_attempts WHERE k=?1 ORDER BY last_ms ASC,rowid ASC",
                )
                .map_err(db)?;
            query
                .query_map([key], |row| row.get(0))
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?
        };
        let needed = candidates
            .len()
            .saturating_add(usize::from(previous.is_none()))
            .saturating_sub(8);
        let removable: Vec<_> = candidates
            .into_iter()
            .filter(|d| {
                d.as_slice() != digest && !protected.iter().any(|p| p.as_slice() == d.as_slice())
            })
            .take(needed)
            .collect();
        if removable.len() != needed {
            return Err(Error::Invalid("stream_capacity"));
        }
        for old in removable {
            self.conn
                .execute(
                    "DELETE FROM stream_attempts WHERE k=?1 AND digest=?2",
                    params![key, old],
                )
                .map_err(db)?;
        }
        self.conn.execute("INSERT INTO stream_attempts(k,digest,last_ms,attempt,created) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(k,digest) DO UPDATE SET last_ms=excluded.last_ms,attempt=excluded.attempt,created=excluded.created",params![key,digest.as_slice(),timestamp_ms as i64,attempt as i64,created as i64]).map_err(db)?;
        Ok(Some(created))
    }
}

impl Journal {
    pub(crate) fn stream_admit(&mut self, r: &mut StreamRecord) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        super::command_store::stream_check_admission(&tx, &r.binding.client, true, false)?;
        r.active = true;
        if tx
            .execute(
                "UPDATE live_streams SET active=1,record=?2 WHERE k=?1 AND active=0",
                params![r.key(), encode(r)?],
            )
            .map_err(db)?
            != 1
        {
            return Err(Error::Invalid("stream_admission_conflict"));
        }
        tx.execute(
            "UPDATE command_replay_state SET admitted=admitted+1 WHERE id=1",
            [],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marmish_stream::{
        codec::Codec,
        wire::{Binding, Terminal, WireErrorCode},
    };
    fn setup() -> (tempfile::TempDir, Journal) {
        let directory = super::super::private_test_directory();
        let journal = Journal::open(
            &directory.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        (directory, journal)
    }
    fn record(id: u8) -> StreamRecord {
        StreamRecord {
            binding: Binding {
                stream: [id; 16],
                session: [9; 16],
                client: [1; 32],
                server: [2; 32],
            },
            group: vec![5],
            server_side: true,
            hello: true,
            open_digest: None,
            argv: Vec::new(),
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
        }
    }
    #[test]
    fn active_server_query_excludes_retained_records_and_keeps_lost_holds() {
        let (_directory, mut journal) = setup();
        let hello = record(1);
        journal.stream_insert(&hello).unwrap();
        let mut terminal = record(2);
        terminal.terminal = Some(Terminal::Cancelled);
        terminal.state = "terminal".into();
        journal.stream_insert(&terminal).unwrap();
        let mut preparing = record(3);
        preparing.state = "preparing".into();
        journal.stream_insert(&preparing).unwrap();
        journal.stream_admit(&mut preparing).unwrap();
        let mut lost = record(4);
        lost.state = "lost".into();
        journal.stream_insert(&lost).unwrap();
        journal.stream_admit(&mut lost).unwrap();
        let records = journal.stream_active_server_records().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|r| r.key() == preparing.key()));
        assert!(records.iter().any(|r| r.key() == lost.key() && r.active));
        assert_eq!(journal.stream_records().unwrap().len(), 4);
    }
    #[test]
    fn abandoned_inactive_streams_expire_only_past_replay_window() {
        let (_directory, mut journal) = setup();
        let mut hello = record(1);
        hello.binding.stream[..8].copy_from_slice(&100u64.to_be_bytes());
        let mut invalidated = hello.clone();
        invalidated.binding.stream[15] = 2;
        invalidated.projection_invalidated = true;
        let mut unknown = hello.clone();
        unknown.binding.stream[15] = 3;
        unknown.state = "unknown_launch".into();
        let mut pending = hello.clone();
        pending.binding.stream[15] = 4;
        for r in [&hello, &invalidated, &unknown, &pending] {
            journal.stream_insert(r).unwrap();
        }
        journal
            .stream_queue_publication(&StreamPublication {
                group: pending.group.clone(),
                event_id: "pending".into(),
                stream_key: pending.key(),
                payload: vec![],
                state: "unsubmitted".into(),
                native_id: None,
                evidence: None,
                intent: None,
            })
            .unwrap();
        journal.stream_maintain(100 + 7 * 86400 + 60).unwrap();
        assert_eq!(journal.stream_records().unwrap().len(), 4);
        journal.stream_maintain(100 + 7 * 86400 + 61).unwrap();
        assert!(journal.stream_get(&hello.key()).unwrap().is_none());
        assert!(journal.stream_get(&invalidated.key()).unwrap().is_none());
        assert!(journal.stream_get(&unknown.key()).unwrap().is_some());
        assert!(journal.stream_get(&pending.key()).unwrap().is_some());
    }
    #[test]
    fn client_open_intent_and_running_records_survive_fence_age() {
        let (_directory, mut journal) = setup();
        for (id, state, opened) in [
            (1, "hello", false),
            (2, "opening", true),
            (3, "running", true),
            (4, "transport_uncertain", true),
        ] {
            let mut r = record(id);
            r.binding.stream[..8].copy_from_slice(&100u64.to_be_bytes());
            r.server_side = false;
            r.state = state.into();
            r.open_digest = opened.then_some([1; 32]);
            journal.stream_insert(&r).unwrap();
        }
        journal.stream_maintain(100 + 8 * 86400).unwrap();
        let records = journal.stream_records().unwrap();
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|r| r.open_digest.is_some()));
    }
    #[test]
    fn slot_failure_rolls_back_pruning_and_cancel_has_own_credit() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        for id in 1..=4u8 {
            journal
                .conn
                .execute(
                    "INSERT INTO stream_slots(k,digest,sequence,request) VALUES(?1,?2,?3,X'')",
                    params![r.key(), [id; 32].as_slice(), i64::from(id)],
                )
                .unwrap();
        }
        assert!(
            journal
                .stream_request_slot(&r.key(), &[8; 32], u64::MAX, b"bad")
                .is_err()
        );
        assert!(journal.stream_slot(&r.key(), &[1; 32]).unwrap().is_some());
        let request = marmish_stream::wire::Request {
            binding: r.binding.clone(),
            sequence: 0,
            body: marmish_stream::wire::RequestBody::Cancel,
        };
        let bytes = marmish_stream::wire::encode_request(&request).unwrap();
        journal
            .stream_request_slot(&r.key(), &[9; 32], 0, &bytes)
            .unwrap();
        assert_eq!(journal.stream_requests(&r.key()).unwrap().len(), 5);
        // A failed ordinary request cannot commit its control-slot pruning.
        assert!(
            journal
                .stream_request_slot(&r.key(), &[7; 32], 1, b"full")
                .is_err()
        );
        assert!(journal.stream_slot(&r.key(), &[9; 32]).unwrap().is_some());
        journal
            .conn
            .execute(
                "INSERT INTO stream_slots(k,digest,sequence,request) VALUES(?1,?2,0,X'')",
                params![r.key(), [10u8; 32].as_slice()],
            )
            .unwrap();
        let other = record(2);
        journal.stream_insert(&other).unwrap();
        for id in 0..124u8 {
            journal
                .conn
                .execute(
                    "INSERT INTO stream_slots(k,digest,sequence,request) VALUES(?1,?2,1,X'')",
                    params![other.key(), [id; 32].as_slice()],
                )
                .unwrap();
        }
        assert!(
            journal
                .stream_request_slot(&r.key(), &[11; 32], 0, b"status")
                .is_err()
        );
        assert!(journal.stream_slot(&r.key(), &[9; 32]).unwrap().is_some());
        assert!(journal.stream_slot(&r.key(), &[10; 32]).unwrap().is_some());
    }
    #[test]
    fn enqueue_capacity_failure_does_not_burn_retry_ledger() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        let publication = |id: String| StreamPublication {
            group: r.group.clone(),
            event_id: id,
            stream_key: r.key(),
            payload: b"frame".to_vec(),
            state: "unsubmitted".into(),
            native_id: None,
            evidence: None,
            intent: None,
        };
        for n in 0..124 {
            journal
                .stream_queue_publication(&publication(format!("full-{n}")))
                .unwrap();
        }
        assert!(
            journal
                .stream_enqueue_attempt(&r.key(), b"retry", 100000, |_| Ok(publication(
                    "retry".into()
                )))
                .is_err()
        );
        let attempts: i64 = journal
            .conn
            .query_row("SELECT count(*) FROM stream_attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(attempts, 0);
        journal
            .stream_publication_state(&r.group, "full-0", "failed")
            .unwrap();
        journal
            .stream_publication_state(&r.group, "full-0", "retired_failed")
            .unwrap();
        journal
            .stream_enqueue_attempt(&r.key(), b"retry", 100000, |_| {
                Ok(publication("retry".into()))
            })
            .unwrap();
        assert!(journal.stream_publication_owned(&r.group, "retry").unwrap());
    }
    #[test]
    fn publication_uncertainty_and_retirement_cannot_regress() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        let p = StreamPublication {
            group: r.group.clone(),
            event_id: "event".into(),
            stream_key: r.key(),
            payload: vec![],
            state: "unsubmitted".into(),
            native_id: None,
            evidence: None,
            intent: None,
        };
        journal.stream_queue_publication(&p).unwrap();
        journal
            .stream_publication_state(&r.group, "event", "prepared")
            .unwrap();
        journal
            .stream_publication_queued(&r.group, "event", b"intent")
            .unwrap();
        journal
            .conn
            .execute(
                "UPDATE stream_publications SET native_id=?1",
                [b"native".as_slice()],
            )
            .unwrap();
        journal
            .stream_publication_state(&r.group, "event", "uncertain")
            .unwrap();
        journal
            .stream_publication_refused(&r.group, b"intent")
            .unwrap();
        journal
            .stream_publication_invalidated(&r.group, b"native")
            .unwrap();
        journal
            .stream_publication_state(&r.group, "event", "failed")
            .unwrap();
        journal
            .stream_publication_state(&r.group, "event", "retired_failed")
            .unwrap();
        assert_eq!(
            journal
                .stream_publication(&r.group, "event")
                .unwrap()
                .unwrap()
                .state,
            "uncertain"
        );
        journal
            .stream_publication_state(&r.group, "event", "acked")
            .unwrap();
        journal
            .stream_publication_queued(&r.group, "event", b"late")
            .unwrap();
        journal
            .stream_publication_state(&r.group, "event", "uncertain")
            .unwrap();
        assert_eq!(
            journal
                .stream_publication(&r.group, "event")
                .unwrap()
                .unwrap()
                .state,
            "acked"
        );
    }
    #[test]
    fn refreshed_retry_survives_eight_newer_rowids() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        assert_eq!(
            journal.stream_attempt(&r.key(), b"active", 100000).unwrap(),
            Some(100)
        );
        for id in 0..7u8 {
            journal
                .stream_attempt(&r.key(), &[id], 100001 + u64::from(id))
                .unwrap();
        }
        assert_eq!(
            journal.stream_attempt(&r.key(), b"active", 100250).unwrap(),
            Some(101)
        );
        journal.stream_attempt(&r.key(), b"new", 100251).unwrap();
        assert_eq!(
            journal.stream_attempt(&r.key(), b"active", 100749).unwrap(),
            None
        );
        assert_eq!(
            journal.stream_attempt(&r.key(), b"active", 100750).unwrap(),
            Some(102)
        );
    }
    #[test]
    fn restart_preserves_cancel_before_open_and_immutable_launch_fence() {
        let (_directory, mut journal) = setup();
        let mut cancelled = record(1);
        cancelled.cancel = true;
        cancelled.terminal = Some(Terminal::Cancelled);
        cancelled.state = "terminal".into();
        journal.stream_insert(&cancelled).unwrap();
        let mut launched = record(2);
        launched.open_digest = Some([3; 32]);
        launched.state = "launching".into();
        journal.stream_insert(&launched).unwrap();
        journal.stream_admit(&mut launched).unwrap();
        journal.stream_init().unwrap();
        let cancelled = journal.stream_get(&cancelled.key()).unwrap().unwrap();
        assert!(cancelled.cancel);
        assert_eq!(cancelled.terminal, Some(Terminal::Cancelled));
        let launched = journal.stream_get(&launched.key()).unwrap().unwrap();
        assert_eq!(launched.state, "unknown_launch");
        assert!(launched.active);
        assert_eq!(launched.open_digest, Some([3; 32]));
        assert_eq!(
            launched.terminal,
            Some(Terminal::Failed {
                code: WireErrorCode::Uncertain
            })
        );
        assert!(!journal.streams_idle().unwrap());
        journal.stream_init().unwrap();
        let restarted = journal.stream_get(&launched.key()).unwrap().unwrap();
        assert_eq!(restarted.state, "unknown_launch");
        assert!(restarted.active);
        assert_eq!(
            restarted.child_disposition,
            Some(crate::stream_service::ChildDisposition::Unknown)
        );
    }
    #[test]
    fn restart_diagnostics_do_not_release_quota_or_authorize_signalling() {
        use crate::stream_service::ChildDisposition;
        let (_directory, mut journal) = setup();
        let mut absent = record(1);
        absent.state = "running".into();
        // A different boot proves only this leader absent, not its descendants.
        absent.identity = Some(
            serde_json::to_vec(&marmish_engine::LiveProcessRecord {
                boot_id: [255; 16],
                pid: 1,
                pgid: 1,
                start_ticks: 1,
            })
            .unwrap(),
        );
        journal.stream_insert(&absent).unwrap();
        journal.stream_admit(&mut absent).unwrap();
        let mut unknown = record(2);
        unknown.state = "running".into();
        unknown.identity = Some(b"invalid diagnostic metadata".to_vec());
        journal.stream_insert(&unknown).unwrap();
        journal.stream_admit(&mut unknown).unwrap();
        journal.stream_init().unwrap();
        for (original, disposition) in [
            (absent, ChildDisposition::Absent),
            (unknown, ChildDisposition::Unknown),
        ] {
            let recovered = journal.stream_get(&original.key()).unwrap().unwrap();
            assert_eq!(recovered.state, "lost");
            assert!(recovered.active);
            assert_eq!(recovered.identity, original.identity);
            assert_eq!(recovered.entry().child_disposition, Some(disposition));
            assert_eq!(
                recovered.terminal,
                Some(Terminal::Failed {
                    code: WireErrorCode::Uncertain
                })
            );
        }
        assert!(!journal.streams_idle().unwrap());
    }
    #[test]
    fn lost_stream_holds_shared_quota_until_explicit_reconciliation() {
        let (_directory, mut journal) = setup();
        for id in 1..=4 {
            let mut r = record(id);
            journal.stream_insert(&r).unwrap();
            r.state = "preparing".into();
            journal.stream_admit(&mut r).unwrap();
        }
        journal.stream_init().unwrap();
        let mut fifth = record(5);
        journal.stream_insert(&fifth).unwrap();
        assert!(journal.stream_admit(&mut fifth).is_err());
        let mut lost = journal.stream_get(&record(1).key()).unwrap().unwrap();
        assert_eq!(lost.state, "lost");
        lost.state = "reconciled_possible_live".into();
        journal.stream_finish(&mut lost).unwrap();
        journal.stream_admit(&mut fifth).unwrap();
        let budget: (i64, i64) = journal
            .conn
            .query_row(
                "SELECT admitted,finished FROM command_replay_state WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(budget, (5, 1));
    }
    #[test]
    fn controls_remain_available_with_current_and_previous_data_slots() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        let k = r.key();
        for id in 1..=2 {
            journal
                .stream_request_slot(&k, &[id; 32], 0, &[id])
                .unwrap();
        }
        journal
            .stream_request_slot(&k, &[3; 32], 1, b"exchange1")
            .unwrap();
        journal
            .stream_request_slot(&k, &[4; 32], 2, b"exchange2")
            .unwrap();
        journal
            .stream_request_slot(&k, &[5; 32], 0, b"cancel")
            .unwrap();
        assert!(journal.stream_slot(&k, &[5; 32]).unwrap().is_some());
        journal
            .stream_request_slot(&k, &[6; 32], 3, b"exchange3")
            .unwrap();
        assert!(journal.stream_slot(&k, &[3; 32]).unwrap().is_none());
        assert!(journal.stream_slot(&k, &[4; 32]).unwrap().is_some());
        let count: i64 = journal
            .conn
            .query_row("SELECT count(*) FROM stream_slots", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 4);
    }
    #[test]
    fn reply_snapshot_can_advance_for_same_cancel_digest() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        let k = r.key();
        let digest = [8; 32];
        journal
            .stream_request_slot(&k, &digest, 0, b"cancel")
            .unwrap();
        journal
            .stream_response_slot(&k, &digest, b"requested")
            .unwrap();
        journal
            .stream_response_slot(&k, &digest, b"confirmed")
            .unwrap();
        assert_eq!(
            journal.stream_slot(&k, &digest).unwrap().unwrap(),
            (b"cancel".to_vec(), Some(b"confirmed".to_vec()))
        );
    }
    #[test]
    fn cancel_publication_has_reserved_credit_when_regular_lane_is_full() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        for id in 0..124 {
            journal
                .stream_queue_publication(&StreamPublication {
                    group: r.group.clone(),
                    event_id: format!("regular-{id}"),
                    stream_key: r.key(),
                    payload: b"regular".to_vec(),
                    state: "unsubmitted".into(),
                    native_id: None,
                    evidence: None,
                    intent: None,
                })
                .unwrap();
        }
        let mut publication = StreamPublication {
            group: r.group.clone(),
            event_id: "overflow".into(),
            stream_key: r.key(),
            payload: b"regular".to_vec(),
            state: "unsubmitted".into(),
            native_id: None,
            evidence: None,
            intent: None,
        };
        assert!(journal.stream_queue_publication(&publication).is_err());
        let request = marmish_stream::wire::Request {
            binding: r.binding.clone(),
            sequence: 0,
            body: marmish_stream::wire::RequestBody::Cancel,
        };
        let app = cgka_traits::app_event::MarmotAppEvent::new(
            hex::encode(r.binding.client),
            1,
            30101,
            vec![vec!["marmish".into(), "stream/1".into()]],
            String::from_utf8(marmish_stream::wire::encode_request(&request).unwrap()).unwrap(),
        );
        publication.event_id = app.id.clone();
        publication.payload = app.encode().unwrap();
        journal.stream_queue_publication(&publication).unwrap();
        assert_eq!(journal.stream_publications().unwrap().len(), 125);
    }
    #[test]
    fn publication_ownership_survives_prepared_queued_refused_and_invalidation() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        let publication = StreamPublication {
            group: r.group.clone(),
            event_id: "event".into(),
            stream_key: r.key(),
            payload: b"frame".to_vec(),
            state: "unsubmitted".into(),
            native_id: None,
            evidence: None,
            intent: None,
        };
        journal.stream_queue_publication(&publication).unwrap();
        assert!(journal.stream_publication_owned(&r.group, "event").unwrap());
        journal
            .stream_publication_state(&r.group, "event", "prepared")
            .unwrap();
        assert!(!journal.streams_idle().unwrap());
        journal
            .stream_publication_queued(&r.group, "event", b"intent")
            .unwrap();
        journal
            .stream_publication_refused(&r.group, b"intent")
            .unwrap();
        assert_eq!(journal.stream_publications().unwrap()[0].state, "failed");
        assert!(journal.stream_publication_owned(&r.group, "event").unwrap());
        journal.stream_queue_publication(&publication).unwrap();
        assert_eq!(journal.stream_publications().unwrap().len(), 1);
        journal
            .conn
            .execute(
                "UPDATE stream_publications SET native_id=?1",
                [b"native".as_slice()],
            )
            .unwrap();
        journal
            .stream_publication_invalidated(&r.group, b"native")
            .unwrap();
        assert_eq!(journal.stream_publications().unwrap()[0].state, "failed");
    }
    #[test]
    fn stream_completed_payloads_reclaim_more_than_peer_slot_budget() {
        let (_directory, mut journal) = setup();
        for id in 1..=160u8 {
            let mut r = record(id);
            // Valid, nonexpired wire identity; retain launch/cancel fences.
            r.binding.stream[..8].copy_from_slice(&now().unwrap().to_be_bytes());
            r.cancel = true;
            r.terminal = Some(Terminal::Cancelled);
            r.state = "terminal".into();
            r.completed_at = Some(now().unwrap());
            r.payload_drained = true;
            r.open_digest = Some([id; 32]);
            r.last_exchange_digest = Some([id; 32]);
            journal.stream_insert(&r).unwrap();
            for seq in 1..=2 {
                journal
                    .stream_request_slot(&r.key(), &[seq as u8; 32], seq, b"data")
                    .unwrap();
            }
            journal.stream_maintain(now().unwrap()).unwrap();
            assert!(journal.stream_slot(&r.key(), &[1; 32]).unwrap().is_none());
            let retained = journal.stream_get(&r.key()).unwrap().unwrap();
            assert!(retained.cancel);
            assert_eq!(retained.open_digest, r.open_digest);
            assert_eq!(retained.last_exchange_digest, r.last_exchange_digest);
        }
        assert_eq!(journal.stream_records().unwrap().len(), 160);
    }
    #[test]
    fn stream_payload_expiry_does_not_erase_launch_fence_or_active_unknown() {
        let (_directory, mut journal) = setup();
        let timestamp = now().unwrap();
        let mut r = record(1);
        r.binding.stream[..8].copy_from_slice(&timestamp.to_be_bytes());
        r.state = "terminal".into();
        r.terminal = Some(Terminal::Exited { code: 0 });
        r.completed_at = Some(timestamp.saturating_sub(86401));
        r.argv = vec![b"secret argument".to_vec()];
        journal.stream_insert(&r).unwrap();
        journal
            .stream_request_slot(&r.key(), &[1; 32], 1, b"private bytes")
            .unwrap();
        journal.stream_maintain(timestamp).unwrap();
        let retained = journal.stream_get(&r.key()).unwrap().unwrap();
        assert!(retained.payload_expired && retained.argv.is_empty());
        assert!(journal.stream_slot(&r.key(), &[1; 32]).unwrap().is_none());
        let mut lost = record(2);
        lost.active = true;
        lost.state = "unknown_launch".into();
        lost.terminal = Some(Terminal::Failed {
            code: WireErrorCode::Uncertain,
        });
        journal.stream_insert(&lost).unwrap();
        journal
            .stream_maintain(timestamp.saturating_add(8 * 86400))
            .unwrap();
        assert!(journal.stream_get(&lost.key()).unwrap().unwrap().active);
        assert!(journal.stream_get(&r.key()).unwrap().is_none());
    }
    #[test]
    fn stream_retry_scheduler_deduplicates_pending_and_uses_bounded_backoff() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        let frame = b"logical-frame";
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 100000).unwrap(),
            Some(100)
        );
        let app = cgka_traits::app_event::MarmotAppEvent::new(
            hex::encode(r.binding.server),
            100,
            30101,
            vec![],
            String::from_utf8(frame.to_vec()).unwrap(),
        );
        journal
            .stream_queue_publication(&StreamPublication {
                group: r.group.clone(),
                event_id: app.id.clone(),
                stream_key: r.key(),
                payload: app.encode().unwrap(),
                state: "unsubmitted".into(),
                native_id: None,
                evidence: None,
                intent: None,
            })
            .unwrap();
        for ms in (100100..110000).step_by(100) {
            assert_eq!(journal.stream_attempt(&r.key(), frame, ms).unwrap(), None);
        }
        assert_eq!(journal.stream_publications().unwrap().len(), 1);
        journal
            .stream_publication_state_at(&r.group, &app.id, "acked", 100000)
            .unwrap();
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 100249).unwrap(),
            None
        );
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 100250).unwrap(),
            Some(101)
        );
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 100749).unwrap(),
            None
        );
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 100750).unwrap(),
            Some(102)
        );
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 101749).unwrap(),
            None
        );
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 101750).unwrap(),
            Some(103)
        );
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 103749).unwrap(),
            None
        );
        assert_eq!(
            journal.stream_attempt(&r.key(), frame, 103750).unwrap(),
            Some(104)
        );
    }
    #[test]
    fn stream_retired_failure_drains_but_ambiguity_remains_owned() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        let p = StreamPublication {
            group: r.group.clone(),
            event_id: "failed".into(),
            stream_key: r.key(),
            payload: b"failure".to_vec(),
            state: "unsubmitted".into(),
            native_id: None,
            evidence: None,
            intent: None,
        };
        journal.stream_queue_publication(&p).unwrap();
        journal
            .stream_publication_state(&r.group, "failed", "failed")
            .unwrap();
        assert!(!journal.streams_idle().unwrap());
        journal
            .stream_publication_state(&r.group, "failed", "retired_failed")
            .unwrap();
        assert!(journal.streams_idle().unwrap());
        assert!(
            journal
                .stream_publication_owned(&r.group, "failed")
                .unwrap()
        );
        journal
            .stream_publication_state(&r.group, "failed", "uncertain")
            .unwrap();
        assert!(journal.streams_idle().unwrap()); // Retirement is irreversible.
    }
    #[test]
    fn stream_historical_publications_do_not_hide_pending_or_retain_payloads() {
        let (_directory, mut journal) = setup();
        let r = record(1);
        journal.stream_insert(&r).unwrap();
        for n in 0..1100 {
            journal.conn.execute("INSERT INTO stream_publications(group_id,event_id,k,payload,state) VALUES(?1,?2,?3,X'','acked')",params![r.group, format!("old-{n}"),r.key()]).unwrap();
        }
        let app = cgka_traits::app_event::MarmotAppEvent::new(
            hex::encode(r.binding.server),
            100,
            30101,
            vec![],
            "pending-frame",
        );
        let p = StreamPublication {
            group: r.group.clone(),
            event_id: app.id.clone(),
            stream_key: r.key(),
            payload: app.encode().unwrap(),
            state: "unsubmitted".into(),
            native_id: None,
            evidence: None,
            intent: None,
        };
        journal.stream_queue_publication(&p).unwrap();
        assert_eq!(journal.stream_publications().unwrap()[0].event_id, app.id);
        assert_eq!(
            journal
                .stream_attempt(&r.key(), b"pending-frame", 100000)
                .unwrap(),
            None
        );
        journal
            .stream_publication_state(&r.group, &app.id, "acked")
            .unwrap();
        let row = journal
            .stream_publications()
            .unwrap()
            .into_iter()
            .find(|p| p.event_id == app.id)
            .unwrap();
        assert!(row.payload.is_empty() && row.evidence.is_none() && row.intent.is_none());
    }
}
