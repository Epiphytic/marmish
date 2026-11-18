use super::io::Input;
use super::*;
use crate::terminal_fence::{AttachmentFence, FenceState};
use guard::Sample;
use io::Completion;
use tokio::time::{sleep, timeout};

fn seconds(value: u64) -> Duration {
    Duration::from_secs(value)
}

fn at(value: u64) -> Option<Sample> {
    Some(Sample {
        continuous: seconds(value),
        wall: seconds(1000 + value),
    })
}

fn token(byte: u8) -> SourceId {
    SourceId {
        leaf: [byte; 32],
        epoch: 2,
        subscription: NonZeroU64::MIN,
    }
}

pub(super) fn source(byte: u8) -> (Arc<Source>, Arc<AttachmentFence>) {
    let command = AttachmentFence::new(NonZeroU64::MIN, [9; 32]);
    (Source::new(token(byte), Arc::clone(&command)), command)
}

pub(super) fn claim() -> Claim {
    Claim {
        random: [4; 16],
        generation: 1,
    }
}

#[test]
fn terminal_runtime_core_revoked_binding_cannot_commit() {
    let (old, fence) = source(5);
    let old_claim = claim();
    assert_eq!(
        old.install(old_claim, at(1).unwrap()),
        GuardResult::Applied(())
    );
    let effects = std::cell::Cell::new(0);
    let apply = || effects.set(effects.get() + 1);
    let update = fence.begin_update().unwrap();
    assert_eq!(
        old.with_effect(old_claim, seconds(9), || at(2), apply),
        GuardResult::Withheld
    );
    assert_eq!(update.resolve(None), FenceState::Retired);
    assert_eq!(
        old.with_effect(old_claim, seconds(9), || at(3), apply),
        GuardResult::Retired(Reason::Authority)
    );

    let (successor, _) = source(6);
    let successor_claim = Claim {
        generation: 2,
        ..old_claim
    };
    assert_eq!(
        successor.install(successor_claim, at(3).unwrap()),
        GuardResult::Applied(())
    );
    assert_eq!(
        successor.with_effect(successor_claim, seconds(9), || at(4), apply),
        GuardResult::Applied(())
    );
    assert_eq!(
        old.with_effect(old_claim, seconds(9), || at(5), apply),
        GuardResult::Retired(Reason::Authority)
    );
    assert_eq!(effects.get(), 1);
}

#[test]
fn core_tests_terminal_live_supervisor_exact_clock_and_authority() {
    let (source, fence) = source(5);
    let claim = claim();
    assert_eq!(
        source.install(claim, at(30).unwrap()),
        GuardResult::Applied(())
    );
    assert_eq!(
        source.renew(token(8), claim, seconds(31), || at(31)),
        GuardResult::Applied(false)
    );
    assert_eq!(
        source.renew(token(5), claim, seconds(32), || at(31)),
        GuardResult::Applied(false)
    );
    assert_eq!(
        source.renew(token(5), claim, seconds(31), || at(31)),
        GuardResult::Applied(true)
    );
    assert_eq!(
        source.renew(token(5), claim, seconds(31), || at(32)),
        GuardResult::Applied(false)
    );

    let suspect = fence.begin_update().unwrap();
    assert_eq!(
        source.with_effect(
            claim,
            seconds(45),
            || at(44),
            || panic!("suspect allowed an effect")
        ),
        GuardResult::<()>::Withheld
    );
    assert_eq!(suspect.resolve(Some([9; 32])), FenceState::Live);
    assert_eq!(
        source.with_effect(claim, seconds(45), || at(44), || 1),
        GuardResult::Applied(1)
    );
    assert_eq!(
        source.with_effect(
            claim,
            seconds(45),
            || at(45),
            || panic!("expired head replayed")
        ),
        GuardResult::<()>::Retired(Reason::Deadline)
    );
    assert_eq!(
        source.with_effect(
            claim,
            seconds(80),
            || at(46),
            || panic!("retired source revived")
        ),
        GuardResult::<()>::Retired(Reason::Deadline)
    );
}

#[test]
fn core_tests_terminal_live_supervisor_suspend_drift_and_revoke() {
    for (clock, reason) in [
        (at(661), Reason::Idle),
        (at(0), Reason::Clock),
        (None, Reason::Clock),
        (
            Some(Sample {
                continuous: seconds(22),
                wall: seconds(1012),
            }),
            Reason::Clock,
        ),
    ] {
        let (source, _) = source(5);
        let claim = claim();
        assert_eq!(
            source.install(claim, at(1).unwrap()),
            GuardResult::Applied(())
        );
        assert_eq!(source.health(claim, || clock), GuardResult::Retired(reason));
        assert_eq!(
            source.renew(token(5), claim, seconds(2), || at(2)),
            GuardResult::Retired(reason)
        );
    }
    let (live, fence) = source(8);
    let claim = claim();
    assert_eq!(
        live.install(claim, at(1).unwrap()),
        GuardResult::Applied(())
    );
    fence.retire();
    assert_eq!(
        live.with_effect(
            claim,
            seconds(99),
            || at(2),
            || panic!("retired Command effect")
        ),
        GuardResult::<()>::Retired(Reason::Authority)
    );
    let (budgeted, _) = source(6);
    assert_eq!(
        budgeted.install(claim, at(1).unwrap()),
        GuardResult::Applied(())
    );
    let lifetime = 8 * 60 * 60;
    for value in (600..lifetime).step_by(600) {
        assert_eq!(
            budgeted.renew(token(6), claim, seconds(value), || at(value)),
            GuardResult::Applied(true)
        );
    }
    assert_eq!(
        budgeted.health(claim, || at(lifetime + 1)),
        GuardResult::Retired(Reason::Budget)
    );
}

#[test]
fn core_tests_terminal_live_supervisor_revocation_and_withheld_head() {
    let (source, fence) = source(9);
    let claim = claim();
    assert_eq!(
        source.install(claim, at(1).unwrap()),
        GuardResult::Applied(())
    );
    let (entered, entered_rx) = mpsc::sync_channel(0);
    let (finish, finish_rx) = mpsc::sync_channel(0);
    let (retired, retired_rx) = mpsc::sync_channel(0);
    thread::scope(|scope| {
        let scoped_source = Arc::clone(&source);
        scope.spawn(move || {
            let _ = scoped_source.with_effect(
                claim,
                seconds(10),
                || at(2),
                || {
                    entered.send(()).unwrap();
                    finish_rx.recv_timeout(seconds(2)).unwrap();
                },
            );
        });
        entered_rx.recv_timeout(seconds(2)).unwrap();
        let command = Arc::clone(&fence);
        scope.spawn(move || {
            command.retire();
            retired.send(()).unwrap();
        });
        let until = Instant::now() + seconds(1);
        while fence.state() != FenceState::Retired {
            assert!(Instant::now() < until);
            thread::yield_now();
        }
        assert!(retired_rx.try_recv().is_err());
        assert_eq!(
            source.with_effect(
                claim,
                seconds(10),
                || at(2),
                || panic!("contended a native turn")
            ),
            GuardResult::<()>::Withheld,
        );
        finish.send(()).unwrap();
        retired_rx.recv_timeout(seconds(2)).unwrap();
    });
    assert_eq!(
        source.with_effect(
            claim,
            seconds(10),
            || at(2),
            || panic!("native turn after retirement")
        ),
        GuardResult::<()>::Retired(Reason::Authority)
    );
}

fn start_request(account: Account, selection: StartChoice, source: Arc<Source>) -> VerifiedStart {
    VerifiedStart {
        account,
        selection,
        rows: 24,
        cols: 80,
        source,
        deadline: native_now() + seconds(8),
        room_hold: None,
    }
}

#[test]
fn core_tests_terminal_live_supervisor_bounded_independent_queue_and_cancel() {
    let (starts, requests) = mpsc::sync_channel(MAX_LIVE);
    let (releases, release_requests) = mpsc::sync_channel(2);
    let stopping = Arc::new(AtomicBool::new(false));
    let control = LiveControl {
        start: starts,
        confirm: mpsc::sync_channel(MAX_LIVE).0,
        start_limit: Arc::default(),
        release: releases,
        owner: thread::current(),
        stopping: Arc::clone(&stopping),
        submission: Arc::new(Mutex::new(())),
    };
    let (source, _) = source(1);
    let make = || start_request([2; 32], StartChoice::New, Arc::clone(&source));
    let replies: Vec<_> = (0..MAX_LIVE)
        .map(|_| {
            let (send, reply) = oneshot::channel();
            assert_eq!(control.submit_start(make(), send), Ok(()));
            reply
        })
        .collect();
    assert_eq!(
        control.submit_start(make(), oneshot::channel().0),
        Err(QueueError::Full)
    );
    for id in [[1; 16], [2; 16]] {
        assert_eq!(control.release(SlotId(id), oneshot::channel().0), Ok(()));
    }
    assert_eq!(
        control.release(SlotId([3; 16]), oneshot::channel().0),
        Err(QueueError::Full)
    );
    assert!(release_requests.try_recv().is_ok());
    let (cancelled, reply, _permit) = requests.try_recv().unwrap();
    drop(replies);
    assert!(matches!(
        AdmissionBudget::open(&cancelled, &reply, &stopping),
        Err(Error::Invalid("terminal_owner_unavailable"))
    ));
    stopping.store(true, Ordering::Release);
    assert_eq!(
        control.release(SlotId([3; 16]), oneshot::channel().0),
        Err(QueueError::Stopped)
    );
}

#[test]
fn core_tests_terminal_live_supervisor_stop_answers_queued_start_and_release() {
    let (start, starts) = mpsc::sync_channel(1);
    let (release, releases) = mpsc::sync_channel(1);
    let (source, _) = source(1);
    let (send, reply) = oneshot::channel();
    start
        .try_send((
            start_request([2; 32], StartChoice::New, source),
            send,
            Arc::new(StartLimit::default()).try_acquire().unwrap(),
        ))
        .ok()
        .unwrap();
    let (send, released) = oneshot::channel();
    release.try_send((SlotId([1; 16]), send)).unwrap();
    answer_stopped(&starts, &releases);
    assert!(matches!(
        reply.blocking_recv().unwrap(),
        StartOutcome::Refused(Error::Invalid("terminal_owner_unavailable"))
    ));
    assert!(matches!(
        released.blocking_recv().unwrap(),
        Err(Error::Invalid("terminal_owner_unavailable"))
    ));
    assert!(matches!(starts.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(matches!(
        releases.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn core_tests_terminal_live_confirmation_bypasses_held_start_lock_and_is_exact() {
    let (wake, _reader) = Wake::open().unwrap();
    let (source, _) = source(1);
    let port = Port::new(Arc::clone(&source), wake);
    let (confirm, confirms) = mpsc::sync_channel(MAX_LIVE);
    let control = LiveControl {
        start: mpsc::sync_channel(MAX_LIVE).0,
        confirm,
        start_limit: Arc::default(),
        release: mpsc::sync_channel(2).0,
        owner: thread::current(),
        stopping: Arc::default(),
        submission: Arc::new(Mutex::new(())),
    };
    let slot = SlotId([2; 16]);
    let confirmation = Confirmation::new(seconds(10));
    let mut admitted = AdmittedTerminal {
        slot,
        claim: claim(),
        port: Arc::clone(&port),
        role: Role::Writer,
        confirmation: Some(Arc::clone(&confirmation)),
        confirmation_sent: false,
        closed_reported: false,
    };
    let link = Active {
        account: [3; 32],
        inventory: tmux::Inventory {
            session: 1,
            pid: 2,
            directory: (3, 4),
            socket: (5, 6),
        },
        writer: None,
        claim: claim(),
        role: Role::Writer,
        confirmation: Some(Arc::clone(&confirmation)),
        pending_start: None,
        port,
        retiring: None,
    };
    let _held = control.submission.lock().unwrap();
    let response = admitted.submit_confirm(&control, seconds(9)).unwrap();
    assert!(matches!(
        admitted.submit_confirm(&control, seconds(9)),
        Err(QueueError::Stopped)
    ));
    assert!(admitted.event(true).is_none());
    assert!(matches!(
        admitted
            .submit(Operation::new(1, seconds(20), Input::Resize { rows: 24, cols: 80 }).unwrap()),
        Err(PushError::Closed(Reason::Authority))
    ));
    let mut request = confirms.try_recv().unwrap();
    assert!(request.matches(&link, slot));
    assert!(!request.matches(&link, SlotId([3; 16])));
    request.role = Role::Observer;
    assert!(!request.matches(&link, slot));
    request.role = Role::Writer;
    request.claim.generation += 1;
    assert!(!request.matches(&link, slot));
    request.claim = claim();
    request.source = Source::new(source.id, AttachmentFence::new(NonZeroU64::MIN, [9; 32]));
    assert!(!request.matches(&link, slot));
    assert_eq!(request.refusal(&confirmation, Some(seconds(8))), None);
    assert_eq!(
        request.refusal(&confirmation, Some(seconds(9))),
        Some(Reason::Deadline)
    );
    drop(response);
    assert_eq!(
        request.refusal(&confirmation, Some(seconds(8))),
        Some(Reason::Detached)
    );
}

#[test]
fn core_tests_terminal_live_confirmation_full_is_an_invariant_failure() {
    let (confirm, _confirmations) = mpsc::sync_channel(0);
    let control = LiveControl {
        start: mpsc::sync_channel(0).0,
        confirm,
        start_limit: Arc::default(),
        release: mpsc::sync_channel(0).0,
        owner: thread::current(),
        stopping: Arc::default(),
        submission: Arc::new(Mutex::new(())),
    };
    let (source, _) = source(1);
    let (reply, _response) = oneshot::channel();
    assert_eq!(
        control.submit_confirm(confirm::Request {
            slot: SlotId([1; 16]),
            claim: claim(),
            role: Role::Writer,
            source: Arc::clone(&source),
            until: seconds(10),
            reply,
        }),
        Err(QueueError::Full)
    );
    assert_eq!(source.reason(), Some(Reason::OwnerLost));
    assert!(control.stopping.load(Ordering::Acquire));
}

fn native_now() -> Duration {
    guard::sample().unwrap().continuous
}

async fn submit(control: &LiveControl, request: VerifiedStart) -> StartOutcome {
    let (send, reply) = oneshot::channel();
    control.submit_start(request, send).unwrap();
    timeout(seconds(9), reply).await.unwrap().unwrap()
}

fn attached(outcome: StartOutcome) -> AdmittedTerminal {
    match outcome {
        StartOutcome::Attached(attached) => attached,
        StartOutcome::Busy { .. } => panic!("unexpected occupied slot"),
        StartOutcome::Refused(error) => panic!("native start refused: {error:?}"),
    }
}

pub(super) async fn event(claim: &mut AdmittedTerminal) -> Event {
    timeout(seconds(5), async {
        loop {
            if let Some(event) = claim.try_event() {
                return event;
            }
            claim.changed().await;
        }
    })
    .await
    .expect("native event deadline")
}

pub(super) async fn send_input(claim: &AdmittedTerminal, mut operation: Operation) {
    loop {
        assert!(
            native_now() < operation.deadline,
            "native input admission deadline"
        );
        match claim.submit(operation) {
            Ok(()) => return,
            Err(PushError::Contended(original)) => {
                operation = original;
                sleep(Duration::from_millis(1)).await;
            }
            Err(PushError::Full) => panic!("input slot unexpectedly full"),
            Err(PushError::Closed(reason)) => panic!("input slot closed: {reason:?}"),
        }
    }
}

async fn exchange(
    claim: &mut AdmittedTerminal,
    sequence: u64,
    input: &'static [u8],
    marker: &[u8],
) -> Duration {
    let start = Instant::now();
    let until = native_now() + seconds(4);
    send_input(
        claim,
        Operation::new(sequence, until, Input::Bytes(input.to_vec())).unwrap(),
    )
    .await;
    let duplicate =
        Operation::new(sequence + 1, until, Input::Resize { rows: 30, cols: 90 }).unwrap();
    assert!(matches!(
        claim.submit(duplicate),
        Err(PushError::Full | PushError::Contended(_))
    ));
    let mut output = Vec::new();
    let mut complete = false;
    while !complete || !output.windows(marker.len()).any(|bytes| bytes == marker) {
        assert!(output.len() < 65536, "output fixture cap");
        match event(claim).await {
            Event::Output(bytes) => output.extend_from_slice(&bytes),
            Event::Complete(Completion::Written(n, count)) => {
                assert_eq!((n, count), (sequence, input.len()));
                assert!(!complete);
                complete = true;
            }
            event => panic!("unexpected native event: {event:?}"),
        }
    }
    start.elapsed()
}

async fn stop_and_recover(owner: &mut LiveSupervisor, profile: Arc<ProfileLease>) -> ProfileLease {
    owner.stop();
    timeout(seconds(5), async {
        loop {
            if let Some(result) = owner.join_if_finished() {
                result.unwrap();
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    match Arc::try_unwrap(profile) {
        Ok(profile) => profile,
        Err(_) => panic!("native owner did not release its lease"),
    }
}

#[tokio::test]
#[ignore = "uses real private tmux, SQLCipher and a file-backed profile"]
async fn terminal_live_supervisor_cancel_after_prepare_is_recoverable() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let fixture = super::super::tests::Fixture::new().await;
    let (profile, material) = fixture.identity();
    let account = fixture.accounts[0];
    let (wake, _reader) = Wake::open().unwrap();
    let mut owner = ControlOwner {
        config: CommandConfig::load(profile.root()).unwrap(),
        registry: Registry::open(&profile, &material).unwrap(),
        manager: Manager::new(&fixture.tmux, profile.root()).unwrap(),
        daemon: fixture.daemon.clone(),
        active: HashMap::new(),
        offers: Ledger::default(),
        parked: VecDeque::new(),
        departure: VecDeque::new(),
        quarantine: HashSet::new(),
        wake,
        stopping: Arc::new(AtomicBool::new(false)),
        profile: Arc::new(profile),
    };
    let cancelled = Slot {
        id: SlotId([201; 16]),
        account,
        default: true,
        marker: [204; 16],
        state: State::Prepared,
        proof: None,
    };
    owner.registry.prepare(cancelled).unwrap();
    let until = Instant::now() + seconds(3);
    assert!(matches!(
        owner
            .manager
            .create_until(cancelled, 24, 80, until, || Err(STOPPED)),
        Err(Error::Invalid("terminal_owner_unavailable"))
    ));
    let socket = owner.manager.socket(cancelled.id);
    assert!(!socket.parent().unwrap().exists());
    let unexpected = Slot {
        id: SlotId([202; 16]),
        default: false,
        ..cancelled
    };
    owner.registry.prepare(unexpected).unwrap();
    let possible = owner.manager.socket(unexpected.id);
    let directory = possible.parent().unwrap();
    fs::write(directory, b"fixture").unwrap();
    assert!(owner.release(unexpected.id).is_err());
    assert!(directory.is_file());
    fs::remove_file(directory).unwrap();
    fs::create_dir(directory).unwrap();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o000)).unwrap();
    assert!(owner.release(unexpected.id).is_err());
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir(directory).unwrap();
    owner.release(unexpected.id).unwrap();

    let (source, _) = source(1);
    let start = start_request(account, StartChoice::Default, source);
    let (reply, _receiver) = oneshot::channel();
    let stopping = Arc::clone(&owner.stopping);
    let budget = AdmissionBudget::open(&start, &reply, &stopping).unwrap();
    let rows = owner.registry.slots().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].id == cancelled.id);
    let (slot, _) = owner
        .select(&budget, rows.len(), Some(cancelled), true)
        .unwrap();
    assert!(slot != cancelled.id);
    let rows = owner.registry.slots().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].id == slot && rows[0].proof.is_some());
}

#[tokio::test]
#[ignore = "uses real private tmux and native marmishd; set MARMISH_TEST_DAEMON"]
async fn terminal_live_supervisor_default_new_busy_restart() {
    let fixture = super::super::tests::Fixture::new().await;
    let (profile, material) = fixture.identity();
    let profile = Arc::new(profile);
    let account = fixture.accounts[0];
    let mut owner = LiveSupervisor::start(
        Arc::clone(&profile),
        &material,
        &fixture.daemon,
        &fixture.tmux,
    )
    .unwrap();
    let (first, fence) = source(1);
    let started = Instant::now();
    let mut default = attached(
        submit(
            &owner.control,
            start_request(account, StartChoice::Default, Arc::clone(&first)),
        )
        .await,
    );
    let cold = started.elapsed();
    let (second, _) = source(2);
    assert!(matches!(
        submit(&owner.control, start_request(account, StartChoice::Default, second)).await,
        StartOutcome::Busy { slot, writer, .. } if slot == default.slot && writer == default.claim
    ));
    let mut samples = vec![];
    for sequence in 1..=100 {
        samples.push(
            exchange(
                &mut default,
                sequence,
                b"state=MARK; printf '[%s]\\n' \"$state\"\r",
                b"[MARK]",
            )
            .await,
        );
    }
    let until = native_now() + seconds(4);
    send_input(
        &default,
        Operation::new(101, until, Input::Resize { rows: 30, cols: 90 }).unwrap(),
    )
    .await;
    loop {
        match event(&mut default).await {
            Event::Complete(Completion::Resized(101)) => break,
            Event::Output(_) => (),
            event => panic!("unexpected resize event: {event:?}"),
        }
    }
    let accepted = native_now();
    assert_eq!(
        default.renew(first.id, accepted),
        GuardResult::Applied(true)
    );
    let (third, _) = source(3);
    let mut isolated = attached(
        submit(
            &owner.control,
            start_request(account, StartChoice::New, third),
        )
        .await,
    );
    assert!(isolated.slot != default.slot);
    exchange(
        &mut isolated,
        1,
        b"printf '[%s]\\n' \"${state:-ABSENT}\"\r",
        b"[ABSENT]",
    )
    .await;

    // Fill the isolated reader's bounded publication and staging slots while
    // continuing both local control and I/O on the default attachment.
    send_input(
        &isolated,
        Operation::new(
            2,
            native_now() + seconds(4),
            Input::Bytes(b"head -c 65536 /dev/zero | tr '\\000' x\r".to_vec()),
        )
        .unwrap(),
    )
    .await;
    timeout(seconds(2), async {
        while !isolated.port.output_is_queued() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("isolated reader output never reached its one-slot bound");
    let (release, released) = oneshot::channel();
    owner.control.release(isolated.slot, release).unwrap();
    assert!(matches!(
        timeout(seconds(2), released).await.unwrap().unwrap(),
        Err(Error::Invalid("terminal_cleanup_pending"))
    ));
    exchange(
        &mut default,
        102,
        b"printf '[%s]\\n' \"$state\"\r",
        b"[MARK]",
    )
    .await;
    let suspect = fence.begin_update().unwrap();
    send_input(
        &default,
        Operation::new(
            103,
            native_now() + Duration::from_millis(80),
            Input::Bytes(b"state=INVALID\r".to_vec()),
        )
        .unwrap(),
    )
    .await;
    loop {
        match event(&mut default).await {
            Event::Output(_) => (),
            Event::Closed(Reason::Deadline) => break,
            event => panic!("suspect head unexpectedly progressed: {event:?}"),
        }
    }
    assert!(default.try_event().is_none());
    assert_eq!(suspect.resolve(Some([9; 32])), FenceState::Live);
    isolated.close();
    assert!(Arc::strong_count(&profile) > 1);
    let profile = Arc::new(stop_and_recover(&mut owner, profile).await);

    let started = Instant::now();
    let mut restarted = LiveSupervisor::start(
        Arc::clone(&profile),
        &material,
        &fixture.daemon,
        &fixture.tmux,
    )
    .unwrap();
    let (resumed, _) = source(4);
    let mut recovered = attached(
        submit(
            &restarted.control,
            start_request(account, StartChoice::Default, resumed),
        )
        .await,
    );
    let warm = started.elapsed();
    assert!(
        recovered.slot == default.slot && recovered.claim.generation > default.claim.generation
    );
    exchange(
        &mut recovered,
        1,
        b"printf '[%s]\\n' \"$state\"\r",
        b"[MARK]",
    )
    .await;
    let (reply, result) = oneshot::channel();
    restarted.control.release(recovered.slot, reply).unwrap();
    assert!(matches!(
        timeout(seconds(2), result).await.unwrap().unwrap(),
        Err(Error::Invalid("terminal_cleanup_pending"))
    ));
    drop(stop_and_recover(&mut restarted, profile).await);

    fixture.set_terminal_config(&[fixture.accounts[1]]);
    let (lease, _) = fixture.identity();
    let profile = Arc::new(lease);
    let mut revoked = LiveSupervisor::start(
        Arc::clone(&profile),
        &material,
        &fixture.daemon,
        &fixture.tmux,
    )
    .unwrap();
    let (stale, _) = source(7);
    assert!(matches!(
        submit(
            &revoked.control,
            start_request(account, StartChoice::Default, stale)
        )
        .await,
        StartOutcome::Refused(Error::Invalid("terminal_grant_denied"))
    ));
    let profile = stop_and_recover(&mut revoked, profile).await;
    let mut manager = Manager::new(&fixture.tmux, profile.root()).unwrap();
    let registry = Registry::open(&profile, &material).unwrap();
    for row in registry.slots().unwrap() {
        if row.account == account {
            assert!(matches!(row.state, State::Retiring(_)));
            assert!(manager.inventory(row, None).is_err());
        }
    }

    samples.sort();
    eprintln!(
        "terminal_live_native samples={} echo_including_coalescing_p50_us={} echo_including_coalescing_p95_us={} cold_attach_ms={} warm_restart_attach_ms={}",
        samples.len(),
        percentile(&samples, 50).as_micros(),
        percentile(&samples, 95).as_micros(),
        cold.as_millis(),
        warm.as_millis()
    );
}

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    sorted[(sorted.len() * percent).div_ceil(100) - 1]
}

#[test]
fn core_tests_terminal_live_supervisor_nearest_rank_percentiles() {
    let observations: Vec<_> = (1..=100).map(seconds).collect();
    assert_eq!(percentile(&observations, 50), seconds(50));
    assert_eq!(percentile(&observations, 95), seconds(95));
    assert_eq!(percentile(&observations[..12], 95), seconds(12));
}

struct HeldProcess(std::process::Child);

impl Drop for HeldProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore = "uses real private tmux and native marmishd; set MARMISH_TEST_DAEMON"]
async fn terminal_live_supervisor_adopts_legacy_and_releases_at_registry_cap() {
    use super::super::store::{Host, Proof};
    use std::fs;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    use std::os::unix::net::UnixListener;

    let fixture = super::super::tests::Fixture::new().await;
    let (profile, material) = fixture.identity();
    let profile = Arc::new(profile);
    let registered = fixture.accounts[0];
    let next_account = fixture.accounts[1];
    let mut manager = Manager::new(&fixture.tmux, profile.root()).unwrap();
    let mut registry = Registry::open(&profile, &material).unwrap();
    let legacy = Slot {
        id: SlotId([1; 16]),
        account: registered,
        default: true,
        marker: [3; 16],
        state: State::Prepared,
        proof: None,
    };
    registry.prepare(legacy).unwrap();
    let live = manager.create(legacy, 24, 80).unwrap();
    registry.ready(legacy, live.session, None).unwrap();

    let retiring = Slot {
        id: SlotId([2; 16]),
        account: next_account,
        default: true,
        marker: [4; 16],
        state: State::Prepared,
        proof: None,
    };
    let mut process = HeldProcess(
        std::process::Command::new("/bin/sleep")
            .arg("20")
            .spawn()
            .unwrap(),
    );
    let pid = i32::try_from(process.0.id()).unwrap();
    let socket = manager.socket(retiring.id);
    let dir = socket.parent().unwrap();
    fs::DirBuilder::new().mode(0o700).create(dir).unwrap();
    drop(UnixListener::bind(&socket).unwrap());
    let id = |path| {
        let metadata = fs::symlink_metadata(path).unwrap();
        (metadata.dev(), metadata.ino())
    };
    let proof = Proof {
        host: Host::current().unwrap(),
        pid,
        directory: id(dir),
        socket: id(&socket),
    };
    registry.prepare(retiring).unwrap();
    registry.ready(retiring, 1, Some(proof)).unwrap();
    for index in 3..=super::super::MAX_SLOTS as u8 {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(manager.socket(SlotId([index; 16])).parent().unwrap())
            .unwrap();
        registry
            .prepare(Slot {
                id: SlotId([index; 16]),
                account: registered,
                default: false,
                marker: [index + 1; 16],
                state: State::Prepared,
                proof: None,
            })
            .unwrap();
    }
    drop(registry);
    drop(manager);

    let mut owner = LiveSupervisor::start(
        Arc::clone(&profile),
        &material,
        &fixture.daemon,
        &fixture.tmux,
    )
    .unwrap();
    let (existing, _) = source(1);
    let mut attached_legacy = attached(
        submit(
            &owner.control,
            start_request(registered, StartChoice::Default, existing),
        )
        .await,
    );
    assert!(attached_legacy.slot == legacy.id);
    exchange(
        &mut attached_legacy,
        1,
        b"printf '[%s]\\n' 'ADOPTED'\r",
        b"[ADOPTED]",
    )
    .await;

    let (fresh, _) = source(2);
    assert!(matches!(
        submit(
            &owner.control,
            start_request(registered, StartChoice::New, fresh)
        )
        .await,
        StartOutcome::Refused(Error::Invalid("terminal_capacity"))
    ));
    process.0.kill().unwrap();
    assert!(!process.0.wait().unwrap().success());
    let (reply, result) = oneshot::channel();
    owner.control.release(retiring.id, reply).unwrap();
    timeout(seconds(3), result).await.unwrap().unwrap().unwrap();
    assert!(!socket.parent().unwrap().exists());
    let (replacement, _) = source(3);
    let mut freshly_attached = attached(
        submit(
            &owner.control,
            start_request(next_account, StartChoice::Default, replacement),
        )
        .await,
    );
    assert!(freshly_attached.slot != retiring.id && freshly_attached.slot != legacy.id);
    exchange(
        &mut freshly_attached,
        1,
        b"printf '[%s]\\n' 'FRESH'\r",
        b"[FRESH]",
    )
    .await;
    let profile = stop_and_recover(&mut owner, profile).await;
    let registry = Registry::open(&profile, &material).unwrap();
    let rows = registry.slots().unwrap();
    assert!(
        rows.iter()
            .any(|row| row.id == legacy.id && row.proof.is_some())
    );
    assert!(!rows.iter().any(|row| row.id == retiring.id));
    drop(registry);

    // A reserved emergency reaper retains the exact lease with its pending child.
    let profile = Arc::new(profile);
    let mut pending = backend::ChildState::new(
        std::process::Command::new("/bin/sleep")
            .arg("20")
            .spawn()
            .unwrap(),
    );
    pending._profile = Some(Arc::clone(&profile));
    assert_eq!(pending.shutdown(Instant::now()).unwrap(), Shutdown::Pending);
    let Err(profile) = Arc::try_unwrap(profile) else {
        panic!("pending worker released the profile lease");
    };
    let reaper = backend::reserve_reaper().unwrap();
    assert!(reaper.send(pending).is_ok());
    timeout(seconds(4), async {
        while Arc::strong_count(&profile) > 1 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(Arc::try_unwrap(profile).is_ok());
}

#[tokio::test]
#[ignore = "uses real private tmux, three observers and native marmishd"]
async fn terminal_live_supervisor_checked_observers_takeover_and_named_reattach() {
    use crate::terminal_frames::Size;
    use crate::terminal_selection_wire_v2::{Choice, Target};

    let fixture = super::super::tests::Fixture::new().await;
    let (profile, material) = fixture.identity();
    let profile = Arc::new(profile);
    let account = fixture.accounts[0];
    let mut owner = LiveSupervisor::start(
        Arc::clone(&profile),
        &material,
        &fixture.daemon,
        &fixture.tmux,
    )
    .unwrap();
    let control = &owner.control;
    let request = |choice, target, id| {
        let (source, _) = source(id);
        VerifiedStart {
            account,
            selection: StartChoice::Checked {
                scope: Scope {
                    server: [1; 32],
                    client: account,
                    command: [9; 32],
                    generation: 1,
                },
                request: Request {
                    choice,
                    target,
                    size: Size { rows: 24, cols: 80 },
                },
            },
            rows: 24,
            cols: 80,
            source,
            deadline: native_now() + seconds(8),
            room_hold: Some(Arc::new(())),
        }
    };
    async fn confirm(control: &LiveControl, claim: &mut AdmittedTerminal) {
        let ack = claim
            .submit_confirm(control, native_now() + seconds(4))
            .unwrap();
        timeout(seconds(4), ack).await.unwrap().unwrap().unwrap();
    }
    let mut writer = attached(submit(control, request(Choice::Regular, Target::Default, 1)).await);
    assert!(matches!(
        writer.submit(
            Operation::new(
                1,
                native_now() + seconds(4),
                Input::Resize { rows: 30, cols: 90 }
            )
            .unwrap()
        ),
        Err(PushError::Closed(Reason::Authority))
    ));
    confirm(control, &mut writer).await;
    exchange(
        &mut writer,
        1,
        b"state=HANDOVER; printf '[%s]\\n' $state\r",
        b"[HANDOVER]",
    )
    .await;

    let named = Target::Named(writer.slot.0);
    let mut observers = Vec::new();
    for id in 2..5 {
        let mut observer = attached(submit(control, request(Choice::Observe, named, id)).await);
        assert!(matches!(observer.role(), Role::Observer));
        assert_eq!(observer.slot.0, writer.slot.0);
        assert!(
            observer.event(true).is_none(),
            "display left the native queue before exact confirmation"
        );
        confirm(control, &mut observer).await;
        assert!(matches!(
            observer.submit(
                Operation::new(
                    1,
                    native_now() + seconds(4),
                    Input::Resize { rows: 30, cols: 90 }
                )
                .unwrap()
            ),
            Err(PushError::Closed(Reason::Authority))
        ));
        observers.push(observer);
    }
    assert!(matches!(
        submit(control, request(Choice::Observe, named, 5)).await,
        StartOutcome::Refused(Error::Invalid("terminal_capacity"))
    ));
    let live_offer = match submit(control, request(Choice::Takeover, named, 5)).await {
        StartOutcome::Busy {
            offer: Some(ticket),
            ..
        } => ticket,
        _ => panic!("writer did not return a live offer at observer capacity"),
    };
    let nonce = if live_offer.bytes()[..16] == [123; 16] {
        [124; 16]
    } else {
        [123; 16]
    };
    let invalid_offer = Ticket::new(
        nonce,
        live_offer.slot,
        live_offer.claim,
        live_offer.generation,
    )
    .unwrap();
    for (ticket, expected) in [
        (invalid_offer, "terminal_offer_stale"),
        (live_offer, "terminal_capacity"),
    ] {
        assert!(matches!(
            submit(control, request(Choice::Observe, Target::Offer(ticket), 5)).await,
            StartOutcome::Refused(Error::Invalid(reason)) if reason == expected
        ));
    }
    exchange(
        &mut writer,
        2,
        b"printf '[%s]' DISPLAYONLY\r",
        b"[DISPLAYONLY]",
    )
    .await;
    let mut display = Vec::new();
    while !display
        .windows(b"[DISPLAYONLY]".len())
        .any(|part| part == b"[DISPLAYONLY]")
    {
        assert!(display.len() < 65536, "observer display fixture cap");
        match event(&mut observers[0]).await {
            Event::Output(bytes) => display.extend_from_slice(&bytes),
            other => panic!("unexpected observer event: {other:?}"),
        }
    }
    let ticket = match submit(control, request(Choice::Takeover, named, 6)).await {
        StartOutcome::Busy {
            offer: Some(ticket),
            ..
        } => ticket,
        _ => panic!("explicit takeover did not return a live offer"),
    };
    let mut successor =
        attached(submit(control, request(Choice::Takeover, Target::Offer(ticket), 7)).await);
    assert!(successor.slot == writer.slot);
    assert!(successor.claim != writer.claim);
    confirm(control, &mut successor).await;
    for link in observers.iter().chain([&writer]) {
        assert_eq!(link.port.source.reason(), Some(Reason::Replaced));
    }
    assert!(matches!(
        submit(control, request(Choice::Takeover, Target::Offer(ticket), 8)).await,
        StartOutcome::Refused(Error::Invalid("terminal_offer_stale"))
    ));
    exchange(
        &mut successor,
        1,
        b"printf '[%s]\\n' $state\r",
        b"[HANDOVER]",
    )
    .await;
    drop(successor);
    let cutoff = Instant::now() + seconds(3);
    let mut reattached = loop {
        match submit(control, request(Choice::Regular, named, 9)).await {
            StartOutcome::Attached(terminal) => break terminal,
            StartOutcome::Refused(Error::Invalid("terminal_slot_unavailable"))
                if Instant::now() < cutoff =>
            {
                sleep(Duration::from_millis(20)).await;
            }
            StartOutcome::Refused(error) => panic!("named reattach failed: {error:?}"),
            StartOutcome::Busy { .. } => panic!("unexpected old writer"),
        }
    };
    confirm(control, &mut reattached).await;
    exchange(
        &mut reattached,
        1,
        b"printf '[%s]\\n' $state\r",
        b"[HANDOVER]",
    )
    .await;
    drop(reattached);
    let _ = stop_and_recover(&mut owner, profile).await;
}
