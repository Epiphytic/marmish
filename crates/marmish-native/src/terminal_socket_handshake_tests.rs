use super::*;
use crate::terminal_ipc_wire::{self, Direction, Record};
use std::os::unix::fs::PermissionsExt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
    task::JoinHandle,
};

const STEP: std::time::Duration = std::time::Duration::from_secs(3);

fn single_request() -> Start {
    Start {
        remote: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into(),
        session: "0123456789abcdef0123456789abcdef".into(),
        relays: vec!["wss://relay.example/".into()],
        attach: Attach::Default,
        rows: 24,
        columns: 80,
    }
}

struct Listener {
    _directory: tempfile::TempDir,
    path: std::path::PathBuf,
    starts: mpsc::Receiver<PendingStart>,
    ordinary: mpsc::Receiver<super::super::Pending>,
    task: JoinHandle<Result<()>>,
}

impl Listener {
    fn open() -> Self {
        Self::with_route(true)
    }

    fn with_route(enabled: bool) -> Self {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = directory.path().join("m");
        let listener = super::super::bind(&path).unwrap();
        let (ordinary_send, ordinary) = mpsc::channel(2);
        let (_, status) = watch::channel(Response::success("status"));
        let (terminal, starts) = Intake::new();
        let task = tokio::spawn(super::super::serve_inner(
            listener,
            ordinary_send,
            status,
            enabled.then_some(terminal),
        ));
        Self {
            _directory: directory,
            path,
            starts,
            ordinary,
            task,
        }
    }

    async fn offer(&mut self) -> (PendingStart, UnixStream) {
        let mut stream = UnixStream::connect(&self.path).await.unwrap();
        write_frame_body(
            &mut stream,
            &Request::TerminalStart {
                request: single_request(),
            },
        )
        .await
        .unwrap();
        let pending = timeout_at(Instant::now() + STEP, self.starts.recv())
            .await
            .unwrap()
            .unwrap();
        (pending, stream)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn terminal_socket_core_request_and_reply_keep_both_directions_open() {
    let mut fixture = Listener::open();
    let path = fixture.path.clone();
    let client =
        tokio::spawn(async move { start(&path, single_request(), Instant::now() + STEP).await });
    let pending = timeout_at(Instant::now() + STEP, fixture.starts.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.request.columns, 80);
    assert!(fixture.ordinary.try_recv().is_err());
    let owned = pending.admit().await.unwrap();
    let mut server = UnixStream::from_std(owned.stream).unwrap();
    let first = terminal_ipc_wire::encode(Direction::ToClient, Role::Writer, Record::ResizeApplied)
        .unwrap();
    server.write_all(&first).await.unwrap();
    let mut client = UnixStream::from_std(client.await.unwrap().unwrap()).unwrap();
    let mut actual = [0; 4];
    client.read_exact(&mut actual).await.unwrap();
    assert_eq!(actual.as_slice(), first);
    let input =
        terminal_ipc_wire::encode(Direction::ToDaemon, Role::Writer, Record::Input(b"\0\xff"))
            .unwrap();
    client.write_all(&input).await.unwrap();
    let mut actual = [0; 6];
    server.read_exact(&mut actual).await.unwrap();
    assert_eq!(actual.as_slice(), input);
}

#[tokio::test]
async fn terminal_runtime_core_pending_and_attached_have_independent_capacity() {
    let mut fixture = Listener::open();
    let mut pending = Vec::new();
    for _ in 0..PENDING {
        pending.push(fixture.offer().await);
    }
    assert!(matches!(
        start(&fixture.path, single_request(), Instant::now() + STEP).await,
        Err(Error::Invalid("terminal_capacity"))
    ));
    let mut attached = Vec::new();
    for (candidate, client) in pending {
        attached.push(candidate.admit().await.unwrap());
        drop(client);
    }
    for _ in PENDING..ATTACHED {
        let (candidate, client) = fixture.offer().await;
        attached.push(candidate.admit().await.unwrap());
        drop(client);
    }
    let (candidate, mut client) = fixture.offer().await;
    assert!(matches!(
        candidate.admit().await,
        Err(Error::Invalid("terminal_capacity"))
    ));
    let response: Response = read_frame(&mut client).await.unwrap();
    assert!(!response.ok);
    assert_eq!(response.error.as_deref(), Some("terminal_capacity"));
    assert_eq!(client.read(&mut [0]).await.unwrap(), 0);
    drop(attached.pop());
    let (candidate, _client) = fixture.offer().await;
    assert!(candidate.admit().await.is_ok());
}

#[tokio::test]
async fn terminal_socket_core_ordinary_bytes_and_half_close() {
    let mut fixture = Listener::open();
    let mut stream = UnixStream::connect(&fixture.path).await.unwrap();
    let body = b"{\"operation\":\"command_cleanup_status\"}";
    let mut frame = Vec::from((body.len() as u32).to_be_bytes());
    frame.extend_from_slice(body);
    stream.write_all(&frame).await.unwrap();
    stream.shutdown().await.unwrap();
    let pending = timeout_at(Instant::now() + STEP, fixture.ordinary.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(pending.request, Request::CommandCleanupStatus {}));
    assert!(fixture.starts.try_recv().is_err());
    pending.respond.send(Response::success("old")).ok().unwrap();
    let mut received = Vec::new();
    stream.read_to_end(&mut received).await.unwrap();
    let expected = b"{\"ok\":true,\"result\":\"old\",\"error\":null}";
    assert_eq!(received[..4], (expected.len() as u32).to_be_bytes());
    assert_eq!(&received[4..], expected);

    let (mut local, mut peer) = UnixStream::pair().unwrap();
    write_frame(&mut local, &Request::CommandCleanupStatus {})
        .await
        .unwrap();
    received.clear();
    peer.read_to_end(&mut received).await.unwrap();
    assert_eq!(received, frame);
}

#[tokio::test]
async fn terminal_socket_core_declines_unknown_fields_modes_and_expired_reply() {
    let mut fixture = Listener::open();
    let mut stream = UnixStream::connect(&fixture.path).await.unwrap();
    let body = serde_json::json!({
        "operation": "terminal_start",
        "request": {
            "remote": single_request().remote, "session": single_request().session,
            "rows": 24, "columns": 80, "relays": ["wss://relay.example/"],
            "attach": "default", "digest": "invalid",
        },
    });
    write_frame_body(&mut stream, &body).await.unwrap();
    let answer: Response = read_frame(&mut stream).await.unwrap();
    assert_eq!(answer.error.as_deref(), Some("invalid_request"));
    assert!(fixture.starts.try_recv().is_err());

    let path = fixture.path.clone();
    let unsupported = Start {
        attach: Attach::Observe,
        ..single_request()
    };
    let client =
        tokio::spawn(async move { start(&path, unsupported, Instant::now() + STEP).await });
    let pending = timeout_at(Instant::now() + STEP, fixture.starts.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        pending.admit().await,
        Err(Error::Invalid("terminal_mode_unavailable"))
    ));
    assert!(matches!(
        client.await.unwrap(),
        Err(Error::Invalid("terminal_mode_unavailable"))
    ));

    let (mut pending, mut client) = fixture.offer().await;
    pending.deadline = Instant::now();
    assert!(matches!(pending.admit().await, Err(Error::Timeout)));
    assert_eq!(client.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn terminal_socket_core_exact_ready_frame_and_early_binary_suffix() {
    let mut fixture = Listener::open();
    let (pending, mut client) = fixture.offer().await;
    let attached = pending.admit().await.unwrap();
    let mut server = UnixStream::from_std(attached.stream).unwrap();
    let frame = [0, 2, 1, 8];
    server.write_all(&frame).await.unwrap();
    let expected = br#"{"ok":true,"terminal_stream":"writer"}"#;
    assert_eq!(client.read_u32().await.unwrap(), expected.len() as u32);
    let mut actual = vec![0; expected.len() + frame.len()];
    client.read_exact(&mut actual).await.unwrap();
    assert_eq!(&actual[..expected.len()], expected);
    assert_eq!(&actual[expected.len()..], frame);
}

#[tokio::test]
async fn terminal_socket_core_rejects_mixed_or_unsupported_client_replies() {
    let fixture = Listener::open();
    let scripted_path = fixture._directory.path().join("scripted");
    let listener = super::super::bind(&scripted_path).unwrap();
    for body in [
        br#"{"ok":true}"#.as_slice(),
        br#"{"ok":true,"terminal_stream":"observer"}"#,
        br#"{"ok":true,"terminal_stream":"writer","result":null}"#,
        br#"{"ok":true,"terminal_stream":"writer","command":null}"#,
        br#"{"ok":true,"terminal_stream":"writer","stream":null}"#,
        br#"{"ok":false,"terminal_stream":"writer","error":"terminal_busy"}"#,
        br#"{"ok":true,"ok":true,"terminal_stream":"writer"}"#,
    ] {
        let (result, ()) = tokio::join!(
            start(&scripted_path, single_request(), Instant::now() + STEP),
            async {
                let (mut server, _) = listener.accept().await.unwrap();
                let _: Request = read_frame(&mut server).await.unwrap();
                server.write_u32(body.len() as u32).await.unwrap();
                server.write_all(body).await.unwrap();
            },
        );
        assert!(matches!(
            result,
            Err(Error::Invalid("terminal_start_response"))
        ));
    }
}

#[tokio::test]
async fn terminal_socket_core_unrouted_invalid_and_absent_responses() {
    let fixture = Listener::with_route(false);
    assert!(matches!(
        start(&fixture.path, single_request(), Instant::now() + STEP).await,
        Err(Error::Invalid("terminal_unavailable"))
    ));

    let fixture = Listener::open();
    let mut client = UnixStream::connect(&fixture.path).await.unwrap();
    let request = Request::TerminalStart {
        request: Start {
            rows: 1,
            ..single_request()
        },
    };
    write_frame_body(&mut client, &request).await.unwrap();
    let reply: Response = read_frame(&mut client).await.unwrap();
    assert_eq!(reply.error.as_deref(), Some("terminal_start_invalid"));
    assert_eq!(client.read(&mut [0]).await.unwrap(), 0);

    let script = fixture._directory.path().join("scripted");
    let listener = super::super::bind(&script).unwrap();
    let deadline = Instant::now() + std::time::Duration::from_millis(100);
    let (result, ()) = tokio::join!(start(&script, single_request(), deadline), async {
        let (mut server, _) = timeout_at(deadline, listener.accept())
            .await
            .unwrap()
            .unwrap();
        let _: Request = timeout_at(deadline, read_frame(&mut server))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(server.read(&mut [0]).await.unwrap(), 0);
    },);
    assert!(matches!(result, Err(Error::Timeout)));
}

#[tokio::test]
#[ignore = "local Unix socket timing"]
async fn terminal_socket_handshake_bench() {
    let mut fixture = Listener::open();
    let mut samples = [0u128; 100];
    for sample in &mut samples {
        let path = fixture.path.clone();
        let began = std::time::Instant::now();
        let (client, server) = tokio::join!(
            start(&path, single_request(), Instant::now() + STEP),
            async {
                let pending = timeout_at(Instant::now() + STEP, fixture.starts.recv())
                    .await
                    .unwrap()
                    .unwrap();
                pending.admit().await.unwrap()
            },
        );
        *sample = began.elapsed().as_nanos();
        drop((client.unwrap(), server));
    }
    samples.sort_unstable();
    eprintln!(
        "100 same-UID socket admissions: p50={}ns p95={}ns",
        samples[49], samples[94]
    );
}

#[tokio::test]
async fn terminal_socket_core_pending_owner_watches_deadline_eof_and_early_bytes() {
    let mut fixture = Listener::open();
    let (mut expires, expiry_client) = fixture.offer().await;
    expires.deadline = Instant::now() + std::time::Duration::from_millis(10);
    let expires = expires.register().unwrap();
    assert!(matches!(expires.cancelled().await, Error::Timeout));
    drop(expires);
    drop(expiry_client);

    let (closed, closed_client) = fixture.offer().await;
    drop(closed_client);
    let closed = closed.register().unwrap();
    assert!(matches!(
        closed.cancelled().await,
        Error::Invalid("terminal_start_closed")
    ));
    drop(closed);

    let (early, mut early_client) = fixture.offer().await;
    early_client.write_all(b"X").await.unwrap();
    let early = early.register().unwrap();
    assert!(matches!(
        early.cancelled().await,
        Error::Invalid("terminal_start_invalid")
    ));
    drop(early);
    drop(early_client);

    let (fresh, mut fresh_client) = fixture.offer().await;
    let attached = fresh.admit().await.unwrap();
    let response: Ready = read_frame(&mut fresh_client).await.unwrap();
    assert!(response.ok);
    let mut daemon = terminal_socket_session::DaemonSocket::new(attached, Role::Writer);
    let now = std::time::Instant::now();
    daemon.flush(now, now + STEP).unwrap();
    let mut packet = [0; 12];
    fresh_client.read_exact(&mut packet).await.unwrap();
    let mut decoder = terminal_ipc_wire::Decoder::new(Direction::ToClient, Role::Writer);
    let (_, record) = decoder.push(&packet).unwrap();
    assert!(matches!(record, Some(Record::Credit(_))));
}
