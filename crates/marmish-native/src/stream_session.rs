//! Bounded stream transport without process-global stdio or filesystem work.
//! Dropping an exchange leaves its exact request available for retry; it never
//! requests remote cancellation.
use crate::{
    Error, Result,
    command_cli::Context,
    ipc,
    stream_service::{StreamLocalRequest, StreamLocalResult},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use marmish_stream::{
    codec::{self, Codec},
    wire::{self, Binding, Request, RequestBody, ResponseBody, Terminal},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;

fn invalid(reason: &'static str) -> Error {
    Error::Invalid(reason)
}
fn protocol<T>(value: marmish_stream::Result<T>) -> Result<T> {
    value.map_err(|_| invalid("stream_protocol"))
}

#[derive(Clone)]
pub(crate) struct ControlHandle {
    context: Arc<Context>,
    relays: Vec<String>,
    pub(crate) binding: Binding,
    idle: Duration,
    open_attempted: Arc<AtomicBool>,
}

impl ControlHandle {
    pub(crate) fn open_attempted(&self) -> bool {
        self.open_attempted.load(Ordering::Acquire)
    }

    async fn send(&self, request: &Pending) -> Result<ResponseBody> {
        loop {
            let local = StreamLocalRequest::Frame {
                frame_base64: STANDARD.encode(&request.encoded),
                relays: self.relays.clone(),
            };
            let result = tokio::time::timeout_at(
                request.deadline,
                ipc::stream_request(&self.context.socket, local),
            )
            .await
            .map_err(|_| Error::Timeout)??;
            match result {
                StreamLocalResult::Pending => (),
                StreamLocalResult::Frame { frame_base64 } => {
                    if frame_base64.len() > 44_000 {
                        return Err(invalid("stream_response_size"));
                    }
                    let bytes = STANDARD
                        .decode(&frame_base64)
                        .map_err(|_| invalid("stream_response_base64"))?;
                    let response = protocol(wire::decode_response(&bytes))?;
                    if response.binding != self.binding
                        || response.sequence != request.sequence
                        || response.request_sha256 != request.digest
                    {
                        return Err(invalid("stream_response_binding"));
                    }
                    // Preserve finite remote errors for the caller, including
                    // typed copy refusal and its authenticated prior binding.
                    return Ok(response.body);
                }
                _ => return Err(invalid("stream_local_response")),
            }
            tokio::time::sleep_until(
                (Instant::now() + Duration::from_millis(100)).min(request.deadline),
            )
            .await;
            if Instant::now() >= request.deadline {
                return Err(Error::Timeout);
            }
        }
    }

    async fn call(&self, body: RequestBody, deadline: Instant) -> Result<ResponseBody> {
        let pending = Pending::new(&self.binding, 0, body, deadline)?;
        self.send(&pending).await
    }

    pub(crate) async fn status(&self) -> Result<ResponseBody> {
        self.call(RequestBody::Status, Instant::now() + self.idle)
            .await
    }

    /// Terminal response is returned unchanged. The frontend decides whether it
    /// proves cleanup or reports Uncertain; no local PID is used for signalling.
    pub(crate) async fn cancel(&self) -> Result<Terminal> {
        if !self.open_attempted() {
            return Err(invalid("stream_interrupted_before_open"));
        }
        let deadline = Instant::now() + self.idle.min(Duration::from_secs(60));
        loop {
            let response = self.call(RequestBody::Cancel, deadline).await?;
            match response {
                ResponseBody::Exchange {
                    terminal: Some(terminal),
                    ..
                } => return Ok(terminal),
                ResponseBody::Exchange { .. } => (),
                ResponseBody::Error {
                    code: wire::WireErrorCode::Capacity,
                } => (),
                ResponseBody::Error { code } => return Err(Error::Stream(code)),
                _ => return Err(invalid("stream_cancel_response")),
            }
            tokio::time::sleep_until((Instant::now() + Duration::from_millis(500)).min(deadline))
                .await;
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
        }
    }
}

#[derive(Clone)]
struct Pending {
    encoded: Vec<u8>,
    digest: wire::Digest,
    sequence: u64,
    deadline: Instant,
    body: RequestBody,
}
impl Pending {
    fn new(binding: &Binding, sequence: u64, body: RequestBody, deadline: Instant) -> Result<Self> {
        let encoded = protocol(wire::encode_request(&Request {
            binding: binding.clone(),
            sequence,
            body: body.clone(),
        }))?;
        let digest = wire::request_digest(&encoded);
        Ok(Self {
            encoded,
            digest,
            sequence,
            deadline,
            body,
        })
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct OutputAcks {
    pub stdout: u64,
    pub stderr: u64,
}

pub(crate) struct OutputBlock {
    pub offset: u64,
    pub bytes: Vec<u8>,
}
pub(crate) struct Snapshot {
    pub stdin_accepted: u64,
    pub stdin_delivered: u64,
    pub stdout: Option<OutputBlock>,
    pub stderr: Option<OutputBlock>,
    pub stdout_eof: Option<u64>,
    pub stderr_eof: Option<u64>,
    pub terminal: Option<Terminal>,
}

pub(crate) struct StreamSession {
    control: ControlHandle,
    codec: Codec,
    pending: Option<Pending>,
    next_sequence: u64,
    input_end: u64,
    accepted: u64,
    delivered: u64,
    input_closed: bool,
    eof: bool,
    acks: OutputAcks,
    received: OutputAcks,
    stdout_eof: Option<u64>,
    stderr_eof: Option<u64>,
    terminal: Option<Terminal>,
}

impl StreamSession {
    pub(crate) fn new(
        context: Arc<Context>,
        relays: Vec<String>,
        binding: Binding,
        idle: Duration,
        codec: Codec,
    ) -> Self {
        Self {
            control: ControlHandle {
                context,
                relays,
                binding,
                idle,
                open_attempted: Arc::new(AtomicBool::new(false)),
            },
            codec,
            pending: None,
            next_sequence: 1,
            input_end: 0,
            accepted: 0,
            delivered: 0,
            input_closed: false,
            eof: false,
            acks: OutputAcks::default(),
            received: OutputAcks::default(),
            stdout_eof: None,
            stderr_eof: None,
            terminal: None,
        }
    }
    pub(crate) fn control(&self) -> ControlHandle {
        self.control.clone()
    }

    /// Used for negotiation and Open. No await precedes retaining the request.
    /// Re-enter with the identical body after a dropped future or local error.
    pub(crate) async fn setup(&mut self, body: RequestBody) -> Result<ResponseBody> {
        self.setup_with_timeout(body, self.control.idle).await
    }

    pub(crate) async fn setup_with_timeout(
        &mut self,
        body: RequestBody,
        timeout: Duration,
    ) -> Result<ResponseBody> {
        if !matches!(
            body,
            RequestBody::Hello { .. }
                | RequestBody::CopyHello { .. }
                | RequestBody::Open { .. }
                | RequestBody::OpenCopy { .. }
        ) {
            return Err(invalid("stream_setup_request"));
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.body != body || p.sequence != 0)
        {
            return Err(invalid("stream_request_pending"));
        }
        if self.pending.is_none() {
            self.pending = Some(Pending::new(
                &self.control.binding,
                0,
                body.clone(),
                Instant::now() + timeout.min(self.control.idle),
            )?);
        }
        if matches!(
            body,
            RequestBody::Open { .. } | RequestBody::OpenCopy { .. }
        ) {
            self.control.open_attempted.store(true, Ordering::Release);
        }
        let response = self
            .control
            .send(self.pending.as_ref().expect("request retained"))
            .await?;
        self.pending = None;
        Ok(response)
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn input_closed(&self) -> bool {
        self.input_closed
    }
    pub(crate) fn can_send(&self, length: usize) -> bool {
        !self.input_closed
            && !self.eof
            && length <= marmish_stream::MAX_BLOCK
            && self.input_end.saturating_sub(self.delivered) + length as u64
                <= marmish_stream::WINDOW_BYTES as u64
    }

    pub(crate) async fn exchange(
        &mut self,
        input: Option<&[u8]>,
        eof: bool,
        acks: OutputAcks,
    ) -> Result<Snapshot> {
        if self.pending.is_some() {
            return Err(invalid("stream_request_pending"));
        }
        if acks.stdout < self.acks.stdout
            || acks.stderr < self.acks.stderr
            || acks.stdout > self.received.stdout
            || acks.stderr > self.received.stderr
        {
            return Err(invalid("stream_output_ack"));
        }
        if input.is_some_and(|bytes| bytes.is_empty() || !self.can_send(bytes.len()))
            || (eof && self.input_closed)
        {
            return Err(invalid("stream_input_state"));
        }
        let block = input
            .map(|bytes| protocol(codec::encode(self.input_end, bytes, self.codec)))
            .transpose()?;
        let end = self
            .input_end
            .checked_add(input.map_or(0, |b| b.len() as u64))
            .ok_or(invalid("stream_offset"))?;
        let body = RequestBody::Exchange {
            stdin: block,
            stdin_eof: ((eof || self.eof) && !self.input_closed).then_some(end),
            stdout_ack: acks.stdout,
            stderr_ack: acks.stderr,
        };
        self.pending = Some(Pending::new(
            &self.control.binding,
            self.next_sequence,
            body,
            Instant::now() + self.control.idle,
        )?);
        self.resume().await
    }

    /// Retry exactly the retained exchange, including its original deadline.
    pub(crate) async fn resume(&mut self) -> Result<Snapshot> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(invalid("stream_no_pending_request"))?;
        let RequestBody::Exchange {
            stdin,
            stdin_eof,
            stdout_ack,
            stderr_ack,
        } = &pending.body
        else {
            return Err(invalid("stream_pending_setup"));
        };
        let sent_end = match stdin {
            Some(block) => protocol(block.end())?,
            None => self.input_end,
        };
        let eof = stdin_eof.is_some();
        let acks = OutputAcks {
            stdout: *stdout_ack,
            stderr: *stderr_ack,
        };
        let response = self.control.send(pending).await?;
        if let ResponseBody::Error { code } = response {
            return Err(Error::Stream(code));
        }
        let ResponseBody::Exchange {
            stdin_accepted,
            stdin_delivered,
            stdin_closed,
            stdout,
            stderr,
            stdout_eof,
            stderr_eof,
            terminal,
            ..
        } = response
        else {
            return Err(invalid("stream_exchange_response"));
        };
        if stdin_delivered < self.delivered
            || stdin_delivered > stdin_accepted
            || stdin_accepted < self.accepted
            || stdin_accepted > sent_end
            || (!stdin_closed && stdin_accepted != sent_end)
            || (self.input_closed && !stdin_closed)
        {
            return Err(invalid("stream_stdin_ack"));
        }
        let decode = |block: Option<codec::EncodedBlock>, ack| -> Result<Option<OutputBlock>> {
            block
                .map(|block| {
                    if block.offset != ack
                        || (block.codec != Codec::None && block.codec != self.codec)
                    {
                        return Err(invalid("stream_output_offset"));
                    }
                    Ok(OutputBlock {
                        offset: block.offset,
                        bytes: protocol(codec::decode(&block, self.codec))?,
                    })
                })
                .transpose()
        };
        let stdout = decode(stdout, acks.stdout)?;
        let stderr = decode(stderr, acks.stderr)?;
        let stdout_end = stdout
            .as_ref()
            .map_or(acks.stdout, |b| b.offset + b.bytes.len() as u64);
        let stderr_end = stderr
            .as_ref()
            .map_or(acks.stderr, |b| b.offset + b.bytes.len() as u64);
        if stdout_eof.is_some_and(|end| end < stdout_end)
            || stderr_eof.is_some_and(|end| end < stderr_end)
            || self.stdout_eof.is_some_and(|end| stdout_eof != Some(end))
            || self.stderr_eof.is_some_and(|end| stderr_eof != Some(end))
            || self.terminal.is_some_and(|old| terminal != Some(old))
        {
            return Err(invalid("stream_output_eof"));
        }
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(invalid("stream_sequence"))?;
        self.input_end = if stdin_closed {
            stdin_accepted
        } else {
            sent_end
        };
        self.accepted = stdin_accepted;
        self.delivered = stdin_delivered;
        self.input_closed = stdin_closed;
        self.eof |= eof || stdin_closed;
        self.acks = acks;
        self.received.stdout = self.received.stdout.max(stdout_end);
        self.received.stderr = self.received.stderr.max(stderr_end);
        self.stdout_eof = stdout_eof;
        self.stderr_eof = stderr_eof;
        self.terminal = terminal;
        self.pending = None;
        Ok(Snapshot {
            stdin_accepted,
            stdin_delivered,
            stdout,
            stderr,
            stdout_eof,
            stderr_eof,
            terminal,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn session(path: std::path::PathBuf) -> StreamSession {
        StreamSession::new(
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
        )
    }

    async fn read(socket: &mut tokio::net::UnixStream) -> (Vec<u8>, Request) {
        let length = socket.read_u32().await.unwrap() as usize;
        assert!(length <= 65_536);
        let mut bytes = vec![0; length];
        socket.read_exact(&mut bytes).await.unwrap();
        let ipc::Request::Stream {
            request: StreamLocalRequest::Frame { frame_base64, .. },
        } = serde_json::from_slice(&bytes).unwrap()
        else {
            panic!("expected stream frame");
        };
        let encoded = STANDARD.decode(frame_base64).unwrap();
        let request = wire::decode_request(&encoded).unwrap();
        (encoded, request)
    }

    async fn reply(socket: &mut tokio::net::UnixStream, response: ipc::Response) {
        let bytes = serde_json::to_vec(&response).unwrap();
        socket.write_u32(bytes.len() as u32).await.unwrap();
        socket.write_all(&bytes).await.unwrap();
    }

    fn terminal(request: &Request, encoded: &[u8]) -> ipc::Response {
        let response = wire::Response {
            binding: request.binding.clone(),
            sequence: request.sequence,
            request_sha256: wire::request_digest(encoded),
            body: ResponseBody::Exchange {
                stdin_accepted: 1,
                stdin_delivered: 0,
                stdin_closed: true,
                stdout: None,
                stderr: None,
                stdout_eof: Some(0),
                stderr_eof: Some(0),
                cancel_requested: true,
                terminal: Some(Terminal::Cancelled),
            },
        };
        ipc::Response::stream(StreamLocalResult::Frame {
            frame_base64: STANDARD.encode(wire::encode_response(&response).unwrap()),
        })
    }

    #[tokio::test]
    async fn dropped_exchange_retries_exact_bytes_without_blocking_control() {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = directory.path().join("s");
        let listener = ipc::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let (encoded, request) = read(&mut first).await;
            assert_eq!(request.sequence, 1);
            reply(
                &mut first,
                ipc::Response::stream(StreamLocalResult::Pending),
            )
            .await;
            drop(first);
            let (mut cancel, _) = listener.accept().await.unwrap();
            let (cancel_bytes, cancel_request) = read(&mut cancel).await;
            assert!(matches!(cancel_request.body, RequestBody::Cancel));
            assert_eq!(cancel_request.sequence, 0);
            reply(&mut cancel, terminal(&cancel_request, &cancel_bytes)).await;
            let (mut retry, _) = listener.accept().await.unwrap();
            let (retried, retried_request) = read(&mut retry).await;
            assert_eq!(encoded, retried);
            reply(&mut retry, terminal(&retried_request, &retried)).await;
            assert!(
                tokio::time::timeout(Duration::from_millis(30), listener.accept())
                    .await
                    .is_err()
            );
        });
        let mut session = session(path);
        session
            .control
            .open_attempted
            .store(true, Ordering::Release);
        assert!(
            tokio::time::timeout(
                Duration::from_millis(30),
                session.exchange(Some(b"x"), false, OutputAcks::default())
            )
            .await
            .is_err()
        );
        let deadline = session.pending.as_ref().unwrap().deadline;
        assert_eq!(session.next_sequence, 1);
        assert_eq!(session.input_end, 0);
        assert_eq!(
            session.control().cancel().await.unwrap(),
            Terminal::Cancelled
        );
        assert_eq!(session.pending.as_ref().unwrap().deadline, deadline);
        let snapshot = session.resume().await.unwrap();
        assert_eq!(snapshot.terminal, Some(Terminal::Cancelled));
        assert_eq!(snapshot.stdin_accepted, 1);
        assert_eq!(session.next_sequence, 2);
        assert!(session.pending.is_none());
        drop(session);
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn invalid_ack_and_preopen_cancel_do_not_send_requests() {
        let mut session = session("/nonexistent/socket".into());
        assert!(matches!(
            session
                .exchange(
                    None,
                    false,
                    OutputAcks {
                        stdout: 1,
                        stderr: 0
                    }
                )
                .await,
            Err(Error::Invalid("stream_output_ack"))
        ));
        assert!(matches!(
            session.control().cancel().await,
            Err(Error::Invalid("stream_interrupted_before_open"))
        ));
        assert!(session.pending.is_none());
        assert!(!session.control().open_attempted());
        assert!(session.can_send(marmish_stream::MAX_BLOCK));
        assert!(!session.can_send(marmish_stream::MAX_BLOCK + 1));
    }

    async fn exchange_reply(
        socket: &mut tokio::net::UnixStream,
        request: &Request,
        encoded: &[u8],
        body: ResponseBody,
    ) {
        let response = wire::Response {
            binding: request.binding.clone(),
            sequence: request.sequence,
            request_sha256: wire::request_digest(encoded),
            body,
        };
        reply(
            socket,
            ipc::Response::stream(StreamLocalResult::Frame {
                frame_base64: STANDARD.encode(wire::encode_response(&response).unwrap()),
            }),
        )
        .await;
    }

    fn snapshot_body(terminal: Option<Terminal>) -> ResponseBody {
        ResponseBody::Exchange {
            stdin_accepted: 0,
            stdin_delivered: 0,
            stdin_closed: true,
            stdout: None,
            stderr: None,
            stdout_eof: Some(0),
            stderr_eof: Some(0),
            cancel_requested: false,
            terminal,
        }
    }

    #[tokio::test]
    async fn mismatched_pending_arguments_cannot_replace_exact_retry() {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = directory.path().join("retry");
        let listener = ipc::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (original, _) = read(&mut socket).await;
            reply(
                &mut socket,
                ipc::Response::stream(StreamLocalResult::Pending),
            )
            .await;
            drop(socket);
            let (mut socket, _) = listener.accept().await.unwrap();
            let (retried, request) = read(&mut socket).await;
            assert_eq!(retried, original);
            reply(&mut socket, terminal(&request, &retried)).await;
        });
        let mut session = session(path);
        assert!(
            tokio::time::timeout(
                Duration::from_millis(30),
                session.exchange(Some(b"x"), false, OutputAcks::default())
            )
            .await
            .is_err()
        );
        let original = session.pending.as_ref().unwrap().encoded.clone();
        let deadline = session.pending.as_ref().unwrap().deadline;
        for (input, eof, acks) in [
            (Some(b"y".as_slice()), false, OutputAcks::default()),
            (None, true, OutputAcks::default()),
            (
                None,
                false,
                OutputAcks {
                    stdout: 1,
                    stderr: 0,
                },
            ),
        ] {
            assert!(matches!(
                session.exchange(input, eof, acks).await,
                Err(Error::Invalid("stream_request_pending"))
            ));
            assert_eq!(session.pending.as_ref().unwrap().encoded, original);
            assert_eq!(session.pending.as_ref().unwrap().deadline, deadline);
        }
        assert_eq!(
            session.resume().await.unwrap().terminal,
            Some(Terminal::Cancelled)
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn output_ack_regression_is_rejected_without_an_ipc_request() {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = directory.path().join("acks");
        let listener = ipc::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            for sequence in 1..=2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (encoded, request) = read(&mut socket).await;
                assert_eq!(request.sequence, sequence);
                let RequestBody::Exchange {
                    stdout_ack,
                    stderr_ack,
                    ..
                } = &request.body
                else {
                    panic!()
                };
                assert_eq!(
                    (*stdout_ack, *stderr_ack),
                    if sequence == 1 { (0, 0) } else { (1, 1) }
                );
                let mut body = snapshot_body(None);
                if let ResponseBody::Exchange {
                    stdout,
                    stderr,
                    stdout_eof,
                    stderr_eof,
                    ..
                } = &mut body
                {
                    *stdout_eof = None;
                    *stderr_eof = None;
                    if sequence == 1 {
                        *stdout = Some(codec::encode(0, b"o", Codec::None).unwrap());
                        *stderr = Some(codec::encode(0, b"e", Codec::None).unwrap());
                    }
                }
                exchange_reply(&mut socket, &request, &encoded, body).await;
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
        });
        let mut session = session(path);
        session
            .exchange(None, false, OutputAcks::default())
            .await
            .unwrap();
        session
            .exchange(
                None,
                false,
                OutputAcks {
                    stdout: 1,
                    stderr: 1,
                },
            )
            .await
            .unwrap();
        for acks in [
            OutputAcks {
                stdout: 0,
                stderr: 1,
            },
            OutputAcks {
                stdout: 1,
                stderr: 0,
            },
        ] {
            assert!(matches!(
                session.exchange(None, false, acks).await,
                Err(Error::Invalid("stream_output_ack"))
            ));
            assert!(session.pending.is_none());
        }
        assert_eq!(session.next_sequence, 3);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn both_output_eofs_do_not_imply_terminal_and_final_ack_is_transmitted() {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = directory.path().join("eofs");
        let listener = ipc::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            for sequence in 1..=3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (encoded, request) = read(&mut socket).await;
                assert_eq!(request.sequence, sequence);
                let RequestBody::Exchange {
                    stdin,
                    stdin_eof,
                    stdout_ack,
                    stderr_ack,
                } = &request.body
                else {
                    panic!()
                };
                assert!(stdin.is_none() && stdin_eof.is_none());
                assert_eq!(
                    (*stdout_ack, *stderr_ack),
                    if sequence == 1 { (0, 0) } else { (1, 1) }
                );
                let mut body =
                    snapshot_body((sequence > 1).then_some(Terminal::Exited { code: 0 }));
                if let ResponseBody::Exchange {
                    stdout,
                    stderr,
                    stdout_eof,
                    stderr_eof,
                    ..
                } = &mut body
                {
                    *stdout_eof = Some(1);
                    *stderr_eof = Some(1);
                    if sequence == 1 {
                        *stdout = Some(codec::encode(0, b"o", Codec::None).unwrap());
                        *stderr = Some(codec::encode(0, b"e", Codec::None).unwrap());
                    }
                }
                exchange_reply(&mut socket, &request, &encoded, body).await;
            }
        });
        let mut session = session(path);
        let first = session
            .exchange(None, false, OutputAcks::default())
            .await
            .unwrap();
        assert_eq!((first.stdout_eof, first.stderr_eof), (Some(1), Some(1)));
        assert!(first.terminal.is_none());
        let acks = OutputAcks {
            stdout: 1,
            stderr: 1,
        };
        assert_eq!(
            session.exchange(None, false, acks).await.unwrap().terminal,
            Some(Terminal::Exited { code: 0 })
        );
        assert_eq!(
            session.exchange(None, false, acks).await.unwrap().terminal,
            Some(Terminal::Exited { code: 0 })
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn pre_helper_denial_remains_failed_denied_and_never_successful_exit() {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = directory.path().join("denied");
        let listener = ipc::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (encoded, request) = read(&mut socket).await;
            exchange_reply(
                &mut socket,
                &request,
                &encoded,
                snapshot_body(Some(Terminal::Failed {
                    code: wire::WireErrorCode::Denied,
                })),
            )
            .await;
        });
        let mut session = session(path);
        let result = session
            .exchange(None, false, OutputAcks::default())
            .await
            .unwrap();
        assert_eq!((result.stdout_eof, result.stderr_eof), (Some(0), Some(0)));
        assert_eq!(
            result.terminal,
            Some(Terminal::Failed {
                code: wire::WireErrorCode::Denied
            })
        );
        assert!(session.input_closed());
        assert!(!session.can_send(1));
        server.await.unwrap();
    }
}
