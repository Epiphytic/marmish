//! Shared explicit-decline cooldown for the checked retirement inventories.

use rusqlite::Connection;

use super::*;

pub(in super::super) fn under_cooldown(
    conn: &Connection,
    server: [u8; 32],
    wall: u64,
) -> std::result::Result<bool, CursorError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_retirement_cooldown WHERE server=?1 AND until>?2)",
        params![server, seconds(wall)?],
        |row| row.get(0),
    )?)
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
pub(super) fn install(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE peer_retirement_cooldown (
    server BLOB NOT NULL PRIMARY KEY CHECK (length(server)=32),
    until INTEGER NOT NULL CHECK (until>=86400)
) STRICT;",
    )?;
    Writers::Cooldown.guard(conn, true)
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
#[derive(Clone, Copy)]
enum Writers {
    Cooldown,
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
impl Writers {
    fn guard(self, conn: &Connection, install: bool) -> Result<()> {
        let table = match self {
            Self::Cooldown => "peer_retirement_cooldown",
        };
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
        Ok(())
    }
}

#[cfg(test)]
pub(super) const DAY: u64 = 86_400;
#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
pub(super) const HISTORY_LIMIT: i64 = 512;

#[cfg(test)]
pub(super) fn decline(tx: &Transaction<'_>, server: [u8; 32], wall: u64) -> Result<()> {
    let writer = Writers::Cooldown;
    writer.guard(tx, false)?;
    tx.execute(
        "DELETE FROM peer_retirement_cooldown WHERE until<=?1",
        [seconds(wall)?],
    )?;
    let count: i64 = tx.query_row("SELECT count(*) FROM peer_retirement_cooldown", [], |row| {
        row.get(0)
    })?;
    if count >= HISTORY_LIMIT {
        return Err(CursorError::Backlog.into());
    }
    let until = wall.checked_add(DAY).ok_or(CursorError::Range)?;
    rows::require_write(tx.execute(
        "INSERT INTO peer_retirement_cooldown VALUES (?1,?2)",
        params![server, seconds(until)?],
    )?)?;
    writer.guard(tx, true)
}
