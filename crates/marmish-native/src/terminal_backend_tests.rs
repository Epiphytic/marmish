use super::*;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::process::Stdio;

#[test]
fn core_tests_terminal_backend_ids_paths_and_socket_mode() {
    assert!(!paths::same_nonroot_ids(&[0, 0], &[11, 11]));
    assert!(!paths::same_nonroot_ids(&[44, 44], &[0, 0]));
    assert!(!paths::same_nonroot_ids(&[44, 44, 45], &[11, 11, 11]));
    assert!(!paths::same_nonroot_ids(&[44, 44, 44], &[11, 11, 12]));
    assert!(paths::same_nonroot_ids(&[44, 44, 44], &[11, 11, 11]));
    assert!(paths::session("a-B_001").is_ok());
    for invalid in ["", "=default", "a:b", "a.b", "é", &"z".repeat(65)] {
        assert!(paths::session(invalid).is_err());
    }

    // SAFETY: geteuid only inspects process identity.
    let uid = unsafe { libc::geteuid() };
    let directory = tempfile::Builder::new()
        .prefix("mwk-b-")
        .tempdir_in("/tmp")
        .unwrap();
    let root = directory.path().canonicalize().unwrap();
    let socket = root.join("sock");
    assert!(paths::socket(&socket, uid).is_err());
    let _listener = UnixListener::bind(&socket).unwrap();
    for mode in [0o600, 0o750, 0o770] {
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(paths::socket(&socket, uid).is_err());
    }
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    paths::socket(&socket, uid).unwrap();
    std::os::unix::fs::symlink(&socket, root.join("alias")).unwrap();
    assert!(paths::socket(&root.join("alias"), uid).is_err());
    assert!(paths::tmux(&root.join("alias"), uid).is_err());
    assert!(paths::tmux(Path::new("tmux"), uid).is_err());
    assert!(paths::daemon(Path::new("marmishd")).is_err());
}

fn sleeping_child() -> ChildState {
    ChildState::new(
        Command::new("/bin/sleep")
            .arg("60")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

fn guarded(child: ChildState) -> Attachment {
    Attachment {
        master: None,
        generation: 1,
        role: Role::Writer,
        child: Some(child),
        reaper: reserve_reaper().unwrap(),
    }
}

#[test]
fn core_tests_terminal_backend_worker_status_and_grace() {
    let mut attachment = guarded(sleeping_child());
    let child = attachment.child.as_mut().unwrap();
    for records in [
        &[][..],
        &[READY],
        &[FAILED],
        &[SESSION, READY, EXEC_FAILED],
        &[255],
    ] {
        let (read, mut write) = os::status_pipe().unwrap();
        write.write_all(records).unwrap();
        drop(write);
        assert!(matches!(
            await_status(read, child, Instant::now() + Duration::from_millis(100)),
            Err(Error::Invalid("terminal_worker_launch"))
        ));
    }
    let (read, _write) = os::status_pipe().unwrap();
    let started = Instant::now();
    assert!(matches!(
        await_status(read, child, started + Duration::from_millis(30)),
        Err(Error::Invalid("terminal_worker_launch"))
    ));
    assert!(started.elapsed() >= Duration::from_millis(30));
    child.signal(libc::SIGKILL).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !child.waitable().unwrap() {
        assert!(Instant::now() < deadline, "terminal_child_not_waitable");
        thread::sleep(CHILD_POLL);
    }
    assert!(matches!(
        exec_grace(child, Instant::now() + EXEC_GRACE),
        Err(Error::Invalid("terminal_worker_launch"))
    ));
    assert!(child.waitable().unwrap());
    assert_eq!(attachment.shutdown(deadline).unwrap(), Shutdown::Reaped);
}

#[test]
fn core_tests_terminal_backend_pending_drop_and_wake() {
    let mut attachment = guarded(sleeping_child());
    let pid = attachment.child.as_ref().unwrap().pid;
    let (wake, mut signal) = os::status_pipe().unwrap();
    signal.write_all(b"x").unwrap();
    assert!(
        attachment
            .poll(false, Some(wake.as_fd()), Instant::now())
            .unwrap()
            .wake
    );
    assert_eq!(
        attachment.shutdown(Instant::now()).unwrap(),
        Shutdown::Pending
    );
    assert_eq!(attachment.child.as_ref().unwrap().pid, pid);
    drop(attachment);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        // SAFETY: inspect only our former child, without consuming its status. The reserved
        // reaper is the only process that can consume it after attachment drop.
        let status = unsafe {
            let mut info = std::mem::zeroed::<libc::siginfo_t>();
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if status != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD) {
            break;
        }
        assert!(Instant::now() < deadline, "terminal_reaper_timeout");
        thread::sleep(CHILD_POLL);
    }
}

#[test]
fn core_tests_terminal_backend_generation_fences_partial_write() {
    let (_read, write) = os::status_pipe().unwrap();
    // SAFETY: file remains exclusively owned, open and live until moved into the attachment.
    unsafe {
        let descriptor = write.as_raw_fd();
        let flags = libc::fcntl(descriptor, libc::F_GETFL);
        assert!(
            flags >= 0 && libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) == 0
        );
    }
    let mut terminal = Attachment {
        master: Some(write),
        generation: 1,
        role: Role::Writer,
        child: None,
        reaper: reserve_reaper().unwrap(),
    };
    let input = [b'x'; 256 * 1024];
    let WriteStatus::Partial(written) = terminal.write(1, &input).unwrap() else {
        panic!("terminal_partial_write")
    };
    assert!(written > 0 && written < input.len());
    terminal.supersede(2).unwrap();
    assert!(matches!(
        terminal.write(1, &input[written..]),
        Err(Error::Invalid("terminal_attachment_generation"))
    ));
    assert!(matches!(
        terminal.resize(1, 24, 80),
        Err(Error::Invalid("terminal_attachment_generation"))
    ));
    assert!(matches!(
        terminal.supersede(1),
        Err(Error::Invalid("terminal_attachment_generation"))
    ));
    assert_eq!(terminal.write(2, &[]).unwrap(), WriteStatus::Complete);
}

#[test]
fn core_tests_terminal_backend_descriptor_inventory() {
    const CASE: &str = "terminal_backend::tests::core_tests_terminal_backend_descriptor_inventory";
    const GUARD: &str = "MARMISH_TEST_TERMINAL_INVENTORY_CHILD";
    const PROBE: RawFd = 98;
    if std::env::var_os(GUARD).is_some_and(|value| value == "1") {
        inventory_child(PROBE);
        // SAFETY: close_inventory scrubbed this dedicated test subprocess; skip harness teardown.
        unsafe { libc::_exit(0) }
    }
    let probe = File::open("/dev/null").unwrap();
    let source = probe.as_raw_fd();
    assert_ne!(source, PROBE);
    let reaper = reserve_reaper().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env(GUARD, "1")
        .args(["--exact", CASE, "--nocapture"])
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    // SAFETY: the test child uses one async-signal-safe syscall to install the deliberate probe.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(source, PROBE) == PROBE {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    }
    let child = command.spawn().unwrap();
    let mut guard = Attachment {
        master: None,
        generation: 1,
        role: Role::Writer,
        child: Some(ChildState::new(child)),
        reaper,
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    let owned = guard.child.as_mut().unwrap();
    while !owned.waitable().unwrap() {
        assert!(
            Instant::now() < deadline,
            "terminal_inventory_subprocess_timeout"
        );
        thread::sleep(CHILD_POLL);
    }
    let result = owned.child.wait().unwrap();
    guard.child.take();
    assert!(result.success(), "terminal_inventory_subprocess_failed");
}

fn inventory_child(probe: RawFd) {
    use std::ffi::CString;
    use std::os::fd::IntoRawFd;
    use std::os::unix::ffi::OsStrExt;

    let (_, status) = os::status_pipe().unwrap();
    let status = status.into_raw_fd();
    let descriptor_exists = |fd| {
        // SAFETY: F_GETFD only inspects a descriptor number in this process.
        (unsafe { libc::fcntl(fd, libc::F_GETFD) }) >= 0
    };
    assert!(descriptor_exists(probe));
    // A very large descriptor number is absent on both platforms, including from /dev/fd.
    assert!(os::close_inventory(i32::MAX).is_err());
    assert!(descriptor_exists(probe));
    let fixture = tempfile::tempdir().unwrap();
    std::fs::write(fixture.path().join(status.to_string()), b"").unwrap();
    let path = CString::new(fixture.path().as_os_str().as_bytes()).unwrap();
    // SAFETY: path is a live NUL-terminated private directory path.
    let directory = unsafe { libc::opendir(path.as_ptr()) };
    assert!(!directory.is_null());
    let missing_own = os::inventory_fds(directory, status);
    // SAFETY: opendir returned a live stream that has remained exclusively ours.
    assert_eq!(unsafe { libc::closedir(directory) }, 0);
    assert!(matches!(
        missing_own,
        Err(Error::Invalid("terminal_descriptors"))
    ));
    drop(fixture);

    os::close_inventory(status).unwrap();
    assert!(!descriptor_exists(probe));
    assert!([0, 1, 2, status].into_iter().all(descriptor_exists));
}
