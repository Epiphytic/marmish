//! Actor-owned encrypted application journal. Never opens the MDK database.
#[path = "command_rooms.rs"]
mod command_rooms;
#[path = "command_store.rs"]
mod command_store;
use crate::foundation::{Body, Envelope};
use crate::{Error, Result};
pub(crate) use command_rooms::*;
pub(crate) use command_store::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::Duration;
use zeroize::Zeroizing;

pub(crate) const JOURNAL_FORMAT: i64 = 5;
pub(crate) const JOURNAL_SUPPORTED_FORMATS: &[i64] = &[0, 3, 4, 5];

#[derive(Clone, Serialize)]
pub struct Conversation {
    pub session: String,
    pub group_id: String,
    pub peer: String,
    pub revoked: bool,
}
#[derive(Clone, Serialize)]
pub struct SendStatus {
    pub application_id: String,
    pub app_event_id: String,
    pub state: String,
}
#[derive(Clone, Serialize)]
pub struct ReceivedMessage {
    pub cursor: u64,
    pub session: String,
    pub application_id: String,
    pub sender: String,
    pub data: Vec<u8>,
    pub invalidated: bool,
}
pub(crate) struct Prepared {
    pub group: Vec<u8>,
    pub envelope: Envelope,
    pub event_id: String,
    pub encoded: Vec<u8>,
}
pub(crate) struct Journal {
    conn: Connection,
}
fn sql_u64(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(column)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
fn sql_optional_u64(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(column)?
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
        })
        .transpose()
}
fn db(_: rusqlite::Error) -> Error {
    Error::Storage("journal")
}

impl Journal {
    pub(crate) fn open(path: &Path, key: &[u8; 32], account: &str) -> Result<Self> {
        let parent = fs::symlink_metadata(path.parent().ok_or(Error::Storage("journal_path"))?)?;
        // The profile lease validates ancestors; additionally refuse a replaced
        // immediate parent and links/foreign-writable database files here.
        if !parent.is_dir()
            || parent.uid() != unsafe { libc::geteuid() }
            || parent.mode() & 0o077 != 0
        {
            return Err(Error::Storage("journal_path"));
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file.sync_all()?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let m = fs::symlink_metadata(path)?;
                if !m.is_file()
                    || m.uid() != parent.uid()
                    || m.mode() & 0o077 != 0
                    || m.nlink() != 1
                {
                    return Err(Error::Storage("journal_path"));
                }
            }
            Err(e) => return Err(e.into()),
        }
        marmish_engine::initialize_storage().map_err(|_| Error::Storage("sqlcipher_init"))?;
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(db)?;
        conn.busy_timeout(Duration::from_secs(2)).map_err(db)?;
        let version: String = conn
            .query_row("PRAGMA cipher_version", [], |r| r.get(0))
            .map_err(db)?;
        if version.is_empty() {
            return Err(Error::Storage("sqlcipher_missing"));
        }
        let mut raw = Zeroizing::new(Vec::with_capacity(67));
        raw.extend_from_slice(b"x'");
        let mut hex_key = Zeroizing::new([0u8; 64]);
        hex::encode_to_slice(key, hex_key.as_mut()).map_err(|_| Error::Storage("journal_key"))?;
        raw.extend_from_slice(hex_key.as_ref());
        raw.push(b'\'');
        // SAFETY: exclusive connection, bounded live byte buffer. No secret SQL.
        if unsafe {
            rusqlite::ffi::sqlite3_key(conn.handle(), raw.as_ptr().cast(), raw.len() as i32)
        } != rusqlite::ffi::SQLITE_OK
        {
            return Err(Error::Storage("journal_key"));
        }
        conn.execute_batch("PRAGMA cipher_compatibility=4; PRAGMA cipher_memory_security=ON; PRAGMA cipher_page_size=4096;").map_err(db)?;
        let page: String = conn
            .query_row("PRAGMA cipher_page_size", [], |r| r.get(0))
            .map_err(db)?;
        if page != "4096" {
            return Err(Error::Storage("cipher_page_size"));
        }
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(db)?;
        let schema: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db)?;
        if !JOURNAL_SUPPORTED_FORMATS.contains(&schema) {
            return Err(Error::Storage("journal_schema"));
        }
        conn.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA synchronous=FULL; PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS binding(account TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS conversations(session TEXT PRIMARY KEY, group_id BLOB NOT NULL UNIQUE, peer TEXT NOT NULL, revoked INTEGER NOT NULL DEFAULT 0,rejected_at INTEGER,cleanup_complete INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS outgoing(group_id BLOB NOT NULL, application_id TEXT NOT NULL, event_id TEXT NOT NULL, envelope BLOB, encoded BLOB, digest TEXT NOT NULL, native_id BLOB, state TEXT NOT NULL, created INTEGER NOT NULL, PRIMARY KEY(group_id,application_id));
            CREATE TABLE IF NOT EXISTS incoming(cursor INTEGER PRIMARY KEY AUTOINCREMENT,group_id BLOB NOT NULL, native_id BLOB NOT NULL, sender TEXT NOT NULL, application_id TEXT NOT NULL, envelope BLOB, digest TEXT NOT NULL, body_type TEXT NOT NULL, authority BLOB, receipt_for TEXT, invalidated INTEGER NOT NULL DEFAULT 0, created INTEGER NOT NULL, UNIQUE(group_id,sender,application_id));
            CREATE TABLE IF NOT EXISTS trust(group_id BLOB PRIMARY KEY,digest TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS queued_bindings(group_id BLOB NOT NULL,intent_id BLOB NOT NULL,event_id TEXT NOT NULL,PRIMARY KEY(group_id,intent_id));
            CREATE TABLE IF NOT EXISTS rejected_groups(group_id BLOB PRIMARY KEY,session_timestamp INTEGER,rejected_at INTEGER NOT NULL,cleanup_complete INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS replay_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),floor INTEGER NOT NULL,admission_reject_through INTEGER);
            INSERT OR IGNORE INTO replay_state VALUES(1,0,NULL);
            CREATE TABLE IF NOT EXISTS incoming_sources(group_id BLOB NOT NULL,native_id BLOB NOT NULL,sender TEXT NOT NULL,application_id TEXT NOT NULL,PRIMARY KEY(group_id,native_id));
            CREATE TABLE IF NOT EXISTS outgoing_sources(group_id BLOB NOT NULL,native_id BLOB NOT NULL,event_id TEXT NOT NULL,PRIMARY KEY(group_id,native_id));
").map_err(db)?;
        let existing: Option<String> = conn
            .query_row("SELECT account FROM binding", [], |r| r.get(0))
            .optional()
            .map_err(db)?;
        match existing {
            Some(value) if value != account => return Err(Error::Storage("journal_identity")),
            None => {
                conn.execute("INSERT INTO binding(account) VALUES(?)", [account])
                    .map_err(db)?;
            }
            _ => {}
        }
        let mut journal = Self { conn };
        journal.command_init()?;
        journal.command_rooms_init()?;
        journal
            .conn
            .pragma_update(None, "user_version", JOURNAL_FORMAT)
            .map_err(db)?;
        Ok(journal)
    }
    pub(crate) fn publication_owned(&self, group: &[u8], event: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM outgoing WHERE group_id=?1 AND event_id=?2)",
                params![group, event],
                |row| row.get(0),
            )
            .map_err(db)
    }
    pub(crate) fn conversation(&self, session: &str) -> Result<Option<(Conversation, Vec<u8>)>> {
        self.conn
            .query_row(
                "SELECT session,group_id,peer,revoked FROM conversations WHERE session=?",
                [session],
                |r| {
                    let group: Vec<u8> = r.get(1)?;
                    Ok((
                        Conversation {
                            session: r.get(0)?,
                            group_id: hex::encode(&group),
                            peer: r.get(2)?,
                            revoked: r.get(3)?,
                        },
                        group,
                    ))
                },
            )
            .optional()
            .map_err(db)
    }
    pub(crate) fn conversation_by_group(&self, group: &[u8]) -> Result<Option<Conversation>> {
        self.conn
            .query_row(
                "SELECT session,peer,revoked FROM conversations WHERE group_id=?",
                [group],
                |r| {
                    Ok(Conversation {
                        session: r.get(0)?,
                        group_id: hex::encode(group),
                        peer: r.get(1)?,
                        revoked: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(db)
    }
    pub(crate) fn conversations(&self) -> Result<Vec<Conversation>> {
        let mut stmt = self
            .conn
            .prepare("SELECT session,group_id,peer,revoked FROM conversations ORDER BY session")
            .map_err(db)?;
        stmt.query_map([], |r| {
            Ok(Conversation {
                session: r.get(0)?,
                group_id: hex::encode(r.get::<_, Vec<u8>>(1)?),
                peer: r.get(2)?,
                revoked: r.get(3)?,
            })
        })
        .map_err(db)?
        .collect::<std::result::Result<_, _>>()
        .map_err(db)
    }
    pub(crate) fn replay_floor(&self) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT floor FROM replay_state WHERE singleton=1",
                [],
                |r| sql_u64(r, 0),
            )
            .map_err(db)
    }
    pub(crate) fn admission_reject_through(&self) -> Result<Option<u64>> {
        self.conn
            .query_row(
                "SELECT admission_reject_through FROM replay_state WHERE singleton=1",
                [],
                |r| sql_optional_u64(r, 0),
            )
            .map_err(db)
    }
    #[cfg(test)]
    pub(crate) fn reject_group(&mut self, group: &[u8]) -> Result<()> {
        self.reject_group_at(group, None, 0)
    }
    pub(crate) fn reject_group_at(
        &mut self,
        group: &[u8],
        session_timestamp: Option<u64>,
        now: u64,
    ) -> Result<()> {
        let at = i64::try_from(now).map_err(|_| Error::Invalid("clock"))?;
        let stamp = session_timestamp
            .map(i64::try_from)
            .transpose()
            .map_err(|_| Error::Invalid("clock"))?;
        let tx = self.conn.transaction().map_err(db)?;
        let accepted: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE group_id=?)",
                [group],
                |r| r.get(0),
            )
            .map_err(db)?;
        if accepted {
            // An accepted room's authority and cleanup obligation never depend
            // on spare space in the unrelated capacity-refusal table.
            tx.execute("UPDATE conversations SET revoked=1,rejected_at=COALESCE(rejected_at,?) WHERE group_id=?", params![at,group]).map_err(db)?;
        } else {
            let (count, exists):(i64,bool)=tx.query_row("SELECT (SELECT count(*) FROM rejected_groups),EXISTS(SELECT 1 FROM rejected_groups WHERE group_id=?)",[group],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
            if count < 4096 || exists {
                tx.execute("INSERT OR IGNORE INTO rejected_groups(group_id,session_timestamp,rejected_at) VALUES(?,?,?)",params![group,stamp,at]).map_err(db)?;
            } else if let Some(stamp) = session_timestamp {
                let floor: u64 = tx
                    .query_row(
                        "SELECT floor FROM replay_state WHERE singleton=1",
                        [],
                        |r| sql_u64(r, 0),
                    )
                    .map_err(db)?;
                // Only temporally eligible policy-approved admissions may
                // advance this conservative, account-wide rejection frontier.
                if now < floor || stamp < floor || stamp > now.saturating_add(300) {
                    return Err(Error::Invalid("admission_timestamp"));
                }
                tx.execute("UPDATE replay_state SET admission_reject_through=MAX(COALESCE(admission_reject_through,0),?) WHERE singleton=1",[i64::try_from(stamp).map_err(|_|Error::Invalid("clock"))?]).map_err(db)?;
            } else {
                return Err(Error::Storage("rejected_group_capacity"));
            }
        }
        tx.execute(
            "UPDATE outgoing SET state='invalidated' WHERE group_id=?",
            [group],
        )
        .map_err(db)?;
        tx.execute(
            "UPDATE incoming SET invalidated=1 WHERE group_id=?",
            [group],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
    pub(crate) fn pending_group_cleanup(&self) -> Result<Vec<Vec<u8>>> {
        let mut stmt=self.conn.prepare("SELECT group_id FROM rejected_groups WHERE cleanup_complete=0 UNION SELECT group_id FROM conversations WHERE revoked=1 AND cleanup_complete=0").map_err(db)?;
        stmt.query_map([], |r| r.get(0))
            .map_err(db)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db)
    }
    /// Caller must first prove native forgetting and absence of pending events.
    pub(crate) fn mark_group_cleanup_complete(&mut self, group: &[u8]) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        tx.execute(
            "UPDATE rejected_groups SET cleanup_complete=1 WHERE group_id=?",
            [group],
        )
        .map_err(db)?;
        tx.execute(
            "UPDATE conversations SET cleanup_complete=1 WHERE group_id=? AND revoked=1",
            [group],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }
    pub(crate) fn group_rejected(&self, group: &[u8]) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM rejected_groups WHERE group_id=?1) OR EXISTS(SELECT 1 FROM conversations WHERE group_id=?1 AND revoked=1)",
                [group],
                |r| r.get(0),
            )
            .map_err(db)
    }
    pub(crate) fn accept_conversation(
        &mut self,
        group: &[u8],
        session: &str,
        peer: &str,
    ) -> Result<()> {
        if self.group_rejected(group)? {
            return Err(Error::Mdk("group_rejected"));
        }
        if let Some(old) = self.conversation_by_group(group)? {
            if old.session != session || old.peer != peer || old.revoked {
                return Err(Error::Mdk("conversation_conflict"));
            }
            return Ok(());
        }
        if self.conversation(session)?.is_some() {
            return Err(Error::Mdk("conversation_conflict"));
        }
        let count: u32 = self
            .conn
            .query_row(
                "SELECT count(*) FROM conversations WHERE revoked=0",
                [],
                |r| r.get(0),
            )
            .map_err(db)?;
        let metadata: u32 = self
            .conn
            .query_row("SELECT count(*) FROM conversations", [], |r| r.get(0))
            .map_err(db)?;
        if count >= 64 || metadata >= 4096 {
            return Err(Error::Mdk("conversation_capacity"));
        }
        self.conn
            .execute(
                "INSERT INTO conversations(session,group_id,peer) VALUES(?,?,?)",
                params![session, group, peer],
            )
            .map_err(db)?;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn revoke(&mut self, group: &[u8], _session: &str, _peer: &str) -> Result<()> {
        self.reject_group(group)
    }
    pub(crate) fn outgoing(&self, group: &[u8], id: &str) -> Result<Option<SendStatus>> {
        self.conn
            .query_row(
                "SELECT event_id,state FROM outgoing WHERE group_id=? AND application_id=?",
                params![group, id],
                |r| {
                    Ok(SendStatus {
                        application_id: id.into(),
                        app_event_id: r.get(0)?,
                        state: r.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(db)
    }
    pub(crate) fn verify_outgoing(
        &self,
        group: &[u8],
        envelope: &Envelope,
    ) -> Result<Option<SendStatus>> {
        let old: Option<String> = self
            .conn
            .query_row(
                "SELECT digest FROM outgoing WHERE group_id=? AND application_id=?",
                params![group, envelope.id()],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if let Some(old) = old {
            if old != crate::foundation::payload_digest(&envelope.encode()?) {
                return Err(Error::Invalid("application_id_conflict"));
            }
            return self.outgoing(group, envelope.id());
        }
        Ok(None)
    }
    pub(crate) fn prepare_send(&mut self, item: &Prepared, now: u64) -> Result<()> {
        Self::prepare_on(&self.conn, item, now)
    }
    fn prepare_on(conn: &Connection, item: &Prepared, now: u64) -> Result<()> {
        let encoded = item.envelope.encode()?;
        let digest = crate::foundation::payload_digest(&encoded);
        let old: Option<String> = conn
            .query_row(
                "SELECT digest FROM outgoing WHERE group_id=? AND application_id=?",
                params![item.group, item.envelope.id()],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if let Some(old) = old {
            if old != digest {
                return Err(Error::Invalid("application_id_conflict"));
            }
            return Ok(());
        }
        Self::check_fresh(conn, item.envelope.id(), now)?;
        Self::capacity(conn, "outgoing", &item.group)?;
        conn.execute("INSERT INTO outgoing(group_id,application_id,event_id,envelope,encoded,digest,state,created) VALUES(?,?,?,?,?,?,'unsubmitted',?)",params![item.group,item.envelope.id(),item.event_id,encoded,item.encoded,digest,i64::try_from(now).map_err(|_|Error::Invalid("clock"))?]).map_err(db)?;
        Ok(())
    }
    pub(crate) fn mark(&mut self, group: &[u8], event_id: &str, state: &str) -> Result<()> {
        if !matches!(
            state,
            "prepared" | "published" | "uncertain" | "invalidated" | "expired"
        ) {
            return Err(Error::Storage("journal_state"));
        }
        // A replayed publish confirmation must not undo a receipt/invalidation.
        self.conn.execute("UPDATE outgoing SET state=? WHERE group_id=? AND event_id=? AND (state IN ('unsubmitted','prepared','uncertain') OR ? IN ('invalidated','expired'))", params![state,group,event_id,state]).map_err(db)?;
        Ok(())
    }
    /// Returns true only for a new projection. Caller must have sealed all
    /// authenticated group/author/purpose checks before entering this method.
    #[cfg(test)]
    pub(crate) fn project(
        &mut self,
        group: &[u8],
        native_id: &[u8],
        envelope: &Envelope,
        now: u64,
    ) -> Result<bool> {
        self.project_authenticated(group, native_id, envelope, now, b"null")
    }
    #[cfg(test)]
    pub(crate) fn project_authenticated(
        &mut self,
        group: &[u8],
        native_id: &[u8],
        envelope: &Envelope,
        now: u64,
        authority: &[u8],
    ) -> Result<bool> {
        self.project_with_receipt(group, native_id, envelope, now, authority, None)
    }
    pub(crate) fn project_with_receipt(
        &mut self,
        group: &[u8],
        native_id: &[u8],
        envelope: &Envelope,
        now: u64,
        authority: &[u8],
        receipt: Option<&Prepared>,
    ) -> Result<bool> {
        let tx = self.conn.transaction().map_err(db)?;
        let projected = Self::project_on(&tx, group, native_id, envelope, now, authority, receipt)?;
        tx.commit().map_err(db)?;
        Ok(projected)
    }
    fn project_on(
        conn: &Connection,
        group: &[u8],
        native_id: &[u8],
        envelope: &Envelope,
        now: u64,
        authority: &[u8],
        receipt: Option<&Prepared>,
    ) -> Result<bool> {
        let bytes = envelope.encode()?;
        let digest = crate::foundation::payload_digest(&bytes);
        let existing: Option<String> = conn
            .query_row(
                "SELECT digest FROM incoming WHERE group_id=? AND sender=? AND application_id=?",
                params![group, envelope.sender(), envelope.id()],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if let Some(existing) = existing {
            if existing != digest {
                return Err(Error::Invalid("application_id_conflict"));
            }
            Self::bind_incoming_source(conn, group, native_id, envelope)?;
            return Ok(false);
        }
        if let Some(receipt) = receipt {
            let Body::Message(data) = envelope.body() else {
                return Err(Error::Invalid("receipt_loop"));
            };
            if receipt.group != group
                || receipt.envelope.session() != envelope.session()
                || receipt.envelope.sender() != envelope.recipient()
                || receipt.envelope.recipient() != envelope.sender()
                || receipt.envelope.id() != crate::foundation::receipt_id(group, envelope)?
                || !matches!(receipt.envelope.body(), Body::Receipt{message_id,payload_sha256} if message_id==envelope.id() && *payload_sha256==crate::foundation::payload_digest(data))
            {
                return Err(Error::Invalid("receipt_binding"));
            }
        }
        Self::check_fresh(conn, envelope.id(), now)?;
        Self::capacity(conn, "incoming", group)?;
        Self::source_capacity(conn, "incoming_sources", group, native_id)?;
        let tx = conn;
        if let Body::Receipt {
            message_id,
            payload_sha256,
        } = envelope.body()
        {
            let raw: Option<(Option<Vec<u8>>, String)> = tx
                .query_row(
                    "SELECT envelope,state FROM outgoing WHERE group_id=? AND application_id=?",
                    params![group, message_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db)?;
            let (raw, state) = raw.ok_or(Error::Invalid("unknown_receipt"))?;
            if matches!(state.as_str(), "invalidated" | "expired") {
                return Err(Error::Invalid("receipt_binding"));
            }
            let raw = raw.ok_or(Error::Invalid("receipt_expired"))?;
            let original = Envelope::decode(&raw)?;
            let Body::Message(data) = original.body() else {
                return Err(Error::Invalid("receipt_loop"));
            };
            if original.session() != envelope.session()
                || original.sender() != envelope.recipient()
                || original.recipient() != envelope.sender()
                || crate::foundation::payload_digest(data) != *payload_sha256
                || matches!(state.as_str(), "invalidated" | "expired")
            {
                return Err(Error::Invalid("receipt_binding"));
            }
            tx.execute(
                "UPDATE outgoing SET state='received' WHERE group_id=? AND application_id=?",
                params![group, message_id],
            )
            .map_err(db)?;
        }
        tx.execute("INSERT INTO incoming(group_id,native_id,sender,application_id,envelope,created,digest,body_type,authority,receipt_for) VALUES(?,?,?,?,?,?,?,?,?,?)", params![group,native_id,envelope.sender(),envelope.id(),bytes,i64::try_from(now).map_err(|_|Error::Invalid("clock"))?,digest,if matches!(envelope.body(),Body::Message(_)){"message"}else{"receipt"},authority,match envelope.body(){Body::Receipt{message_id,..}=>Some(message_id.as_str()),_=>None}]).map_err(db)?;
        tx.execute("INSERT INTO incoming_sources(group_id,native_id,sender,application_id) VALUES(?,?,?,?)",params![group,native_id,envelope.sender(),envelope.id()]).map_err(db)?;
        if let Some(receipt) = receipt {
            Self::prepare_on(conn, receipt, now)?;
        }
        Ok(true)
    }
    pub(crate) fn invalidate(&mut self, group: &[u8], native_id: &[u8]) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        tx.execute("UPDATE outgoing SET state='invalidated' WHERE group_id=? AND application_id IN (SELECT i.receipt_for FROM incoming i JOIN incoming_sources s ON i.group_id=s.group_id AND i.sender=s.sender AND i.application_id=s.application_id WHERE s.group_id=? AND s.native_id=? AND i.receipt_for IS NOT NULL)",params![group,group,native_id]).map_err(db)?;
        tx.execute("UPDATE outgoing SET state='invalidated' WHERE group_id=? AND event_id IN (SELECT event_id FROM outgoing_sources WHERE group_id=? AND native_id=?)",params![group,group,native_id]).map_err(db)?;
        tx.execute("UPDATE incoming SET invalidated=1 WHERE group_id=? AND (sender,application_id) IN (SELECT sender,application_id FROM incoming_sources WHERE group_id=? AND native_id=?)",params![group,group,native_id]).map_err(db)?;
        tx.commit().map_err(db)
    }
    pub(crate) fn status_counts(&self) -> Result<(usize, usize, usize)> {
        let (conversations, received, pending): (u32,u32,u32) = self.conn.query_row("SELECT (SELECT count(*) FROM conversations),(SELECT count(*) FROM incoming WHERE body_type='message' AND envelope IS NOT NULL),(SELECT count(*) FROM outgoing WHERE state IN ('unsubmitted','prepared','uncertain') AND envelope IS NOT NULL)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db)?;
        Ok((conversations as usize, received as usize, pending as usize))
    }
    pub(crate) fn received(&self) -> Result<Vec<ReceivedMessage>> {
        self.received_page(None, 64)
    }
    pub(crate) fn received_page(
        &self,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Vec<ReceivedMessage>> {
        if limit == 0 || limit > 64 || after.is_some_and(|n| n > i64::MAX as u64) {
            return Err(Error::Invalid("received_page"));
        }
        let mut stmt=self.conn.prepare("SELECT cursor,envelope,invalidated FROM incoming WHERE cursor>? AND body_type='message' AND envelope IS NOT NULL ORDER BY cursor LIMIT ?").map_err(db)?;
        let rows = stmt
            .query_map(params![after.unwrap_or(0) as i64, limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })
            .map_err(db)?;
        rows.map(|r| {
            let (cursor, bytes, invalidated) = r.map_err(db)?;
            let e = Envelope::decode(&bytes)?;
            let Body::Message(data) = e.body() else {
                return Err(Error::Storage("message_type"));
            };
            Ok(ReceivedMessage {
                cursor,
                session: e.session().into(),
                application_id: e.id().into(),
                sender: e.sender().into(),
                data: data.clone(),
                invalidated,
            })
        })
        .collect()
    }
    fn capacity(conn: &Connection, table: &str, group: &[u8]) -> Result<()> {
        let sql = match table {
            "incoming" => {
                "SELECT (SELECT count(*) FROM incoming),(SELECT count(*) FROM incoming WHERE group_id=?)"
            }
            "outgoing" => {
                "SELECT (SELECT count(*) FROM outgoing),(SELECT count(*) FROM outgoing WHERE group_id=?)"
            }
            _ => return Err(Error::Storage("capacity_table")),
        };
        let (global, room): (i64, i64) = conn
            .query_row(sql, [group], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db)?;
        if global >= 4096 || room >= 512 {
            return Err(Error::Storage("journal_capacity"));
        }
        Ok(())
    }
    fn check_fresh(conn: &Connection, id: &str, now: u64) -> Result<()> {
        let floor = conn
            .query_row(
                "SELECT floor FROM replay_state WHERE singleton=1",
                [],
                |r| sql_u64(r, 0),
            )
            .map_err(db)?;
        match crate::foundation::classify_new_id(id, now, floor)? {
            crate::foundation::Freshness::Eligible => Ok(()),
            crate::foundation::Freshness::Expired => Err(Error::Invalid("foundation_expired")),
            crate::foundation::Freshness::Deferred => Err(Error::Transport("foundation_future")),
            crate::foundation::Freshness::ExcessiveFuture => {
                Err(Error::Invalid("foundation_future_excessive"))
            }
        }
    }
    pub(crate) fn bind_authority(&self, group: &[u8], digest: &str) -> Result<()> {
        let old: Option<String> = self
            .conn
            .query_row("SELECT digest FROM trust WHERE group_id=?", [group], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db)?;
        match old {
            Some(v) if v != digest => Err(Error::Mdk("authority_changed")),
            Some(_) => Ok(()),
            None => {
                self.conn
                    .execute(
                        "INSERT INTO trust(group_id,digest) VALUES(?,?)",
                        params![group, digest],
                    )
                    .map_err(db)?;
                Ok(())
            }
        }
    }
    fn source_capacity(conn: &Connection, table: &str, group: &[u8], native: &[u8]) -> Result<()> {
        let(count,exists):(i64,bool)=match table{
            "incoming_sources"=>conn.query_row("SELECT (SELECT count(*) FROM incoming_sources),EXISTS(SELECT 1 FROM incoming_sources WHERE group_id=? AND native_id=?)",params![group,native],|r|Ok((r.get(0)?,r.get(1)?))),
            "queued_bindings"=>conn.query_row("SELECT (SELECT count(*) FROM queued_bindings),EXISTS(SELECT 1 FROM queued_bindings WHERE group_id=? AND intent_id=?)",params![group,native],|r|Ok((r.get(0)?,r.get(1)?))),
            "outgoing_sources"=>conn.query_row("SELECT (SELECT count(*) FROM outgoing_sources),EXISTS(SELECT 1 FROM outgoing_sources WHERE group_id=? AND native_id=?)",params![group,native],|r|Ok((r.get(0)?,r.get(1)?))),
            _=>return Err(Error::Storage("source_table"))
        }.map_err(db)?;
        let room: i64 = match table {
            "incoming_sources" => conn.query_row(
                "SELECT count(*) FROM incoming_sources WHERE group_id=?",
                [group],
                |r| r.get(0),
            ),
            "outgoing_sources" => conn.query_row(
                "SELECT count(*) FROM outgoing_sources WHERE group_id=?",
                [group],
                |r| r.get(0),
            ),
            "queued_bindings" => conn.query_row(
                "SELECT count(*) FROM queued_bindings WHERE group_id=?",
                [group],
                |r| r.get(0),
            ),
            _ => return Err(Error::Storage("source_table")),
        }
        .map_err(db)?;
        if (count >= 16384 || room >= 2048) && !exists {
            return Err(Error::Storage("source_capacity"));
        }
        Ok(())
    }
    fn bind_incoming_source(
        conn: &Connection,
        group: &[u8],
        native: &[u8],
        envelope: &Envelope,
    ) -> Result<()> {
        Self::source_capacity(conn, "incoming_sources", group, native)?;
        let prior:Option<(String,String)>=conn.query_row("SELECT sender,application_id FROM incoming_sources WHERE group_id=? AND native_id=?",params![group,native],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
        if let Some((sender, id)) = prior {
            if sender != envelope.sender() || id != envelope.id() {
                return Err(Error::Storage("native_id_conflict"));
            }
            return Ok(());
        }
        conn.execute(
            "INSERT INTO incoming_sources VALUES(?,?,?,?)",
            params![group, native, envelope.sender(), envelope.id()],
        )
        .map_err(db)?;
        Ok(())
    }
    pub(crate) fn bind_queued(&self, group: &[u8], intent: &[u8], event: &str) -> Result<()> {
        Self::source_capacity(&self.conn, "queued_bindings", group, intent)?;
        let old: Option<String> = self
            .conn
            .query_row(
                "SELECT event_id FROM queued_bindings WHERE group_id=? AND intent_id=?",
                params![group, intent],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if old.as_deref().is_some_and(|v| v != event) {
            return Err(Error::Storage("queued_binding_conflict"));
        }
        self.conn
            .execute(
                "INSERT OR IGNORE INTO queued_bindings VALUES(?,?,?)",
                params![group, intent, event],
            )
            .map_err(db)?;
        Ok(())
    }
    pub(crate) fn refuse_queued(&mut self, group: &[u8], intent: &[u8]) -> Result<()> {
        self.conn.execute("UPDATE outgoing SET state='invalidated' WHERE group_id=? AND event_id IN (SELECT event_id FROM queued_bindings WHERE group_id=? AND intent_id=?)",params![group,group,intent]).map_err(db)?;
        Ok(())
    }
    pub(crate) fn bind_native(&self, group: &[u8], event: &str, native: &[u8]) -> Result<()> {
        Self::source_capacity(&self.conn, "outgoing_sources", group, native)?;
        let prior: Option<String> = self
            .conn
            .query_row(
                "SELECT event_id FROM outgoing_sources WHERE group_id=? AND native_id=?",
                params![group, native],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if prior.is_some_and(|v| v != event) {
            return Err(Error::Storage("native_id_conflict"));
        }
        self.conn
            .execute(
                "INSERT OR IGNORE INTO outgoing_sources VALUES(?,?,?)",
                params![group, native, event],
            )
            .map_err(db)?;
        Ok(())
    }
    pub(crate) fn reject(&self, _group: &[u8], _native: &[u8]) -> Result<()> {
        // Malformed application content is re-evaluated deterministically;
        // native processing evidence is retained by MDK until group deletion.
        Ok(())
    }
    pub(crate) fn native_bindings(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut stmt=self.conn.prepare("SELECT s.group_id,s.native_id FROM incoming_sources s JOIN incoming i ON s.group_id=i.group_id AND s.sender=i.sender AND s.application_id=i.application_id WHERE i.invalidated=0 AND i.envelope IS NOT NULL UNION SELECT s.group_id,s.native_id FROM outgoing_sources s JOIN outgoing o ON s.group_id=o.group_id AND s.event_id=o.event_id WHERE o.state NOT IN ('expired','invalidated')").map_err(db)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db)
    }
    #[cfg(test)]
    pub(crate) fn prune(&mut self, now: u64, retention: u64) -> Result<()> {
        self.maintain(now, retention, &BTreeSet::new()).map(|_| ())
    }
    pub(crate) fn maintain(
        &mut self,
        now: u64,
        retention: u64,
        protected_groups: &BTreeSet<Vec<u8>>,
    ) -> Result<u64> {
        if now > 0xffffffffffff {
            return Err(Error::Invalid("clock"));
        }
        let floor =
            i64::try_from(now.saturating_sub(7 * 86400)).map_err(|_| Error::Invalid("clock"))?;
        let cutoff =
            i64::try_from(now.saturating_sub(retention)).map_err(|_| Error::Invalid("clock"))?;
        let tx = self.conn.transaction().map_err(db)?;
        tx.execute(
            "UPDATE replay_state SET floor=MAX(floor,?) WHERE singleton=1",
            [floor],
        )
        .map_err(db)?;
        let floor: u64 = tx
            .query_row(
                "SELECT floor FROM replay_state WHERE singleton=1",
                [],
                |r| sql_u64(r, 0),
            )
            .map_err(db)?;
        let floor_id = format!("{floor:012x}");
        // Expiry changes payload availability independently of replay metadata.
        tx.execute(
            "UPDATE incoming SET envelope=NULL,authority=NULL WHERE created<?",
            [cutoff],
        )
        .map_err(db)?;
        // Native frozen fanouts own retransmission bytes. Application plaintext
        // expires even while their durable reconciliation metadata is protected.
        tx.execute("UPDATE outgoing SET envelope=NULL,encoded=NULL,state=CASE WHEN state='unsubmitted' THEN 'expired' ELSE state END WHERE created<?",[cutoff]).map_err(db)?;
        let mut groups = BTreeSet::new();
        {
            let mut q=tx.prepare("SELECT group_id FROM outgoing UNION SELECT group_id FROM incoming UNION SELECT group_id FROM conversations UNION SELECT group_id FROM rejected_groups").map_err(db)?;
            for row in q.query_map([], |r| r.get::<_, Vec<u8>>(0)).map_err(db)? {
                groups.insert(row.map_err(db)?);
            }
        }
        for group in groups {
            let protected = protected_groups.contains(&group);
            // A never-submitted stale intent cannot be emitted after downtime.
            tx.execute("UPDATE outgoing SET state='expired',envelope=NULL,encoded=NULL WHERE group_id=? AND state='unsubmitted' AND substr(application_id,1,12)<?",params![group,floor_id]).map_err(db)?;
            if !protected {
                tx.execute(
                    "UPDATE outgoing SET state='uncertain' WHERE group_id=? AND state='prepared'",
                    [&group],
                )
                .map_err(db)?;
                tx.execute("UPDATE outgoing SET envelope=NULL,encoded=NULL,state=CASE WHEN state='invalidated' THEN state ELSE 'expired' END WHERE group_id=? AND created<?",params![group,cutoff]).map_err(db)?;
                tx.execute("DELETE FROM outgoing_sources WHERE group_id=?1 AND event_id IN (SELECT event_id FROM outgoing WHERE group_id=?1 AND created<?2 AND substr(application_id,1,12)<?3)",params![group,cutoff,floor_id]).map_err(db)?;
                tx.execute("DELETE FROM queued_bindings WHERE group_id=?1 AND event_id IN (SELECT event_id FROM outgoing WHERE group_id=?1 AND created<?2 AND substr(application_id,1,12)<?3)",params![group,cutoff,floor_id]).map_err(db)?;
                tx.execute("DELETE FROM outgoing WHERE group_id=? AND created<? AND substr(application_id,1,12)<?",params![group,cutoff,floor_id]).map_err(db)?;
            }
            // Retained incoming source mappings track late invalidations only
            // while their parent application record remains in the horizon.
            tx.execute("DELETE FROM incoming_sources WHERE group_id=?1 AND (sender,application_id) IN (SELECT sender,application_id FROM incoming WHERE group_id=?1 AND created<?2 AND substr(application_id,1,12)<?3)",params![group,cutoff,floor_id]).map_err(db)?;
            tx.execute("DELETE FROM incoming WHERE group_id=? AND created<? AND substr(application_id,1,12)<?",params![group,cutoff,floor_id]).map_err(db)?;
            if protected {
                continue;
            }
            let retired:Option<(String,i64,bool)>=tx.query_row("SELECT session,rejected_at,cleanup_complete FROM conversations WHERE group_id=? AND revoked=1",[&group],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db)?;
            if let Some((session, at, complete)) = retired {
                let stamp = crate::foundation::id_timestamp(&session)?;
                let parents_remain:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM incoming WHERE group_id=?1) OR EXISTS(SELECT 1 FROM outgoing WHERE group_id=?1)",[&group],|r|r.get(0)).map_err(db)?;
                if complete
                    && !parents_remain
                    && at >= 0
                    && at < cutoff
                    && stamp.max(at as u64).saturating_add(300) < floor
                {
                    // Retirement removes all group-scoped authority and source
                    // metadata together; upstream forgetting was proven first.
                    for table in [
                        "incoming_sources",
                        "outgoing_sources",
                        "queued_bindings",
                        "incoming",
                        "outgoing",
                        "trust",
                        "conversations",
                    ] {
                        tx.execute(&format!("DELETE FROM {table} WHERE group_id=?"), [&group])
                            .map_err(db)?;
                    }
                }
            }
            let rejected:Option<(Option<u64>,u64,bool)>=tx.query_row("SELECT session_timestamp,rejected_at,cleanup_complete FROM rejected_groups WHERE group_id=?",[&group],|r|Ok((sql_optional_u64(r,0)?,sql_u64(r,1)?,r.get(2)?))).optional().map_err(db)?;
            if let Some((stamp, at, complete)) = rejected
                && complete
                && at < cutoff as u64
                && stamp.unwrap_or(0).max(at).saturating_add(300) < floor
            {
                tx.execute("DELETE FROM rejected_groups WHERE group_id=?", [&group])
                    .map_err(db)?;
            }
        }
        tx.commit().map_err(db)?;
        Ok(floor)
    }
    pub(crate) fn pending(&self) -> Result<Vec<Prepared>> {
        self.pending_state(false)
    }
    pub(crate) fn unsubmitted(&self) -> Result<Vec<Prepared>> {
        self.pending_state(true)
    }
    fn pending_state(&self, unsubmitted: bool) -> Result<Vec<Prepared>> {
        let sql = if unsubmitted {
            "SELECT group_id,envelope,event_id,encoded FROM outgoing WHERE state='unsubmitted' AND envelope IS NOT NULL ORDER BY rowid"
        } else {
            "SELECT group_id,envelope,event_id,encoded FROM outgoing WHERE state IN ('prepared','uncertain') AND envelope IS NOT NULL ORDER BY rowid"
        };
        let mut stmt = self.conn.prepare(sql).map_err(db)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            })
            .map_err(db)?;
        rows.map(|row| {
            let (group, envelope, event_id, encoded) = row.map_err(db)?;
            Ok(Prepared {
                group,
                envelope: Envelope::decode(&envelope)?,
                event_id,
                encoded,
            })
        })
        .collect()
    }
}

#[cfg(test)]
pub(crate) fn private_test_directory() -> tempfile::TempDir {
    private_test_directory_in(&std::env::temp_dir())
}

#[cfg(test)]
pub(crate) fn private_test_directory_in(parent: &Path) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    // Resolve the existing allocation parent, never a database leaf.
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(parent.canonicalize().unwrap())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Journal) {
        let temp = private_test_directory();
        let j = Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)).unwrap();
        (temp, j)
    }
    #[test]
    fn symlink_ancestors_and_immediate_parent_are_rejected() {
        use std::os::unix::fs::{DirBuilderExt, symlink};

        let (temp, journal) = setup();
        drop(journal);
        let real = temp.path().join("real");
        let child = real.join("child");
        fs::DirBuilder::new().mode(0o700).create(&real).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&child).unwrap();
        let alias = temp.path().join("alias");
        let alias2 = temp.path().join("alias2");
        symlink(&real, &alias).unwrap();
        symlink(&child, &alias2).unwrap();

        let through_ancestor = alias.join("child/journal.db");
        assert!(matches!(
            Journal::open(&through_ancestor, &[7; 32], &"ab".repeat(32)),
            Err(Error::Storage("journal"))
        ));
        let path = child.join("journal.db");
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.len(), 0);
        assert_eq!(metadata.mode() & 0o777, 0o600);
        let journal = Journal::open(&path, &[7; 32], &"ab".repeat(32)).unwrap();
        drop(journal);
        let journal = Journal::open(&path, &[7; 32], &"ab".repeat(32)).unwrap();
        drop(journal);
        assert!(matches!(
            Journal::open(&alias2.join("journal.db"), &[7; 32], &"ab".repeat(32)),
            Err(Error::Storage("journal_path"))
        ));
    }
    #[test]
    fn canonical_test_parent_preserves_database_symlink_refusal() {
        use std::os::unix::fs::{DirBuilderExt, symlink};

        let outer = private_test_directory();
        let real = outer.path().join("real");
        fs::DirBuilder::new().mode(0o700).create(&real).unwrap();
        let alias = outer.path().join("alias");
        symlink(&real, &alias).unwrap();
        let temp = private_test_directory_in(&alias);
        let path = temp.path().to_path_buf();
        assert_eq!(path.parent(), Some(real.as_path()));
        assert_eq!(path, path.canonicalize().unwrap());
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(metadata.mode() & 0o777, 0o700);
        let database = path.join("journal.db");
        drop(Journal::open(&database, &[7; 32], &"ab".repeat(32)).unwrap());
        drop(Journal::open(&database, &[7; 32], &"ab".repeat(32)).unwrap());
        let linked_database = path.join("linked.db");
        symlink(&database, &linked_database).unwrap();
        assert!(matches!(
            Journal::open(&linked_database, &[7; 32], &"ab".repeat(32)),
            Err(Error::Storage("journal_path"))
        ));
        drop(temp);
        assert!(!path.exists());
    }
    fn message() -> Envelope {
        Envelope::message(
            format!("{:012x}{}", 1, "01".repeat(10)),
            format!("{:012x}{}", 1, "02".repeat(10)),
            "03".repeat(32),
            "04".repeat(32),
            b"private\0\xff".to_vec(),
        )
        .unwrap()
    }
    #[test]
    fn encrypted_reopen_wrong_key_and_identity() {
        let (temp, j) = setup();
        let p = temp.path().join("journal.db");
        drop(j);
        assert!(!fs::read(&p).unwrap().starts_with(b"SQLite format 3"));
        assert!(Journal::open(&p, &[8; 32], &"ab".repeat(32)).is_err());
        assert!(Journal::open(&p, &[7; 32], &"cd".repeat(32)).is_err());
        assert!(Journal::open(&p, &[7; 32], &"ab".repeat(32)).is_ok());
    }
    #[test]
    fn durable_projection_dedup_and_conflict() {
        let (temp, mut j) = setup();
        let msg = message();
        assert!(j.project(b"group", b"native", &msg, 1).unwrap());
        drop(j);
        let mut j =
            Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)).unwrap();
        assert!(!j.project(b"group", b"native", &msg, 1).unwrap());
        let changed = Envelope::message(
            msg.session().into(),
            msg.id().into(),
            msg.sender().into(),
            msg.recipient().into(),
            b"different".to_vec(),
        )
        .unwrap();
        assert!(j.project(b"group", b"other", &changed, 1).is_err());
        assert_eq!(j.received().unwrap().len(), 1);
        j.invalidate(b"group", b"native").unwrap();
        assert!(j.received().unwrap()[0].invalidated);
    }
    #[test]
    fn receipt_transaction_and_invalidation() {
        let (_temp, mut j) = setup();
        let msg = message();
        j.prepare_send(
            &Prepared {
                group: b"g".to_vec(),
                envelope: msg.clone(),
                event_id: "event".into(),
                encoded: b"encoded".to_vec(),
            },
            1,
        )
        .unwrap();
        let receipt = Envelope::receipt(
            msg.session().into(),
            format!("{:012x}{}", 1, "05".repeat(10)),
            msg.recipient().into(),
            msg.sender().into(),
            msg.id().into(),
            crate::foundation::payload_digest(b"private\0\xff"),
        )
        .unwrap();
        assert!(j.project(b"other", b"r", &receipt, 2).is_err());
        j.project(b"g", b"r", &receipt, 2).unwrap();
        assert!(!j.project(b"g", b"r-duplicate", &receipt, 3).unwrap());
        j.mark(b"g", "event", "published").unwrap();
        assert_eq!(
            j.outgoing(b"g", msg.id()).unwrap().unwrap().state,
            "received"
        );
        j.invalidate(b"g", b"r-duplicate").unwrap();
        assert_eq!(
            j.outgoing(b"g", msg.id()).unwrap().unwrap().state,
            "invalidated"
        );
    }
    #[test]
    fn expiry_keeps_tombstones_and_stable_cursor() {
        let (_temp, mut j) = setup();
        let first = message();
        j.project(b"g", b"first", &first, 1).unwrap();
        let cursor = j.received_page(None, 1).unwrap()[0].cursor;
        let second = Envelope::message(
            first.session().into(),
            format!("{:012x}{}", 1, "06".repeat(10)),
            first.sender().into(),
            first.recipient().into(),
            b"second".to_vec(),
        )
        .unwrap();
        j.project(b"g", b"second", &second, 100).unwrap();
        j.prepare_send(
            &Prepared {
                group: b"g".to_vec(),
                envelope: first.clone(),
                event_id: "e".into(),
                encoded: b"e".to_vec(),
            },
            1,
        )
        .unwrap();
        j.prune(102, 10).unwrap();
        let page = j.received_page(Some(cursor), 1).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].data, b"second");
        assert!(page[0].cursor > cursor);
        assert!(!j.project(b"g", b"repeat", &first, 102).unwrap());
        assert_eq!(j.received().unwrap().len(), 1);
        assert_eq!(
            j.verify_outgoing(b"g", &first).unwrap().unwrap().state,
            "expired"
        );
        let changed = Envelope::message(
            first.session().into(),
            first.id().into(),
            first.sender().into(),
            first.recipient().into(),
            b"changed".to_vec(),
        )
        .unwrap();
        assert!(j.verify_outgoing(b"g", &changed).is_err());
        assert!(j.project(b"g", b"repeat", &changed, 102).is_err());
        assert!(j.received_page(None, 0).is_err());
        assert!(j.received_page(None, 65).is_err());
        assert!(j.received_page(Some(u64::MAX), 1).is_err());
    }
    #[test]
    fn outgoing_and_expired_receipt_invalidation_stays_terminal() {
        let (_temp, mut j) = setup();
        let msg = message();
        j.prepare_send(
            &Prepared {
                group: b"g".to_vec(),
                envelope: msg.clone(),
                event_id: "e".into(),
                encoded: b"encoded".to_vec(),
            },
            1,
        )
        .unwrap();
        j.bind_native(b"g", "e", b"out").unwrap();
        j.mark(b"g", "e", "published").unwrap();
        j.invalidate(b"g", b"out").unwrap();
        assert_eq!(
            j.outgoing(b"g", msg.id()).unwrap().unwrap().state,
            "invalidated"
        );
        j.mark(b"g", "e", "published").unwrap();
        assert_eq!(
            j.outgoing(b"g", msg.id()).unwrap().unwrap().state,
            "invalidated"
        );
        // A receipt's source binding survives content expiry.
        let second = Envelope::message(
            msg.session().into(),
            format!("{:012x}{}", 1, "06".repeat(10)),
            msg.sender().into(),
            msg.recipient().into(),
            b"data".to_vec(),
        )
        .unwrap();
        j.prepare_send(
            &Prepared {
                group: b"g".to_vec(),
                envelope: second.clone(),
                event_id: "e2".into(),
                encoded: b"encoded".to_vec(),
            },
            1,
        )
        .unwrap();
        let receipt = Envelope::receipt(
            second.session().into(),
            format!("{:012x}{}", 1, "07".repeat(10)),
            second.recipient().into(),
            second.sender().into(),
            second.id().into(),
            crate::foundation::payload_digest(b"data"),
        )
        .unwrap();
        j.project(b"g", b"r", &receipt, 2).unwrap();
        j.prune(100, 1).unwrap();
        j.invalidate(b"g", b"r").unwrap();
        assert_eq!(
            j.outgoing(b"g", second.id()).unwrap().unwrap().state,
            "invalidated"
        );
    }
    #[test]
    fn duplicate_native_sources_remain_bound_after_restart_and_expiry() {
        let (temp, mut j) = setup();
        let msg = message();
        assert!(j.project(b"g", b"original", &msg, 1).unwrap());
        assert!(!j.project(b"g", b"duplicate", &msg, 2).unwrap());
        drop(j);
        let mut j =
            Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)).unwrap();
        j.invalidate(b"g", b"duplicate").unwrap();
        assert!(j.received().unwrap()[0].invalidated);
        j.prepare_send(
            &Prepared {
                group: b"g".to_vec(),
                envelope: msg.clone(),
                event_id: "e".into(),
                encoded: b"encoded".to_vec(),
            },
            1,
        )
        .unwrap();
        j.bind_native(b"g", "e", b"first-out").unwrap();
        j.bind_native(b"g", "e", b"second-out").unwrap();
        j.prune(100, 1).unwrap();
        j.invalidate(b"g", b"first-out").unwrap();
        assert_eq!(
            j.outgoing(b"g", msg.id()).unwrap().unwrap().state,
            "invalidated"
        );
        let receipt = Envelope::receipt(
            msg.session().into(),
            format!("{:012x}{}", 1, "05".repeat(10)),
            msg.recipient().into(),
            msg.sender().into(),
            msg.id().into(),
            crate::foundation::payload_digest(b"private\0\xff"),
        )
        .unwrap();
        // A valid receipt cannot resurrect an invalidated original.
        assert!(matches!(
            j.project(b"g", b"r-original", &receipt, 101),
            Err(Error::Invalid("receipt_binding"))
        ));
        assert_eq!(
            j.outgoing(b"g", msg.id()).unwrap().unwrap().state,
            "invalidated"
        );
    }
    #[test]
    fn rejected_group_tombstone_cannot_claim_or_revoke_an_existing_session() {
        let (_temp, mut j) = setup();
        let session = format!("{:012x}{}", 1, "01".repeat(10));
        let peer = "02".repeat(32);
        j.accept_conversation(b"good", &session, &peer).unwrap();
        assert!(matches!(
            j.accept_conversation(b"copy", &session, &peer),
            Err(Error::Mdk("conversation_conflict"))
        ));
        j.reject_group(b"untrusted").unwrap();
        assert!(!j.conversation(&session).unwrap().unwrap().0.revoked);
        assert!(
            j.accept_conversation(b"untrusted", &session, &peer)
                .is_err()
        );
        assert!(
            j.accept_conversation(b"untrusted", &"03".repeat(16), &peer)
                .is_err()
        );
        j.accept_conversation(b"good", &session, &peer).unwrap();
        j.revoke(b"good", &session, &peer).unwrap();
        assert!(j.accept_conversation(b"good", &session, &peer).is_err());
    }
    #[test]
    fn authority_is_immutable_and_journal_capacity_backpressures() {
        let (_temp, mut j) = setup();
        j.bind_authority(b"g", "a").unwrap();
        j.bind_authority(b"g", "a").unwrap();
        assert!(j.bind_authority(b"g", "b").is_err());
        j.conn.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4096) INSERT INTO incoming(group_id,native_id,sender,application_id,envelope,digest,body_type,created) SELECT X'01',X'02','sender',CAST(x AS TEXT),NULL,'digest','message',0 FROM n;").unwrap();
        assert!(matches!(
            j.project(b"g", b"new", &message(), 1),
            Err(Error::Storage("journal_capacity"))
        ));
    }
    #[test]
    fn never_submitted_and_ambiguous_intents_remain_distinct_across_restart() {
        let (temp, mut j) = setup();
        let msg = message();
        let prepared = Prepared {
            group: b"g".to_vec(),
            envelope: msg.clone(),
            event_id: "event".into(),
            encoded: b"exact-original-bytes".to_vec(),
        };
        j.prepare_send(&prepared, 1).unwrap();
        assert!(j.pending().unwrap().is_empty());
        assert_eq!(j.status_counts().unwrap(), (0, 0, 1));
        assert_eq!(j.unsubmitted().unwrap()[0].encoded, prepared.encoded);
        drop(j);
        let mut j =
            Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)).unwrap();
        assert_eq!(j.unsubmitted().unwrap().len(), 1);
        j.mark(b"g", "event", "prepared").unwrap();
        drop(j);
        let mut j =
            Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)).unwrap();
        assert!(j.unsubmitted().unwrap().is_empty());
        assert_eq!(j.pending().unwrap().len(), 1);
        j.mark(b"g", "event", "uncertain").unwrap();
        assert_eq!(j.pending().unwrap()[0].encoded, prepared.encoded);
        assert!(j.unsubmitted().unwrap().is_empty());
        // Re-projecting the same durable intent never erases its ambiguity.
        j.prepare_send(&prepared, 2).unwrap();
        assert_eq!(
            j.outgoing(b"g", msg.id()).unwrap().unwrap().state,
            "uncertain"
        );
    }
    #[test]
    fn queued_refusal_invalidates_only_its_durable_application_binding() {
        let (temp, mut j) = setup();
        let first = message();
        let second = Envelope::message(
            first.session().into(),
            format!("{:012x}{}", 1, "06".repeat(10)),
            first.sender().into(),
            first.recipient().into(),
            b"second".to_vec(),
        )
        .unwrap();
        for (envelope, id) in [(first.clone(), "first"), (second.clone(), "second")] {
            j.prepare_send(
                &Prepared {
                    group: b"g".to_vec(),
                    envelope,
                    event_id: id.into(),
                    encoded: id.as_bytes().to_vec(),
                },
                1,
            )
            .unwrap();
            j.mark(b"g", id, "prepared").unwrap();
        }
        j.bind_queued(b"g", b"queue", "first").unwrap();
        assert!(j.bind_queued(b"g", b"queue", "second").is_err());
        drop(j);
        let mut j =
            Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)).unwrap();
        j.refuse_queued(b"other", b"queue").unwrap();
        assert_eq!(
            j.outgoing(b"g", first.id()).unwrap().unwrap().state,
            "prepared"
        );
        j.refuse_queued(b"g", b"queue").unwrap();
        assert_eq!(
            j.outgoing(b"g", first.id()).unwrap().unwrap().state,
            "invalidated"
        );
        assert_eq!(
            j.outgoing(b"g", second.id()).unwrap().unwrap().state,
            "prepared"
        );
    }
    fn timed_message(at: u64, nonce: u64) -> Envelope {
        Envelope::message(
            format!("{at:012x}{:020x}", 0),
            format!("{at:012x}{nonce:020x}"),
            "03".repeat(32),
            "04".repeat(32),
            b"bytes\0\xff".to_vec(),
        )
        .unwrap()
    }
    fn prepared(group: &[u8], envelope: Envelope, event: &str) -> Prepared {
        Prepared {
            group: group.to_vec(),
            envelope,
            event_id: event.into(),
            encoded: b"native-ciphertext".to_vec(),
        }
    }
    fn count(j: &Journal, table: &str) -> i64 {
        j.conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn replay_floor_is_durable_monotone_and_gc_requires_both_clocks() {
        let (temp, mut j) = setup();
        let old = timed_message(100, 1);
        j.project(b"g", b"n", &old, 100).unwrap();
        // Seven-day horizon elapsed, but a longer TTL retains its binding.
        assert_eq!(j.maintain(700000, 900000, &BTreeSet::new()).unwrap(), 95200);
        assert_eq!(count(&j, "incoming"), 1);
        assert!(!j.project(b"g", b"again", &old, 700000).unwrap());
        j.maintain(1000000, 10, &BTreeSet::new()).unwrap();
        assert_eq!(count(&j, "incoming"), 0);
        assert_eq!(count(&j, "incoming_sources"), 0);
        assert!(matches!(
            j.project(b"g", b"reencrypted", &old, 1000000),
            Err(Error::Invalid("foundation_expired"))
        ));
        let floor = j.replay_floor().unwrap();
        drop(j);
        let mut j =
            Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)).unwrap();
        assert_eq!(j.maintain(100, 1, &BTreeSet::new()).unwrap(), floor);
        assert!(matches!(
            j.project(b"g", b"rollback", &old, 100),
            Err(Error::Invalid("foundation_expired"))
        ));
    }
    #[test]
    fn exact_floor_survives_and_future_deferred_has_no_rows() {
        let (_temp, mut j) = setup();
        j.maintain(700000, 1, &BTreeSet::new()).unwrap();
        let floor = j.replay_floor().unwrap();
        let exact = timed_message(floor, 1);
        j.project(b"g", b"floor", &exact, 700000).unwrap();
        j.maintain(700000, 1, &BTreeSet::new()).unwrap();
        assert_eq!(count(&j, "incoming"), 1);
        assert!(matches!(
            j.project(b"g", b"future", &timed_message(700301, 2), 700000),
            Err(Error::Transport("foundation_future"))
        ));
        assert!(matches!(
            j.project(b"g", b"excess", &timed_message(1400000, 3), 700000),
            Err(Error::Invalid("foundation_future_excessive"))
        ));
        assert_eq!(count(&j, "incoming"), 1);
    }
    #[test]
    fn receipt_capacity_failure_rolls_back_projection_and_sources() {
        let (_temp, mut j) = setup();
        for n in 1..=512 {
            j.prepare_send(&prepared(b"g", timed_message(10, n), &n.to_string()), 10)
                .unwrap();
        }
        let msg = timed_message(10, 999);
        let receipt = Envelope::receipt(
            msg.session().into(),
            crate::foundation::receipt_id(b"g", &msg).unwrap(),
            msg.recipient().into(),
            msg.sender().into(),
            msg.id().into(),
            crate::foundation::payload_digest(b"bytes\0\xff"),
        )
        .unwrap();
        let receipt = prepared(b"g", receipt, "receipt");
        assert!(matches!(
            j.project_with_receipt(b"g", b"n", &msg, 10, b"authority", Some(&receipt)),
            Err(Error::Storage("journal_capacity"))
        ));
        assert_eq!(count(&j, "incoming"), 0);
        assert_eq!(count(&j, "incoming_sources"), 0);
        j.maintain(700000, 1, &BTreeSet::new()).unwrap();
        assert_eq!(count(&j, "outgoing"), 0);
        let fresh = timed_message(700000, 1);
        let receipt = Envelope::receipt(
            fresh.session().into(),
            crate::foundation::receipt_id(b"g", &fresh).unwrap(),
            fresh.recipient().into(),
            fresh.sender().into(),
            fresh.id().into(),
            crate::foundation::payload_digest(b"bytes\0\xff"),
        )
        .unwrap();
        let receipt = prepared(b"g", receipt, "receipt-new");
        assert!(
            j.project_with_receipt(b"g", b"fresh", &fresh, 700000, b"authority", Some(&receipt))
                .unwrap()
        );
        assert!(
            !j.project_with_receipt(
                b"g",
                b"duplicate",
                &fresh,
                700001,
                b"authority",
                Some(&receipt)
            )
            .unwrap()
        );
        assert_eq!(count(&j, "incoming"), 1);
        assert_eq!(count(&j, "outgoing"), 1);
        assert_eq!(count(&j, "incoming_sources"), 2);
    }
    #[test]
    fn native_obligations_protect_rows_and_bindings_until_resolved() {
        let (_temp, mut j) = setup();
        let msg = timed_message(10, 1);
        j.prepare_send(&prepared(b"g", msg.clone(), "e"), 10)
            .unwrap();
        j.mark(b"g", "e", "prepared").unwrap();
        j.bind_native(b"g", "e", b"n").unwrap();
        j.bind_queued(b"g", b"q", "e").unwrap();
        let protected = BTreeSet::from([b"g".to_vec()]);
        j.maintain(700000, 1, &protected).unwrap();
        assert_eq!(count(&j, "outgoing"), 1);
        assert_eq!(count(&j, "outgoing_sources"), 1);
        assert_eq!(count(&j, "queued_bindings"), 1);
        let payload_absent: bool = j
            .conn
            .query_row(
                "SELECT envelope IS NULL AND encoded IS NULL FROM outgoing",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(payload_absent);
        assert_eq!(
            j.outgoing(b"g", msg.id()).unwrap().unwrap().state,
            "prepared"
        );
        j.maintain(700000, 1, &BTreeSet::new()).unwrap();
        for table in ["outgoing", "outgoing_sources", "queued_bindings"] {
            assert_eq!(count(&j, table), 0);
        }
    }
    #[test]
    fn stale_unsubmitted_expires_and_unknown_prepared_becomes_uncertain() {
        let (_temp, mut j) = setup();
        let old = timed_message(10, 1);
        j.prepare_send(&prepared(b"g", old.clone(), "old"), 10)
            .unwrap();
        let recent = timed_message(699999, 2);
        j.prepare_send(&prepared(b"g", recent.clone(), "recent"), 699999)
            .unwrap();
        j.mark(b"g", "recent", "prepared").unwrap();
        j.maintain(700000, 900000, &BTreeSet::new()).unwrap();
        assert_eq!(
            j.outgoing(b"g", old.id()).unwrap().unwrap().state,
            "expired"
        );
        assert_eq!(
            j.outgoing(b"g", recent.id()).unwrap().unwrap().state,
            "uncertain"
        );
        assert!(j.unsubmitted().unwrap().is_empty());
        assert_eq!(j.pending().unwrap().len(), 1);
    }
    #[test]
    fn rejected_metadata_needs_cleanup_and_retires_authority_atomically() {
        let (_temp, mut j) = setup();
        let session = timed_message(10, 1).session().to_owned();
        j.accept_conversation(b"g", &session, &"aa".repeat(32))
            .unwrap();
        j.bind_authority(b"g", "authority").unwrap();
        j.reject_group_at(b"g", Some(10), 20).unwrap();
        assert_eq!(j.pending_group_cleanup().unwrap(), vec![b"g".to_vec()]);
        j.maintain(700000, 1, &BTreeSet::new()).unwrap();
        assert!(j.group_rejected(b"g").unwrap());
        j.mark_group_cleanup_complete(b"g").unwrap();
        j.maintain(700000, 1, &BTreeSet::new()).unwrap();
        assert!(j.conversation(&session).unwrap().is_none());
        assert_eq!(count(&j, "trust"), 0);
        j.reject_group_at(b"other", Some(700000), 700000).unwrap();
        j.mark_group_cleanup_complete(b"other").unwrap();
        j.maintain(700001, 1, &BTreeSet::new()).unwrap();
        assert!(j.group_rejected(b"other").unwrap());
        j.maintain(1400000, 1, &BTreeSet::new()).unwrap();
        assert!(!j.group_rejected(b"other").unwrap());
    }
    #[test]
    fn admission_overflow_frontier_is_monotone_and_not_future_poisoned() {
        let (temp, mut j) = setup();
        j.conn.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4096) INSERT INTO rejected_groups(group_id,rejected_at) SELECT CAST(x AS BLOB),0 FROM n;").unwrap();
        j.reject_group_at(b"overflow", Some(100), 100).unwrap();
        assert_eq!(j.admission_reject_through().unwrap(), Some(100));
        j.reject_group_at(b"older", Some(99), 100).unwrap();
        assert_eq!(j.admission_reject_through().unwrap(), Some(100));
        assert!(j.reject_group_at(b"future", Some(401), 100).is_err());
        assert_eq!(j.admission_reject_through().unwrap(), Some(100));
        assert!(j.reject_group_at(b"unknown", None, 100).is_err());
        drop(j);
        let j = Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)).unwrap();
        assert_eq!(j.admission_reject_through().unwrap(), Some(100));
    }
    #[test]
    fn revoked_rooms_do_not_consume_live_room_slots() {
        let (_temp, mut j) = setup();
        for n in 1u64..=64 {
            let session = format!("{:012x}{n:020x}", 10);
            let group = n.to_be_bytes();
            j.accept_conversation(&group, &session, &"aa".repeat(32))
                .unwrap();
            j.reject_group_at(&group, Some(10), 10).unwrap();
        }
        j.accept_conversation(b"new", &format!("{:012x}{:020x}", 11, 1), &"aa".repeat(32))
            .unwrap();
        assert_eq!(count(&j, "conversations"), 65);
    }
    #[test]
    fn global_and_room_quotas_recover_after_gc_without_lifetime_stall() {
        let (_temp, mut j) = setup();
        for cycle in 0..2 {
            let at = 10 + cycle * 700000;
            // Eight rooms reach the global ceiling without exceeding any room's quota.
            j.conn.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4096) INSERT INTO incoming(group_id,native_id,sender,application_id,envelope,digest,body_type,created) SELECT CAST((x-1)/512 AS BLOB),CAST(x AS BLOB),'sender',printf('%012x%020x',?1,x),NULL,'digest','message',?1 FROM n",[at]).unwrap();
            let msg = timed_message(at as u64, 5000);
            assert!(matches!(
                j.project(b"extra", b"n", &msg, at as u64),
                Err(Error::Storage("journal_capacity"))
            ));
            j.maintain(at as u64 + 700000, 1, &BTreeSet::new()).unwrap();
            assert_eq!(count(&j, "incoming"), 0);
        }
        let now = 1400010;
        j.project(b"fresh", b"n", &timed_message(now, 1), now)
            .unwrap();
        assert_eq!(count(&j, "incoming"), 1);
    }
    #[test]
    fn room_retirement_waits_for_long_payload_ttl_and_future_session_bound() {
        let (_temp, mut j) = setup();
        let msg = timed_message(10, 1);
        j.accept_conversation(b"g", msg.session(), msg.sender())
            .unwrap();
        j.project(b"g", b"n", &msg, 10).unwrap();
        j.bind_authority(b"g", "authority").unwrap();
        j.reject_group_at(b"g", Some(10), 20).unwrap();
        j.mark_group_cleanup_complete(b"g").unwrap();
        j.maintain(700000, 900000, &BTreeSet::new()).unwrap();
        assert_eq!(count(&j, "conversations"), 1);
        assert_eq!(count(&j, "trust"), 1);
        assert_eq!(count(&j, "incoming"), 1);
        j.maintain(1000000, 900000, &BTreeSet::new()).unwrap();
        for table in ["conversations", "trust", "incoming", "incoming_sources"] {
            assert_eq!(count(&j, table), 0);
        }
    }
    #[test]
    fn old_schema_is_rejected_without_migration() {
        let (temp, j) = setup();
        j.conn.execute_batch("PRAGMA user_version=2").unwrap();
        drop(j);
        assert!(matches!(
            Journal::open(&temp.path().join("journal.db"), &[7; 32], &"ab".repeat(32)),
            Err(Error::Storage("journal_schema"))
        ));
    }
    #[test]
    fn total_room_metadata_cap_preserves_existing_cleanup_authority() {
        let (_temp, mut j) = setup();
        j.conn.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4096) INSERT INTO conversations(session,group_id,peer,revoked,rejected_at) SELECT printf('%012x%020x',10,x),CAST(x AS BLOB),'peer',1,20 FROM n;").unwrap();
        assert!(matches!(
            j.accept_conversation(b"extra", timed_message(30, 1).session(), "peer"),
            Err(Error::Mdk("conversation_capacity"))
        ));
        j.reject_group_at(b"1", Some(10), 30).unwrap();
        j.mark_group_cleanup_complete(b"1").unwrap();
        let complete: bool = j
            .conn
            .query_row(
                "SELECT cleanup_complete FROM conversations WHERE group_id=CAST('1' AS BLOB)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(complete);
        assert_eq!(count(&j, "conversations"), 4096);
    }
    #[test]
    fn duplicate_after_payload_expiry_never_recreates_receipt_or_changes_binding() {
        let (_temp, mut j) = setup();
        let msg = timed_message(10, 1);
        let receipt = Envelope::receipt(
            msg.session().into(),
            crate::foundation::receipt_id(b"g", &msg).unwrap(),
            msg.recipient().into(),
            msg.sender().into(),
            msg.id().into(),
            crate::foundation::payload_digest(b"bytes\0\xff"),
        )
        .unwrap();
        let receipt = prepared(b"g", receipt, "r");
        assert!(
            j.project_with_receipt(b"g", b"n", &msg, 10, b"authority", Some(&receipt))
                .unwrap()
        );
        j.maintain(100, 1, &BTreeSet::new()).unwrap();
        assert!(j.received().unwrap().is_empty());
        assert!(
            !j.project_with_receipt(b"g", b"new-native", &msg, 100, b"authority", Some(&receipt))
                .unwrap()
        );
        assert!(j.unsubmitted().unwrap().is_empty());
        assert_eq!(count(&j, "outgoing"), 1);
        let changed = Envelope::message(
            msg.session().into(),
            msg.id().into(),
            msg.sender().into(),
            msg.recipient().into(),
            b"changed".to_vec(),
        )
        .unwrap();
        assert!(matches!(
            j.project_with_receipt(b"g", b"bad", &changed, 100, b"authority", Some(&receipt)),
            Err(Error::Invalid("application_id_conflict"))
        ));
        assert_eq!(count(&j, "incoming_sources"), 2);
        assert!(j.received().unwrap().is_empty());
    }
}
