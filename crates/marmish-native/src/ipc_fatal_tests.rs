use super::*;
use crate::dispatcher_fatal::{FencedStream, Trip};
use std::{
    io,
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll},
};

const STEP: Duration = Duration::from_secs(3);

struct Listener {
    _dir: tempfile::TempDir,
    path: PathBuf,
    cut: Trip,
    accept: Option<UnixListener>,
    drain: Option<oneshot::Sender<()>>,
    queue: mpsc::Receiver<Pending>,
    starts: mpsc::Receiver<terminal_handshake::PendingStart>,
}

impl Listener {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = dir.path().join("s");
        let (cut, _) = Trip::new();
        Self {
            accept: Some(bind(&path).unwrap()),
            _dir: dir,
            path,
            cut,
            drain: None,
            queue: mpsc::channel(1).1,
            starts: terminal_handshake::Intake::new().1,
        }
    }

    fn start(&mut self) -> tokio::task::JoinHandle<Result<()>> {
        self.listen(true)
    }

    fn listen(&mut self, explicit_drain: bool) -> tokio::task::JoinHandle<Result<()>> {
        let (send, queue) = mpsc::channel(2);
        let (terminal, starts) = terminal_handshake::Intake::new();
        let (_, status) = watch::channel(Response::success("active"));
        let (drain, drain_recv) = oneshot::channel();
        self.drain = explicit_drain.then_some(drain);
        self.queue = queue;
        self.starts = starts;
        tokio::spawn(serve_with_listener(
            self.accept.take().unwrap(),
            send,
            status,
            Some(terminal),
            None,
            explicit_drain.then_some(drain_recv),
            None,
            CandidateListener::registered(self.cut.observe(), None),
        ))
    }

    async fn request(&self, request: &Request) -> UnixStream {
        let mut stream = UnixStream::connect(&self.path).await.unwrap();
        write_frame_body(&mut stream, request).await.unwrap();
        stream
    }
}

async fn joined(task: tokio::task::JoinHandle<Result<()>>) {
    tokio::time::timeout(STEP, task)
        .await
        .expect("listener did not drain")
        .unwrap()
        .unwrap();
}

async fn disconnected(stream: &mut UnixStream) {
    let result = tokio::time::timeout(STEP, stream.read_u8())
        .await
        .expect("socket remained open");
    assert!(matches!(
        result.unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
    ));
}

fn terminal_request() -> Request {
    Request::TerminalStart {
        request: terminal_handshake::Start {
            remote: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into(),
            session: "0123456789abcdef0123456789abcdef".into(),
            relays: vec!["wss://relay.example/".into()],
            attach: terminal_handshake::Attach::Default,
            session_slot: None,
            offer: None,
            rows: 24,
            columns: 80,
        },
    }
}

#[tokio::test]
async fn fatal_listener_does_not_admit_queued_status_command_or_terminal() {
    let mut fixture = Listener::new();
    let mut status = fixture.request(&Request::Status {}).await;
    let mut command = fixture.request(&Request::CommandCleanupStatus {}).await;
    let mut terminal = fixture.request(&terminal_request()).await;
    fixture.cut.trip();
    joined(fixture.start()).await;
    disconnected(&mut status).await;
    disconnected(&mut command).await;
    disconnected(&mut terminal).await;
    assert!(matches!(
        fixture.queue.try_recv(),
        Err(mpsc::error::TryRecvError::Disconnected)
    ));
    assert!(matches!(
        fixture.starts.try_recv(),
        Err(mpsc::error::TryRecvError::Disconnected)
    ));
}

#[tokio::test]
async fn fatal_listener_wakes_accept_and_drains_partial_and_pending_exchanges() {
    let mut fixture = Listener::new();
    let task = fixture.listen(false);
    let mut status = fixture.request(&Request::Status {}).await;
    let active = tokio::time::timeout(STEP, read_frame::<Response>(&mut status))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.result, Some(serde_json::json!("active")));
    let mut partial = UnixStream::connect(&fixture.path).await.unwrap();
    let terminal_frame = serde_json::to_vec(&terminal_request()).unwrap();
    partial
        .write_u32(terminal_frame.len().try_into().unwrap())
        .await
        .unwrap();
    partial
        .write_all(&terminal_frame[..terminal_frame.len() - 1])
        .await
        .unwrap();
    let mut command = fixture.request(&Request::CommandCleanupStatus {}).await;
    let pending = tokio::time::timeout(STEP, fixture.queue.recv())
        .await
        .unwrap()
        .unwrap();

    fixture.cut.trip();
    let _ = partial
        .write_all(&terminal_frame[terminal_frame.len() - 1..])
        .await;
    joined(task).await;
    assert!(fixture.starts.try_recv().is_err());
    assert!(pending.respond.send(Response::success("stale")).is_err());
    disconnected(&mut partial).await;
    disconnected(&mut command).await;
    assert_eq!(
        UnixStream::connect(&fixture.path).await.unwrap_err().kind(),
        io::ErrorKind::ConnectionRefused
    );
}

#[tokio::test]
async fn fatal_listener_leaves_pre_cut_terminal_transfer_with_its_owner() {
    let mut fixture = Listener::new();
    let task = fixture.start();
    let mut client = fixture.request(&terminal_request()).await;
    let transferred = tokio::time::timeout(STEP, fixture.starts.recv())
        .await
        .unwrap()
        .unwrap();
    fixture.cut.trip();
    joined(task).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), client.read_u8())
            .await
            .is_err()
    );
    drop(transferred);
    disconnected(&mut client).await;
}

#[tokio::test]
async fn fatal_listener_accepts_explicit_healthy_drain() {
    let mut fixture = Listener::new();
    let task = fixture.start();
    let mut status = fixture.request(&Request::Status {}).await;
    assert!(
        tokio::time::timeout(STEP, read_frame::<Response>(&mut status))
            .await
            .unwrap()
            .unwrap()
            .ok
    );
    fixture.drain.take().unwrap().send(()).unwrap();
    joined(task).await;
    assert!(!fixture.cut.observe().is_tripped());
}

struct PrefixCut {
    trip: Trip,
    writes: Vec<u8>,
}

impl AsyncWrite for PrefixCut {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        assert!(self.writes.is_empty(), "a fresh write crossed the cut");
        self.writes.extend_from_slice(bytes);
        self.trip.trip();
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        panic!("flushed after cut")
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        panic!("shut down after cut")
    }
}

#[tokio::test]
async fn fatal_listener_cannot_write_frame_body_after_admitted_prefix() {
    let (trip, fatal) = Trip::new();
    let mut cut = PrefixCut {
        trip,
        writes: Vec::new(),
    };
    let mut fenced = FencedStream::new(&mut cut, &fatal);
    assert!(
        write_frame(&mut fenced, &Response::success("stale"))
            .await
            .is_err()
    );
    assert_eq!(cut.writes.len(), 4);
}
