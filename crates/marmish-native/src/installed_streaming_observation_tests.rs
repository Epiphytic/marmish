use super::*;
use std::cell::RefCell;
fn lines(bytes: &[u8]) -> Vec<serde_json::Value> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_slice(
                line.strip_prefix(b"marmish installed stream observation: ")
                    .unwrap(),
            )
            .unwrap()
        })
        .collect()
}
#[test]
fn closed_events_bound_size_reserve_terminal_and_isolate_contexts() {
    let bytes = RefCell::new(Vec::new());
    let sink = |line: &[u8]| {
        assert!(line.len() <= 1536);
        bytes.borrow_mut().extend(line);
        Ok(())
    };
    let first = Trace::new("linux-amd64", "macos-arm64", &sink).unwrap();
    let second = Trace::new("linux-arm64", "linux-amd64", &sink).unwrap();
    first.event(Stage::Cell, Outcome::Begin, None);
    second.event(Stage::Cell, Outcome::Begin, None);
    for _ in 0..100 {
        first.event(Stage::Wait, Outcome::Begin, None);
    }
    first.finish(false);
    first.finish(true);
    first.event(Stage::Wait, Outcome::Ok, None);
    second.finish(true);
    let events = lines(&bytes.borrow());
    assert_eq!(events.len(), 50);
    assert_eq!(
        events.iter().filter(|e| e["stage"] == "truncated").count(),
        1
    );
    assert_eq!(events[48]["outcome"], "error");
    assert_eq!(events[49]["client"], "linux-arm64");
    assert!(Trace::new("secret-path", "macos-arm64", &sink).is_none());
    assert_eq!(elapsed_ms(u128::MAX), u64::MAX);
    assert_eq!(io_kind(io::ErrorKind::Unsupported), IoKind::Other);
}
#[test]
fn evidence_is_closed_and_never_formats_io_text() {
    let bytes = RefCell::new(Vec::new());
    let sink = |line: &[u8]| {
        bytes.borrow_mut().extend(line);
        Ok(())
    };
    let trace = Trace::new("linux-amd64", "macos-arm64", &sink).unwrap();
    let error = io::Error::new(io::ErrorKind::BrokenPipe, "SECRET/path/identity");
    trace.event(
        Stage::InputWrite,
        Outcome::Error,
        Some(Evidence::Io {
            io: io_kind(error.kind()),
        }),
    );
    assert_eq!(lines(&bytes.borrow())[0]["evidence"]["io"], "broken_pipe");
    assert!(!String::from_utf8_lossy(&bytes.borrow()).contains("SECRET"));
}
#[tokio::test]
async fn original_runner_parity_and_failing_sink_on_success_and_failures() {
    use super::super::process;
    use std::{ffi::OsString, path::Path, time::Duration};
    let dir = tempfile::tempdir().unwrap();
    let env = process::Environment {
        home: dir.path().into(),
        temporary: dir.path().into(),
        utilities: dir.path().into(),
    };
    let scenarios = [
        (
            "/bin/cat; printf err >&2; exit 7",
            vec![0, 255, 10],
            64,
            2000,
            "capture",
            "ok",
        ),
        (
            "printf %s $$ > ownedpid; exec 0<&-; /bin/sleep 0.1",
            vec![1; 1024 * 1024],
            64,
            2000,
            "input_write",
            "error",
        ),
        (
            "printf %s $$ > ownedpid; while :; do :; done",
            vec![],
            64,
            250,
            "wait",
            "timeout",
        ),
        (
            "/bin/sleep 0.3 & exit 0",
            vec![],
            64,
            50,
            "stdout_join",
            "timeout",
        ),
        (
            "exec 1>&-; /bin/sleep 0.3 & exit 0",
            vec![],
            64,
            50,
            "stderr_join",
            "timeout",
        ),
        ("printf 123456789", vec![], 8, 2000, "overflow", "error"),
    ];
    for (script, input, limit, millis, stage, outcome) in scenarios {
        let args = [OsString::from("-c"), script.into()];
        let baseline = process::run_input(
            Path::new("/bin/sh"),
            &args,
            dir.path(),
            &env,
            tokio::time::Instant::now() + Duration::from_millis(millis),
            limit,
            Some(&input),
        )
        .await;
        if matches!(stage, "input_write" | "wait") {
            assert_reaped(dir.path());
        }
        for failing in [false, true] {
            let bytes = RefCell::new(Vec::new());
            let sink = |line: &[u8]| {
                bytes.borrow_mut().extend(line);
                if failing {
                    Err(io::Error::other("SECRET"))
                } else {
                    Ok(())
                }
            };
            let trace = Trace::new("linux-amd64", "macos-arm64", &sink).unwrap();
            let observed = process::run_input_observed(
                Path::new("/bin/sh"),
                &args,
                dir.path(),
                &env,
                tokio::time::Instant::now() + Duration::from_millis(millis),
                limit,
                (&input, &trace),
            )
            .await;
            match (&baseline, &observed) {
                (Ok(a), Ok(b)) => {
                    assert_eq!(a.status.code(), b.status.code());
                    assert_eq!(a.stdout, b.stdout);
                    assert_eq!(a.stderr, b.stderr);
                }
                (Err(crate::Error::Invalid(a)), Err(crate::Error::Invalid(b))) => assert_eq!(a, b),
                (Err(crate::Error::Timeout), Err(crate::Error::Timeout)) => {}
                _ => panic!("runner parity mismatch"),
            }
            let events = lines(&bytes.borrow());
            if matches!(stage, "input_write" | "wait") {
                assert_reaped(dir.path());
                assert!(
                    events
                        .iter()
                        .any(|e| e["stage"] == "terminate" && e["outcome"] == "begin")
                );
                assert!(
                    events
                        .iter()
                        .any(|e| e["stage"] == "terminate" && e["outcome"] == "ok")
                );
            }
            let decisive = events
                .iter()
                .rposition(|event| event["stage"] != "terminate")
                .unwrap();
            assert_eq!(events[decisive]["stage"], stage);
            assert_eq!(events[decisive]["outcome"], outcome);
            // Cleanup can follow the decisive failure, but cannot hide a later
            // operation stage or turn the original operation into success.
            let cleanup = &events[decisive + 1..];
            if matches!(stage, "input_write" | "wait") {
                assert_eq!(cleanup.len(), 2);
                assert_eq!(cleanup[0]["stage"], "terminate");
                assert_eq!(cleanup[0]["outcome"], "begin");
                assert_eq!(cleanup[1]["stage"], "terminate");
                assert_eq!(cleanup[1]["outcome"], "ok");
            } else {
                assert!(cleanup.is_empty());
            }
            if stage == "input_write" {
                assert!(
                    events
                        .iter()
                        .all(|e| e["evidence"].get("local_pipe_written").is_none())
                );
            }
        }
    }
}

fn assert_reaped(directory: &std::path::Path) {
    let path = directory.join("ownedpid");
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() <= 32);
    let pid: libc::pid_t = std::str::from_utf8(&bytes).unwrap().parse().unwrap();
    assert!(pid > 1);
    // Probe only: no signal is ever sent using a persisted numeric identity.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    std::fs::remove_file(path).unwrap();
}
