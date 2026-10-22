use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::TempDir;

// Serialize fixture creation with fork/exec: a concurrently forked child can
// briefly retain another test's write-open script fd until exec closes it,
// causing Linux ETXTBSY despite the writer already closing in the parent.
static SUBPROCESS_FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());

// This generic runner is private to unit tests; canonical paths must remain
// beneath a test-owned temporary directory. It cannot be a transport backend.
fn fixture_run(
    root: &TempDir,
    executable: &Path,
    args: &[OsString],
    timeout: Duration,
) -> Result<(Vec<u8>, Vec<u8>), AdapterError> {
    let executable = executable
        .canonicalize()
        .map_err(|_| AdapterError::MissingExecutable)?;
    let owned = root.path().canonicalize().unwrap();
    if !executable.starts_with(owned) || !executable.is_file() {
        return Err(AdapterError::InvalidExecutable);
    }
    observe(&executable, args, timeout)
}

fn script(root: &TempDir, content: &str) -> PathBuf {
    let path = root.path().join("synthetic-wn");
    std::fs::write(&path, format!("#!/bin/sh\n{content}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn explicit_executable_and_version_only() {
    let _fixture_guard = SUBPROCESS_FIXTURES.lock().unwrap();
    assert!(matches!(
        RealWn::new("wn".into()),
        Err(AdapterError::InvalidExecutable)
    ));
    assert!(matches!(
        RealWn::new("/no/such/marmish-wn".into()),
        Err(AdapterError::MissingExecutable)
    ));
    let root = TempDir::new().unwrap();
    let executable = script(&root, "[ \"$#\" -eq 1 ] && [ \"$1\" = --version ] || exit 9\nprintf 'synthetic wn fixture 0\\n'\nprintf 'fixture stderr\\n' >&2");
    let observation = RealWn::new(executable).unwrap().version().unwrap();
    assert_eq!(observation.stdout, "synthetic wn fixture 0\n");
    assert_eq!(observation.stderr, "fixture stderr\n");
    assert!(matches!(
        RealWn::new(root.path().to_owned()),
        Err(AdapterError::InvalidExecutable)
    ));
}

#[test]
fn fixture_runner_refuses_paths_outside_owned_directory() {
    let _fixture_guard = SUBPROCESS_FIXTURES.lock().unwrap();
    let root = TempDir::new().unwrap();
    assert!(matches!(
        fixture_run(&root, Path::new("/bin/sh"), &[], PROBE_TIMEOUT),
        Err(AdapterError::InvalidExecutable)
    ));
}

#[test]
fn environment_is_exactly_empty_even_when_ambient_is_poisoned() {
    let _fixture_guard = SUBPROCESS_FIXTURES.lock().unwrap();
    // Environment mutation occurs in a subprocess copy of this test binary, so
    // parallel tests and the parent process never observe the poisoned values.
    const CHILD: &str = "MARMISH_ENVIRONMENT_FIXTURE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::environment_is_exactly_empty_even_when_ambient_is_poisoned",
            ])
            .env(CHILD, "yes")
            .env("PATH", "")
            .env("HOME", "/poison")
            .env("WN_HOME", "/poison")
            .env("WN_ALLOW_LOOPBACK_RELAYS", "1")
            .env("XDG_CONFIG_HOME", "/poison")
            .env("HTTP_PROXY", "http://invalid")
            .env("HTTPS_PROXY", "http://invalid")
            .env("ALL_PROXY", "http://invalid")
            .env("DO_NOT_TRACK", "poison")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let root = TempDir::new().unwrap();
    // Copy env: no shell/runtime can add PWD or locale variables before observation.
    let executable = root.path().join("synthetic-env-observer");
    std::fs::copy("/usr/bin/env", &executable).unwrap();
    let (stdout, stderr) = fixture_run(&root, &executable, &["-0".into()], PROBE_TIMEOUT).unwrap();
    assert_eq!(stdout, b"");
    assert_eq!(stderr, b"");
    let fake = script(
        &root,
        "[ \"$1\" = --version ] || exit 9\nprintf 'empty PATH works\\n'",
    );
    assert_eq!(
        RealWn::new(fake).unwrap().version().unwrap().stdout,
        "empty PATH works\n"
    );
}

#[test]
fn concurrent_output_is_separate_and_capped() {
    let _fixture_guard = SUBPROCESS_FIXTURES.lock().unwrap();
    let root = TempDir::new().unwrap();
    let fake = script(
        &root,
        "i=0\nwhile [ \"$i\" -lt 4000 ]; do printf 'out\\n'; printf 'err\\n' >&2; i=$((i+1)); done",
    );
    let (stdout, stderr) = fixture_run(&root, &fake, &[], Duration::from_secs(10)).unwrap();
    assert_eq!(stdout, b"out\n".repeat(4000));
    assert_eq!(stderr, b"err\n".repeat(4000));
    let fake = script(&root, "while :; do printf '0123456789012345678901234567890123456789012345678901234567890123456789'; done");
    assert!(matches!(
        fixture_run(&root, &fake, &[], PROBE_TIMEOUT),
        Err(AdapterError::OutputLimit)
    ));
    let fake = script(&root, "while :; do printf '0123456789012345678901234567890123456789012345678901234567890123456789' >&2; done");
    assert!(matches!(
        fixture_run(&root, &fake, &[], PROBE_TIMEOUT),
        Err(AdapterError::OutputLimit)
    ));
}

#[test]
fn nonzero_exit_and_diagnostic_control_bytes_are_rejected() {
    let _fixture_guard = SUBPROCESS_FIXTURES.lock().unwrap();
    let root = TempDir::new().unwrap();
    let fake = script(&root, "printf 'sensitive error text' >&2\nexit 7");
    let error = fixture_run(&root, &fake, &[], PROBE_TIMEOUT).unwrap_err();
    assert_eq!(error.to_string(), "subprocess failed");
    let fake = script(&root, "printf '\\033[31mbad'");
    assert!(matches!(
        RealWn::new(fake).unwrap().version(),
        Err(AdapterError::InvalidResponse)
    ));
}

#[test]
fn deadline_covers_descendants_holding_both_pipes_after_parent_exit() {
    let _fixture_guard = SUBPROCESS_FIXTURES.lock().unwrap();
    let root = TempDir::new().unwrap();
    let fake = script(
        &root,
        "/bin/sleep 30 &\nprintf '%s' \"$!\" > descendant\nexit 0",
    );
    // Fixed cwd is /, so use an explicit fixture path for a marker instead.
    let source = std::fs::read_to_string(&fake).unwrap().replace(
        "> descendant",
        &format!("> '{}'", root.path().join("descendant").display()),
    );
    std::fs::write(&fake, source).unwrap();
    let start = Instant::now();
    assert!(matches!(
        fixture_run(&root, &fake, &[], Duration::from_millis(80)),
        Err(AdapterError::Timeout)
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
    let pid: i32 = std::fs::read_to_string(root.path().join("descendant"))
        .unwrap()
        .parse()
        .unwrap();
    // A killed grandchild can briefly remain a zombie awaiting its own parent/init.
    // It must not remain a running process or hold the output pipe open.
    let delivery_deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let proc_stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
        let Ok(stat) = proc_stat else { break };
        let state = stat.rsplit_once(") ").unwrap().1.chars().next().unwrap();
        if state == 'Z' || state == 'X' {
            break;
        }
        assert!(
            Instant::now() < delivery_deadline,
            "descendant remained live: {state}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn network_plans_are_inert_and_gated_before_any_spawn() {
    let _fixture_guard = SUBPROCESS_FIXTURES.lock().unwrap();
    let root = TempDir::new().unwrap();
    let marker = root.path().join("spawn-count");
    let fake = script(&root, &format!("printf 'spawn' >> '{}'", marker.display()));
    let relays = RelaySet::parse(
        &[
            "wss://RELAY.example:443/".into(),
            "wss://relay.example".into(),
        ],
        false,
    )
    .unwrap();
    let plans = [
        CommandPlan::daemon_start(root.path().join("state"), &relays),
        CommandPlan::create_room(
            root.path().join("state"),
            &relays,
            "synthetic-room".into(),
            &["synthetic-member".into()],
            Retention::default(),
        ),
        CommandPlan::sweep_expired(root.path().join("state"), &relays),
    ];
    for plan in plans {
        assert_eq!(plan.class(), CommandClass::NetworkCapable);
        assert_eq!(plan.relays(), &["wss://relay.example/".to_owned()]);
        let result = plan
            .require_live_egress()
            .and_then(|()| fixture_run(&root, &fake, plan.arguments(), PROBE_TIMEOUT));
        assert!(matches!(result, Err(AdapterError::LiveEgressUnverified)));
    }
    assert!(!marker.exists());
    let version = CommandPlan::version();
    assert_eq!(version.class(), CommandClass::OfflineVersion);
    version.require_live_egress().unwrap();
    assert_eq!(version.arguments(), &[OsString::from("--version")]);
    let daemon = CommandPlan::daemon_start(root.path().join("state"), &relays);
    assert_eq!(
        &daemon.arguments()[3..],
        &[
            "daemon",
            "start",
            "--discovery-relays",
            "wss://relay.example/",
            "--default-account-relays",
            "wss://relay.example/"
        ]
        .map(OsString::from)
    );
}

#[test]
fn synthetic_json_and_ndjson_are_bounded_and_structural_only() {
    let fixture = br#"{"fixture":"synthetic","group_id":"mls-group","routing_id":"nostr-routing"}"#;
    let value = parse_json(fixture).unwrap();
    assert_ne!(value["group_id"], value["routing_id"]);
    assert!(parse_json(b"{unfinished").is_err());
    assert!(matches!(
        parse_json(&vec![b' '; OUTPUT_LIMIT + 1]),
        Err(AdapterError::OutputLimit)
    ));
    let records = b"{\"fixture\":\"synthetic\",\"status\":\"pruned\"}\n{\"fixture\":\"synthetic\",\"status\":\"deferred\"}\n";
    assert_eq!(parse_ndjson(&records[..]).unwrap().len(), 2);
    assert!(matches!(
        parse_ndjson(&b"{}"[..]),
        Err(AdapterError::InvalidResponse)
    ));
    assert!(matches!(
        parse_ndjson(&b"{}\n{"[..]),
        Err(AdapterError::InvalidResponse)
    ));
    assert!(matches!(
        parse_ndjson(&vec![b'x'; OUTPUT_LIMIT + 1][..]),
        Err(AdapterError::OutputLimit)
    ));
    assert!(matches!(
        parse_ndjson(&b"{}\n".repeat(201)[..]),
        Err(AdapterError::OutputLimit)
    ));
    // Tiny BufRead buffers exercise an overlong record spread across many reads.
    let bytes = vec![b'x'; OUTPUT_LIMIT + 1];
    let reader = std::io::BufReader::with_capacity(7, bytes.as_slice());
    assert!(matches!(
        parse_ndjson(reader),
        Err(AdapterError::OutputLimit)
    ));
}

#[test]
fn timeout_reaps_direct_child() {
    let _fixture_guard = SUBPROCESS_FIXTURES.lock().unwrap();
    let root = TempDir::new().unwrap();
    let marker = root.path().join("child");
    let fake = script(
        &root,
        &format!(
            "printf '%s' \"$$\" > '{}'\nexec /bin/sleep 30",
            marker.display()
        ),
    );
    assert!(matches!(
        fixture_run(&root, &fake, &[], Duration::from_millis(80)),
        Err(AdapterError::Timeout)
    ));
    let pid: i32 = std::fs::read_to_string(marker).unwrap().parse().unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
}
