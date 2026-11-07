//! Limit mutation is confined to self-executed test children.
use super::*;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::Command;
use std::time::Instant;

const CASE: &str = "MARMISH_TEST_PROCESS_LIMIT_CASE";

fn isolated(name: &str, run: impl FnOnce()) {
    if std::env::var_os(CASE).as_deref() == Some(std::ffi::OsStr::new(name)) {
        run();
        return;
    }
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("process_limits_tests::{name}"),
            "--nocapture",
        ])
        .env(CASE, name)
        .status()
        .unwrap();
    assert!(status.success(), "isolated process limit test failed");
}

fn limits() -> libc::rlimit {
    let mut value = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut value) },
        0
    );
    value
}

fn set_soft(soft: libc::rlim_t) -> libc::rlimit {
    let before = limits();
    let changed = libc::rlimit {
        rlim_cur: soft,
        rlim_max: before.rlim_max,
    };
    assert_eq!(
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &changed) },
        0,
        "required isolated test limit is not supported by runner"
    );
    let actual = limits();
    assert_eq!(actual.rlim_cur, soft);
    assert_eq!(actual.rlim_max, before.rlim_max);
    actual
}

#[cfg(target_os = "macos")]
fn high_soft() -> libc::rlimit {
    assert!(
        limits().rlim_max > 65_536,
        "runner cannot exercise required high-soft normalization"
    );
    set_soft(65_537)
}

#[test]
#[cfg(target_os = "macos")]
fn normalizes_high_soft_and_preserves_hard() {
    isolated("normalizes_high_soft_and_preserves_hard", || {
        let before = high_soft();
        // RED contract: the unchanged engine gate really refuses this limit.
        assert!(matches!(
            platform::prepare_spawn(),
            Err(EngineError::WorkerUnavailable)
        ));
        initialize_process_limits().unwrap();
        assert_eq!(limits().rlim_cur, 65_536);
        assert_eq!(limits().rlim_max, before.rlim_max);
        platform::prepare_spawn().unwrap();
        initialize_process_limits().unwrap();
        assert_eq!(limits().rlim_cur, 65_536);
        assert_eq!(limits().rlim_max, before.rlim_max);
    });
}

#[test]
fn lower_soft_hard_and_descriptor_flags_are_unchanged() {
    isolated("lower_soft_hard_and_descriptor_flags_are_unchanged", || {
        let before = set_soft(limits().rlim_cur.min(1024));
        let file = std::fs::File::open("/dev/null").unwrap();
        let fd = file.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0);
        initialize_process_limits().unwrap();
        initialize_process_limits().unwrap();
        let after = limits();
        assert_eq!(
            (after.rlim_cur, after.rlim_max),
            (before.rlim_cur, before.rlim_max)
        );
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, flags);
    });
}

#[test]
#[cfg(target_os = "linux")]
fn linux_process_limits_are_unchanged() {
    isolated("linux_process_limits_are_unchanged", || {
        let before = limits();
        initialize_process_limits().unwrap();
        let after = limits();
        assert_eq!(
            (after.rlim_cur, after.rlim_max),
            (before.rlim_cur, before.rlim_max)
        );
    });
}

#[test]
fn normalized_startup_executes_offline_exact_bytes_and_status() {
    isolated(
        "normalized_startup_executes_offline_exact_bytes_and_status",
        || {
            if unsafe { libc::geteuid() } == 0 {
                eprintln!("nonroot engine execution branch not exercised as root");
                return;
            }
            #[cfg(target_os = "macos")]
            let before = high_soft();
            initialize_process_limits().unwrap();
            #[cfg(target_os = "macos")]
            {
                assert_eq!(limits().rlim_cur, 65_536);
                assert_eq!(limits().rlim_max, before.rlim_max);
            }
            let root = tempfile::tempdir().unwrap();
            let principal = PrincipalId([1; 32]);
            let server = ServerId([2; 32]);
            let engine = Engine::open(EnginePolicy {
                state_root: root.path().join("engine"),
                worker_executable: std::env::current_exe().unwrap(),
                server,
                allowed_principals: vec![principal],
                allow_shell_command: false,
                command_policy: None,
                cwd: Some(std::env::current_dir().unwrap()),
                limits: EngineLimits::default(),
            })
            .unwrap();
            let context = engine
                .authorize(principal, EndpointId([3; 32]), server, unsafe {
                    libc::geteuid()
                })
                .unwrap();
            let prepared = engine
                .prepare_live_exec(
                    &context,
                    vec![
                        b"/bin/sh".to_vec(),
                        b"-c".to_vec(),
                        b"/bin/cat; printf '\\376\\001' >&2; exit 7".to_vec(),
                    ],
                    None,
                )
                .unwrap();
            let mut child = prepared.spawn().unwrap();
            let pipes = child.take_pipes().unwrap();
            let bytes = b"\0\xff$`'\"\\\n";
            let mut input = pipes.stdin;
            input.write_all(bytes).unwrap();
            drop(input);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !child.try_exit().unwrap() {
                assert!(Instant::now() < deadline, "offline child deadline");
                std::thread::sleep(Duration::from_millis(2));
            }
            // Preserve the live engine's owned-group kill-before-reap contract.
            child.signal_kill().unwrap();
            let status = child.reap_last().unwrap();
            let mut out = Vec::new();
            let mut err = Vec::new();
            let mut stdout = pipes.stdout;
            let mut stderr = pipes.stderr;
            stdout.read_to_end(&mut out).unwrap();
            stderr.read_to_end(&mut err).unwrap();
            assert_eq!(out, bytes);
            assert_eq!(err, b"\xfe\x01");
            assert_eq!(status, ExitOutcome::Code(7));
        },
    );
}
