//! Durable one-time creation fences. A marker is never room or execution authority.
use super::{Journal, command_store, db, sql_u64};
use crate::command_wire::{Account, Digest, Id};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandPreparation {
    pub session: Id,
    pub peer: Account,
    pub relays_sha256: Digest,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PreparationStart {
    Created(CommandPreparation),
    Existing(CommandPreparation),
}
fn read_preparation(row: &rusqlite::Row<'_>) -> rusqlite::Result<CommandPreparation> {
    fn array<const N: usize>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<[u8; N]> {
        row.get::<_, Vec<u8>>(index)?
            .try_into()
            .map_err(|_| rusqlite::Error::InvalidQuery)
    }
    Ok(CommandPreparation {
        session: array(row, 0)?,
        peer: array(row, 1)?,
        relays_sha256: array(row, 2)?,
    })
}
fn lookup(
    conn: &Connection,
    token: &Id,
    peer: &Account,
    digest: &Digest,
) -> Result<Option<CommandPreparation>> {
    let record = conn
        .query_row(
            "SELECT session,peer,relays_sha256 FROM command_preparations WHERE token=?1",
            [token.as_slice()],
            read_preparation,
        )
        .optional()
        .map_err(db)?;
    if record
        .as_ref()
        .is_some_and(|r| &r.peer != peer || &r.relays_sha256 != digest)
    {
        return Err(Error::Invalid("command_preparation_binding"));
    }
    Ok(record)
}
impl Journal {
    pub(crate) fn command_preparations_init(&mut self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS command_preparations(
            token BLOB PRIMARY KEY NOT NULL CHECK(length(token)=16),
            peer BLOB NOT NULL CHECK(length(peer)=32),
            relays_sha256 BLOB NOT NULL CHECK(length(relays_sha256)=32),
            session BLOB NOT NULL UNIQUE CHECK(length(session)=16));",
            )
            .map_err(db)
    }
    pub(crate) fn command_preparation(
        &self,
        token: &Id,
        peer: &Account,
        digest: &Digest,
    ) -> Result<Option<CommandPreparation>> {
        lookup(&self.conn, token, peer, digest)
    }
    pub(crate) fn command_preparation_session(
        &self,
        session: Id,
    ) -> Result<Option<CommandPreparation>> {
        self.conn
            .query_row(
                "SELECT session,peer,relays_sha256 FROM command_preparations WHERE session=?1",
                [session.as_slice()],
                read_preparation,
            )
            .optional()
            .map_err(db)
    }
    /// Only Created authorizes one native creation attempt; even a crash before
    /// that call consumes the fence. Exact retries return the original binding.
    pub(crate) fn command_begin_preparation(
        &mut self,
        token: Id,
        peer: Account,
        relays_sha256: Digest,
        session: Id,
        now: u64,
    ) -> Result<PreparationStart> {
        let tx = self.conn.transaction().map_err(db)?;
        if let Some(record) = lookup(&tx, &token, &peer, &relays_sha256)? {
            return Ok(PreparationStart::Existing(record));
        }
        command_store::freshness(&tx, token, now)?;
        command_store::freshness(&tx, session, now)?;
        let (active, total): (u64, u64) = tx
            .query_row(
                "SELECT count(CASE WHEN revoked=0 THEN 1 END),count(*) FROM command_rooms",
                [],
                |r| Ok((sql_u64(r, 0)?, sql_u64(r, 1)?)),
            )
            .map_err(db)?;
        if active >= 64 || total >= 4096 {
            return Err(Error::Invalid("command_room_capacity"));
        }
        let count: u64 = tx
            .query_row("SELECT count(*) FROM command_preparations", [], |r| {
                sql_u64(r, 0)
            })
            .map_err(db)?;
        if count >= 4096 {
            return Err(Error::Invalid("command_preparation_capacity"));
        }
        let conflict: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM command_preparations WHERE session=?1) OR EXISTS(SELECT 1 FROM command_rooms WHERE session=?1)",
                [session.as_slice()],
                |r| r.get(0),
            )
            .map_err(db)?;
        if conflict {
            return Err(Error::Invalid("command_preparation_binding"));
        }
        tx.execute("INSERT INTO command_preparations(token,peer,relays_sha256,session) VALUES(?1,?2,?3,?4)",
            params![token.as_slice(), peer.as_slice(), relays_sha256.as_slice(), session.as_slice()]).map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(PreparationStart::Created(CommandPreparation {
            session,
            peer,
            relays_sha256,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: u64 = 1_000_000;
    const PEER: Account = [2; 32];
    const RELAYS: Digest = [3; 32];
    fn id(n: u32, time: u64) -> Id {
        let mut id = [0; 16];
        id[..6].copy_from_slice(&time.to_be_bytes()[2..]);
        id[12..].copy_from_slice(&n.to_be_bytes());
        id
    }
    fn open(root: &std::path::Path) -> Journal {
        let mut j = Journal::open(&root.join("journal"), &[4; 32], &"01".repeat(32)).unwrap();
        j.command_preparations_init().unwrap();
        j
    }
    fn begin(j: &mut Journal, n: u32) -> Result<PreparationStart> {
        j.command_begin_preparation(id(n, NOW), PEER, RELAYS, id(n + 10000, NOW), NOW)
    }
    #[test]
    fn creation_fence_survives_reopen_and_new_admission_checks() {
        let root = super::super::private_test_directory();
        let mut j = open(root.path());
        let expected = CommandPreparation {
            session: id(10001, NOW),
            peer: PEER,
            relays_sha256: RELAYS,
        };
        assert_eq!(
            begin(&mut j, 1).unwrap(),
            PreparationStart::Created(expected.clone())
        );
        j.conn
            .execute(
                "UPDATE command_replay_state SET floor=?1 WHERE id=1",
                [(NOW + 1) as i64],
            )
            .unwrap();
        drop(j);
        let mut j = open(root.path());
        assert_eq!(
            j.command_begin_preparation(id(1, NOW), PEER, RELAYS, id(99, 0), 0)
                .unwrap(),
            PreparationStart::Existing(expected.clone())
        );
        assert_eq!(
            j.command_preparation_session(expected.session).unwrap(),
            Some(expected)
        );
        assert!(matches!(
            begin(&mut j, 2),
            Err(Error::Invalid("command_clock_behind_floor"))
        ));
        assert!(
            j.command_preparation(&id(2, NOW), &PEER, &RELAYS)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn immutable_binding_and_unique_session_do_not_allocate() {
        let root = super::super::private_test_directory();
        let mut j = open(root.path());
        begin(&mut j, 1).unwrap();
        for (peer, digest) in [([9; 32], RELAYS), (PEER, [9; 32])] {
            assert!(matches!(
                j.command_preparation(&id(1, NOW), &peer, &digest),
                Err(Error::Invalid("command_preparation_binding"))
            ));
            assert!(matches!(
                j.command_begin_preparation(id(1, NOW), peer, digest, id(22, NOW), NOW),
                Err(Error::Invalid("command_preparation_binding"))
            ));
        }
        assert!(matches!(
            j.command_begin_preparation(id(2, NOW), PEER, RELAYS, id(10001, NOW), NOW),
            Err(Error::Invalid("command_preparation_binding"))
        ));
        assert_eq!(
            j.conn
                .query_row("SELECT count(*) FROM command_preparations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(
            j.command_preparation_session(id(22, NOW))
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn existing_room_session_cannot_become_a_creation_fence() {
        let root = super::super::private_test_directory();
        let mut j = open(root.path());
        j.conn.execute("INSERT INTO command_rooms(group_id,transport,session,peer,direction,authority) VALUES(?1,?1,?2,?3,0,?3)",params![[1u8;32].as_slice(),id(10001,NOW).as_slice(),PEER.as_slice()]).unwrap();
        assert!(matches!(
            begin(&mut j, 1),
            Err(Error::Invalid("command_preparation_binding"))
        ));
        assert!(
            j.command_preparation(&id(1, NOW), &PEER, &RELAYS)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn new_token_and_session_share_existing_freshness_policy() {
        let root = super::super::private_test_directory();
        let mut j = open(root.path());
        j.conn
            .execute(
                "UPDATE command_replay_state SET floor=?1 WHERE id=1",
                [NOW as i64],
            )
            .unwrap();
        for (time, category) in [
            (NOW - 1, "command_expired"),
            (NOW + 301, "command_future_deferred"),
            (NOW + 604801, "command_future_excessive"),
        ] {
            for (token, session) in [(id(1, time), id(2, NOW)), (id(1, NOW), id(2, time))] {
                assert!(
                    matches!(j.command_begin_preparation(token,PEER,RELAYS,session,NOW),Err(Error::Invalid(label)) if label==category)
                );
            }
        }
        assert!(matches!(
            begin(&mut j, 1).unwrap(),
            PreparationStart::Created(_)
        ));
    }
    #[test]
    fn permanent_marker_capacity_preserves_existing_fences() {
        let root = super::super::private_test_directory();
        let mut j = open(root.path());
        let tx = j.conn.transaction().unwrap();
        for n in 0..4096 {
            tx.execute(
                "INSERT INTO command_preparations VALUES(?1,?2,?3,?4)",
                params![
                    id(n, NOW).as_slice(),
                    PEER.as_slice(),
                    RELAYS.as_slice(),
                    id(n + 10000, NOW).as_slice()
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert!(matches!(
            begin(&mut j, 1).unwrap(),
            PreparationStart::Existing(_)
        ));
        assert!(matches!(
            begin(&mut j, 4096),
            Err(Error::Invalid("command_preparation_capacity"))
        ));
    }
    #[test]
    fn room_capacities_are_checked_before_fence_insert() {
        let root = super::super::private_test_directory();
        let mut j = open(root.path());
        for (start, end) in [(0u32, 64u32), (64, 4096)] {
            let tx = j.conn.transaction().unwrap();
            for n in start..end {
                tx.execute("INSERT INTO command_rooms(group_id,transport,session,peer,direction,authority,revoked) VALUES(?1,?1,?2,?3,0,?3,?4)",params![n.to_be_bytes().as_slice(),id(n,NOW).as_slice(),PEER.as_slice(),i64::from(start>0)]).unwrap();
            }
            tx.commit().unwrap();
            assert!(matches!(
                begin(&mut j, 5000),
                Err(Error::Invalid("command_room_capacity"))
            ));
            j.conn
                .execute("UPDATE command_rooms SET revoked=1", [])
                .unwrap();
        }
        assert!(matches!(
            begin(&mut j, 5000),
            Err(Error::Invalid("command_room_capacity"))
        ));
        assert!(
            j.command_preparation(&id(5000, NOW), &PEER, &RELAYS)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn schema_rejects_malformed_binding_lengths() {
        let root = super::super::private_test_directory();
        let j = open(root.path());
        for field in 0..4 {
            let mut values = [vec![1u8; 16], vec![2u8; 32], vec![3u8; 32], vec![4u8; 16]];
            values[field].pop();
            assert!(
                j.conn
                    .execute(
                        "INSERT INTO command_preparations VALUES(?1,?2,?3,?4)",
                        params![values[0], values[1], values[2], values[3]]
                    )
                    .is_err()
            );
        }
    }
}
