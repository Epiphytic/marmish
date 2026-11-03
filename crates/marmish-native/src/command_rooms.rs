//! Actor-owned command-room bindings on the existing encrypted journal connection.
//! Native membership and purpose verification remain the adapter's responsibility.
use super::{Journal, command_store, db, sql_u64};
use crate::command_wire::{Account, Digest, Id, id_timestamp};
use crate::mdk_adapter::CommandDirection;
use crate::{Error, Result};
use rusqlite::{OptionalExtension, params};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CommandRoom {
    pub group: Vec<u8>,
    pub transport: Vec<u8>,
    pub session: Id,
    pub peer: Account,
    pub direction: CommandDirection,
    pub authority: Digest,
    pub revoked: bool,
}
fn row_room(row: &rusqlite::Row<'_>) -> rusqlite::Result<CommandRoom> {
    fn array<const N: usize>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<[u8; N]> {
        row.get::<_, Vec<u8>>(index)?
            .try_into()
            .map_err(|_| rusqlite::Error::InvalidQuery)
    }
    let direction = match row.get::<_, i64>(4)? {
        0 => CommandDirection::Client,
        1 => CommandDirection::Server,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(CommandRoom {
        group: row.get(0)?,
        transport: row.get(1)?,
        session: array(row, 2)?,
        peer: array(row, 3)?,
        direction,
        authority: array(row, 5)?,
        revoked: row.get(6)?,
    })
}
impl Journal {
    pub(crate) fn command_rooms_init(&mut self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS command_rooms(
            group_id BLOB PRIMARY KEY,transport BLOB NOT NULL UNIQUE,session BLOB NOT NULL UNIQUE,
            peer BLOB NOT NULL,direction INTEGER NOT NULL CHECK(direction IN (0,1)),
            authority BLOB NOT NULL,revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)));
            CREATE TABLE IF NOT EXISTS command_engine_binding(singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            version INTEGER NOT NULL,root BLOB NOT NULL,key_id BLOB NOT NULL);").map_err(db)
    }
    pub(crate) fn command_room(&self, group: &[u8]) -> Result<Option<CommandRoom>> {
        self.conn.query_row("SELECT group_id,transport,session,peer,direction,authority,revoked FROM command_rooms WHERE group_id=?1",[group],row_room).optional().map_err(db)
    }
    /// Active rooms only; revoked metadata is retained until a proven safe GC policy.
    pub(crate) fn command_rooms(&self) -> Result<Vec<CommandRoom>> {
        let mut statement = self.conn.prepare("SELECT group_id,transport,session,peer,direction,authority,revoked FROM command_rooms WHERE revoked=0 ORDER BY group_id LIMIT 65").map_err(db)?;
        let rows = statement
            .query_map([], row_room)
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        if rows.len() > 64 {
            return Err(Error::Storage("command_room_capacity"));
        }
        Ok(rows)
    }
    pub(crate) fn command_accept_room(&mut self, room: &CommandRoom) -> Result<()> {
        if room.revoked
            || room.group.is_empty()
            || room.group.len() > 1024
            || room.transport.is_empty()
            || room.transport.len() > 1024
        {
            return Err(Error::Mdk("command_room_binding"));
        }
        if let Some(existing) = self.command_room(&room.group)? {
            return if existing == *room {
                Ok(())
            } else {
                Err(Error::Mdk("command_room_conflict"))
            };
        }
        // Existing bindings survive advancement of the replay floor. New room
        // identities cannot resurrect a retired session.
        if id_timestamp(room.session) < self.command_floor()? {
            return Err(Error::Mdk("command_room_expired"));
        }
        let tx = self.conn.transaction().map_err(db)?;
        let total: u64 = tx
            .query_row("SELECT count(*) FROM command_rooms", [], |r| sql_u64(r, 0))
            .map_err(db)?;
        if total >= 4096 {
            return Err(Error::Mdk("command_room_lifetime_exhausted"));
        }
        let conflict:bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM conversations WHERE group_id=?1 OR session=?2) OR EXISTS(SELECT 1 FROM command_rooms WHERE transport=?3 OR session=?4)",params![room.group,hex::encode(room.session),room.transport,room.session.as_slice()],|row|row.get(0)).map_err(db)?;
        if conflict {
            return Err(Error::Mdk("command_room_conflict"));
        }
        let count: u64 = tx
            .query_row(
                "SELECT count(*) FROM command_rooms WHERE revoked=0",
                [],
                |row| sql_u64(row, 0),
            )
            .map_err(db)?;
        if count >= 64 {
            return Err(Error::Mdk("command_room_capacity"));
        }
        let direction = match room.direction {
            CommandDirection::Client => 0,
            CommandDirection::Server => 1,
        };
        tx.execute("INSERT INTO command_rooms(group_id,transport,session,peer,direction,authority) VALUES(?1,?2,?3,?4,?5,?6)",params![room.group,room.transport,room.session.as_slice(),room.peer.as_slice(),direction,room.authority.as_slice()]).map_err(db)?;
        tx.commit().map_err(db)
    }
    pub(crate) fn command_revoke_room(&mut self, group: &[u8]) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        if tx
            .execute(
                "UPDATE command_rooms SET revoked=1 WHERE group_id=?1",
                [group],
            )
            .map_err(db)?
            == 0
        {
            return Err(Error::Mdk("command_room_unknown"));
        }
        command_store::revoke_group_in(&tx, group)?;
        tx.commit().map_err(db)
    }
    /// Persist the non-secret derivation identity before opening the native engine.
    pub(crate) fn command_bind_engine(
        &mut self,
        version: u32,
        root: &Digest,
        key: &Digest,
    ) -> Result<()> {
        if version != 1 {
            return Err(Error::Invalid("command_engine_version"));
        }
        let tx = self.conn.transaction().map_err(db)?;
        let existing: Option<(u32, Vec<u8>, Vec<u8>)> = tx
            .query_row(
                "SELECT version,root,key_id FROM command_engine_binding WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(db)?;
        match existing {
            Some((v, r, k)) if v == version && r == root && k == key => {}
            Some(_) => return Err(Error::Storage("command_engine_binding")),
            None => {
                tx.execute(
                    "INSERT INTO command_engine_binding VALUES(1,?1,?2,?3)",
                    params![version, root.as_slice(), key.as_slice()],
                )
                .map_err(db)?;
            }
        }
        tx.commit().map_err(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn setup() -> (tempfile::TempDir, Journal) {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let journal =
            Journal::open(&root.path().join("journal"), &[4; 32], &"01".repeat(32)).unwrap();
        (root, journal)
    }
    fn room(n: u8) -> CommandRoom {
        CommandRoom {
            group: vec![n; 32],
            transport: vec![n; 32],
            session: [n; 16],
            peer: [2; 32],
            direction: CommandDirection::Server,
            authority: [3; 32],
            revoked: false,
        }
    }
    #[test]
    fn immutable_bindings_duplicates_and_cross_purpose_are_refused() {
        let (_root, mut j) = setup();
        let first = room(1);
        j.command_accept_room(&first).unwrap();
        j.command_accept_room(&first).unwrap();
        for changed in 0..5 {
            let mut next = first.clone();
            match changed {
                0 => next.direction = CommandDirection::Client,
                1 => next.authority = [4; 32],
                2 => next.peer = [5; 32],
                3 => next.transport = vec![9; 32],
                _ => next.session = [9; 16],
            };
            assert!(matches!(
                j.command_accept_room(&next),
                Err(Error::Mdk("command_room_conflict"))
            ));
        }
        for collision in 0..2 {
            let mut next = room(2);
            if collision == 0 {
                next.transport = first.transport.clone()
            } else {
                next.session = first.session
            };
            assert!(j.command_accept_room(&next).is_err());
        }
        j.accept_conversation(&[3; 32], &hex::encode([3; 16]), &"02".repeat(32))
            .unwrap();
        assert!(j.command_accept_room(&room(3)).is_err());
        let mut session_collision = room(4);
        session_collision.session = [3; 16];
        assert!(j.command_accept_room(&session_collision).is_err());
        assert_eq!(j.command_rooms().unwrap().len(), 1);
    }
    #[test]
    fn live_capacity_and_irreversible_revocation_survive_reopen() {
        let (root, mut j) = setup();
        for n in 1..=64 {
            j.command_accept_room(&room(n)).unwrap();
        }
        assert!(matches!(
            j.command_accept_room(&room(65)),
            Err(Error::Mdk("command_room_capacity"))
        ));
        j.command_revoke_room(&room(1).group).unwrap();
        j.command_revoke_room(&room(1).group).unwrap();
        assert!(j.command_room(&room(1).group).unwrap().unwrap().revoked);
        assert!(j.command_accept_room(&room(1)).is_err());
        j.command_accept_room(&room(65)).unwrap();
        drop(j);
        let j = Journal::open(&root.path().join("journal"), &[4; 32], &"01".repeat(32)).unwrap();
        assert_eq!(j.command_rooms().unwrap().len(), 64);
        assert!(j.command_room(&room(1).group).unwrap().unwrap().revoked);
    }
    #[test]
    fn revocation_rolls_back_if_ledger_invalidation_cannot_commit() {
        let (_root, mut j) = setup();
        j.command_accept_room(&room(1)).unwrap();
        j.conn.execute("DROP TABLE command_requests", []).unwrap();
        assert!(j.command_revoke_room(&room(1).group).is_err());
        assert!(!j.command_room(&room(1).group).unwrap().unwrap().revoked);
    }
    #[test]
    fn lifetime_cap_is_terminal_even_with_no_active_rooms_and_survives_reopen() {
        let (root, mut j) = setup();
        let tx = j.conn.transaction().unwrap();
        for n in 0u64..4096 {
            let bytes = n.to_be_bytes();
            let mut session = [0; 16];
            session[8..].copy_from_slice(&bytes);
            tx.execute("INSERT INTO command_rooms(group_id,transport,session,peer,direction,authority,revoked) VALUES(?1,?1,?2,?3,1,?3,1)", params![bytes.as_slice(),session.as_slice(),[1u8;32].as_slice()]).unwrap();
        }
        tx.commit().unwrap();
        assert!(j.command_rooms().unwrap().is_empty());
        assert!(matches!(
            j.command_accept_room(&room(1)),
            Err(Error::Mdk("command_room_lifetime_exhausted"))
        ));
        drop(j);
        let mut j =
            Journal::open(&root.path().join("journal"), &[4; 32], &"01".repeat(32)).unwrap();
        assert!(matches!(
            j.command_accept_room(&room(2)),
            Err(Error::Mdk("command_room_lifetime_exhausted"))
        ));
    }
    #[test]
    fn advancing_floor_rejects_new_sessions_but_preserves_existing_binding() {
        let (_root, mut j) = setup();
        let existing = room(1);
        j.command_accept_room(&existing).unwrap();
        j.conn
            .execute(
                "UPDATE command_replay_state SET floor=?1",
                [i64::try_from(id_timestamp(room(2).session) + 1).unwrap()],
            )
            .unwrap();
        j.command_accept_room(&existing).unwrap();
        assert!(matches!(
            j.command_accept_room(&room(2)),
            Err(Error::Mdk("command_room_expired"))
        ));
        assert_eq!(j.command_rooms().unwrap().len(), 1);
    }
    #[test]
    fn engine_derivation_reference_is_immutable() {
        let (root, mut j) = setup();
        j.command_bind_engine(1, &[1; 32], &[2; 32]).unwrap();
        j.command_bind_engine(1, &[1; 32], &[2; 32]).unwrap();
        assert!(j.command_bind_engine(2, &[1; 32], &[2; 32]).is_err());
        assert!(j.command_bind_engine(1, &[3; 32], &[2; 32]).is_err());
        assert!(j.command_bind_engine(1, &[1; 32], &[3; 32]).is_err());
        drop(j);
        let mut j =
            Journal::open(&root.path().join("journal"), &[4; 32], &"01".repeat(32)).unwrap();
        j.command_bind_engine(1, &[1; 32], &[2; 32]).unwrap();
        assert!(j.command_bind_engine(1, &[3; 32], &[2; 32]).is_err());
    }
}
