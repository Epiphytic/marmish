use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn absent_backend() -> (tempfile::TempDir, Command) {
    let directory = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_marmish-keychain"));
    command
        .env_clear()
        .env("HOME", directory.path())
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}/absent-bus", directory.path().display()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    (directory, command)
}

#[test]
fn malformed_requests_and_arguments_have_fixed_nonsecret_errors() {
    for (input, argument) in [
        (b"private diagnostic marker".to_vec(), None),
        (vec![b'x'; 8193], None),
        (
            br#"{"version":1,"action":"probe","extra":"private diagnostic marker"}"#.to_vec(),
            None,
        ),
        (
            br#"{"version":1,"action":"probe"}"#.to_vec(),
            Some("private diagnostic marker"),
        ),
    ] {
        let (_directory, mut command) = absent_backend();
        if let Some(argument) = argument {
            command.arg(argument);
        }
        let mut child = command.spawn().unwrap();
        // Argument rejection can close stdin before this write completes.
        let _ = child.stdin.take().unwrap().write_all(&input);
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert_eq!(
            output.stdout,
            b"{\"version\":1,\"ok\":false,\"error\":\"malformed\"}\n"
        );
        assert!(output.stderr.is_empty());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn provably_absent_bus_is_unavailable_without_fallback() {
    let (_directory, mut command) = absent_backend();
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"version":1,"action":"probe"}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert_eq!(
        output.stdout,
        b"{\"version\":1,\"ok\":false,\"error\":\"unavailable\"}\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn stalled_stdin_is_bounded_by_the_internal_watchdog() {
    let (_directory, mut command) = absent_backend();
    let mut child = command.spawn().unwrap();
    let input = child.stdin.take().unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(20) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("helper watchdog did not terminate stalled stdin");
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    drop(input);
    assert_eq!(status.code(), Some(124));
    assert!(started.elapsed() >= Duration::from_secs(14));
    let output = child.wait_with_output().unwrap();
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}
