use super::*;
use crate::dispatcher_fatal::Trip;
use std::{os::unix::fs::PermissionsExt, sync::mpsc};

fn profile() -> (tempfile::TempDir, Arc<ProfileLease>) {
    let dir = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let lease = Arc::new(ProfileLease::acquire(dir.path(), "owned-stop").unwrap());
    (dir, lease)
}

fn engine_custody(engine: CommandEngine) -> Engine {
    Engine::InOwner(ManuallyDrop::new(Box::new(engine)))
}

#[tokio::test]
async fn incomplete_engine_turn_drains_again_until_actual_worker_join() {
    let (_dir, lease) = profile();
    let (trip, fatal) = Trip::new();
    let handle = Handle::current();
    let admission = Admission::new(&Arc::new(trip), || true);
    // A single Stop cannot finish the worker, so Complete requires a
    // second bounded turn without recreating or abandoning the engine.
    let mut engine = engine_custody(CommandEngine::candidate_wait_for_two_stops_for_test(
        admission,
    ));
    engine
        .observe(
            &fatal,
            &lease,
            &handle,
            Instant::now() + Duration::from_secs(2),
        )
        .await;
    assert!(matches!(engine, Engine::Joined { quiesced: false }));
}

#[tokio::test]
async fn cut_retains_in_flight_engine_then_joins_the_original_worker() {
    let (_dir, lease) = profile();
    let (trip, fatal) = Trip::new();
    let handle = Handle::current();
    let trip = Arc::new(trip);
    let admission = Admission::new(&trip, || true);
    let (release, gate) = mpsc::channel();
    let mut engine = engine_custody(CommandEngine::gated_candidate_stop_for_test(
        gate, admission,
    ));
    engine.start_turn(
        &fatal,
        &lease,
        &handle,
        Instant::now() + Duration::from_secs(1),
    );
    assert!(matches!(engine, Engine::InTurn(_)));
    // The sticky cut is sufficient before notify delivers its wake.
    trip.mark_cut();
    engine
        .observe(
            &fatal,
            &lease,
            &handle,
            Instant::now() + Duration::from_millis(300),
        )
        .await;
    assert!(matches!(engine, Engine::FatalJoin(_)));
    assert_eq!(engine.barrier(), Barrier::Pending);
    release.send(()).unwrap();
    engine
        .observe(
            &fatal,
            &lease,
            &handle,
            Instant::now() + Duration::from_secs(2),
        )
        .await;
    assert_eq!(engine.barrier(), Barrier::Complete);
    assert!(!engine.quiesced());
}

#[tokio::test]
async fn refused_engine_transfer_and_failed_observer_cannot_prove_completion() {
    let (_dir, lease) = profile();
    let (trip, fatal) = Trip::new();
    let handle = Handle::current();
    let (release, gate) = mpsc::channel();
    let mut refused = engine_custody(CommandEngine::gated_stop_for_test(gate));
    trip.notify();
    refused.start_turn(
        &fatal,
        &lease,
        &handle,
        Instant::now() + Duration::from_secs(1),
    );
    assert!(matches!(refused, Engine::Uncertain(Some(_))));
    assert_eq!(refused.barrier(), Barrier::PossiblyLive);
    release.send(()).unwrap();

    let mut lost = Engine::InTurn(handle.spawn_blocking(|| panic!("test observer failed")));
    lost.observe(
        &fatal,
        &lease,
        &handle,
        Instant::now() + Duration::from_secs(2),
    )
    .await;
    assert!(matches!(lost, Engine::Uncertain(None)));
    assert_eq!(lost.barrier(), Barrier::PossiblyLive);
}
