use super::*;
use crate::{
    dispatcher_fatal::Trip,
    ipc::{self, IO_TIMEOUT, Pending, Request as LocalRequest, bind, exchange},
    peer_candidate_startup::{self, Startup, WorkerStart},
};
use peer_read::{Failure, Reply, Request, Selection, client_request};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use tokio::{
    sync::{Semaphore, mpsc, oneshot, watch},
    task::JoinHandle,
    time::timeout,
};

const LIST: Request = Request::List {};
const TEST_BUDGET: Duration = Duration::from_secs(3);

struct Fixture {
    _directory: tempfile::TempDir,
    path: PathBuf,
    producer: Producer,
    intake: Intake,
    startup: Startup,
    worker: WorkerStart,
    trip: Trip,
    incoming: mpsc::Receiver<Pending>,
    drain: oneshot::Sender<()>,
    task: JoinHandle<crate::Result<()>>,
}

impl Fixture {
    fn start(mismatched_path: bool, pause: Option<TestPause>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("s");
        let expected = if mismatched_path {
            directory.path().join("different")
        } else {
            path.clone()
        };
        let listener = bind(&path).unwrap();
        let (startup, registration, worker) = peer_candidate_startup::pair();
        let (trip, fatal) = Trip::new();
        let (mut offer, producer) =
            Offer::new(expected, registration, NonZeroU64::MIN, fatal).unwrap();
        offer.intake.pause = pause;
        let intake = offer.intake();
        let (queue, incoming) = mpsc::channel(1);
        let (_, status) = watch::channel(Response::success(json!({"state":"active"})));
        let (drain, draining) = oneshot::channel();
        let task = tokio::spawn(ipc::serve_with_listener(
            listener,
            queue,
            status,
            None,
            None,
            Some(draining),
            None,
            ipc::CandidateListener {
                fatal: None,
                registration: ipc::CandidateRegistration::View(offer),
            },
        ));
        Self {
            _directory: directory,
            path,
            producer,
            intake,
            startup,
            worker,
            trip,
            incoming,
            drain,
            task,
        }
    }

    async fn request(&self, request: Request) -> Result<Reply, Failure> {
        timeout(TEST_BUDGET, client_request(&self.path, request))
            .await
            .unwrap()
    }

    async fn assert_owner_untouched(&mut self) {
        let status = exchange(&self.path, &LocalRequest::Status {}, IO_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(status.result, Some(json!({"state":"active"})));
        assert!(matches!(
            self.incoming.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    async fn drain(self) {
        self.drain.send(()).unwrap();
        timeout(TEST_BUDGET, self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!self.intake.current());
        assert!(self.intake.reader.admit(NonZeroU64::MIN).is_none());
    }
}

fn pause() -> (
    TestPause,
    mpsc::UnboundedReceiver<()>,
    mpsc::UnboundedReceiver<(bool, bool)>,
) {
    let (arrived, arrivals) = mpsc::unbounded_channel();
    let (dropped, drops) = mpsc::unbounded_channel();
    (
        TestPause {
            arrived,
            resume: Arc::new(Semaphore::new(0)),
            dropped,
        },
        arrivals,
        drops,
    )
}

#[tokio::test]
async fn live_listener_waits_for_both_edges_and_serves_exact_read_only_rows() {
    let mut fixture = Fixture::start(false, None);
    assert_eq!(fixture.request(LIST).await, Err(Failure::Unavailable));
    fixture.startup.ordinary_ready();
    assert_eq!(fixture.request(LIST).await, Err(Failure::Unavailable));
    assert!(fixture.intake.current());

    let row = pending_view::publish_live_fixture(&fixture.producer);
    assert_eq!(
        fixture.request(LIST).await,
        Ok(Reply::List { entries: vec![row] })
    );
    assert_eq!(
        fixture
            .request(Request::Show {
                selection: row.selection
            })
            .await,
        Ok(Reply::Show { entry: row })
    );
    let selection = Selection {
        generation: row.selection.generation + 1,
        ..row.selection
    };
    assert_eq!(
        fixture.request(Request::Show { selection }).await,
        Err(Failure::Unavailable)
    );
    fixture.assert_owner_untouched().await;

    let Fixture {
        worker,
        drain,
        task,
        intake,
        ..
    } = fixture;
    let mut ready = timeout(TEST_BUDGET, worker.ready()).await.unwrap().unwrap();
    assert!(ready.is_current());
    drain.send(()).unwrap();
    timeout(TEST_BUDGET, task).await.unwrap().unwrap().unwrap();
    timeout(TEST_BUDGET, ready.stopped()).await.unwrap();
    assert!(intake.reader.admit(NonZeroU64::MIN).is_none());
}

#[tokio::test]
async fn mismatched_listener_drops_projection_with_ordinary_service_intact() {
    let mut fixture = Fixture::start(true, None);
    fixture.startup.ordinary_ready();
    assert_eq!(fixture.request(LIST).await, Err(Failure::Unavailable));
    assert!(!fixture.intake.readiness.is_current());
    assert!(fixture.intake.reader.admit(NonZeroU64::MIN).is_none());
    fixture.assert_owner_untouched().await;
    fixture.drain().await;
}

#[tokio::test]
async fn fifth_read_is_unavailable_and_fatal_refuses_four_admitted_replies() {
    let (pause, mut arrived, _) = pause();
    let mut fixture = Fixture::start(false, Some(pause.clone()));
    fixture.startup.ordinary_ready();
    let _row = pending_view::publish_live_fixture(&fixture.producer);
    let clients: Vec<_> = (0..4)
        .map(|_| {
            let path = fixture.path.clone();
            tokio::spawn(async move { client_request(&path, LIST).await })
        })
        .collect();
    for _ in 0..4 {
        assert_eq!(
            timeout(TEST_BUDGET, arrived.recv()).await.unwrap(),
            Some(())
        );
    }
    assert_eq!(fixture.request(LIST).await, Err(Failure::Unavailable));
    assert!(matches!(
        arrived.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    fixture.trip.notify();
    pause.resume.add_permits(4);
    for client in clients {
        assert_eq!(
            timeout(TEST_BUDGET, client).await.unwrap().unwrap(),
            Err(Failure::Unavailable)
        );
    }
    fixture.assert_owner_untouched().await;
    fixture.drain().await;
}

#[tokio::test]
async fn draining_reaps_admitted_exchange_and_revokes_projection() {
    let (pause, mut arrived, mut dropped) = pause();
    let fixture = Fixture::start(false, Some(pause));
    fixture.startup.ordinary_ready();
    let _row = pending_view::publish_live_fixture(&fixture.producer);
    let path = fixture.path.clone();
    let client = tokio::spawn(async move { client_request(&path, LIST).await });
    assert_eq!(
        timeout(TEST_BUDGET, arrived.recv()).await.unwrap(),
        Some(())
    );
    fixture.drain().await;
    assert_eq!(dropped.try_recv().unwrap(), (false, false));
    assert_eq!(
        timeout(TEST_BUDGET, client).await.unwrap().unwrap(),
        Err(Failure::Unavailable)
    );
}

#[tokio::test]
async fn stopped_handshake_cannot_be_restored_by_listener_registration() {
    let mut fixture = Fixture::start(false, None);
    fixture.startup.stop();
    fixture.startup.ordinary_ready();
    let _row = pending_view::publish_live_fixture(&fixture.producer);
    assert_eq!(fixture.request(LIST).await, Err(Failure::Unavailable));
    assert!(!fixture.intake.readiness.is_current());
    fixture.assert_owner_untouched().await;
    fixture.drain().await;
}

#[test]
fn dropping_unregistered_offer_stops_its_read_view_and_handshake() {
    let (startup, registration, _worker) = peer_candidate_startup::pair();
    let (_trip, fatal) = Trip::new();
    let (offer, producer) = Offer::new(
        PathBuf::from("unused"),
        registration,
        NonZeroU64::MIN,
        fatal,
    )
    .unwrap();
    let intake = offer.intake();
    let _row = pending_view::publish_live_fixture(&producer);
    assert!(intake.reader.admit(NonZeroU64::MIN).is_some());
    drop(offer);
    startup.ordinary_ready();
    assert!(intake.reader.admit(NonZeroU64::MIN).is_none());
    assert!(!intake.readiness.is_current());
}

#[tokio::test]
async fn aborting_service_stops_projection_and_registers_permanent_stop() {
    let mut fixture = Fixture::start(false, None);
    fixture.startup.ordinary_ready();
    let row = pending_view::publish_live_fixture(&fixture.producer);
    assert_eq!(
        fixture.request(LIST).await,
        Ok(Reply::List { entries: vec![row] })
    );
    fixture.assert_owner_untouched().await;
    fixture.task.abort();
    assert!(
        timeout(TEST_BUDGET, fixture.task)
            .await
            .unwrap()
            .unwrap_err()
            .is_cancelled()
    );
    fixture.startup.ordinary_ready();
    assert!(!fixture.intake.readiness.is_current());
    assert!(fixture.intake.reader.admit(NonZeroU64::MIN).is_none());
}

#[tokio::test]
async fn readiness_and_view_stops_refuse_a_read_admitted_on_a_live_listener() {
    enum Cut {
        Readiness,
        View,
    }
    for cut in [Cut::Readiness, Cut::View] {
        let (pause, mut arrived, _) = pause();
        let mut fixture = Fixture::start(false, Some(pause.clone()));
        fixture.startup.ordinary_ready();
        let _row = pending_view::publish_live_fixture(&fixture.producer);
        let path = fixture.path.clone();
        let client = tokio::spawn(async move { client_request(&path, LIST).await });
        assert_eq!(
            timeout(TEST_BUDGET, arrived.recv()).await.unwrap(),
            Some(())
        );
        match cut {
            Cut::Readiness => fixture.startup.stop(),
            Cut::View => fixture.intake.reader.stop(),
        }
        pause.resume.add_permits(1);
        assert_eq!(
            timeout(TEST_BUDGET, client).await.unwrap().unwrap(),
            Err(Failure::Unavailable)
        );
        assert_eq!(fixture.request(LIST).await, Err(Failure::Unavailable));
        fixture.assert_owner_untouched().await;
        fixture.drain().await;
    }
}

#[tokio::test]
async fn passive_registration_checks_kernel_path_and_stops_on_drop() {
    use crate::ipc::PassiveRegistration;

    let directory = tempfile::tempdir().unwrap();
    let actual = directory.path().join("daemon.sock");
    let listener = bind(&actual).unwrap();
    let (startup, registration, worker) = peer_candidate_startup::pair();
    startup.ordinary_ready();
    let wrong = PassiveRegistration {
        path: directory.path().join("other.sock"),
        registration,
    };
    assert!(wrong.register(&listener).is_none());
    assert!(worker.ready().await.is_none());

    let (startup, registration, worker) = peer_candidate_startup::pair();
    let current = registration.readiness();
    let matching = PassiveRegistration {
        path: actual,
        registration,
    }
    .register(&listener)
    .unwrap();
    assert!(!current.is_current());
    startup.ordinary_ready();
    let ready = worker.ready().await.unwrap();
    assert!(ready.is_current());
    drop(matching);
    assert!(!current.is_current());
}
