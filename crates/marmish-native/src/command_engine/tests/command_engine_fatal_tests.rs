use super::*;
use crate::command_lifecycle::keep_fatal_engine;
use crate::dispatcher_fatal::{Fatal, Trip};
use crate::identity::ProfileLease;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn checked(fatal: Fatal) -> Mode {
    Mode {
        checked: Some(Admission::new(fatal, || true)),
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

fn reopen(settings: &EngineSettings) -> engine::Result<engine::Engine> {
    engine::Engine::open_with_master(
        settings.policy.clone(),
        settings.binding.clone(),
        engine::ProvidedMaster::new(*settings.master),
    )
}

fn fatal_join(handle: CommandEngine) -> FatalEngineJoin {
    handle.into_fatal_join().ok().unwrap()
}

#[test]
fn early_fatal_creates_no_engine_directory_or_bootstrap_event() {
    let root = tempfile::tempdir().unwrap();
    let settings = engine_settings(&root);
    let storage = settings.policy.state_root.clone();
    let (trip, fatal) = Trip::new();
    trip.trip();
    let mut handle =
        CommandEngine::start_candidate(settings, vec![], Admission::new(fatal, || panic!()))
            .unwrap();
    let stop = deadline();
    while handle
        .thread
        .as_ref()
        .is_some_and(|thread| !thread.is_finished())
    {
        assert!(Instant::now() < stop);
        thread::yield_now();
    }
    assert!(matches!(
        handle.events.try_recv(),
        Err(TryRecvError::Disconnected)
    ));
    assert!(!storage.exists());
    assert!(matches!(
        handle.stop_outcome(deadline()),
        Ok(StopOutcome::Fatal)
    ));
    fatal_join(handle).wait().unwrap();
}

#[test]
fn candidate_cleanup_fails_before_moving_live_engine() {
    let root = tempfile::tempdir().unwrap();
    let settings = engine_settings(&root);
    let reopen_settings = EngineSettings {
        policy: settings.policy.clone(),
        binding: settings.binding.clone(),
        master: settings.master.clone(),
        canonical_native_image: None,
    };
    let (_trip, fatal) = Trip::new();
    let checks = Arc::new(AtomicUsize::new(0));
    let admitted = checks.clone();
    let mut handle = CommandEngine::start_candidate(
        settings,
        vec![],
        Admission::new(fatal, move || {
            admitted.fetch_add(1, Ordering::Relaxed);
            true
        }),
    )
    .unwrap();
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(2)),
        Ok(Event::Ready)
    ));
    handle.readiness = Readiness::Ready;
    handle.try_prepare_cleanup().unwrap();
    assert!(matches!(
        handle.events.recv_timeout(Duration::from_secs(2)),
        Ok(Event::Cleanup(CleanupState::Failed))
    ));
    // Ready, registration, cleanup admission and failure publication have run;
    // healthy ordinary stop then checks both Stop and the final engine close.
    let checked_before_stop = checks.load(Ordering::Relaxed);
    assert!(checked_before_stop >= 5);
    assert!(matches!(
        reopen(&reopen_settings),
        Err(engine::EngineError::AlreadyOpen)
    ));
    assert!(handle.shutdown(deadline()).unwrap().complete);
    assert!(checks.load(Ordering::Relaxed) >= checked_before_stop + 2);
    assert!(reopen(&reopen_settings).is_ok());
}

#[tokio::test]
async fn full_queues_disconnect_before_join_and_keeper_retains_lease() {
    let (mut handle, operations, controls, events) = handle();
    let (_live_in, live) = mpsc::sync_channel(CAPACITY);
    let (trip, fatal) = Trip::new();
    let checks = Arc::new(AtomicUsize::new(0));
    let admitted = checks.clone();
    let (sending, wait_send) = mpsc::sync_channel(1);
    let (exited, wait_exit) = mpsc::sync_channel(1);
    let (release_join, join_gate) = mpsc::sync_channel(1);
    handle.mode = Mode {
        checked: Some(Admission::new(fatal, move || {
            if admitted.fetch_add(1, Ordering::Relaxed) == 1 {
                sending.send(()).unwrap();
            }
            true
        })),
    };
    let mut worker = scheduler();
    worker.mode = handle.mode.clone();

    for _ in 0..CAPACITY {
        handle.controls.try_send(Control::FailCleanup).unwrap();
        events.try_send(Event::Ready).unwrap();
    }
    handle.thread = Some(thread::spawn(move || {
        worker.run(operations, live, controls, events);
        exited.send(()).unwrap();
        join_gate.recv_timeout(Duration::from_secs(2)).unwrap();
    }));
    wait_send.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(wait_exit.try_recv().is_err());
    handle.controls.try_send(Control::FailCleanup).unwrap();
    assert!(matches!(
        handle.controls.try_send(Control::Stop),
        Err(TrySendError::Full(_))
    ));
    trip.trip();
    assert!(matches!(
        handle.stop_outcome(deadline()),
        Ok(StopOutcome::Fatal)
    ));
    // The event queue is still full. The physical join handoff must disconnect
    // its receiver to let the worker return from the already-admitted send.

    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let lease = Arc::new(ProfileLease::acquire(root.path(), "account").unwrap());
    let mut keeper = keep_fatal_engine(fatal_join(handle), lease);
    wait_exit.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut keeper)
            .await
            .is_err()
    );
    assert!(ProfileLease::acquire(root.path(), "account").is_err());
    assert_eq!(checks.load(Ordering::Relaxed), 2);
    release_join.send(()).unwrap();
    keeper.await.unwrap().unwrap();
    assert!(ProfileLease::acquire(root.path(), "account").is_ok());
}

#[test]
fn completed_panicked_thread_stays_failed_after_fatal_handoff() {
    let (mut handle, _operations, _controls, _events) = handle();
    let (trip, fatal) = Trip::new();
    handle.mode = checked(fatal);
    handle.thread = Some(thread::spawn(|| panic!("fixture worker panic")));
    assert!(matches!(
        handle.stop_outcome(deadline()),
        Err(Error::Storage("engine_thread_failed"))
    ));
    assert!(handle.thread.is_none());
    trip.trip();
    assert!(matches!(
        handle.stop_outcome(deadline()),
        Ok(StopOutcome::Fatal)
    ));
    assert!(matches!(
        fatal_join(handle).wait(),
        Err(Error::Storage("engine_thread_failed"))
    ));
}

#[test]
fn stale_cached_observations_and_queue_admissions_are_hidden() {
    let (mut handle, _operations, controls, events) = handle();
    let (trip, fatal) = Trip::new();
    handle.mode = checked(fatal);
    events
        .send(Event::CopyGc(Box::new(CopyGcCompletion {
            key: "fixture".into(),
            revision: 1,
            cleaned: true,
        })))
        .unwrap();
    events.send(Event::Cleanup(CleanupState::Quiesced)).unwrap();
    let op = operation(1, EngineOperation::Observe);
    events
        .send(Event::Completion(Box::new(EngineCompletion {
            operation: op.id,
            key: op.key.clone(),
            revision: op.revision,
            result: EngineObservation::Missing,
        })))
        .unwrap();
    events
        .send(Event::LivePrepared(Box::new(LivePrepared {
            operation_id: [1; 16],
            stream_id: [2; 16],
            revision: 1,
            result: Err(engine::EngineError::WorkerUnavailable),
        })))
        .unwrap();
    handle.collect();
    trip.trip();
    assert!(handle.readiness() == Readiness::Opening);
    assert!(handle.cleanup_state() == CleanupState::Failed);
    assert!(handle.take_copy_gc().is_none());
    assert!(handle.try_completion().is_none());
    assert!(handle.take_live_prepared().is_none());
    assert!(!handle.retire_failed_bootstrap_live([2; 16]));
    assert!(matches!(
        handle.try_enqueue(op),
        Err(QueueBusy::Unavailable)
    ));
    assert!(handle.try_prepare_cleanup().is_err());
    assert!(handle.fail_cleanup().is_err());
    assert!(
        handle
            .ack_committed(ack(1, CommittedDecision::DefinitelyUnsubmitted))
            .is_err()
    );
    assert!(matches!(controls.try_recv(), Err(TryRecvError::Empty)));
    fatal_join(handle).wait().unwrap();
}

#[test]
fn late_finite_failure_never_updates_bookkeeping_or_emits() {
    let (handle, operations, controls, events) = handle();
    let (_live_in, live) = mpsc::sync_channel(CAPACITY);
    let (trip, fatal) = Trip::new();
    let checks = Arc::new(AtomicUsize::new(0));
    let admitted = checks.clone();
    let mut worker = scheduler();
    worker.mode = Mode {
        checked: Some(Admission::new(fatal, move || {
            if admitted.fetch_add(1, Ordering::Relaxed) == 1 {
                trip.trip();
            }
            true
        })),
    };

    let mut op = operation(1, EngineOperation::Observe);
    op.engine_known = true;
    handle.operations.try_send(op).unwrap();
    // The scheduler fixture has no engine. The first turn therefore produces
    // an actual Unavailable result, and the next check observes the fatal cut.
    worker.run(operations, live, controls, events);

    assert_eq!(checks.load(Ordering::Relaxed), 2);
    assert!(worker.pending.is_empty());
    assert!(worker.known.is_empty());
    assert!(worker.unresolved.is_empty());
    assert!(worker.awaiting.is_empty());
    assert!(matches!(
        handle.events.try_recv(),
        Err(TryRecvError::Disconnected)
    ));
}

#[test]
fn owner_retirement_drops_healthy_slots_and_parks_fatal_slots() {
    struct Sentinel<'a>(&'a AtomicUsize);
    impl Drop for Sentinel<'_> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    let (trip, fatal) = Trip::new();
    let mode = checked(fatal);
    let drops = AtomicUsize::new(0);
    mode.retire_storage((Sentinel(&drops), Sentinel(&drops)));
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    trip.trip();
    mode.retire_storage((Sentinel(&drops), Sentinel(&drops)));
    assert_eq!(drops.load(Ordering::Relaxed), 2);
}

#[test]
fn handle_finished_before_report_still_selects_fatal() {
    let (mut handle, _operations, _controls, _events) = handle();
    let (trip, fatal) = Trip::new();
    handle.mode = checked(fatal);
    assert!(matches!(
        handle.stop_outcome(deadline()),
        Ok(StopOutcome::Ordinary(StopReport { complete: true, .. }))
    ));
    trip.trip();
    assert!(matches!(
        handle.stop_outcome(deadline()),
        Ok(StopOutcome::Fatal)
    ));
    fatal_join(handle).wait().unwrap();
}

#[test]
fn cut_after_admitted_stop_parks_real_engine_until_process_exit() {
    const CHILD: &str = "MARMISH_TEST_FATAL_STOP_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                concat!(
                    module_path!(),
                    "::cut_after_admitted_stop_parks_real_engine_until_process_exit"
                )
                .trim_start_matches("marmish_native::"),
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            status.status.success() && String::from_utf8_lossy(&status.stdout).contains("1 passed"),
            "{}
{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        return;
    }

    let root = tempfile::tempdir().unwrap();
    let settings = engine_settings(&root);
    let worker_engine = reopen(&settings).unwrap();
    let (mut handle, operations, controls, events) = handle();
    let (_live_in, live) = mpsc::sync_channel(CAPACITY);
    let (trip, fatal) = Trip::new();
    handle.mode = checked(fatal);
    let (stopping, wait_stopping) = mpsc::sync_channel(1);
    let (release, wait_release) = mpsc::sync_channel(1);
    let mode = handle.mode.clone();
    handle.thread = Some(thread::spawn(move || {
        let mut worker = Worker::with_mode(worker_engine, vec![], mode);
        worker.run(operations, live, controls, events);
        stopping.send(()).unwrap();
        wait_release.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(worker);
    }));

    handle.controls.try_send(Control::Stop).unwrap();
    wait_stopping.recv_timeout(Duration::from_secs(2)).unwrap();
    trip.trip();
    assert!(matches!(
        handle.stop_outcome(deadline()),
        Ok(StopOutcome::Fatal)
    ));
    let joined = fatal_join(handle);
    release.send(()).unwrap();
    joined.wait().unwrap();
    // The real engine's lock is still present in this disposable process.
    assert!(matches!(
        reopen(&settings),
        Err(engine::EngineError::AlreadyOpen)
    ));
}
