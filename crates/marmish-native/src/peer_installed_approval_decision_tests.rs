use super::super::super::super::tests::{Fixture, WALL, joined_pending};
use super::*;

const CARRIER: [u8; 32] = [0x56; 32];

struct Case {
    backing: Fixture,
    checkpoint: Checkpoint,
    origin: Origin,
    generation: u64,
    deadline: u64,
}

#[derive(Debug, Eq, PartialEq)]
struct State {
    state: u8,
    approval: Option<u8>,
    decided_at: Option<u64>,
    origin_floor: u64,
    global_floor: u64,
}

impl Case {
    fn new() -> Self {
        let backing = Fixture::new();
        let (checkpoint, source, _) = joined_pending(&backing, &Keys::generate());
        checkpoint
            .cursor
            .conn
            .execute_batch(custody_schema::InstalledKind::ApprovalV3.ddl())
            .unwrap();
        let (generation, deadline) = checkpoint
            .cursor
            .conn
            .query_row(
                "SELECT generation,pending_deadline FROM peer_pending_custody WHERE delivery_carrier=?1",
                [CARRIER],
                |row| Ok((read_seconds(row, 0)?, read_seconds(row, 1)?)),
            )
            .unwrap();
        Self {
            backing,
            checkpoint,
            origin: source.origin,
            generation,
            deadline,
        }
    }

    fn snapshot(&self) -> State {
        let HardenedPeerConnection(conn) = self.backing.open();
        state(&conn, self.origin)
    }

    fn record(
        &mut self,
        generation: u64,
        at: u64,
        verify: impl FnOnce(&Transaction<'_>) -> Result<()>,
    ) -> Result<usize> {
        let tx = self.checkpoint.cursor.begin(self.origin)?;
        record(tx, self.origin, CARRIER, generation, at, verify)
    }
}

fn state(conn: &rusqlite::Connection, origin: Origin) -> State {
    conn.query_row(
        "SELECT c.state,c.approval,c.activating_at,o.wall,v.wall
         FROM peer_pending_custody c
         CROSS JOIN peer_cursor_origin o CROSS JOIN peer_consent_version v
         WHERE c.delivery_carrier=?1 AND o.origin=?2 AND v.singleton=1",
        params![CARRIER, origin],
        |row| {
            Ok(State {
                state: row.get(0)?,
                approval: row.get(1)?,
                decided_at: row
                    .get::<_, Option<i64>>(2)?
                    .map(u64::try_from)
                    .transpose()
                    .unwrap(),
                origin_floor: read_seconds(row, 3)?,
                global_floor: read_seconds(row, 4)?,
            })
        },
    )
    .unwrap()
}

#[test]
fn approval_precommit_refusal_rolls_back_exact_decision_and_both_floors() {
    let mut case = Case::new();
    let origin = case.origin;
    let before = case.snapshot();
    let at = WALL + 3;
    assert_eq!(before.state, 2);
    assert!(before.origin_floor < at && before.global_floor < at);
    let unchanged_stage = case.backing.inventory();
    for refused in [
        ApprovalFailure::Unavailable,
        ApprovalFailure::Clock,
        ApprovalFailure::Expired,
    ] {
        assert_eq!(
            case.record(case.generation, at, |tx| {
                assert_eq!(
                    state(tx, origin),
                    State {
                        state: 3,
                        approval: Some(0),
                        decided_at: Some(at),
                        origin_floor: at,
                        global_floor: at
                    }
                );
                Err(refused)
            }),
            Err(refused)
        );
        assert_eq!(case.snapshot(), before);
        assert_eq!(case.backing.inventory(), unchanged_stage);
    }
    assert!(
        case.record(case.generation + 1, at, |_| panic!(
            "generation mismatch wrote"
        ))
        .is_err()
    );
    assert_eq!(case.snapshot(), before);
    assert_eq!(
        check_wall(case.deadline, at, case.deadline),
        Err(ApprovalFailure::Expired)
    );
    assert_eq!(
        check_wall(at - 1, at, case.deadline),
        Err(ApprovalFailure::Clock)
    );
    assert_eq!(case.record(case.generation, at, |_| Ok(())), Ok(1));
    let recorded = case.snapshot();
    assert_eq!(recorded.decided_at, Some(at));
    assert_eq!((recorded.origin_floor, recorded.global_floor), (at, at));
    assert!(
        case.record(case.generation, at + 1, |_| panic!(
            "already Activating wrote"
        ))
        .is_err()
    );
    assert_eq!(case.snapshot(), recorded);
    assert_eq!(case.backing.inventory(), unchanged_stage);
}

#[test]
fn approval_failed_sqlite_commit_is_typed_ambiguous() {
    let mut case = Case::new();
    let before = case.snapshot();
    case.checkpoint.cursor.conn.execute_batch(
        "PRAGMA foreign_keys=ON;
         CREATE TEMP TABLE approved_parent(id INTEGER PRIMARY KEY);
         CREATE TEMP TABLE approved_fault(id INTEGER REFERENCES approved_parent(id) DEFERRABLE INITIALLY DEFERRED);"
    ).unwrap();
    let origin = case.origin;
    let at = WALL + 3;
    assert_eq!(
        case.record(case.generation, at, |tx| {
            assert_eq!(state(tx, origin).decided_at, Some(at));
            tx.execute("INSERT INTO approved_fault VALUES (1)", [])
                .unwrap();
            Ok(())
        }),
        Err(ApprovalFailure::Ambiguous)
    );
    assert_eq!(case.snapshot(), before);
}
