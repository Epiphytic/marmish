//! Scripted local IPC coverage for finite failure draining across the real stream codec.
use crate::{
    Error,
    command_cli::Context,
    copy_io::CopyIo,
    ipc,
    stream_service::{StreamLocalRequest, StreamLocalResult},
    stream_session::StreamSession,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use marmish_stream::{
    codec::{self, Codec},
    copy_protocol::{CopyError, Frame, FrameDecoder},
    wire::{self, Binding, Request, RequestBody, ResponseBody, Terminal},
};
use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

async fn request(socket: &mut UnixStream) -> (Vec<u8>, Request) {
    let length = socket.read_u32().await.unwrap() as usize;
    assert!(length <= 65_536);
    let mut bytes = vec![0; length];
    socket.read_exact(&mut bytes).await.unwrap();
    let ipc::Request::Stream {
        request: StreamLocalRequest::Frame { frame_base64, .. },
    } = serde_json::from_slice(&bytes).unwrap()
    else {
        panic!("expected stream frame")
    };
    let encoded = STANDARD.decode(frame_base64).unwrap();
    let decoded = wire::decode_request(&encoded).unwrap();
    (encoded, decoded)
}

async fn reply(socket: &mut UnixStream, encoded: &[u8], request: &Request, body: ResponseBody) {
    let response = wire::Response {
        binding: request.binding.clone(),
        sequence: request.sequence,
        request_sha256: wire::request_digest(encoded),
        body,
    };
    let local = ipc::Response::stream(StreamLocalResult::Frame {
        frame_base64: STANDARD.encode(wire::encode_response(&response).unwrap()),
    });
    let bytes = serde_json::to_vec(&local).unwrap();
    socket.write_u32(bytes.len() as u32).await.unwrap();
    socket.write_all(&bytes).await.unwrap();
}

async fn scenario(output: Vec<u8>, expected: CopyError, peer_closed: bool, credit: bool) {
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let path = directory.path().join("socket");
    let listener = ipc::bind(&path).unwrap();
    let output_length = output.len() as u64;
    let server = tokio::spawn(async move {
        let mut input_end = 0;
        let mut error_seen = false;
        let mut eof_seen = false;
        let mut terminal_sent = false;
        let mut decoder = FrameDecoder::default();
        let mut sequence = 1;
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (encoded, request) = request(&mut socket).await;
            assert_eq!(request.sequence, sequence);
            sequence += 1;
            let RequestBody::Exchange {
                stdin,
                stdin_eof,
                stdout_ack,
                stderr_ack,
            } = &request.body
            else {
                panic!("failure drain must never send Cancel or another control message")
            };
            assert!(*stdout_ack <= output_length);
            assert!(*stderr_ack <= 3);
            if let Some(block) = stdin {
                assert!(
                    !peer_closed,
                    "a remote Error with closed stdin must not be answered"
                );
                assert_eq!(block.offset, input_end);
                let bytes = codec::decode(block, Codec::None).unwrap();
                input_end += bytes.len() as u64;
                let (consumed, frame) = decoder.feed(&bytes).unwrap();
                assert_eq!(consumed, bytes.len());
                assert!(matches!(frame, Some(Frame::Error(code)) if code == expected));
                assert!(!error_seen);
                error_seen = true;
            }
            if let Some(end) = stdin_eof {
                assert!(!peer_closed);
                assert!(error_seen, "finite Error must precede EOF");
                assert_eq!(*end, input_end);
                eof_seen = true;
            }
            let terminal = peer_closed && sequence > 2 || eof_seen;
            let final_ack = terminal_sent && *stdout_ack == output_length && *stderr_ack == 3;
            let body = ResponseBody::Exchange {
                stdin_accepted: input_end,
                stdin_delivered: input_end,
                stdin_closed: peer_closed || eof_seen,
                stdout: (*stdout_ack == 0).then(|| codec::encode(0, &output, Codec::None).unwrap()),
                stderr: (terminal && *stderr_ack == 0)
                    .then(|| codec::encode(0, b"log", Codec::None).unwrap()),
                stdout_eof: terminal.then_some(output_length),
                stderr_eof: terminal.then_some(3),
                cancel_requested: false,
                terminal: terminal.then_some(Terminal::Exited { code: 1 }),
            };
            reply(&mut socket, &encoded, &request, body).await;
            terminal_sent |= terminal;
            if final_ack {
                assert_eq!(error_seen, !peer_closed);
                assert_eq!(eof_seen, !peer_closed);
                break;
            }
        }
    });
    let mut io = copy_io(path);
    if credit {
        io.configure_credit(0, [0; 32]).unwrap();
    }
    let failure = io.next().await;
    assert!(matches!(failure, Err(Error::Copy(code)) if code == expected));
    assert_eq!(io.peer_error_received(), peer_closed);
    io.abort_failure(expected).await.unwrap();
    assert!(io.cleanup_confirmed());
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap();
}

fn copy_io(path: std::path::PathBuf) -> CopyIo {
    let session = StreamSession::new(
        Arc::new(Context {
            socket: path,
            profile: "test".into(),
        }),
        vec![],
        Binding {
            stream: [1; 16],
            session: [2; 16],
            client: [3; 32],
            server: [4; 32],
        },
        Duration::from_secs(3),
        Codec::None,
    );
    CopyIo::new(session, Codec::None)
}

#[tokio::test]
async fn malformed_application_frame_drains_replayed_bytes_error_eof_and_final_ack() {
    tokio::time::timeout(
        Duration::from_secs(5),
        scenario(vec![255; 8], CopyError::Protocol, false, false),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn invalid_checkpoint_credit_drains_without_reinterpreting_replay() {
    let bytes = Frame::Checkpoint {
        offset: 1,
        sha256: [9; 32],
    }
    .encode()
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        scenario(bytes, CopyError::HashMismatch, false, true),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn remote_error_closed_input_drains_terminal_and_acks_without_reply_error() {
    let bytes = Frame::Error(CopyError::NotFound).encode().unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        scenario(bytes, CopyError::NotFound, true, false),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn lost_transport_during_failure_drain_never_confirms_cleanup_or_sends_cancel() {
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let path = directory.path().join("socket");
    let listener = ipc::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let (_, frame) = request(&mut socket).await;
        assert!(matches!(frame.body, RequestBody::Exchange { .. }));
        // Close the owned connection and listener before any reply. No terminal
        // or final ACK exists, so a local error cannot count as remote cleanup.
    });
    let mut io = copy_io(path);
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        io.abort_failure(CopyError::Protocol),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(Error::Io(_) | Error::Transport(_))));
    assert!(!io.cleanup_confirmed());
    server.await.unwrap();
}
