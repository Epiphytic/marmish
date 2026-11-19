use super::super::super::super::tests::{Fixture, UID, URL, WALL, joined_pending};
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
        guard: impl FnMut(ApprovalGuard) -> Result<u64>,
        verify: impl FnOnce(&Transaction<'_>) -> Result<()>,
    ) -> Result<usize> {
        let tx = self.checkpoint.cursor.begin(self.origin)?;
        record(
            tx,
            Target {
                origin: self.origin,
                carrier: CARRIER,
                generation,
                initial_wall: at,
            },
            |tx, _| Ok((WALL + 2, self.deadline, state(tx, self.origin).decided_at)),
            guard,
            |tx, _| verify(tx),
        )
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
            case.record(
                case.generation,
                at,
                |step| match step {
                    ApprovalGuard::Consume => Ok(at),
                    ApprovalGuard::Verify => Err(refused),
                },
                |tx| {
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
                    Ok(())
                }
            ),
            Err(refused)
        );
        assert_eq!(case.snapshot(), before);
        assert_eq!(case.backing.inventory(), unchanged_stage);
    }
    assert!(
        case.record(
            case.generation + 1,
            at,
            |_| Ok(at),
            |_| panic!("generation mismatch wrote")
        )
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
    assert_eq!(
        case.record(case.generation, at, |_| Ok(at), |_| Ok(())),
        Ok(1)
    );
    let recorded = case.snapshot();
    assert_eq!(recorded.decided_at, Some(at));
    assert_eq!((recorded.origin_floor, recorded.global_floor), (at, at));
    assert!(
        case.record(
            case.generation,
            at + 1,
            |_| Ok(at + 1),
            |_| panic!("already Activating wrote")
        )
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
        case.record(
            case.generation,
            at,
            |_| Ok(at),
            |tx| {
                assert_eq!(state(tx, origin).decided_at, Some(at));
                tx.execute("INSERT INTO approved_fault VALUES (1)", [])
                    .unwrap();
                Ok(())
            }
        ),
        Err(ApprovalFailure::Ambiguous)
    );
    assert_eq!(case.snapshot(), before);
}

#[test]
fn approval_prewrite_clock_and_exact_retry_leave_the_row_and_floors_unchanged() {
    let mut case = Case::new();
    let origin = case.origin;
    let at = WALL + 3;
    let before = case.snapshot();
    for (sample, reason) in [
        (at - 1, ApprovalFailure::Clock),
        (case.deadline, ApprovalFailure::Expired),
    ] {
        assert_eq!(
            case.record(
                case.generation,
                at,
                |_| Ok(sample),
                |_| panic!("refusal wrote")
            ),
            Err(reason),
        );
        assert_eq!(case.snapshot(), before);
    }
    assert_eq!(
        case.record(
            case.generation,
            at,
            |step| match step {
                ApprovalGuard::Consume => Err(ApprovalFailure::Unavailable),
                ApprovalGuard::Verify => panic!("refusal reached final verification"),
            },
            |_| panic!("refusal wrote"),
        ),
        Err(ApprovalFailure::Unavailable),
    );
    assert_eq!(case.snapshot(), before);
    case.record(case.generation, at, |_| Ok(at), |_| Ok(()))
        .unwrap();
    let after = case.snapshot();
    let deadline = case.deadline;
    for (initial, consumed, final_wall, expected) in [
        (at + 1, at + 1, at + 2, Ok(())),
        (at, at - 1, at, Err(ApprovalFailure::Clock)),
        (at, at + 1, at, Err(ApprovalFailure::Clock)),
        (at, deadline, deadline, Err(ApprovalFailure::Expired)),
        (at, deadline - 1, deadline, Err(ApprovalFailure::Expired)),
    ] {
        assert_eq!(
            retry(
                &mut case.checkpoint.cursor.conn,
                origin,
                initial,
                |tx, _| {
                    assert_eq!(state(tx, origin), after);
                    Ok((WALL + 2, deadline, after.decided_at))
                },
                |step| Ok(match step {
                    ApprovalGuard::Consume => consumed,
                    ApprovalGuard::Verify => final_wall,
                })
            ),
            expected,
        );
        assert_eq!(case.snapshot(), after);
    }
    assert_eq!(
        retry(
            &mut case.checkpoint.cursor.conn,
            origin,
            at,
            |_, _| Err(ApprovalFailure::Stale),
            |step| match step {
                ApprovalGuard::Consume => Ok(at),
                ApprovalGuard::Verify => panic!("retry accepted a missing exact row"),
            },
        ),
        Err(ApprovalFailure::Stale),
    );
    assert_eq!(case.snapshot(), after);
    assert_eq!(
        retry(
            &mut case.checkpoint.cursor.conn,
            origin,
            at,
            |_, _| Ok((WALL + 2, deadline, Some(at + 1))),
            |step| match step {
                ApprovalGuard::Consume => Ok(at + 1),
                ApprovalGuard::Verify => panic!("retry accepted a regressed stored floor"),
            },
        ),
        Err(ApprovalFailure::Integrity),
    );
    assert_eq!(case.snapshot(), after);
}

#[test]
fn approval_read_uses_exact_installed_schema_and_manual_source() {
    use custody_schema::InstalledKind::{ApprovalV3, V2};

    for (kind, mode, denied) in [
        (V2, ReceivePolicy::Manual, ApprovalFailure::Stale),
        (ApprovalV3, ReceivePolicy::AutoAny, ApprovalFailure::Stale),
        (ApprovalV3, ReceivePolicy::Manual, ApprovalFailure::Stale),
    ] {
        let f = Fixture::new();
        drop(
            Checkpoint::attach_installed_v2(
                f.open(),
                f.client.clone(),
                UID,
                URL,
                Policy::default(),
            )
            .unwrap(),
        );
        let mut cp = Checkpoint::attach_installed_epoch(
            f.open(),
            f.client.clone(),
            UID,
            URL,
            Policy::default(),
            mode,
            kind,
        )
        .unwrap();
        let source = cp.bind_source(URL).unwrap();
        let scope = ApprovalScope {
            source: &source,
            account: f.client.public_key(),
            origin: URL,
            selector: Selector {
                server: Keys::generate().public_key(),
                id: [1; 16],
                revision: [2; 32],
            },
            generation: 1,
        };
        let inventory = f.inventory();
        assert_eq!(cp.read_installed_approval(scope, WALL).err(), Some(denied));
        if matches!(kind, ApprovalV3)
            && matches!(cp.installed_receive_policy(), Some(ReceivePolicy::Manual))
        {
            assert!(cp.approval_context(scope).is_ok());
            let wrong_account = ApprovalScope {
                account: Keys::generate().public_key(),
                ..scope
            };
            assert_eq!(
                cp.read_installed_approval(wrong_account, WALL).err(),
                Some(ApprovalFailure::Unavailable)
            );
        } else {
            assert_eq!(cp.approval_context(scope).err(), Some(denied));
        }
        assert_eq!(f.inventory(), inventory);
    }
}
