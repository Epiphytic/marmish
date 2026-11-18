//! Exact, export-incapable fixture attachment and checked writer guards.

use rusqlite::{Connection, TransactionBehavior};
use std::sync::OnceLock;

use super::super::super::super::{HardenedPeerConnection, check_hardening, stored_origins};
use super::super::{OUTBOX_TRIGGER, SLOT_TABLE, stage_schema};
use super::*;

const RETIREMENT_CLAIM_MARKER: &str = "retirement epoch 1: never exported";
const EPOCH: &str = "
CREATE TABLE peer_retirement_epoch (
    epoch INTEGER PRIMARY KEY CHECK (epoch=1),
    native_export INTEGER NOT NULL CHECK (native_export=0)
) STRICT;
INSERT INTO peer_retirement_epoch VALUES (1,0);
CREATE TRIGGER peer_retirement_epoch_insert BEFORE INSERT ON peer_retirement_epoch
BEGIN SELECT RAISE(ABORT, 'immutable retirement epoch'); END;
CREATE TRIGGER peer_retirement_epoch_update BEFORE UPDATE ON peer_retirement_epoch
BEGIN SELECT RAISE(ABORT, 'immutable retirement epoch'); END;
CREATE TRIGGER peer_retirement_epoch_delete BEFORE DELETE ON peer_retirement_epoch
BEGIN SELECT RAISE(ABORT, 'immutable retirement epoch'); END;
CREATE TABLE peer_retirement_intent (
    class INTEGER NOT NULL CHECK (class IN (0,1)),
    generation INTEGER NOT NULL CHECK (generation>0),
    server BLOB NOT NULL CHECK (length(server)=32),
    offer_id BLOB NOT NULL CHECK (length(offer_id)=16),
    revision BLOB NOT NULL CHECK (length(revision)=32),
    origin BLOB NOT NULL CHECK (length(origin)=32),
    action INTEGER NOT NULL CHECK (action IN (0,1)),
    step INTEGER NOT NULL CHECK (step IN (0,1,2,3)),
    started INTEGER NOT NULL CHECK (started>=0),
    witness BLOB CHECK (length(witness) IN (32,48)),
    receipt_until INTEGER,
    CHECK ((step IN (0,1) AND witness IS NOT NULL AND receipt_until IS NULL)
        OR (step=2 AND witness IS NULL AND receipt_until IS NULL)
        OR (step=3 AND witness IS NULL AND receipt_until>started)),
    PRIMARY KEY (class,generation)
) STRICT;
CREATE UNIQUE INDEX peer_retirement_active_class ON peer_retirement_intent(class)
WHERE step<3;
CREATE TABLE peer_retirement_cooldown (
    server BLOB NOT NULL PRIMARY KEY CHECK (length(server)=32),
    until INTEGER NOT NULL CHECK (until>=86400)
) STRICT;
CREATE TRIGGER peer_retirement_freeze_stage BEFORE UPDATE ON peer_provisional_stage_v1
WHEN EXISTS (
    SELECT 1 FROM peer_retirement_intent r
    WHERE r.class=OLD.class AND r.generation=OLD.generation AND r.step<3
)
BEGIN SELECT RAISE(ABORT, 'stage is retiring'); END;
";

#[derive(Clone, Copy)]
pub(super) enum Writers {
    Intent,
    Cooldown,
    Release,
}

const INTENT: &str = "peer_retirement_intent";
const COOLDOWN: &str = "peer_retirement_cooldown";

fn guarded_tables(writers: Writers) -> &'static [&'static str] {
    match writers {
        Writers::Intent | Writers::Release => &[INTENT],
        Writers::Cooldown => &[COOLDOWN],
    }
}

fn write_guards(conn: &Connection, tables: &[&str], install: bool) -> Result<()> {
    for table in tables {
        for verb in ["INSERT", "UPDATE", "DELETE"] {
            let name = format!("{table}_{verb}");
            let sql = if install {
                format!(
                    "CREATE TRIGGER {name} BEFORE {verb} ON {table}
                    BEGIN SELECT RAISE(ABORT,'retirement owner required'); END"
                )
            } else {
                format!("DROP TRIGGER {name}")
            };
            conn.execute_batch(&sql)?;
        }
    }
    Ok(())
}

pub(super) fn guard(conn: &Connection, writers: Writers, install: bool) -> Result<()> {
    write_guards(conn, guarded_tables(writers), install)?;
    if matches!(writers, Writers::Release) {
        let sql = if install {
            stage_schema::DELETE_STAGE
        } else {
            "DROP TRIGGER peer_stage_delete_guard"
        };
        conn.execute_batch(sql)?;
    }
    Ok(())
}

fn install_epoch(conn: &Connection) -> Result<()> {
    conn.execute_batch("DROP TRIGGER peer_provisioning_reject_claim")?;
    conn.execute_batch(
        &stage_schema::STAGE_CLAIM_TRIGGER.replace("provisional stage v1", RETIREMENT_CLAIM_MARKER),
    )?;
    conn.execute_batch(EPOCH)?;
    write_guards(conn, &[INTENT, COOLDOWN], true)
}

pub(in super::super) type Entry = (String, String, String, Option<String>);

pub(in super::super) fn inventory(conn: &Connection) -> Result<Vec<Entry>> {
    let temp: i64 = conn.query_row("SELECT count(*) FROM sqlite_temp_schema", [], |r| r.get(0))?;
    if temp != 0 {
        return Err(CursorError::Binding.into());
    }
    let mut query = conn.prepare(
        "SELECT type,name,tbl_name,sql FROM main.sqlite_schema ORDER BY type,name,tbl_name",
    )?;
    let rows = query.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(in super::super) fn expected(epoch: bool) -> Result<&'static [Entry]> {
    static STAGE: OnceLock<Vec<Entry>> = OnceLock::new();
    static RETIREMENT: OnceLock<Vec<Entry>> = OnceLock::new();
    let cache = if epoch { &RETIREMENT } else { &STAGE };
    if let Some(entries) = cache.get() {
        return Ok(entries);
    }
    let entries = expected_from_creation_ddl(epoch)?;
    Ok(cache.get_or_init(|| entries))
}

fn expected_from_creation_ddl(epoch: bool) -> Result<Vec<Entry>> {
    let conn = Connection::open_in_memory()?;
    for schema in [
        super::super::super::super::SCHEMA,
        rows::SCHEMA,
        SLOT_TABLE,
        OUTBOX_TRIGGER,
        stage_schema::STAGE_CLAIM_TRIGGER,
    ] {
        conn.execute_batch(schema)?;
    }
    for (_, sql) in stage_schema::SCHEMA {
        conn.execute_batch(sql)?;
    }
    if epoch {
        install_epoch(&conn)?;
    }
    inventory(&conn)
}

pub(crate) fn check_epoch(conn: &Connection) -> Result<()> {
    if inventory(conn)? != expected(true)? {
        return Err(CursorError::Binding.into());
    }
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM peer_retirement_epoch WHERE epoch=1 AND native_export=0",
        [],
        |r| r.get(0),
    )?;
    if count != 1 {
        return Err(CursorError::Binding.into());
    }
    Ok(())
}

pub(super) fn open(
    HardenedPeerConnection(mut conn): HardenedPeerConnection,
    account: [u8; 32],
    mut origins: Vec<Origin>,
) -> Result<CursorStore> {
    if origins.is_empty() || origins.len() > super::super::super::super::MAX_ORIGINS {
        return Err(CursorError::Binding.into());
    }
    origins.sort_unstable();
    check_hardening(&conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let actual = inventory(&tx)?;
    let migrate = actual == expected(false)?;
    if !migrate && actual != expected(true)? {
        return Err(CursorError::Binding.into());
    }
    for statement in [
        "SELECT account FROM peer_cursor_version WHERE singleton=1 AND version=1",
        "SELECT account FROM peer_consent_version WHERE singleton=1 AND version=2",
    ] {
        let stored: [u8; 32] = tx.query_row(statement, [], |r| r.get(0))?;
        if stored != account {
            return Err(CursorError::Binding.into());
        }
    }
    let slots: i64 = tx.query_row(
        "SELECT count(*) FROM peer_provisioning_slot WHERE class IN (0,1)",
        [],
        |r| r.get(0),
    )?;
    if slots != 2 || stored_origins(&tx)? != origins {
        return Err(CursorError::Binding.into());
    }
    if migrate {
        install_epoch(&tx)?;
    }
    check_epoch(&tx)?;
    tx.commit()?;
    Ok(CursorStore { conn, origins })
}
