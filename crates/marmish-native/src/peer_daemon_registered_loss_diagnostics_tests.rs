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
    peer.shutdown().await.unwrap();
    assert_eq!(status_socket_state(&controller), "closed");

    let (controller, peer) = UnixStream::pair().unwrap();
    controller.set_nonblocking(true).unwrap();
    let mut controller = AsyncUnixStream::from_std(controller).unwrap();
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
    let observation = observe_child_log(&log, ChildEnd::FixtureRefused);
    assert_eq!(observation.state, child_log::ReadState::Complete);
    assert_eq!(observation.known, ["release_frame"]);
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
    let observation = observe_child_log(&log, ChildEnd::FixtureRefused);
    assert_eq!(observation.state, child_log::ReadState::Complete);
    assert_eq!(observation.known, ["status_degraded_before_cut"]);
}

const ABORT_CHILD: &str = concat!(module_path!(), "::projected_abort_child");
const ABORT_MODE: &str = "MARMISH_TEST_PROJECTED_ABORT";
const SIGABRT: ChildEnd = ChildEnd::Signaled(libc::SIGABRT);

#[test]
#[ignore = "requires the isolated diagnostics controller and disabled core dumps"]
fn projected_abort_child() {
    let mode = std::env::var(ABORT_MODE).unwrap();
    let mut limit = libc::rlimit {
        rlim_cur: libc::RLIM_INFINITY,
        rlim_max: libc::RLIM_INFINITY,
    };
    // SAFETY: the local buffer is initialized and valid for this output.
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
    assert_eq!(limit.rlim_max, 0);
    crate::diagnostics::enable();
    use super::super::Edge;
    // A Ready marker would put SampleWritten before the first Status edge.
    Edge::SampleWritten.record(Scenario::Ready);
    for edge in Edge::ALL {
        edge.record(Scenario::Status);
    }
    let stderr = std::io::stderr();
    let mut err = stderr.lock();
    err.write_all(child_log::OWNED_FD_LINE).unwrap();
    err.write_all(b"\n").unwrap();
    drop(err);
    match mode.as_str() {
        "abort" => std::process::abort(),
        "exit" => std::process::exit(0),
        _ => panic!("unexpected diagnostic fixture mode"),
    }
}

#[tokio::test]
async fn runtime_line_needs_an_observed_sigabrt() {
    for (mode, end, expected) in [
        ("abort", SIGABRT, child_log::AbortClass::OwnedFdClosed),
        (
            "exit",
            ChildEnd::OtherExit,
            child_log::AbortClass::NotAborted,
        ),
    ] {
        use std::os::unix::process::CommandExt;

        let log = tempfile::tempfile().unwrap();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .env(ABORT_MODE, mode)
            .args([
                "--ignored",
                "--exact",
                "--nocapture",
                ABORT_CHILD.split_once("::").unwrap().1,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log.try_clone().unwrap()));
        // SAFETY: this fixture makes only libc calls before exec, with an
        // initialized input value. The child cannot produce a core file.
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_CORE, &limit) != 0 || libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = OwnedChild(Some(command.spawn().unwrap()));
        assert_eq!(
            timeout_at(Instant::now() + STEP * 3, child.failure_status(true))
                .await
                .unwrap(),
            end
        );
        assert!(child.0.is_none(), "exact private child reaped");
        let observation = observe_child_log(&log, end);
        assert!(observation.context);
        assert_eq!(observation.edges, super::super::Edge::ALL);
        assert!(!observation.out_of_order);
        assert_eq!(observation.abort, expected);
        assert!(!observation.unclassified_tail);
    }
}

fn marker(edge: super::super::Edge) -> String {
    format!(
        "{}\n",
        serde_json::json!({
            "schema": 1,
            "event": "diagnostic",
            "stage": "registered_fault_edge",
            "category": edge.code(),
        })
    )
}

#[test]
fn log_projection_reports_first_edges_in_order_and_suppresses_private_bytes() {
    use super::super::Edge;
    use child_log::{AbortClass, ReadState, project};
    let mut bytes = b"private preamble\n{\"schema\":2,\"event\":\"context\"}\n".to_vec();
    let before = project(Ok(&bytes), SIGABRT);
    assert!(!before.context && !before.unclassified_tail);
    bytes.extend_from_slice(b"{\"schema\":1,\"event\":\"context\"}\r\n");
    bytes.extend_from_slice(b"private reset at next edge\n");
    bytes.extend_from_slice(marker(Edge::RegisterStarted).as_bytes());
    bytes.extend_from_slice(marker(Edge::ImageStarted).as_bytes());
    bytes.extend_from_slice(marker(Edge::RegisterStarted).as_bytes());
    bytes.extend_from_slice(b"{\"schema\":1,\"event\":\"diagnostic\",\"stage\":\"registered_fault\",\"category\":\"sample_written\"}\n");
    bytes.extend_from_slice(b"{\"schema\":2,\"event\":\"diagnostic\",\"stage\":\"registered_fault_edge\",\"category\":\"sample_written\"}\n");
    bytes.extend_from_slice(b"\xffprivate non-UTF8\n");
    bytes.extend_from_slice(b"private partial final line");
    let no_tail = project(Ok(&bytes), SIGABRT);
    assert!(no_tail.context && !no_tail.unclassified_tail && no_tail.out_of_order);
    assert_eq!(no_tail.edges, [Edge::RegisterStarted, Edge::ImageStarted]);
    assert_eq!(no_tail.state, ReadState::Complete);
    assert_eq!(no_tail.abort, AbortClass::Unrecognized);
    bytes.push(b'\n');
    let tail = project(Ok(&bytes), SIGABRT);
    assert!(tail.unclassified_tail);
    assert!(!format!("{tail:?}").contains("private"));
}

#[test]
fn log_projection_checks_exact_literal_tail_and_unreadable_input() {
    use child_log::{AbortClass, OWNED_FD_LINE, ReadState, project};

    assert_eq!(
        OWNED_FD_LINE,
        b"fatal runtime error: IO Safety violation: owned file descriptor already closed, aborting"
    );
    let mut bytes = b"{\"schema\":1,\"event\":\"context\"}\nspoofed prefix ".to_vec();
    bytes.extend_from_slice(OWNED_FD_LINE);
    bytes.push(b'\n');
    bytes.extend_from_slice(OWNED_FD_LINE);
    bytes.extend_from_slice(b"\r\n");
    assert_eq!(project(Ok(&bytes), SIGABRT).abort, AbortClass::Unrecognized);
    assert!(project(Ok(&bytes), SIGABRT).unclassified_tail);
    bytes.extend_from_slice(b"{\"schema\":1,\"event\":\"context\"}\n");
    for _ in 0..2 {
        bytes.extend_from_slice(OWNED_FD_LINE);
        bytes.push(b'\n');
    }
    let matching = project(Ok(&bytes), SIGABRT);
    assert_eq!(matching.abort, AbortClass::OwnedFdClosed);
    assert!(!matching.unclassified_tail);
    bytes.extend_from_slice(b"trailing non-JSON\n");
    assert!(project(Ok(&bytes), SIGABRT).unclassified_tail);
    assert_eq!(
        project(Ok(&bytes), ChildEnd::Signaled(libc::SIGTERM)).abort,
        AbortClass::NotAborted
    );
    let unreadable = tempfile::NamedTempFile::new().unwrap();
    let output_only = OpenOptions::new()
        .write(true)
        .open(unreadable.path())
        .unwrap();
    let observation = observe_child_log(&output_only, SIGABRT);
    assert_eq!(observation.state, ReadState::Unreadable);
    assert_eq!(observation.abort, AbortClass::Unknown);
    assert_eq!(
        project(Err(io::Error::from(io::ErrorKind::Interrupted)), SIGABRT).abort,
        AbortClass::Unknown
    );
}

#[test]
fn log_projection_respects_exact_prefix_and_complete_record_boundaries() {
    use super::super::Edge;
    use child_log::{AbortClass, CAP, OWNED_FD_LINE, ReadState, project};

    let mut complete = OWNED_FD_LINE.to_vec();
    complete.push(b'\n');
    let edge = marker(Edge::SampleWritten);
    for record in [complete.as_slice(), edge.as_bytes()] {
        for extent in [CAP - 1, CAP, CAP + 1] {
            let mut bytes = vec![b'\n'; extent - record.len()];
            bytes.extend_from_slice(record);
            let seen = project(Ok(&bytes), SIGABRT);
            if extent <= CAP {
                assert_eq!(seen.state, ReadState::Complete);
                if record == complete {
                    assert_eq!(seen.abort, AbortClass::OwnedFdClosed);
                } else {
                    assert_eq!(seen.edges, [Edge::SampleWritten]);
                }
            } else {
                assert_eq!(seen.state, ReadState::Truncated);
                assert_eq!(seen.abort, AbortClass::Unknown);
                assert!(seen.edges.is_empty());
            }
        }
        let mut bytes = vec![b'\n'; CAP - record.len() / 2];
        bytes.extend_from_slice(record);
        let split = project(Ok(&bytes), SIGABRT);
        assert_eq!(split.state, ReadState::Truncated);
        assert_eq!(split.abort, AbortClass::Unknown);
        assert!(split.edges.is_empty());
    }
    let mut bytes = complete;
    bytes.resize(CAP + 1, b'\n');
    let early = project(Ok(&bytes), SIGABRT);
    assert_eq!(early.state, ReadState::Truncated);
    assert_eq!(early.abort, AbortClass::OwnedFdClosed);
}
