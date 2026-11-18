use super::*;
use std::cell::Cell;

const WRITER: Claim = Claim {
    random: [10; 16],
    generation: 10,
};
const FIRST_OBSERVER: Claim = Claim {
    random: [11; 16],
    generation: 11,
};
const LATER_OBSERVER: Claim = Claim {
    random: [12; 16],
    generation: 14,
};
const SUCCESSOR: Claim = Claim {
    random: [15; 16],
    generation: 15,
};
const DEADLINE: Duration = Duration::from_secs(10);

fn seconds(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn replacement(observers: &[Claim]) -> Replacement {
    Replacement::begin(WRITER, observers, DEADLINE, Duration::ZERO).unwrap()
}

fn beginning_launch() -> Replacement {
    let mut pending = replacement(&[]);
    assert!(pending.reaped(WRITER));
    assert!(pending.record_departure(DepartureEpoch::Old, Departure::Empty, seconds(1)));
    assert!(pending.begin_successor(SUCCESSOR, seconds(2)));
    pending
}

#[test]
fn core_tests_terminal_takeover_retains_writer_unit_until_exact_departure_and_ack() {
    let mut pending = replacement(&[FIRST_OBSERVER, LATER_OBSERVER]);
    assert_eq!(pending.units(), 3);
    assert!(!pending.record_departure(DepartureEpoch::Old, Departure::Empty, seconds(1)));
    assert!(!pending.begin_successor(SUCCESSOR, seconds(1)));
    assert!(!pending.reaped(SUCCESSOR));
    assert!(pending.reaped(LATER_OBSERVER));
    assert!(!pending.reaped(LATER_OBSERVER));
    assert_eq!(pending.units(), 2);
    assert!(pending.reaped(WRITER));
    assert_eq!(pending.units(), 2);
    assert!(pending.phase(seconds(2)) == Phase::RetiringOld);
    assert!(pending.reaped(FIRST_OBSERVER));
    assert_eq!(pending.units(), 1);
    assert!(pending.phase(seconds(2)) == Phase::VerifyDeparture(DepartureEpoch::Old));

    assert!(pending.record_departure(DepartureEpoch::Old, Departure::Occupied, seconds(2)));
    assert!(!pending.begin_successor(SUCCESSOR, seconds(2)));
    assert!(pending.record_departure(DepartureEpoch::Old, Departure::TimedOut, seconds(3)));
    assert!(!pending.record_departure(
        DepartureEpoch::Successor(SUCCESSOR),
        Departure::Empty,
        seconds(3)
    ));
    assert!(pending.record_departure(DepartureEpoch::Old, Departure::Empty, seconds(4)));
    assert!(pending.phase(seconds(4)) == Phase::StartSuccessor);
    assert!(!pending.begin_successor(WRITER, seconds(4)));
    assert!(!pending.begin_successor(
        Claim {
            random: WRITER.random,
            ..SUCCESSOR
        },
        seconds(4)
    ));
    assert!(!pending.begin_successor(
        Claim {
            random: [1; 16],
            ..LATER_OBSERVER
        },
        seconds(4)
    ));
    assert!(pending.begin_successor(SUCCESSOR, seconds(5)));
    assert!(!pending.begin_successor(SUCCESSOR, seconds(5)));
    assert!(pending.phase(seconds(5)) == Phase::LaunchingSuccessor);
    assert!(!pending.confirm(SUCCESSOR, seconds(5), || panic!("not ready")));
    assert!(!pending.startup(WRITER, Startup::NeverStarted, seconds(5)));
    let ready = Startup::Ready {
        confirm_until: seconds(9),
    };
    assert!(pending.startup(SUCCESSOR, ready, seconds(6)));
    assert!(!pending.startup(SUCCESSOR, Startup::NeverStarted, seconds(6)));
    assert!(pending.phase(seconds(6)) == Phase::AwaitingConfirm);
    let wrong_claim = Claim {
        random: [17; 16],
        ..SUCCESSOR
    };
    assert!(!pending.confirm(wrong_claim, seconds(7), || panic!("wrong claim")));
    assert_eq!(pending.units(), 1);
    let acknowledgements = Cell::new(0);
    assert!(pending.confirm(SUCCESSOR, seconds(8), || {
        acknowledgements.set(acknowledgements.get() + 1);
        true
    }));
    assert!(!pending.confirm(SUCCESSOR, seconds(8), || panic!(
        "duplicate acknowledgement"
    )));
    assert_eq!(acknowledgements.get(), 1);
    assert_eq!(pending.units(), 0);
    assert!(pending.phase(seconds(8)) == Phase::Transferred);
    assert!(!pending.reaped(SUCCESSOR));
}

#[test]
fn core_tests_terminal_takeover_invalid_reservation_has_no_native_work() {
    assert!(Replacement::begin(WRITER, &[], DEADLINE, DEADLINE).is_none());
    for invalid in [
        FIRST_OBSERVER,
        Claim {
            random: [2; 16],
            ..FIRST_OBSERVER
        },
        Claim {
            random: [0; 16],
            generation: 12,
        },
        Claim {
            random: [2; 16],
            generation: 0,
        },
        WRITER,
    ] {
        assert!(
            Replacement::begin(WRITER, &[FIRST_OBSERVER, invalid], DEADLINE, seconds(1)).is_none()
        );
    }
    assert!(
        Replacement::begin(
            WRITER,
            &[
                FIRST_OBSERVER,
                LATER_OBSERVER,
                SUCCESSOR,
                Claim {
                    random: [17; 16],
                    generation: 17
                }
            ],
            DEADLINE,
            seconds(1)
        )
        .is_none()
    );
}

#[test]
fn core_tests_terminal_takeover_withdrawal_keeps_old_cleanup_and_never_restarts() {
    let mut pending = replacement(&[FIRST_OBSERVER]);
    pending.withdraw();
    assert!(pending.phase(seconds(1)) == Phase::RetiringOld);
    assert!(pending.reaped(WRITER));
    assert_eq!(pending.units(), 2);
    assert!(!pending.begin_successor(SUCCESSOR, seconds(1)));
    assert!(pending.reaped(FIRST_OBSERVER));
    assert!(pending.record_departure(DepartureEpoch::Old, Departure::Empty, seconds(2)));
    assert!(pending.phase(seconds(2)) == Phase::Detached);
    assert_eq!(pending.units(), 0);
    assert!(!pending.begin_successor(SUCCESSOR, seconds(2)));

    let mut after_departure = replacement(&[]);
    assert!(after_departure.reaped(WRITER));
    assert!(after_departure.record_departure(DepartureEpoch::Old, Departure::Empty, seconds(1)));
    after_departure.withdraw();
    assert!(after_departure.phase(seconds(2)) == Phase::Detached);
    assert!(!after_departure.begin_successor(SUCCESSOR, seconds(2)));
}

#[test]
fn core_tests_terminal_takeover_expired_or_uncertain_departure_is_quarantined() {
    let mut pending = replacement(&[FIRST_OBSERVER]);
    assert!(pending.reaped(WRITER));
    assert!(pending.phase(DEADLINE) == Phase::RetiringOld);
    assert_eq!(pending.units(), 2);
    assert!(pending.reaped(FIRST_OBSERVER));
    assert!(!pending.record_departure(DepartureEpoch::Old, Departure::Empty, DEADLINE));
    assert!(pending.phase(DEADLINE) == Phase::Quarantine);
    assert_eq!(pending.units(), 0);
    assert!(!pending.begin_successor(SUCCESSOR, DEADLINE));

    let mut foreign_inventory = replacement(&[]);
    assert!(foreign_inventory.reaped(WRITER));
    assert!(foreign_inventory.record_departure(
        DepartureEpoch::Old,
        Departure::Uncertain,
        seconds(1)
    ));
    assert!(foreign_inventory.phase(seconds(1)) == Phase::Quarantine);
    assert!(!foreign_inventory.record_departure(DepartureEpoch::Old, Departure::Empty, seconds(2)));
}

#[test]
fn core_tests_terminal_takeover_failed_ack_reaps_successor_without_using_old_proof() {
    let mut pending = beginning_launch();
    let ready = Startup::Ready {
        confirm_until: seconds(7),
    };
    assert!(pending.startup(SUCCESSOR, ready, seconds(3)));
    assert!(!pending.confirm(SUCCESSOR, seconds(4), || false));
    assert!(pending.phase(seconds(4)) == Phase::RetiringSuccessor(SUCCESSOR));
    assert!(!pending.confirm(SUCCESSOR, seconds(4), || panic!(
        "the acknowledgement failed"
    )));
    assert_eq!(pending.units(), 1);
    assert!(pending.reaped(SUCCESSOR));
    assert!(!pending.reaped(SUCCESSOR));
    assert!(!pending.record_departure(DepartureEpoch::Old, Departure::Empty, seconds(5)));
    let epoch = DepartureEpoch::Successor(SUCCESSOR);
    assert!(pending.record_departure(epoch, Departure::Empty, seconds(5)));
    assert!(pending.phase(seconds(5)) == Phase::Detached);
    assert_eq!(pending.units(), 0);
}

#[test]
fn core_tests_terminal_takeover_confirmation_checks_both_deadlines_and_hard_window() {
    for (ready_at, confirm_until, expired_at) in [(3, 4, 4), (8, 18, 10), (3, 18, 8)] {
        let mut pending = beginning_launch();
        let ready = Startup::Ready {
            confirm_until: seconds(confirm_until),
        };
        assert!(pending.startup(SUCCESSOR, ready, seconds(ready_at)));
        assert!(!pending.confirm(SUCCESSOR, seconds(expired_at), || panic!(
            "arrival at cutoff"
        )));
        assert!(pending.phase(seconds(expired_at)) == Phase::RetiringSuccessor(SUCCESSOR));
        assert_eq!(pending.units(), 1);
        assert!(pending.reaped(SUCCESSOR));
    }
}

#[test]
fn core_tests_terminal_takeover_late_and_failed_native_start_are_accounted() {
    let mut late = beginning_launch();
    late.withdraw();
    let ready = Startup::Ready {
        confirm_until: seconds(13),
    };
    assert!(late.startup(SUCCESSOR, ready, seconds(11)));
    assert!(late.phase(seconds(11)) == Phase::RetiringSuccessor(SUCCESSOR));
    assert_eq!(late.units(), 1);
    assert!(late.reaped(SUCCESSOR));
    assert!(late.phase(seconds(11)) == Phase::Quarantine);

    let mut never = beginning_launch();
    assert!(never.startup(SUCCESSOR, Startup::NeverStarted, seconds(3)));
    assert!(never.phase(seconds(3)) == Phase::Detached);
    assert_eq!(never.units(), 0);
    assert!(!never.begin_successor(SUCCESSOR, seconds(3)));

    let mut failed = beginning_launch();
    assert!(failed.startup(SUCCESSOR, Startup::NeedsCleanup, seconds(3)));
    assert!(failed.phase(seconds(3)) == Phase::RetiringSuccessor(SUCCESSOR));
    assert!(!failed.reaped(WRITER));
    assert_eq!(failed.units(), 1);
    assert!(failed.reaped(SUCCESSOR));
    assert!(failed.record_departure(
        DepartureEpoch::Successor(SUCCESSOR),
        Departure::Empty,
        seconds(4)
    ));
    assert!(failed.phase(seconds(4)) == Phase::Detached);
}

#[test]
fn core_tests_terminal_takeover_native_exit_before_confirm_fences_the_claim() {
    let mut pending = beginning_launch();
    let ready = Startup::Ready {
        confirm_until: seconds(7),
    };
    assert!(pending.startup(SUCCESSOR, ready, seconds(3)));
    assert!(pending.reaped(SUCCESSOR));
    assert!(!pending.confirm(SUCCESSOR, seconds(3), || panic!("successor already reaped")));
    assert!(
        pending.phase(seconds(3)) == Phase::VerifyDeparture(DepartureEpoch::Successor(SUCCESSOR))
    );
    assert_eq!(pending.units(), 1);
}
