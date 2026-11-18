use super::*;
use crate::peer_invitation::store::Error as CursorError;

fn qualify(service: &mut Service, now: Instant) {
    assert_eq!(service.poll_connect(now), ConnectionAction::Connect);
    assert_eq!(service.poll_connect(now), ConnectionAction::Idle);
    assert!(service.batch().is_none());
    assert!(service.authenticated(now));
    assert!(!service.authenticated(now));
    assert_eq!(service.poll_connect(now), ConnectionAction::Idle);
}

fn retire(service: &mut Service, now: Instant, reason: Retirement, seconds: u64) -> Instant {
    let ready = now + Duration::from_secs(seconds);
    service.refuse(now, Refusal::RetireRelay(reason));
    for _ in 0..4 {
        assert_eq!(
            service.poll_connect(now),
            ConnectionAction::WaitUntil(ready)
        );
        assert!(!service.authenticated(now));
        assert!(service.batch().is_none());
    }
    // Duplicate error reporting never advances the backoff or shortens the wait.
    service.refuse(now, Refusal::RetireRelay(reason));
    assert_eq!(
        service.poll_connect(ready - Duration::from_nanos(1)),
        ConnectionAction::WaitUntil(ready)
    );
    ready
}

#[test]
fn installed_delivery_repeated_bad_frames_persist_backoff_across_reauth() {
    let mut now = Instant::now();
    let mut service = Service::new(now);
    for (delay, reason) in [
        (1, Retirement::Grammar),
        (2, Retirement::Grammar),
        (4, Retirement::Size),
        (8, Retirement::LexicalConflict),
        (16, Retirement::PageContract),
        (32, Retirement::Grammar),
        (60, Retirement::Size),
        (60, Retirement::LexicalConflict),
    ] {
        qualify(&mut service, now);
        now = retire(&mut service, now, reason, delay);
    }
    qualify(&mut service, now);
}

#[test]
fn installed_delivery_authentication_frame_failure_waits_before_next_attempt() {
    let start = Instant::now();
    let mut service = Service::new(start);
    assert_eq!(service.poll_connect(start), ConnectionAction::Connect);
    let ready = retire(&mut service, start, Retirement::Grammar, 1);
    qualify(&mut service, ready);
    retire(&mut service, ready, Retirement::Size, 2);
}

#[test]
fn installed_delivery_ordinary_disconnect_preserves_retirement_budget() {
    let start = Instant::now();
    let mut service = Service::new(start);
    qualify(&mut service, start);
    let ready = retire(&mut service, start, Retirement::Grammar, 1);
    service.disconnected(ready - Duration::from_nanos(1));
    assert_eq!(
        service.poll_connect(ready - Duration::from_nanos(1)),
        ConnectionAction::WaitUntil(ready)
    );
    qualify(&mut service, ready);

    service.disconnected(ready);
    assert!(service.batch().is_none());
    qualify(&mut service, ready);
    retire(&mut service, ready, Retirement::LexicalConflict, 2);
}

#[test]
fn installed_delivery_uses_exclusive_finite_history_saved_and_live_batch() {
    let start = Instant::now();
    let mut service = Service::new(start);
    qualify(&mut service, start);
    {
        let mut batch = service.batch().unwrap();
        for _ in 0..8 {
            assert_eq!(batch.choose(true, true, true), Work::Retry);
            assert_eq!(batch.choose(true, true, true), Work::History);
        }
        assert_eq!(batch.choose(true, true, true), Work::Live);
        for _ in 0..4 {
            assert_eq!(batch.choose(true, true, true), Work::Yield);
        }
    }
    let mut batch = service.batch().unwrap();
    assert_eq!(batch.choose(false, true, true), Work::Retry);
    assert_eq!(batch.choose(false, false, true), Work::Live);
    assert_eq!(batch.choose(false, false, true), Work::Yield);
}

#[test]
fn installed_delivery_resets_backoff_only_after_selected_durable_page_progress() {
    let start = Instant::now();
    let mut service = Service::new(start);
    qualify(&mut service, start);
    let now = retire(&mut service, start, Retirement::Grammar, 1);
    qualify(&mut service, now);

    let cursor = Cursor {
        origin: [1; 32],
        revision: 20,
        anchor: 10,
        lower: 1,
        upper: 11,
        until: 10,
        ties: vec![],
        visible: 0,
    };
    {
        let mut batch = service.batch().unwrap();
        assert_eq!(batch.choose(true, true, true), Work::Retry);
        batch.history_result(&cursor, &Ok::<_, ()>(((), None)));
        assert_eq!(batch.choose(true, true, true), Work::History);
        batch.history_result(
            &cursor,
            &Err::<((), Option<Cursor>), _>(CursorError::Storage),
        );
        assert_eq!(batch.choose(true, false, false), Work::History);
        batch.history_result(&cursor, &Ok::<_, ()>(((), Some(cursor.clone()))));
        assert_eq!(batch.choose(true, false, false), Work::History);
        let foreign = Cursor {
            origin: [2; 32],
            revision: 21,
            ..cursor.clone()
        };
        batch.history_result(&cursor, &Ok::<_, ()>(((), Some(foreign))));
    }
    let now = retire(&mut service, now, Retirement::Grammar, 2);
    qualify(&mut service, now);
    {
        let mut batch = service.batch().unwrap();
        assert_eq!(batch.choose(true, false, false), Work::History);
        let next = Cursor {
            revision: 21,
            ..cursor.clone()
        };
        batch.history_result(&cursor, &Ok::<_, ()>(((), Some(next))));
    }
    let now = retire(&mut service, now, Retirement::Grammar, 1);
    qualify(&mut service, now);
    {
        let mut batch = service.batch().unwrap();
        assert_eq!(batch.choose(true, false, false), Work::History);
        batch.history_result(&cursor, &Ok::<_, ()>(((), None)));
    }
    retire(&mut service, now, Retirement::Grammar, 1);
}

#[test]
fn installed_delivery_refuses_local_and_indeterminate_failures_without_backoff() {
    for (error, reason) in [
        (Error::Socket(Failure::Grammar), Retirement::Grammar),
        (Error::Socket(Failure::Size), Retirement::Size),
        (
            Error::Checkpoint(consent_checkpoint::Error::Index),
            Retirement::LexicalConflict,
        ),
    ] {
        assert_eq!(Refusal::from(error), Refusal::RetireRelay(reason));
    }

    let now = Instant::now();
    for error in [
        Error::Binding,
        Error::Clock,
        Error::Socket(Failure::Retired),
        Error::Socket(Failure::Capacity),
        Error::Checkpoint(consent_checkpoint::Error::Source),
        Error::Checkpoint(consent_checkpoint::Error::Cursor(CursorError::Backlog)),
        Error::Cursor(CursorError::ClockRegressed),
        Error::Cursor(CursorError::RelayContract),
        Error::Cursor(CursorError::Conflict),
        Error::Cursor(CursorError::Page),
        Error::Cursor(CursorError::Stale),
        Error::Cursor(CursorError::Storage),
    ] {
        let mut service = Service::new(now);
        qualify(&mut service, now);
        let refusal = Refusal::from(error);
        assert_eq!(refusal, Refusal::Stop);
        service.refuse(now, refusal);
        assert_eq!(
            service.poll_connect(now + Duration::from_secs(61)),
            ConnectionAction::Stop
        );
        assert!(service.batch().is_none());
    }
}

#[test]
fn installed_delivery_stops_on_owner_monotonic_regression() {
    let start = Instant::now();
    let mut service = Service::new(start);
    qualify(&mut service, start + Duration::from_secs(1));
    assert_eq!(service.poll_connect(start), ConnectionAction::Stop);
    assert!(!service.authenticated(start + Duration::from_secs(2)));
    assert!(service.batch().is_none());
}
