use super::*;
use crate::{
    dispatcher_fatal::Trip,
    ipc::{self, IO_TIMEOUT, Request, peer_read_listener::Offer, read_frame, write_frame_body},
    peer_candidate_startup::{self, Startup, WorkerStart},
};
use serde_json::json;
use std::{
    num::NonZeroU64,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

struct FixtureRuntime(PathBuf);

impl Drop for FixtureRuntime {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("s"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    _runtime: FixtureRuntime,
    path: PathBuf,
    lease: Arc<ProfileLease>,
    pair: Arc<Pair>,
    worker: Worker,
    _edges: (Startup, WorkerStart),
    trip: Trip,
    drain: oneshot::Sender<()>,
    task: JoinHandle<crate::Result<()>>,
    queue: mpsc::Receiver<ipc::Pending>,
}

impl Fixture {
    fn start(attach: bool, path_matches: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let lease = Arc::new(ProfileLease::acquire(directory.path(), "local").unwrap());
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(lease.runtime_dir())
            .unwrap();
        let runtime = FixtureRuntime(lease.runtime_dir().to_path_buf());
        let path = lease.socket_path();
        let expected = if path_matches {
            path.clone()
        } else {
            directory.path().join("wrong")
        };
        let listener = ipc::bind(&path).unwrap();
        let (startup, registration, edge) = peer_candidate_startup::pair();
        let (trip, fatal) = Trip::new();
        let (mut offer, _) =
            Offer::new(expected, registration, NonZeroU64::MIN, fatal.clone()).unwrap();
        let worker = if attach {
            offer.pair_approval_for_test(lease.clone())
        } else {
            pair(
                path.clone(),
                lease.clone(),
                peer_candidate_startup::pair().1.readiness(),
                fatal.clone(),
            )
            .1
        };
        let pair = worker.0.clone();
        let (send, queue) = mpsc::channel(1);
        let (_, status) = watch::channel(Response::success(json!({"state":"active"})));
        let (drain, draining) = oneshot::channel();
        startup.ordinary_ready();
        let task = tokio::spawn(ipc::serve_with_listener(
            listener,
            send,
            status,
            None,
            None,
            Some(draining),
            None,
            ipc::CandidateListener::registered(fatal, Some(offer)),
        ));
        Self {
            _directory: directory,
            _runtime: runtime,
            path,
            lease,
            pair,
            worker,
            _edges: (startup, edge),
            trip,
            drain,
            task,
            queue,
        }
    }

    async fn socket(&self, selector: Selection) -> UnixStream {
        let mut stream = UnixStream::connect(&self.path).await.unwrap();
        use nostr::nips::nip19::ToBech32 as _;
        let wire = json!({"operation":"peer_approve","request":{"version":1,"selection":{
            "server":selector.server.to_bech32().unwrap(), "offer_id":hex::encode(selector.offer_id),
            "revision":hex::encode(selector.revision), "generation":selector.generation
        }}});
        write_frame_body(&mut stream, &wire).await.unwrap();
        stream
    }

    async fn submitted(&self) -> Arc<Exchange> {
        for _ in 0..128 {
            if let Some(exchange) = self
                .pair
                .state
                .lock()
                .unwrap()
                .slot
                .as_ref()
                .map(|slot| slot.exchange.clone())
            {
                return exchange;
            }
            tokio::task::yield_now().await;
        }
        panic!("approval did not arrive")
    }

    async fn assert_ordinary_untouched(&mut self) {
        let response = ipc::exchange(&self.path, &Request::Status {}, IO_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(response.result, Some(json!({"state":"active"})));
        assert!(matches!(
            self.queue.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }
}

fn selection() -> Selection {
    Selection {
        server: nostr::key::Keys::generate().public_key(),
        offer_id: [1; 16],
        revision: [2; 32],
        generation: 3,
    }
}

#[tokio::test(start_paused = true)]
async fn one_real_exchange_checks_owner_and_returns_only_consumed_decision() {
    let mut fixture = Fixture::start(true, true);
    let selector = selection();
    let mut socket = fixture.socket(selector).await;
    fixture.submitted().await;
    assert_eq!(
        peer_approve::client_request(&fixture.path, selector).await,
        Err(Failure::Busy)
    );
    let claim = fixture.worker.try_claim().unwrap();
    assert!(fixture.worker.try_claim().is_none());
    assert_eq!(claim.selection(), selector);
    assert!(claim.matches_lease(&fixture.lease));
    let other = Arc::new(ProfileLease::acquire(fixture._directory.path(), "another").unwrap());
    assert!(!claim.matches_lease(&other));
    assert_eq!(claim.deadline(), Instant::now() + CLAIM_BUDGET);
    let consumed = claim.consume(&fixture.lease, selector).unwrap();
    assert!(consumed.check_live() && consumed.matches_lease(&fixture.lease));
    assert_eq!(consumed.selection(), selector);
    assert_eq!(consumed.deadline(), Instant::now() + CLAIM_BUDGET);
    consumed.complete(Ok(()));
    let response: Response = read_frame(&mut socket).await.unwrap();
    assert_eq!(
        response.result,
        Some(json!({"version":1,"selection":selector,"state":"activating"}))
    );
    fixture.assert_ordinary_untouched().await;
    fixture.drain.send(()).unwrap();
    fixture.task.await.unwrap().unwrap();
    assert!(fixture.pair.is_closed());
}

#[tokio::test(start_paused = true)]
async fn waiting_equality_is_busy_while_claimed_equality_is_uncertain() {
    let fixture = Fixture::start(true, true);
    let selector = selection();
    let mut socket = fixture.socket(selector).await;
    let exchange = fixture.submitted().await;
    tokio::time::advance(exchange.claim_until - Instant::now()).await;
    assert!(fixture.worker.try_claim().is_none());
    let response: Response = read_frame(&mut socket).await.unwrap();
    assert_eq!(response.error.as_deref(), Some("busy"));
    assert!(fixture.pair.state.lock().unwrap().slot.is_none());

    let mut socket = fixture.socket(selector).await;
    let exchange = fixture.submitted().await;
    tokio::time::advance(exchange.claim_until - Instant::now() - Duration::from_millis(1)).await;
    let claim = fixture.worker.try_claim().unwrap();
    let cutoff = claim.deadline();
    let permit = claim.consume(&fixture.lease, selector).unwrap();
    tokio::time::advance(cutoff - Instant::now()).await;
    assert!(!permit.check_live());
    permit.complete(Ok(()));
    let response: Response = read_frame(&mut socket).await.unwrap();
    assert_eq!(response.error.as_deref(), Some("operation_uncertain"));
    fixture.drain.send(()).unwrap();
    fixture.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn raw_socket_check_refuses_eof_and_extra_bytes_before_listener_runs() {
    for pipelined in [false, true] {
        let fixture = Fixture::start(true, true);
        let selector = selection();
        let mut stream = fixture.socket(selector).await;
        let exchange = fixture.submitted().await;
        let claim = fixture.worker.try_claim().unwrap();
        assert!(claim.check_live());

        if pipelined {
            assert_eq!(stream.try_write(&[1]).unwrap(), 1);
        } else {
            stream.shutdown().await.unwrap();
        }
        // No handoff to the listener is needed to detect kernel-visible EOF/input.
        assert!(!awaiting_reply(&exchange.reader));
        assert!(!claim.check_live());
        assert!(claim.consume(&fixture.lease, selector).is_none());
        fixture.drain.send(()).unwrap();
        fixture.task.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn path_absence_wrong_uid_stop_and_fatal_are_fail_closed() {
    for (attach, matched) in [(false, true), (true, false)] {
        let mut fixture = Fixture::start(attach, matched);
        assert_eq!(
            peer_approve::client_request(&fixture.path, selection()).await,
            Err(Failure::Unavailable)
        );
        fixture.assert_ordinary_untouched().await;
        fixture.drain.send(()).unwrap();
        fixture.task.await.unwrap().unwrap();
    }

    let fixture = Fixture::start(true, true);
    let uid = fixture.lease.checked_owner_uid().unwrap();
    assert!(!fixture.pair.current(uid.wrapping_add(1)));
    let owner_path = fixture.lease.root().join("owner.lock");
    std::fs::set_permissions(&owner_path, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(fixture.lease.checked_owner_uid().is_err());
    assert_eq!(
        peer_approve::client_request(&fixture.path, selection()).await,
        Err(Failure::Unavailable)
    );
    std::fs::set_permissions(owner_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let selection = selection();
    let _stream = fixture.socket(selection).await;
    fixture.submitted().await;
    let claim = fixture.worker.try_claim().unwrap();
    fixture.trip.trip();
    assert!(!claim.check_live());
    fixture.task.await.unwrap().unwrap();
    assert!(fixture.pair.is_closed());
}

#[tokio::test]
async fn registration_requires_the_exact_lease_path_and_fatal_once() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let lease = Arc::new(ProfileLease::acquire(directory.path(), "local").unwrap());
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(lease.runtime_dir())
        .unwrap();
    let _runtime = FixtureRuntime(lease.runtime_dir().to_path_buf());
    let (_trip, fatal) = Trip::new();
    let (_other_trip, other_fatal) = Trip::new();
    let readiness = peer_candidate_startup::pair().1.readiness();
    let wrong_path = directory.path().join("wrong");
    let wrong_listener = ipc::bind(&wrong_path).unwrap();
    let (wrong, _wrong_worker) = pair(wrong_path, lease.clone(), readiness.clone(), fatal.clone());
    assert!(!wrong.register(&wrong_listener, Some(&fatal)));
    assert!(!wrong.0.state.lock().unwrap().registered);

    let path = lease.socket_path();
    let listener = ipc::bind(&path).unwrap();
    let (paired, _worker) = pair(path, lease, readiness, fatal.clone());
    assert!(!paired.register(&listener, None));
    assert!(!paired.register(&listener, Some(&other_fatal)));
    assert!(!paired.0.state.lock().unwrap().registered);
    assert!(paired.register(&listener, Some(&fatal)));
    assert!(!paired.register(&listener, Some(&fatal)));
}

#[tokio::test]
async fn claimed_busy_is_uncertain_and_worker_drop_wakes_an_open_exchange() {
    let fixture = Fixture::start(true, true);
    let selector = selection();
    let mut socket = fixture.socket(selector).await;
    fixture.submitted().await;
    fixture.worker.try_claim().unwrap().refuse(Failure::Busy);
    let response: Response = read_frame(&mut socket).await.unwrap();
    assert_eq!(response.error.as_deref(), Some("operation_uncertain"));

    let mut socket = fixture.socket(selector).await;
    fixture.submitted().await;
    let claim = fixture.worker.try_claim().unwrap();
    drop(fixture.worker);
    assert!(fixture.pair.is_closed());
    assert!(!claim.check_live());
    assert!(claim.consume(&fixture.lease, selector).is_none());
    let received =
        tokio::time::timeout(Duration::from_secs(1), read_frame::<Response>(&mut socket))
            .await
            .expect("dropped worker did not close an open exchange");
    assert!(received.is_err());
    fixture.drain.send(()).unwrap();
    fixture.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn unclaimed_disconnect_releases_slot_and_no_stale_claim_can_finish() {
    let fixture = Fixture::start(true, true);
    let selector = selection();
    let abandoned = fixture.socket(selector).await;
    let old = fixture.submitted().await;
    drop(abandoned);
    for _ in 0..128 {
        if fixture.pair.state.lock().unwrap().slot.is_none() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(fixture.pair.state.lock().unwrap().slot.is_none());
    let mut socket = fixture.socket(selector).await;
    let current = fixture.submitted().await;
    fixture.pair.revoke(&old);
    assert!(!Arc::ptr_eq(&old, &current));
    let claim = fixture.worker.try_claim().unwrap();
    claim.refuse(Failure::Stale);
    let response: Response = read_frame(&mut socket).await.unwrap();
    assert_eq!(response.error.as_deref(), Some("peer_stale"));
    fixture.drain.send(()).unwrap();
    fixture.task.await.unwrap().unwrap();
}
