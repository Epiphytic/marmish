use super::*;

fn child(command: &[u8]) -> SpawnedChild {
    let account = launcher::local_account(Some(std::path::Path::new("/tmp"))).unwrap();
    launcher::spawn_child(&ChildSpec {
        environment: launcher::child_environment(&account),
        cwd: account.cwd.clone(),
        account,
        request: CommandRequest::Exec {
            argv: vec![b"/bin/sh".to_vec(), b"-c".to_vec(), command.to_vec()],
        },
        umask: 0o077,
    })
    .unwrap()
}
fn owner(spawned: SpawnedChild) -> (ChildOwner, ChildStdin, File, File) {
    let SpawnedChild {
        child,
        identity,
        pgid,
        stdin,
        stdout,
        stderr,
    } = spawned;
    (
        ChildOwner {
            child,
            identity,
            pgid,
            worker: launcher::process_identity(std::process::id() as i32).unwrap(),
            reaped: false,
        },
        stdin,
        pipe_file(stdout).unwrap(),
        pipe_file(stderr).unwrap(),
    )
}
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "bounded process operation timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn separate_pipes_preserve_bytes_and_reap_status() {
    let (mut owner, stdin, mut stdout, mut stderr) =
        owner(child(b"printf 'a\\000b'; printf 'err' >&2; exit 7"));
    drop(stdin);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_eof = false;
    let mut err_eof = false;
    until(|| {
        for (pipe, bytes, eof) in [
            (&mut stdout, &mut out, &mut out_eof),
            (&mut stderr, &mut err, &mut err_eof),
        ] {
            let mut buf = [0; 256];
            match pipe.read(&mut buf) {
                Ok(0) => *eof = true,
                Ok(n) => bytes.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => panic!("pipe: {e}"),
            }
        }
        owner.exited().unwrap() && out_eof && err_eof
    });
    owner.signal(libc::SIGKILL).unwrap();
    assert_eq!(owner.reap().unwrap(), ExitOutcome::Code(7));
    assert_eq!(out, b"a\0b");
    assert_eq!(err, b"err");
}
#[test]
fn unreaped_leader_reserves_group_for_pipe_holding_descendant_cleanup() {
    let (mut owner, stdin, mut stdout, _stderr) = owner(child(b"/bin/sleep 30 & exit 0"));
    drop(stdin);
    until(|| owner.exited().unwrap());
    let mut buf = [0; 16];
    assert_eq!(
        stdout.read(&mut buf).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    owner.signal(libc::SIGKILL).unwrap();
    until(|| matches!(stdout.read(&mut buf), Ok(0)));
    assert_eq!(owner.reap().unwrap(), ExitOutcome::Code(0));
}
#[test]
fn forged_identity_never_signals_and_drop_cleans_verified_child() {
    let (mut owner, _stdin, _stdout, _stderr) = owner(child(b"exec /bin/sleep 30"));
    let original = owner.identity.clone();
    owner.identity.start_ticks += 1;
    assert_eq!(
        owner.signal(libc::SIGKILL),
        Err(EngineError::RecoveryUncertain)
    );
    owner.identity = original.clone();
    assert!(!owner.exited().unwrap());
    drop(owner);
    assert!(launcher::process_identity(original.pid).is_err());
}

fn with_claimed_command(
    command: &[u8],
    exercise: impl FnOnce(
        &StateRoot,
        JobId,
        &mut Store,
        &ProcessIdentity,
        crate::store::WorkerClaim,
        SpawnedChild,
        crate::store::OwnerKey,
    ),
) {
    use crate::store::{InputOwner, OwnerKey, WorkerCredential};
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = StateRoot::open(temp.path()).unwrap();
    let frontend = root.lock_frontend().unwrap();
    let keys = root.initialize_storage(&frontend).unwrap();
    let account = launcher::local_account(Some(temp.path())).unwrap();
    let policy = EnginePolicy {
        state_root: temp.path().to_path_buf(),
        worker_executable: PathBuf::from("/usr/bin/true"),
        server: ServerId([2; 32]),
        allowed_principals: vec![PrincipalId([3; 32])],
        allow_shell_command: false,
        cwd: Some(temp.path().to_path_buf()),
        limits: EngineLimits::default(),
    };
    let epoch = [4; 16];
    let mut store = Store::create_frontend(&root, &keys, &policy, &account, epoch).unwrap();
    let owner = InputOwner {
        owner: OwnerKey {
            principal: PrincipalId([3; 32]),
            server: policy.server,
            target_uid: account.uid,
        },
        endpoint: EndpointId([5; 32]),
    };
    let now = launcher::boot_time().unwrap();
    let mut deadline = now;
    deadline.millis += 60_000;
    let hash = [6; 32];
    store
        .issue_token(&owner, epoch, hash, now, deadline)
        .unwrap();
    let request = CommandRequest::Exec {
        argv: vec![b"/bin/sh".to_vec(), b"-c".to_vec(), command.to_vec()],
    };
    let job = JobId([7; 16]);
    let credential = WorkerCredential([8; 32]);
    root.prepare_job(job, &credential).unwrap();
    let _guard = root.lock_job(job).unwrap();
    store
        .admit(
            &owner,
            epoch,
            hash,
            now,
            InvocationId([9; 16]),
            &request,
            job,
            crate::admission::sha256(&credential.0),
        )
        .unwrap();
    let worker = launcher::process_identity(std::process::id() as i32).unwrap();
    let ClaimResult::Claimed(claim) = store.claim_worker(job, &credential, &worker).unwrap() else {
        panic!("claim")
    };
    let spec = ChildSpec {
        environment: launcher::child_environment(&account),
        cwd: account.cwd.clone(),
        account,
        request,
        umask: 0o077,
    };
    let spawned = launcher::spawn_child(&spec).unwrap();
    exercise(&root, job, &mut store, &worker, claim, spawned, owner.owner);
}

#[test]
fn complete_supervision_commits_shell_exit_and_separate_output() {
    with_claimed_command(
        b"printf 'out\\000'; printf 'err\\000' >&2; exit 7",
        |root, job, store, worker, claim, spawned, owner| {
            assert_eq!(
                supervise(
                    root,
                    &root.load_storage_keys().unwrap(),
                    job,
                    store,
                    worker,
                    claim,
                    spawned
                ),
                Ok(())
            );
            assert_eq!(
                store.status(&owner, job).unwrap().outcome,
                Some(ExitOutcome::Code(7))
            );
        },
    );
}

#[test]
fn stale_progress_reloads_rotation_but_same_generation_conflict_is_fatal() {
    with_claimed_command(
        b"exec /bin/sleep 30",
        |_, job, store, worker, _, spawned, owner_key| {
            let (child, stdin, _out, _err) = owner(spawned);
            store
                .mark_running(job, worker, &child.identity, child.pgid)
                .unwrap();
            let input_owner = crate::store::InputOwner {
                owner: owner_key,
                endpoint: EndpointId([5; 32]),
            };
            let generation = InputGeneration([10; 16]);
            store.begin_input(job, &input_owner, generation).unwrap();
            let now = launcher::boot_time().unwrap();
            let deadline = crate::store::BootTime {
                millis: now.millis + 60_000,
                ..now
            };
            store
                .queue_input(job, &input_owner, generation, 0, b"abc", now, deadline)
                .unwrap();
            let InputWork::Deliver(delivery) = store.take_input(job, worker, now).unwrap() else {
                panic!("delivery")
            };
            let mut input = InputPump {
                pipe: Some(stdin),
                delivery: Some(delivery),
                written: 1,
                progress_pending: true,
                failure: None,
                close_pending: false,
                deferred: Deferred::new(),
            };
            // A same-generation unknown record is an invariant error, not rotation.
            let record = input.delivery.as_ref().unwrap().record;
            input.delivery.as_mut().unwrap().record = i64::MAX;
            assert!(matches!(
                input.persist_progress(job, store, worker),
                Err(EngineError::InputConflict | EngineError::StaleGeneration)
            ));
            input.delivery.as_mut().unwrap().record = record;
            store.fail_input(job, worker, generation, true).unwrap();
            let replacement = InputGeneration([11; 16]);
            store.begin_input(job, &input_owner, replacement).unwrap();
            assert!(!input.persist_progress(job, store, worker).unwrap());
            assert!(input.persist_progress(job, store, worker).unwrap());
            assert!(input.delivery.is_none());
            assert_eq!(
                store.controls(job, worker).unwrap().generation,
                Some(replacement)
            );
        },
    );
}

#[test]
fn quiet_worker_interval_polls_without_database_writes() {
    with_claimed_command(
        b"exec /bin/sleep 30",
        |root, job, store, worker, claim, spawned, _| {
            let (child, stdin, stdout, stderr) = owner(spawned);
            store
                .mark_running(job, worker, &child.identity, child.pgid)
                .unwrap();
            let input = InputPump {
                pipe: Some(stdin),
                delivery: None,
                written: 0,
                progress_pending: false,
                failure: None,
                close_pending: false,
                deferred: Deferred::new(),
            };
            let mut out = OutputPump::open(
                root,
                &root.load_storage_keys().unwrap(),
                job,
                store,
                worker,
                Stream::Stdout,
                stdout,
                claim.limits.spool_bytes_per_stream,
            )
            .unwrap();
            let mut err = OutputPump::open(
                root,
                &root.load_storage_keys().unwrap(),
                job,
                store,
                worker,
                Stream::Stderr,
                stderr,
                claim.limits.spool_bytes_per_stream,
            )
            .unwrap();
            let baseline = store.test_total_changes();
            let mut cadence = ControlCadence::new();
            let mut polls = 0;
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(800) {
                if cadence.due() {
                    assert!(!store.controls(job, worker).unwrap().cancel_requested);
                    assert!(matches!(
                        store
                            .take_input(job, worker, launcher::boot_time().unwrap())
                            .unwrap(),
                        InputWork::Idle
                    ));
                    polls += 1;
                    cadence.polled();
                }
                assert_eq!(out.drain().unwrap() + err.drain().unwrap(), 0);
                out.flush(root, job, store, worker).unwrap();
                err.flush(root, job, store, worker).unwrap();
                wait_for_io(&out, &err, &input, cadence.until_poll()).unwrap();
            }
            // Assert the real SQLite write count, including idle input and empty
            // output flushing, rather than inferring it from timer configuration.
            assert_eq!(store.test_total_changes(), baseline);
            assert!((3..=8).contains(&polls), "quiet control polls: {polls}");
        },
    );
}

#[test]
fn expired_payload_after_queued_eof_closes_child_stdin_and_preserves_gap() {
    // The watchdog makes the original hanging behavior fail with a signal
    // outcome instead of hanging the test suite. It inherits none of our pipes.
    with_claimed_command(
        b"(/bin/sleep 3; kill -TERM \"$$\") </dev/null >/dev/null 2>&1 & exec /bin/cat",
        |root, job, store, worker, claim, spawned, owner_key| {
            let input_owner = crate::store::InputOwner {
                owner: owner_key.clone(),
                endpoint: EndpointId([5; 32]),
            };
            let generation = InputGeneration([12; 16]);
            store.begin_input(job, &input_owner, generation).unwrap();
            let now = launcher::boot_time().unwrap();
            let deadline = crate::store::BootTime {
                millis: now.millis + 1,
                ..now
            };
            store
                .queue_input(job, &input_owner, generation, 0, b"expired", now, deadline)
                .unwrap();
            store.queue_eof(job, &input_owner, generation, 7).unwrap();
            std::thread::sleep(Duration::from_millis(5));
            supervise(
                root,
                &root.load_storage_keys().unwrap(),
                job,
                store,
                worker,
                claim,
                spawned,
            )
            .unwrap();
            let status = store.status(&owner_key, job).unwrap();
            assert_eq!(status.outcome, Some(ExitOutcome::Code(0)));
            assert_eq!(status.input.health, InputHealth::Gap);
            assert_eq!(status.input.delivered, 0);
            assert!(status.input.gap_count > 0);
            assert_eq!(status.stdout.produced, 0);
            assert_eq!(
                store
                    .begin_input(job, &input_owner, InputGeneration([13; 16]))
                    .unwrap_err(),
                EngineError::InputClosed
            );
        },
    );
}

#[test]
fn input_control_backoff_waits_instead_of_polling_writable_pipe() {
    with_claimed_command(
        b"exec /bin/sleep 30",
        |root, job, store, worker, claim, spawned, _| {
            let (child, stdin, stdout, stderr) = owner(spawned);
            store
                .mark_running(job, worker, &child.identity, child.pgid)
                .unwrap();
            let mut input = InputPump {
                pipe: Some(stdin),
                delivery: Some(Delivery {
                    record: 1,
                    generation: InputGeneration([1; 16]),
                    offset: 0,
                    bytes: b"a".to_vec(),
                    deadline: launcher::boot_time().unwrap(),
                }),
                written: 0,
                progress_pending: false,
                failure: None,
                close_pending: false,
                deferred: Deferred::new(),
            };
            let out = OutputPump::open(
                root,
                &root.load_storage_keys().unwrap(),
                job,
                store,
                worker,
                Stream::Stdout,
                stdout,
                claim.limits.spool_bytes_per_stream,
            )
            .unwrap();
            let err = OutputPump::open(
                root,
                &root.load_storage_keys().unwrap(),
                job,
                store,
                worker,
                Stream::Stderr,
                stderr,
                claim.limits.spool_bytes_per_stream,
            )
            .unwrap();
            input.deferred.next = Instant::now() + Duration::from_secs(1);
            let started = Instant::now();
            wait_for_io(&out, &err, &input, Duration::from_millis(50)).unwrap();
            assert!(
                started.elapsed() >= Duration::from_millis(30),
                "writable stdin bypassed control backoff"
            );
        },
    );
}

#[test]
fn failed_launch_state_is_classified_without_mismatched_cas_retry() {
    with_claimed_command(b"exec /bin/sleep 30", |_, job, store, _, _, spawned, _| {
        let (child, _stdin, _stdout, _stderr) = owner(spawned);
        let started = Instant::now();
        mark_worker_failure(store, job).unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            store.job_state(job).unwrap(),
            InvocationState::UnknownLaunch
        );
        mark_worker_failure(store, job).unwrap();
        drop(child);
    });
}
