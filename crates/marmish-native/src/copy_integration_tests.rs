//! Hermetic production-client/hidden-helper qualification over local framed IPC.
use crate::{
    command_cli::Context,
    copy_client,
    copy_io::CopyIo,
    ipc,
    stream_service::{StreamLocalRequest, StreamLocalResult},
    stream_session::StreamSession,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use marmish_stream::{
    MAX_BLOCK,
    codec::{self, Codec},
    copy_protocol::{self as cp, AttemptMode, Direction, HelperContext, Launch},
    windows::SendWindow,
    wire::{self, Binding, RequestBody, ResponseBody, Terminal},
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, process::ExitStatusExt},
    },
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    sync::{mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};

// Libtest writes status to stdout before a test starts. Protocol output uses an
// independent inherited pipe, so no harness bytes enter the production decoder.
#[test]
#[ignore = "owned helper subprocess entrypoint"]
fn helper_child() {
    fn inherited(name: &str) -> File {
        let fd: i32 = std::env::var(name).unwrap().parse().unwrap();
        assert!(fd > 3);
        unsafe { File::from_raw_fd(fd) }
    }
    let context = inherited("MARMISH_TEST_COPY_CONTEXT_FD");
    let output = inherited("MARMISH_TEST_COPY_OUTPUT_FD");
    let input = unsafe { File::from_raw_fd(libc::dup(0)) };
    std::process::exit(crate::copy_helper::run(context, input, output) as i32);
}

fn relocate(file: File) -> File {
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 4) };
    assert!(fd >= 4);
    unsafe { File::from_raw_fd(fd) }
}
fn context(launch: &Launch, codec: Codec, stream: [u8; 16]) -> HelperContext {
    let meta = crate::native_manifest::compiled_release_metadata();
    let mut commit = [0; 20];
    for (index, byte) in commit.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&meta.commit[index * 2..index * 2 + 2], 16).unwrap();
    }
    HelperContext {
        context_version: 1,
        helper_protocol: 1,
        client: launch.client,
        server: launch.server,
        group: vec![8; 32],
        session: [7; 16],
        stream,
        transfer: launch.transfer,
        direction: launch.direction,
        launch_digest: launch.digest(codec).unwrap(),
        retention_seconds: cp::MIN_RETENTION_SECS as u64,
        expected_version: meta.version.into(),
        expected_target: meta.target.into(),
        expected_commit: commit,
    }
}
async fn read_pipe<R: AsyncRead + Unpin>(mut pipe: R, send: mpsc::Sender<Option<Vec<u8>>>) {
    loop {
        let mut bytes = vec![0; MAX_BLOCK];
        let n = pipe.read(&mut bytes).await.unwrap();
        bytes.truncate(n);
        if send.send((n != 0).then_some(bytes)).await.is_err() || n == 0 {
            break;
        }
    }
}
fn spawn_helper(
    launch: &Launch,
    codec: Codec,
    stream: [u8; 16],
) -> (tokio::process::Child, tokio::fs::File) {
    let (reader, mut writer) = std::io::pipe().unwrap();
    writer
        .write_all(&context(launch, codec, stream).encode().unwrap())
        .unwrap();
    drop(writer);
    let context = relocate(File::from(OwnedFd::from(reader)));
    let (output, output_writer) = std::io::pipe().unwrap();
    let output_writer = relocate(File::from(OwnedFd::from(output_writer)));
    let context_fd = context.as_raw_fd();
    let output_fd = output_writer.as_raw_fd();
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "copy_integration_tests::helper_child",
            "--ignored",
            "--nocapture",
        ])
        .env("MARMISH_TEST_COPY_CONTEXT_FD", context_fd.to_string())
        .env("MARMISH_TEST_COPY_OUTPUT_FD", output_fd.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    unsafe {
        command.pre_exec(move || {
            for fd in [context_fd, output_fd] {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let child = command.spawn().unwrap();
    drop(context);
    drop(output_writer);
    (
        child,
        tokio::fs::File::from_std(File::from(OwnedFd::from(output))),
    )
}

#[derive(Clone, Default)]
struct Flow {
    data: Vec<(u64, usize)>,
    resumes: Vec<u64>,
    checkpoints: usize,
    done: bool,
    committed: bool,
}
impl Flow {
    fn bytes(&self) -> u64 {
        self.data.iter().map(|(_, n)| *n as u64).sum()
    }
    fn observe(&mut self, decoder: &mut cp::FrameDecoder, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let (n, frame) = decoder.feed(bytes).unwrap();
            assert!(n > 0);
            bytes = &bytes[n..];
            match frame {
                Some(cp::Frame::Data { offset, bytes }) => {
                    assert!(self.data.len() < 1024);
                    self.data.push((offset, bytes.len()));
                }
                Some(cp::Frame::Resume { offset, .. }) => {
                    assert!(self.resumes.len() < 8);
                    self.resumes.push(offset);
                }
                Some(cp::Frame::Checkpoint { .. }) => self.checkpoints += 1,
                Some(cp::Frame::Done { .. }) => self.done = true,
                Some(cp::Frame::Commit { .. }) => self.committed = true,
                _ => {}
            }
        }
    }
}
#[derive(Clone, Default)]
struct Metrics {
    input: Flow,
    output: Flow,
}
fn ingest(
    window: &mut SendWindow,
    receive: &mut mpsc::Receiver<Option<Vec<u8>>>,
    mut observe: Option<(&mut cp::FrameDecoder, &mut Flow)>,
) {
    while window.credit() >= MAX_BLOCK && window.eof().is_none() {
        match receive.try_recv() {
            Ok(Some(bytes)) => {
                if let Some((decoder, flow)) = &mut observe {
                    flow.observe(decoder, &bytes);
                }
                window.push(&bytes).unwrap();
            }
            Ok(None) => {
                window.finish(window.produced()).unwrap();
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => panic!("missing pipe EOF"),
        }
    }
    assert!(window.buffered() <= marmish_stream::WINDOW_BYTES);
}
fn block(window: &SendWindow, codec: Codec) -> Option<codec::EncodedBlock> {
    let bytes = window.read(window.acknowledged(), MAX_BLOCK).unwrap();
    (!bytes.is_empty()).then(|| codec::encode(window.acknowledged(), &bytes, codec).unwrap())
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    Checkpoint,
    Commit,
}
struct Bridge {
    metrics: Arc<Mutex<Metrics>>,
    task: JoinHandle<bool>,
    stop: Option<oneshot::Sender<()>>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Bridge {
    fn metrics(&self) -> Metrics {
        self.metrics.lock().unwrap().clone()
    }
    async fn finish(mut self) -> bool {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(10), &mut self.task)
            .await
            .unwrap()
            .unwrap()
    }
    async fn start(socket: &Path, launch: &Launch, codec: Codec, stream: [u8; 16]) -> Self {
        Self::start_fault(socket, launch, codec, stream, Fault::None).await
    }
    async fn start_fault(
        socket: &Path,
        launch: &Launch,
        codec: Codec,
        stream: [u8; 16],
        fault: Fault,
    ) -> Self {
        let listener = UnixListener::bind(socket).unwrap();
        let metrics = Arc::new(Mutex::new(Metrics::default()));
        let observed = metrics.clone();
        // Deliberately delay reverse checkpoints for fresh Put until the whole
        // durable window is occupied. This exercises the production residual
        // ReadBounded path instead of reproducing its credit logic in a test.
        let delay_checkpoints = launch.direction == Direction::Put
            && launch.mode == AttemptMode::Create
            && fault == Fault::None;
        let (mut child, output) = spawn_helper(launch, codec, stream);
        let mut input = child.stdin.take();
        let stderr = child.stderr.take().unwrap();
        let (out_send, mut out_receive) = mpsc::channel(1);
        let (err_send, mut err_receive) = mpsc::channel(1);
        let mut readers = JoinSet::new();
        readers.spawn(read_pipe(output, out_send));
        readers.spawn(read_pipe(stderr, err_send));
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut stdout = SendWindow::default();
            let mut stderr = SendWindow::default();
            let mut accepted = 0;
            let mut input_decoder = cp::FrameDecoder::default();
            let mut output_decoder = cp::FrameDecoder::default();
            let mut terminal = None;
            let mut disconnected = false;
            let mut cache: Option<([u8; 32], Vec<u8>)> = None;
            loop {
                let connection = tokio::select! {
                    _=&mut stopped => break,
                    result=listener.accept()=>result.unwrap().0,
                };
                let mut connection = connection;
                let Ok(n) = connection.read_u32().await else {
                    continue;
                };
                let n = n as usize;
                assert!(n <= 65536);
                let mut bytes = vec![0; n];
                if connection.read_exact(&mut bytes).await.is_err() {
                    continue;
                }
                let ipc::Request::Stream {
                    request: StreamLocalRequest::Frame { frame_base64, .. },
                } = serde_json::from_slice(&bytes).unwrap()
                else {
                    panic!("unexpected local request")
                };
                let raw = STANDARD.decode(frame_base64).unwrap();
                if disconnected {
                    continue;
                }
                let request = wire::decode_request(&raw).unwrap();
                let digest = wire::request_digest(&raw);
                let response = if let Some((old_digest, encoded)) = &cache
                    && *old_digest == digest
                {
                    encoded.clone()
                } else {
                    let RequestBody::Exchange {
                        stdin,
                        stdin_eof,
                        stdout_ack,
                        stderr_ack,
                    } = request.body
                    else {
                        panic!("expected Exchange")
                    };
                    stdout.acknowledge(stdout_ack).unwrap();
                    stderr.acknowledge(stderr_ack).unwrap();
                    if let Some(block) = stdin {
                        assert_eq!(block.offset, accepted);
                        let bytes = codec::decode(&block, codec).unwrap();
                        observed
                            .lock()
                            .unwrap()
                            .input
                            .observe(&mut input_decoder, &bytes);
                        input.as_mut().unwrap().write_all(&bytes).await.unwrap();
                        accepted += bytes.len() as u64;
                    }
                    if let Some(end) = stdin_eof {
                        assert_eq!(end, accepted);
                        input.take();
                    }
                    // Give the bounded helper pipe pump a scheduling opportunity.
                    tokio::task::yield_now().await;
                    {
                        let mut probe = observed.lock().unwrap();
                        ingest(
                            &mut stdout,
                            &mut out_receive,
                            Some((&mut output_decoder, &mut probe.output)),
                        );
                    }
                    ingest(&mut stderr, &mut err_receive, None);
                    let disconnect = {
                        let probe = observed.lock().unwrap();
                        (fault == Fault::Checkpoint && probe.output.checkpoints > 0)
                            || (fault == Fault::Commit && probe.output.committed)
                    };
                    if disconnect {
                        disconnected = true;
                        // Lose the real IPC reply after observing durable helper
                        // output. The caller sees transport failure, not Error.
                        continue;
                    }
                    let hold_checkpoint = {
                        let probe = observed.lock().unwrap();
                        delay_checkpoints
                            && probe.output.checkpoints > 0
                            && !probe.input.done
                            && probe.input.bytes() < cp::DURABLE_WINDOW
                    };
                    if terminal.is_none()
                        && let Some(status) = child.try_wait().unwrap()
                    {
                        terminal = Some(if let Some(code) = status.code() {
                            Terminal::Exited {
                                code: code.try_into().unwrap(),
                            }
                        } else {
                            Terminal::Signaled {
                                signal: status.signal().unwrap().try_into().unwrap(),
                            }
                        });
                    }
                    let body = ResponseBody::Exchange {
                        stdin_accepted: accepted,
                        stdin_delivered: accepted,
                        stdin_closed: input.is_none() || terminal.is_some(),
                        stdout: if hold_checkpoint {
                            None
                        } else {
                            block(&stdout, codec)
                        },
                        stderr: block(&stderr, codec),
                        stdout_eof: stdout.eof(),
                        stderr_eof: stderr.eof(),
                        cancel_requested: false,
                        terminal: terminal
                            .filter(|_| stdout.eof().is_some() && stderr.eof().is_some()),
                    };
                    let encoded = wire::encode_response(&wire::Response {
                        binding: request.binding,
                        sequence: request.sequence,
                        request_sha256: digest,
                        body,
                    })
                    .unwrap();
                    cache = Some((digest, encoded.clone()));
                    encoded
                };
                let response = ipc::Response {
                    ok: true,
                    result: None,
                    error: None,
                    command: None,
                    stream: Some(StreamLocalResult::Frame {
                        frame_base64: STANDARD.encode(response),
                    }),
                };
                let bytes = serde_json::to_vec(&response).unwrap();
                // Deadline cancellation intentionally drops an outstanding IPC
                // socket; its exact response remains cached for retry.
                if connection.write_u32(bytes.len() as u32).await.is_ok() {
                    let _ = connection.write_all(&bytes).await;
                }
            }
            input.take();
            let forced = tokio::time::timeout(Duration::from_secs(3), child.wait())
                .await
                .is_err();
            if forced {
                child.kill().await.unwrap();
                child.wait().await.unwrap();
            }
            readers.shutdown().await;
            !forced
        });
        Self {
            metrics,
            task,
            stop: Some(stop),
        }
    }
}
fn launch(source: &Path, destination: &Path, direction: Direction) -> Launch {
    Launch {
        version: cp::VERSION,
        mode: AttemptMode::Create,
        transfer: [5; 32],
        client: [1; 32],
        server: [2; 32],
        direction,
        source: source.as_os_str().as_bytes().to_vec(),
        destination: destination.as_os_str().as_bytes().to_vec(),
        overwrite: false,
        preserve_mode: true,
        preserve_mtime: true,
    }
}
fn io(socket: &Path, launch: &Launch, codec: Codec, stream: [u8; 16]) -> CopyIo {
    let binding = Binding {
        stream,
        session: [7; 16],
        client: launch.client,
        server: launch.server,
    };
    CopyIo::new(
        StreamSession::new(
            Arc::new(Context {
                socket: socket.into(),
                profile: "test".into(),
            }),
            vec![],
            binding,
            Duration::from_secs(20),
            codec,
        ),
        codec,
    )
}
fn pattern(length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| ((index * 73 + index / 251) % 256) as u8)
        .collect()
}
fn set_metadata(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640)).unwrap();
    File::open(path)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789)),
        )
        .unwrap();
}
fn assert_metadata(source: &Path, destination: &Path) {
    use std::os::unix::fs::MetadataExt;
    let source = std::fs::metadata(source).unwrap();
    let destination = std::fs::metadata(destination).unwrap();
    assert_eq!(destination.mode() & 0o777, source.mode() & 0o777);
    assert_eq!(
        (destination.mtime(), destination.mtime_nsec()),
        (source.mtime(), source.mtime_nsec())
    );
}
async fn attempt_observed(
    socket: &Path,
    launch: Launch,
    codec: Codec,
    stream: [u8; 16],
) -> (crate::Result<()>, Metrics) {
    let bridge = Bridge::start(socket, &launch, codec, stream).await;
    let mut io = io(socket, &launch, codec, stream);
    let result = tokio::time::timeout(
        Duration::from_secs(120),
        copy_client::run(
            &mut io,
            launch,
            stream,
            cp::MIN_RETENTION_SECS as u64,
            Arc::new(AtomicBool::new(false)),
        ),
    )
    .await
    .expect("bounded copy qualification");
    let metrics = bridge.metrics();
    let exited = bridge.finish().await;
    if result.is_ok() || matches!(result, Err(crate::Error::Copy(_))) {
        assert!(exited, "deterministic protocol result orphaned helper");
    }
    (result, metrics)
}
async fn attempt(
    socket: &Path,
    launch: Launch,
    codec: Codec,
    stream: [u8; 16],
) -> crate::Result<()> {
    attempt_observed(socket, launch, codec, stream).await.0
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn raw_and_zlib_put_get_preserve_large_binary_special_paths_and_metadata() {
    for codec in [Codec::None, Codec::Zlib] {
        for direction in [Direction::Put, Direction::Get] {
            let dir = tempfile::tempdir().unwrap();
            let dir = dir.path().canonicalize().unwrap();
            let source = dir.join("-source ' \" $ ; \\ \n.bin");
            let destination = dir.join("-destination ' \" $ ; \\ \n.bin");
            let bytes = pattern(cp::DURABLE_WINDOW as usize + 17003);
            std::fs::write(&source, &bytes).unwrap();
            set_metadata(&source);
            let (result, metrics) = attempt_observed(
                &dir.join("ipc"),
                launch(&source, &destination, direction),
                codec,
                [3; 16],
            )
            .await;
            result.unwrap();
            let flow = if direction == Direction::Put {
                &metrics.input
            } else {
                &metrics.output
            };
            assert_eq!(flow.bytes(), bytes.len() as u64);
            assert_eq!(flow.data.first().unwrap().0, 0);
            assert!(
                flow.data
                    .iter()
                    .all(|(_, length)| *length <= (MAX_BLOCK - 13))
            );
            if direction == Direction::Put {
                assert!(
                    flow.data
                        .iter()
                        .any(
                            |(offset, length)| offset + *length as u64 == cp::DURABLE_WINDOW
                                && *length < (MAX_BLOCK - 13)
                        ),
                    "production sender must consume residual durable credit"
                );
            }
            let copied = std::fs::read(&destination).unwrap();
            assert_eq!(Sha256::digest(&copied), Sha256::digest(&bytes));
            assert_eq!(copied, bytes);
            assert_metadata(&source, &destination);
        }
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_files_and_committed_resume_are_exact_and_existing_destination_is_refused() {
    for direction in [Direction::Put, Direction::Get] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let source = dir.join("empty");
        let destination = dir.join("received");
        std::fs::write(&source, []).unwrap();
        set_metadata(&source);
        let original = launch(&source, &destination, direction);
        attempt(&dir.join("first"), original.clone(), Codec::Zlib, [3; 16])
            .await
            .unwrap();
        assert_eq!(std::fs::metadata(&destination).unwrap().len(), 0);
        assert_metadata(&source, &destination);
        let mut resume = original.clone();
        resume.mode = AttemptMode::Resume;
        attempt(&dir.join("resume"), resume, Codec::Zlib, [4; 16])
            .await
            .unwrap();
        let mut conflict = original;
        conflict.transfer = [9; 32];
        let refused = attempt(&dir.join("refuse"), conflict, Codec::None, [6; 16]).await;
        assert!(
            matches!(refused, Err(crate::Error::Copy(cp::CopyError::Conflict))),
            "unexpected refusal: {refused:?}"
        );
        assert_eq!(std::fs::metadata(&destination).unwrap().len(), 0);
        assert_metadata(&source, &destination);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn durable_partial_resume_checks_prefix_and_rejects_changed_source() {
    use crate::copy_files::{CopyReceiver, trusted_base};
    for direction in [Direction::Put, Direction::Get] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let source = dir.join("source");
        let destination = dir.join("destination");
        let bytes = pattern(cp::DURABLE_WINDOW as usize + 123);
        std::fs::write(&source, &bytes).unwrap();
        set_metadata(&source);
        let original = launch(&source, &destination, direction);
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(&source).unwrap();
        let manifest = cp::Manifest {
            version: original.version,
            transfer: original.transfer,
            client: original.client,
            server: original.server,
            direction,
            source: original.source.clone(),
            destination: original.destination.clone(),
            overwrite: false,
            preserve_mode: true,
            preserve_mtime: true,
            size: bytes.len() as u64,
            sha256: Sha256::digest(&bytes).into(),
            mode: Some(metadata.mode() & 0o777),
            mtime: Some(cp::Mtime {
                seconds: metadata.mtime(),
                nanos: metadata.mtime_nsec().try_into().unwrap(),
            }),
        };
        let base = trusted_base(Path::new(".")).unwrap();
        let mut receiver = CopyReceiver::open_or_recover(
            &base,
            &original.destination,
            manifest.clone(),
            AttemptMode::Create,
            [3; 16],
            crate::copy_worker::now().unwrap(),
            cp::MIN_RETENTION_SECS,
        )
        .ok()
        .unwrap();
        let prefix_len = cp::CHECKPOINT_INTERVAL as usize + 117;
        for (index, chunk) in bytes[..prefix_len].chunks(MAX_BLOCK).enumerate() {
            receiver
                .receive_data((index * MAX_BLOCK) as u64, chunk)
                .ok()
                .unwrap();
        }
        let prefix = receiver
            .checkpoint(crate::copy_worker::now().unwrap())
            .ok()
            .unwrap();
        assert_eq!(prefix.offset, prefix_len as u64);
        drop(receiver);
        assert!(!destination.exists());
        let mut resume = original.clone();
        resume.mode = AttemptMode::Resume;
        // A different source manifest cannot adopt the retained transfer.
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        std::fs::write(&source, &changed).unwrap();
        let changed_result =
            attempt(&dir.join("changed"), resume.clone(), Codec::None, [4; 16]).await;
        assert!(
            matches!(
                changed_result,
                Err(crate::Error::Copy(cp::CopyError::Conflict))
            ),
            "unexpected changed-source result: {changed_result:?}"
        );
        assert!(!destination.exists());
        std::fs::write(&source, &bytes).unwrap();
        set_metadata(&source);
        // Keep the immutable manifest identical, but corrupt retained bytes:
        // this must exercise prefix hashing rather than manifest comparison.
        use std::os::unix::fs::FileExt;
        let retained = std::fs::OpenOptions::new()
            .write(true)
            .open(dir.join(manifest.namespace()).join("data"))
            .unwrap();
        retained.write_at(&[bytes[0] ^ 1], 0).unwrap();
        retained.sync_all().unwrap();
        let corrupt = attempt(&dir.join("corrupt"), resume.clone(), Codec::None, [6; 16]).await;
        assert!(
            matches!(
                corrupt,
                Err(crate::Error::Copy(cp::CopyError::HashMismatch))
            ),
            "unexpected corrupted-prefix result: {corrupt:?}"
        );
        assert!(!destination.exists());
        retained.write_at(&bytes[..1], 0).unwrap();
        retained.sync_all().unwrap();
        drop(retained);
        let (result, metrics) =
            attempt_observed(&dir.join("resume"), resume, Codec::Zlib, [5; 16]).await;
        result.unwrap();
        let (data, replies) = if direction == Direction::Put {
            (&metrics.input, &metrics.output)
        } else {
            (&metrics.output, &metrics.input)
        };
        assert_eq!(data.data.first().unwrap().0, prefix_len as u64);
        assert_eq!(data.bytes(), (bytes.len() - prefix_len) as u64);
        assert!(data.bytes() < bytes.len() as u64);
        assert!(replies.resumes.contains(&(prefix_len as u64)));
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        assert_metadata(&source, &destination);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pre_cancelled_get_before_commit_retains_prepared_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let source = dir.join("source");
    let destination = dir.join("destination");
    std::fs::write(&source, pattern(65537)).unwrap();
    let launch = launch(&source, &destination, Direction::Get);
    let socket = dir.join("ipc");
    let stream = [3; 16];
    let bridge = Bridge::start(&socket, &launch, Codec::Zlib, stream).await;
    let mut io = io(&socket, &launch, Codec::Zlib, stream);
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        copy_client::run(
            &mut io,
            launch.clone(),
            stream,
            cp::MIN_RETENTION_SECS as u64,
            Arc::new(AtomicBool::new(true)),
        ),
    )
    .await
    .unwrap();
    assert!(
        matches!(result, Err(crate::Error::Invalid("copy_cancelled"))),
        "{result:?}"
    );
    // This fixture has no administrative Cancel endpoint. Explicit protocol
    // failure cleanly ends the sender after proving the pre-commit local latch.
    io.abort_failure(cp::CopyError::Protocol).await.unwrap();
    assert!(
        bridge.finish().await,
        "cleanup must not force-kill the helper"
    );
    assert!(!destination.exists());
    let checkpoint = cp::Checkpoint::decode(
        &std::fs::read(dir.join(launch.namespace()).join("checkpoint")).unwrap(),
    )
    .unwrap();
    assert_eq!(checkpoint.phase, cp::Phase::CommitPrepared);
    assert_eq!(checkpoint.manifest.size, 65537);
    assert!(dir.join(launch.namespace()).join("data").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn local_and_exited_helper_missing_sources_preserve_not_found_and_end_helper() {
    for direction in [Direction::Put, Direction::Get] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let source = dir.join("missing");
        let destination = dir.join("destination");
        assert!(matches!(
            attempt(
                &dir.join("ipc"),
                launch(&source, &destination, direction),
                Codec::None,
                [3; 16]
            )
            .await,
            Err(crate::Error::Copy(cp::CopyError::NotFound))
        ));
        assert!(!destination.exists());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disconnected_put_resumes_checkpoint_and_lost_commit_reply_recovers_receipt() {
    for fault in [Fault::Checkpoint, Fault::Commit] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let source = dir.join("source");
        let destination = dir.join("destination");
        let bytes = pattern(cp::CHECKPOINT_INTERVAL as usize + 65537);
        std::fs::write(&source, &bytes).unwrap();
        set_metadata(&source);
        let mut launch = launch(&source, &destination, Direction::Put);
        let socket = dir.join("lost");
        let bridge = Bridge::start_fault(&socket, &launch, Codec::Zlib, [3; 16], fault).await;
        let mut io = io(&socket, &launch, Codec::Zlib, [3; 16]);
        let failure = tokio::time::timeout(
            Duration::from_secs(40),
            copy_client::run(
                &mut io,
                launch.clone(),
                [3; 16],
                cp::MIN_RETENTION_SECS as u64,
                Arc::new(AtomicBool::new(false)),
            ),
        )
        .await
        .expect("lost IPC reply must terminate within bounded timeout");
        if fault == Fault::Commit {
            assert!(
                matches!(
                    failure,
                    Err(crate::Error::Copy(cp::CopyError::CommitUncertain))
                ),
                "{failure:?}"
            );
        } else {
            assert!(failure.is_err(), "fault must interrupt real exchange");
        }
        assert!(
            bridge.finish().await,
            "disconnect cleanup must not force-kill helper"
        );
        let checkpoint = cp::Checkpoint::decode(
            &std::fs::read(dir.join(launch.namespace()).join("checkpoint")).unwrap(),
        )
        .unwrap();
        let durable = checkpoint.durable_offset;
        assert!(durable > 0);
        if fault == Fault::Commit {
            assert_eq!(checkpoint.phase, cp::Phase::Committed);
            assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        } else {
            assert!(!destination.exists());
        }
        launch.mode = AttemptMode::Resume;
        let (result, metrics) =
            attempt_observed(&dir.join("resume"), launch, Codec::Zlib, [4; 16]).await;
        result.unwrap();
        assert_eq!(metrics.input.bytes(), bytes.len() as u64 - durable);
        if fault == Fault::Commit {
            assert!(metrics.input.data.is_empty());
        } else {
            assert_eq!(metrics.input.data.first().unwrap().0, durable);
        }
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        assert_metadata(&source, &destination);
    }
}

// Read one small fixture frame without consuming bytes belonging to its successor.
// The timeout also bounds a helper which produces Progress indefinitely.
async fn helper_frame(
    output: &mut tokio::fs::File,
    decoder: &mut cp::FrameDecoder,
) -> Option<cp::Frame> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let mut byte = [0];
            if output.read(&mut byte).await.unwrap() == 0 {
                decoder.finish().unwrap();
                return None;
            }
            let (used, frame) = decoder.feed(&byte).unwrap();
            assert_eq!(used, 1);
            match frame {
                Some(cp::Frame::Progress { .. }) | None => (),
                Some(frame) => return Some(frame),
            }
        }
    })
    .await
    .expect("bounded real-helper frame")
}
async fn helper_send(input: &mut tokio::process::ChildStdin, frame: cp::Frame) {
    tokio::time::timeout(
        Duration::from_secs(5),
        input.write_all(&frame.encode().unwrap()),
    )
    .await
    .expect("bounded real-helper input")
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn malformed_finish_after_durable_receipt_is_uncertain_and_resume_needs_no_data() {
    for fault in ["precommit", "malformed_finish", "eof", "peer_error"] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let source = dir.join("source");
        let destination = dir.join("received");
        let bytes = b"a durable nonempty committed receipt";
        std::fs::write(&source, bytes).unwrap();
        let mut original = launch(&source, &destination, Direction::Put);
        original.preserve_mode = false;
        original.preserve_mtime = false;
        let hash: [u8; 32] = Sha256::digest(bytes).into();
        let empty_hash: [u8; 32] = Sha256::digest([]).into();
        let size = bytes.len() as u64;
        let manifest = cp::Manifest {
            version: original.version,
            transfer: original.transfer,
            client: original.client,
            server: original.server,
            direction: Direction::Put,
            source: original.source.clone(),
            destination: original.destination.clone(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
            size,
            sha256: hash,
            mode: None,
            mtime: None,
        };
        let (mut child, mut output) = spawn_helper(&original, Codec::None, [3; 16]);
        let mut input = child.stdin.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut decoder = cp::FrameDecoder::default();
        helper_send(&mut input, cp::Frame::Start(original.clone(), Codec::None)).await;
        helper_send(&mut input, cp::Frame::Manifest(manifest)).await;
        assert!(matches!(helper_frame(&mut output, &mut decoder).await,
            Some(cp::Frame::Resume { phase: cp::Phase::Created, offset: 0, sha256 }) if sha256 == empty_hash));
        let postcommit = fault != "precommit";
        if postcommit {
            helper_send(
                &mut input,
                cp::Frame::Prefix {
                    offset: 0,
                    sha256: empty_hash,
                },
            )
            .await;
            helper_send(
                &mut input,
                cp::Frame::Data {
                    offset: 0,
                    bytes: bytes.to_vec(),
                },
            )
            .await;
            helper_send(&mut input, cp::Frame::Done { size, sha256: hash }).await;
            assert!(matches!(helper_frame(&mut output, &mut decoder).await,
                Some(cp::Frame::Checkpoint { offset, sha256 }) if offset == size && sha256 == hash));
            assert!(matches!(helper_frame(&mut output, &mut decoder).await,
                Some(cp::Frame::Commit { transfer, size: committed_size, sha256 })
                    if transfer == original.transfer && committed_size == size && sha256 == hash));
            // Observe the actual durable destination before injecting a fault.
            assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        }
        if fault == "eof" {
            drop(input);
        } else {
            // Done is valid framing, but invalid as either Prefix or Finish.
            let frame = if fault == "peer_error" {
                cp::Frame::Error(cp::CopyError::LocalIo)
            } else {
                cp::Frame::Done {
                    size: 0,
                    sha256: hash,
                }
            };
            helper_send(&mut input, frame).await;
            if fault != "peer_error" {
                let expected = if postcommit {
                    cp::CopyError::CommitUncertain
                } else {
                    cp::CopyError::Protocol
                };
                assert!(matches!(helper_frame(&mut output, &mut decoder).await,
                    Some(cp::Frame::Error(code)) if code == expected));
            }
            drop(input);
        }
        if fault == "eof" {
            assert!(matches!(
                helper_frame(&mut output, &mut decoder).await,
                Some(cp::Frame::Error(cp::CopyError::CommitUncertain))
            ));
        }
        assert!(
            helper_frame(&mut output, &mut decoder).await.is_none(),
            "{fault}: extra response"
        );
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.code(), Some(1), "{fault}");
        let mut diagnostic = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            stderr.take(4097).read_to_end(&mut diagnostic),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(diagnostic.len() <= 4096);
        let diagnostic = String::from_utf8(diagnostic).unwrap();
        if fault == "peer_error" {
            assert!(
                diagnostic.is_empty(),
                "explicit peer error must not be relabelled"
            );
        } else {
            assert!(
                diagnostic.contains(if postcommit {
                    "CommitUncertain"
                } else {
                    "Protocol"
                }),
                "{fault}: {diagnostic}"
            );
        }
        let checkpoint = cp::Checkpoint::decode(
            &std::fs::read(dir.join(original.namespace()).join("checkpoint")).unwrap(),
        )
        .unwrap();
        if postcommit {
            assert_eq!(checkpoint.phase, cp::Phase::Committed);
            assert_eq!(checkpoint.manifest.sha256, hash);
            original.mode = AttemptMode::Resume;
            let (result, metrics) =
                attempt_observed(&dir.join("resume"), original, Codec::None, [4; 16]).await;
            result.unwrap();
            assert!(
                metrics.input.data.is_empty(),
                "receipt recovery retransmitted data"
            );
            assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        } else {
            assert_ne!(checkpoint.phase, cp::Phase::Committed);
            assert!(!destination.exists());
        }
    }
}
