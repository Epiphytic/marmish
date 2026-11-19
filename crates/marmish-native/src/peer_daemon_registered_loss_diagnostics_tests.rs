//! Fast failure-path checks for the registered dispatcher test controller.
use super::*;
use std::process::Stdio;
use tokio::{io::AsyncWriteExt, net::UnixListener};

#[tokio::test]
async fn controller_reports_only_known_frame_failures() {
    for (input, expected) in [
        (&b""[..], FrameFailure::Header(io::ErrorKind::UnexpectedEof)),
        (
            b"S\0\0\0\x01",
            FrameFailure::Payload(io::ErrorKind::UnexpectedEof),
        ),
        (b"S\0\0\0\xff", FrameFailure::Bound),
        (b"W\0\0\0\0", FrameFailure::Tag),
        (
            b"X\0\0\0\x0cfinish_basis",
            FrameFailure::Refused(Some(Refusal::FinishBasis)),
        ),
        (b"X\0\0\0\x01!", FrameFailure::Refused(None)),
    ] {
        let (mut controller, mut sender) = AsyncUnixStream::pair().unwrap();
        sender.write_all(input).await.unwrap();
        drop(sender);
        assert_eq!(
            frame(&mut controller, b'S', 32, Instant::now() + STEP).await,
            Err(expected)
        );
    }
    let (mut controller, _sender) = AsyncUnixStream::pair().unwrap();
    assert_eq!(
        frame(
            &mut controller,
            b'P',
            32,
            Instant::now() + Duration::from_millis(5)
        )
        .await,
        Err(FrameFailure::Deadline)
    );
}

async fn fixture_respond(listener: &UnixListener, response: Option<ipc::Response>) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    stream.read_to_end(&mut request).await.unwrap();
    assert!(request.len() > 4, "real framed Status request");
    if let Some(response) = response {
        let body = serde_json::to_vec(&response).unwrap();
        stream
            .write_u32(body.len().try_into().unwrap())
            .await
            .unwrap();
        stream.write_all(&body).await.unwrap();
    }
}

fn status(state: &str) -> ipc::Response {
    ipc::Response::success(serde_json::json!({"state": state}))
}

#[tokio::test]
async fn status_retries_before_send_or_after_activating_only() {
    let private = tempfile::tempdir().unwrap();
    let path = private.path().join("ipc");
    let end = Instant::now() + STEP;
    let (result, fixture) = tokio::join!(
        wait_active(&path, end),
        timeout_at(end, async {
            sleep(Duration::from_millis(35)).await;
            let old = UnixListener::bind(&path).unwrap();
            drop(old); // A socket node with no listener exercises ConnectionRefused.
            sleep(Duration::from_millis(35)).await;
            fs::remove_file(&path).unwrap();
            let real = UnixListener::bind(&path).unwrap();
            fixture_respond(&real, Some(status("activating"))).await;
            fixture_respond(&real, Some(status("active"))).await;
        })
    );
    assert_eq!(result, Ok(()));
    fixture.expect("bounded paired Status fixture");

    fs::remove_file(&path).unwrap();
    let listener = UnixListener::bind(&path).unwrap();
    for (response, expected) in [
        (None, StatusSetupFailure::Reply),
        (
            Some(status("transport_degraded")),
            StatusSetupFailure::TransportDegraded,
        ),
        (
            Some(ipc::Response::failure("unavailable")),
            StatusSetupFailure::Unexpected,
        ),
    ] {
        let end = Instant::now() + STEP;
        let (result, fixture) = tokio::join!(
            wait_active(&path, end),
            timeout_at(end, fixture_respond(&listener, response))
        );
        assert_eq!(result, Err(expected));
        fixture.expect("bounded paired Status fixture");
    }

    let (mut rejected, _open) = AsyncUnixStream::pair().unwrap();
    rejected.shutdown().await.unwrap();
    assert_eq!(
        status_turn(&mut rejected).await,
        Err(StatusSetupFailure::Send)
    );
}

#[tokio::test]
async fn stale_status_socket_uses_a_non_consuming_kernel_snapshot() {
    let (mut controller, mut peer) = AsyncUnixStream::pair().unwrap();
    assert_eq!(status_socket_state(&controller), "pending");
    peer.write_all(b"x").await.unwrap();
    assert_eq!(status_socket_state(&controller), "bytes_available");
    let end = Instant::now() + STEP;
    assert_eq!(
        timeout_at(end, controller.read_u8())
            .await
            .unwrap()
            .unwrap(),
        b'x'
    );
    drop(peer);
    assert_eq!(status_socket_state(&controller), "closed");

    let (mut controller, peer) = AsyncUnixStream::pair().unwrap();
    controller.write_all(b"x").await.unwrap();
    drop(peer);
    assert_eq!(status_socket_state(&controller), "reset");
}

#[test]
fn status_with_non_status_result_fields_is_unexpected() {
    let mut command = status("active");
    command.command = Some(crate::command_local::LocalResult::LocalBusy);
    let mut stream = status("active");
    stream.stream = Some(crate::stream_service::StreamLocalResult::Pending);
    let mut terminal = status("active");
    terminal.terminal_stream = Some(crate::terminal_ipc_wire::Role::Observer);
    for response in [
        command,
        stream,
        terminal,
        status("unknown"),
        ipc::Response::success(serde_json::json!({})),
    ] {
        assert_eq!(status_state(&response), StatusState::Unexpected);
    }
}

fn grouped_shell(script: &str) -> (OwnedChild, tokio::process::ChildStdout) {
    use std::os::unix::process::CommandExt;

    let mut command = std::process::Command::new("/bin/sh");
    command
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: this private fixture calls only setsid before exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    let stdout = tokio::process::ChildStdout::from_std(child.stdout.take().unwrap()).unwrap();
    (OwnedChild(Some(child)), stdout)
}

#[tokio::test]
async fn post_eof_failure_classifies_and_reaps_the_exact_child() {
    for (script, expected) in [
        ("exit 1", ChildEnd::FatalExit),
        ("exit 86", ChildEnd::FixtureRefused),
        ("exit 101", ChildEnd::TestFailed),
        ("kill -TERM $$", ChildEnd::Signaled(libc::SIGTERM)),
    ] {
        let (mut child, _stdout) = grouped_shell(script);
        assert_eq!(child.failure_status(true).await, expected);
        assert!(child.0.is_none());
    }
}

#[tokio::test]
async fn failure_cleans_descendant_before_reaping_private_session_leader() {
    let (mut child, mut stdout) = grouped_shell("sleep 30 & exit 86");
    assert_eq!(child.failure_status(true).await, ChildEnd::FixtureRefused);
    assert!(child.0.is_none());
    let mut bytes = [0; 1];
    assert_eq!(
        timeout_at(Instant::now() + STEP, stdout.read(&mut bytes))
            .await
            .expect("private descendant released its pipe")
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn observation_failures_retain_the_exact_child_guard() {
    let (mut child, _stdout) = grouped_shell("exit 86");
    let pid = child.pid();
    assert_eq!(
        child
            .observe_failure(
                false,
                |_| Err(io::Error::from_raw_os_error(libc::ECHILD)),
                |_| panic!("waitid did not identify a child")
            )
            .await,
        ChildEnd::ObservationFailed,
    );
    assert_eq!(child.pid(), pid);
    assert_eq!(
        child
            .observe_failure(true, waitable, |observed| {
                assert_eq!(observed, pid);
                false
            })
            .await,
        ChildEnd::ObservationFailed,
    );
    assert_eq!(child.pid(), pid);
    assert_eq!(child.failure_status(true).await, ChildEnd::FixtureRefused);
    assert!(child.0.is_none());

    // A plain child inherits the existing session and is not its leader.
    let mut foreign = OwnedChild(Some(
        std::process::Command::new("/bin/sh")
            .args(["-c", "exit 86"])
            .spawn()
            .unwrap(),
    ));
    let pid = foreign.pid();
    assert_eq!(
        foreign.failure_status(true).await,
        ChildEnd::ObservationFailed
    );
    assert_eq!(foreign.pid(), pid);
    drop(foreign);
}

const REFUSAL_CHILD: &str = concat!(module_path!(), "::refusal_without_controller");

#[test]
#[ignore = "requires a launcher-owned private startup and control descriptor"]
fn refusal_without_controller() {
    let (_, control) = crate::daemon_launch::StartupReporter::claim_fault_fixture().unwrap();
    crate::diagnostics::enable();
    let _args = super::super::start_control(control).unwrap();
    std::thread::sleep(super::super::BOOT);
    super::super::refuse_at(Refusal::Boot);
}

#[tokio::test]
async fn refusal_log_survives_an_unwritable_controller() {
    let private = tempfile::tempdir().unwrap();
    let log = tempfile::tempfile_in(private.path()).unwrap();
    let (mut controller, controlled) = UnixStream::pair().unwrap();
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let selector = REFUSAL_CHILD.split_once("::").unwrap().1;
    let args = ["--ignored", "--exact", "--nocapture", selector].map(OsString::from);
    let (child, _report) = candidate_parent::spawn_fault_fixture(
        &executable,
        &args,
        log.try_clone().unwrap(),
        OwnedFd::from(controlled),
    )
    .unwrap();
    let mut child = OwnedChild(Some(child));
    controller.set_write_timeout(Some(STEP)).unwrap();
    send(
        &mut controller,
        b'B',
        &boot(Scenario::Status, [OsStr::new("fixture"); 4]),
    )
    .unwrap();
    drop(controller);
    assert_eq!(child.failure_status(true).await, ChildEnd::FixtureRefused);
    assert_eq!(
        known_child_diagnostics(&log),
        ("read", vec!["release_frame"])
    );
}

const DEGRADED_CHILD: &str = concat!(module_path!(), "::degraded_status_child");

#[tokio::test]
#[ignore = "requires a launcher-owned private startup and control descriptor"]
async fn degraded_status_child() {
    let (_, stream) = crate::daemon_launch::StartupReporter::claim_fault_fixture().unwrap();
    crate::diagnostics::enable();
    let _args = super::super::start_control(stream).unwrap();
    let control = super::super::CONTROL.get().unwrap();
    control
        .active_status
        .store(1, std::sync::atomic::Ordering::Release);
    let (_trip, fatal) = crate::dispatcher_fatal::Trip::new();
    super::super::before_status_write(
        &status("transport_degraded"),
        Some(&fatal),
        Instant::now() + STEP,
    )
    .await;
    panic!("second Status degraded without a categorized refusal");
}

#[tokio::test]
async fn ready_child_refuses_degraded_status_before_displacement() {
    let log = tempfile::tempfile().unwrap();
    let (mut controller, controlled) = UnixStream::pair().unwrap();
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let selector = DEGRADED_CHILD.split_once("::").unwrap().1;
    let args = ["--ignored", "--exact", "--nocapture", selector].map(OsString::from);
    let (child, _report) = candidate_parent::spawn_fault_fixture(
        &executable,
        &args,
        log.try_clone().unwrap(),
        OwnedFd::from(controlled),
    )
    .unwrap();
    let mut child = OwnedChild(Some(child));
    controller.set_write_timeout(Some(STEP)).unwrap();
    send(
        &mut controller,
        b'B',
        &boot(Scenario::Status, [OsStr::new("fixture"); 4]),
    )
    .unwrap();
    controller.set_nonblocking(true).unwrap();
    let mut controller = AsyncUnixStream::from_std(controller).unwrap();
    assert_eq!(
        frame(&mut controller, b'S', 32, Instant::now() + STEP).await,
        Err(FrameFailure::Refused(Some(
            Refusal::StatusDegradedBeforeCut
        ))),
    );
    assert_eq!(child.failure_status(true).await, ChildEnd::FixtureRefused);
    assert_eq!(
        known_child_diagnostics(&log),
        ("read", vec!["status_degraded_before_cut"])
    );
}
