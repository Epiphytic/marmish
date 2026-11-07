use super::*;
pub(super) fn fixture() -> (tempfile::TempDir, process::Environment) {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let env = process::Environment {
        home: root.path().join("home"),
        temporary: root.path().join("tmp"),
        utilities: root.path().join("utilities"),
    };
    directory(&env.home).unwrap();
    directory(&env.temporary).unwrap();
    directory(&env.utilities).unwrap();
    (root, env)
}
#[tokio::test]
async fn early_exit_preserves_exit_and_partial_input_and_both_pipes() {
    let (root, env) = fixture();
    let (_, signal) = watch::channel(false);
    let output = capture(
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "printf diagnostic; printf reason >&2; exit 7".into(),
        ],
        root.path(),
        &env,
        &vec![1; 1024 * 1024],
        Instant::now() + Duration::from_secs(5),
        signal,
    )
    .await
    .unwrap();
    assert_eq!(output.status.unwrap().code(), Some(7));
    assert_eq!(output.stdout.bytes, b"diagnostic");
    assert_eq!(output.stderr.bytes, b"reason");
    assert!(output.epipe);
    assert!(output.written < 1024 * 1024);
    assert!(output.cleanup);
}
#[tokio::test]
async fn deadline_kills_owned_child_without_discarding_capture() {
    let (root, env) = fixture();
    let (_, signal) = watch::channel(false);
    let output = capture(
        Path::new("/bin/sh"),
        &["-c".into(), "printf before; while :; do :; done".into()],
        root.path(),
        &env,
        &[],
        Instant::now() + Duration::from_millis(100),
        signal,
    )
    .await
    .unwrap();
    assert!(output.timed_out);
    assert!(output.cleanup);
    assert_eq!(output.stdout.bytes, b"before");
    assert_eq!(output.status.unwrap().signal(), Some(libc::SIGKILL));
}
#[test]
fn publication_failure_marker_overrides_partially_published_report() {
    let (root, _) = fixture();
    let report = Report {
        schema: 1,
        kind: "macos-stream-diagnostic",
        qualification: false,
        same_host_only: true,
        relay: RELAY,
        pins: None,
        probe_sha256: "ab".repeat(32),
        stage: "report".into(),
        failure: None,
        observations: vec![],
        cleanup: Finalized::default(),
        passed: true,
    };
    write_private(&root.path().join("complete.json"), b"existing", 0o600).unwrap();
    assert!(publish(root.path(), &report).is_err());
    assert!(root.path().join("report.json").exists());
    assert!(root.path().join("failure.json").exists());
    assert_eq!(
        fs::read(root.path().join("complete.json")).unwrap(),
        b"existing"
    );
}
#[tokio::test]
async fn overflow_drains_without_unbounded_retention() {
    let (root, env) = fixture();
    let (_, signal) = watch::channel(false);
    let c = capture(
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "/usr/bin/head -c 1056768 /dev/zero; printf end >&2; exit 7".into(),
        ],
        root.path(),
        &env,
        &[],
        Instant::now() + Duration::from_secs(5),
        signal,
    )
    .await
    .unwrap();
    assert!(c.stdout.overflow);
    assert_eq!(c.stdout.bytes.len(), CAP);
    assert_eq!(c.stderr.bytes, b"end");
    assert_eq!(c.status.unwrap().code(), Some(7));
    assert!(c.cleanup);
}
#[tokio::test]
async fn signal_latch_joins_owned_child_and_preserves_output() {
    let (root, env) = fixture();
    let (send, signal) = watch::channel(false);
    let signaler = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        send.send(true).unwrap();
    });
    let c = capture(
        Path::new("/bin/sh"),
        &["-c".into(), "printf before; while :; do :; done".into()],
        root.path(),
        &env,
        &[],
        Instant::now() + Duration::from_secs(5),
        signal,
    )
    .await
    .unwrap();
    signaler.await.unwrap();
    assert!(c.interrupted);
    assert!(c.cleanup);
    assert_eq!(c.stdout.bytes, b"before");
    assert_eq!(c.status.unwrap().signal(), Some(libc::SIGKILL));
}
#[tokio::test]
async fn descendant_held_pipes_are_bounded_and_sibling_is_not_signalled() {
    let (root, env) = fixture();
    let (_, signal) = watch::channel(false);
    let mut sibling = process::command(
        Path::new("/bin/sh"),
        &["-c".into(), "while :; do :; done".into()],
        root.path(),
        &env,
    )
    .spawn()
    .unwrap();
    let c = capture(
        Path::new("/bin/sh"),
        &["-c".into(), "/bin/sleep 30 & printf before; exit 7".into()],
        root.path(),
        &env,
        &[],
        Instant::now() + Duration::from_secs(5),
        signal,
    )
    .await
    .unwrap();
    let sibling_alive = !edge_rsync::leader_exited(&sibling).unwrap();
    edge_rsync::reap_owned_group(&mut sibling).await.unwrap();
    assert!(sibling_alive);
    assert!(c.timed_out);
    assert!(c.cleanup);
    assert_eq!(c.status.unwrap().code(), Some(7));
    assert_eq!(c.stdout.bytes, b"before");
}
#[tokio::test]
async fn spawn_failure_is_a_finite_capture_not_lost_case() {
    let (root, env) = fixture();
    let (_, signal) = watch::channel(false);
    let c = capture(
        &root.path().join("absent"),
        &[],
        root.path(),
        &env,
        &[],
        Instant::now() + Duration::from_secs(1),
        signal,
    )
    .await
    .unwrap();
    assert!(c.input_error);
    assert!(c.status.is_none());
    assert!(c.cleanup);
    assert!(c.stdout.bytes.is_empty());
}
pub(super) fn entry(direction: &str, stream: &str) -> streaming_cleanup::Entry {
    streaming_cleanup::Entry {
        stream: stream.into(),
        session: "ab".repeat(16),
        client: "cd".repeat(32),
        server: "ef".repeat(32),
        direction: direction.into(),
        state: "terminal".into(),
        cancel_requested: false,
        terminal: Some(marmish_stream::wire::Terminal::Exited { code: 7 }),
        child_disposition: None,
        open_digest: Some("12".repeat(32)),
        group_sha256: "34".repeat(32),
        active: false,
    }
}
#[test]
fn list_fallback_requires_unique_new_exact_cross_endpoint_binding() {
    let accounts = vec!["cd".repeat(32), "ef".repeat(32)];
    let a = entry("client", &"a".repeat(32));
    let s = entry("server", &a.stream);
    assert!(
        choose(
            &[],
            std::slice::from_ref(&a),
            std::slice::from_ref(&s),
            &accounts,
            None
        )
        .is_ok()
    );
    assert!(
        choose(
            std::slice::from_ref(&a),
            std::slice::from_ref(&a),
            std::slice::from_ref(&s),
            &accounts,
            None
        )
        .is_err()
    );
    let b = entry("client", &"b".repeat(32));
    let t = entry("server", &b.stream);
    assert!(matches!(
        choose(&[], &[a.clone(), b], &[s.clone(), t], &accounts, None),
        Err(Error::Invalid("list_ambiguous"))
    ));
    let mut wrong = s.clone();
    wrong.session = "ff".repeat(16);
    assert!(
        choose(
            &[],
            std::slice::from_ref(&a),
            &[wrong],
            &accounts,
            Some(&a.stream)
        )
        .is_err()
    );
    assert!(!bound(&a, &s, &accounts[..1]));
}
#[test]
fn observation_preserves_failure_evidence_without_private_identifiers() {
    let c = Capture {
        status: Some(ExitStatus::from_raw(7 << 8)),
        stdout: Pipe {
            bytes: vec![0, 255],
            ..Pipe::default()
        },
        stderr: Pipe {
            bytes: b"/private/profile secret diagnostic".to_vec(),
            ..Pipe::default()
        },
        cleanup: true,
        ..Capture::default()
    };
    let mut observation = Observation::captured("raw", b"input", &c, Instant::now());
    check_capture(&mut observation, &c, &"ab".repeat(32), &c.stderr.bytes).unwrap();
    let json = serde_json::to_string(&observation).unwrap();
    assert!(!json.contains("private/profile"));
    assert!(!json.contains("secret"));
    assert_eq!(observation.exit, Some(7));
    assert_eq!(observation.stdout_bytes, 2);
    assert_eq!(observation.failure.as_deref(), Some("hash_mismatch"));
    assert!(!observation.offsets_available);
}
pub(super) fn valid_pins() -> Pins {
    let file = FilePin {
        sha256: "ab".repeat(32),
        bytes: 1,
    };
    let artifact = |id, name: &str, bytes, sha256: &str| ArtifactPin {
        id,
        name: name.into(),
        bytes,
        sha256: sha256.into(),
    };
    Pins {
        schema: 1,
        kind: "macos-stream-diagnostic-pins".into(),
        qualification: false,
        repository: "Epiphytic/marmish".into(),
        candidate_run_id: RUN.into(),
        candidate_attempt: "1".into(),
        candidate_commit: CANDIDATE.into(),
        harness_commit: crate::native_manifest::compiled_release_metadata()
            .commit
            .into(),
        target: TARGET.into(),
        version: "0.2.3".into(),
        old_harness_sha256: "cd".repeat(32),
        artifacts: Artifacts {
            product: artifact(
                13570516008,
                "v02-candidate-aarch64-apple-darwin-42358105270-1",
                47988691,
                "f6c27bfab5b3dd2370eb38fa93f99f1c48ae906a7f04c6f45a59bf2bc758f970",
            ),
            tooling: artifact(
                13570605471,
                "v02-installed-tools-aarch64-apple-darwin-42358105270-1",
                9460623,
                "6cf4a04d8ee549bd86d9f7a256fefd7cfdcdb68eaccc189ba5c03b2c1315895c",
            ),
            bootstrap: artifact(
                13570810425,
                "v02-pinned-bootstrap-42358105270-1",
                3543,
                "974f86f238a8798ad2e565f300930070073cc5097748df07c30a9626c871d2ba",
            ),
        },
        manifest: file.clone(),
        tooling_record: file.clone(),
        bootstrap: file.clone(),
        fixture: file,
    }
}
#[test]
fn pins_are_closed_and_frozen_candidate_cannot_be_rebound() {
    let pins = valid_pins();
    pins.validate().unwrap();
    let mut wrong = pins.clone();
    wrong.candidate_commit = "ab".repeat(20);
    assert!(wrong.validate().is_err());
    let mut wrong = pins.clone();
    wrong.artifacts.product.bytes += 1;
    assert!(wrong.validate().is_err());
    let mut value = serde_json::to_value(&pins).unwrap();
    value["secret"] = "disallowed".into();
    assert!(serde_json::from_value::<Pins>(value).is_err());
}
#[test]
fn configured_server_is_single_client_fixture_only_without_command_count_limit() {
    let peer = "ab".repeat(32);
    let w = Path::new("/owned/work");
    let fixture = Path::new("/owned/work/fixture/main");
    for role in ["server", "client"] {
        let raw = serde_json::json!({"version":1,"role":role,"tor":"off","relays":[RELAY],"aliases":{},"command_policy":{"version":1},"execution_clients":if role=="server"{vec![peer.clone()]}else{vec![]},"trusted_remotes":if role=="client"{vec![peer.clone()]}else{vec![]},"cwd":"/","limits":{}});
        let bytes = configure_value(raw.clone(), role, &peer, w, fixture).unwrap();
        let config: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(config["cwd"], "/owned/work");
        assert_eq!(config["limits"]["max_uptime"], 1200);
        assert_eq!(config["limits"]["active_per_principal"], 4);
        assert!(config["limits"].get("max_commands").is_none());
        assert_eq!(
            config["command_policy"]["defaults"]["allow_exec"],
            if role == "server" {
                serde_json::json!([fixture])
            } else {
                serde_json::json!([])
            }
        );
        let mut extra = raw.clone();
        extra["execution_clients"] = serde_json::json!([peer, "cd".repeat(32)]);
        assert!(configure_value(extra, role, &peer, w, fixture).is_err());
    }
}
#[test]
fn stream_argv_uses_npub_and_preserves_six_literal_arguments() {
    let (root, _) = fixture();
    let socket = root.path().join("socket");
    let nonce = "ab".repeat(32);
    let account = "cd".repeat(32);
    let literals = streaming_fixture::arguments(&nonce, "macos-arm64").unwrap();
    assert_eq!(literals.len(), 6);
    assert_eq!(literals[4], "");
    assert!(literals[5].to_str().unwrap().contains('\n'));
    for compress in [false, true] {
        let command = stream_args(root.path(), &socket, &account, &nonce, compress).unwrap();
        let target = command.iter().position(|v| v == "stream").unwrap() + 1;
        records::assert_public_target(&command[target], &account);
        assert_eq!(&command[command.len() - 6..], literals.as_slice());
        assert_eq!(command.iter().any(|v| v == "--compress"), compress);
    }
    assert!(stream_args(root.path(), &socket, "not-an-account", &nonce, false).is_err());
}

pub(super) fn successful_capture(nonce: &str) -> Capture {
    Capture {
        status: Some(ExitStatus::from_raw(7 << 8)),
        stdout: Pipe {
            bytes: streaming_fixture::bytes(nonce, 43, streaming_fixture::STDOUT_BYTES).unwrap(),
            ..Pipe::default()
        },
        stderr: Pipe {
            bytes: streaming_fixture::bytes(nonce, 149, streaming_fixture::STDERR_BYTES).unwrap(),
            ..Pipe::default()
        },
        written: streaming_fixture::STDIN_BYTES,
        cleanup: true,
        ..Capture::default()
    }
}
fn blank_report() -> Report<'static> {
    Report {
        schema: 1,
        kind: "macos-stream-diagnostic",
        qualification: false,
        same_host_only: true,
        relay: RELAY,
        pins: None,
        probe_sha256: "ab".repeat(32),
        stage: "stream_capture".into(),
        failure: None,
        observations: vec![],
        cleanup: Finalized::default(),
        passed: false,
    }
}
#[test]
fn capture_classifies_finite_failures_and_prefix_without_losing_evidence() {
    let n = "ab".repeat(32);
    for (kind, expected) in [
        (0, "interrupted"),
        (1, "budget_exhausted"),
        (2, "cleanup_unconfirmed"),
        (3, "capture_overflow"),
        (4, "capture_io"),
        (5, "input_epipe"),
        (6, "child_signal"),
        (7, "child_nonzero"),
        (8, "hash_mismatch"),
    ] {
        let mut c = successful_capture(&n);
        match kind {
            0 => c.interrupted = true,
            1 => c.timed_out = true,
            2 => c.cleanup = false,
            3 => c.stdout.overflow = true,
            4 => c.input_error = true,
            5 => c.epipe = true,
            6 => c.status = Some(ExitStatus::from_raw(libc::SIGKILL)),
            7 => c.status = Some(ExitStatus::from_raw(23 << 8)),
            _ => c.stdout.bytes[0] ^= 1,
        }
        let mut o = Observation::captured("raw", b"", &c, Instant::now());
        check_capture(&mut o, &c, &n, &c.stderr.bytes).unwrap();
        assert_eq!(o.failure.as_deref(), Some(expected));
    }
    let case = StreamCase {
        nonce: n.clone(),
        case: "raw".into(),
        input: vec![],
        args: vec![],
    };
    let mut c = successful_capture(&n);
    let mut o = Observation::captured("raw", b"", &c, Instant::now());
    assert!(inspect_capture(&mut o, &case, &c).unwrap().is_none());
    assert_eq!(o.failure.as_deref(), Some("stream_id_absent"));
    c.stderr.bytes = [
        b"marmish stream: invalid\n".as_slice(),
        c.stderr.bytes.as_slice(),
    ]
    .concat();
    let mut o = Observation::captured("raw", b"", &c, Instant::now());
    inspect_capture(&mut o, &case, &c).unwrap();
    assert_eq!(o.failure.as_deref(), Some("stream_id_malformed"));
    c.stderr.bytes = [
        format!("marmish stream: {}\n", "a".repeat(32)).as_bytes(),
        successful_capture(&n).stderr.bytes.as_slice(),
    ]
    .concat();
    let mut o = Observation::captured("raw", b"", &c, Instant::now());
    assert_eq!(
        inspect_capture(&mut o, &case, &c).unwrap(),
        Some("a".repeat(32))
    );
    assert!(o.failure.is_none());
}
#[test]
fn journals_validate_terminal_and_refuse_concurrent_fallback() {
    let a = entry("client", &"a".repeat(32));
    let s = entry("server", &a.stream);
    let accounts = vec![a.client.clone(), a.server.clone()];
    let n = "ab".repeat(32);
    let capture = successful_capture(&n);
    for (terminal, active, cancel) in [
        (None, true, false),
        (
            Some(marmish_stream::wire::Terminal::Exited { code: 23 }),
            false,
            false,
        ),
        (a.terminal, false, true),
    ] {
        let mut changed = s.clone();
        changed.terminal = terminal;
        changed.active = active;
        changed.cancel_requested = cancel;
        let mut o = Observation::captured("raw", b"", &capture, Instant::now());
        inspect_journals(
            &mut o,
            &[],
            std::slice::from_ref(&a),
            &[changed],
            &accounts,
            Some(&a.stream),
            true,
        );
        assert!(o.binding_equal);
        assert_eq!(o.failure.as_deref(), Some("terminal_mismatch"));
    }
    let mut o = Observation::captured("raw", b"", &capture, Instant::now());
    inspect_journals(
        &mut o,
        &[],
        std::slice::from_ref(&a),
        std::slice::from_ref(&s),
        &accounts,
        None,
        false,
    );
    assert_eq!(o.failure.as_deref(), Some("list_ambiguous"));
    assert!(!o.binding_equal);
    let mut o = Observation::captured("raw", b"", &capture, Instant::now());
    inspect_journals(&mut o, &[], &[], &[], &accounts, Some(&a.stream), true);
    assert_eq!(o.failure.as_deref(), Some("list_absent"));
    let mut o = Observation::captured("raw", b"", &capture, Instant::now());
    inspect_journals(&mut o, &[], &[a], &[s], &accounts, None, true);
    assert!(o.binding_equal);
    assert!(o.failure.is_none());
}
#[tokio::test]
async fn finalize_orders_every_action_and_stops_both_after_failures() {
    struct Actions {
        calls: Vec<CleanupStep>,
        fail: CleanupStep,
    }
    impl CleanupActions for Actions {
        async fn perform(&mut self, step: CleanupStep) -> bool {
            self.calls.push(step);
            step != self.fail
        }
    }
    let order = vec![
        CleanupStep::ClientCancel,
        CleanupStep::ServerCancel,
        CleanupStep::ClientIdle,
        CleanupStep::ServerIdle,
        CleanupStep::ClientStop,
        CleanupStep::ServerStop,
    ];
    for fail in &order {
        let mut actions = Actions {
            calls: vec![],
            fail: *fail,
        };
        let result = finalize_actions(&mut actions).await;
        assert_eq!(actions.calls, order);
        assert!(!finalized(&result));
        assert_eq!(result.client_stop, *fail != CleanupStep::ClientStop);
        assert_eq!(result.server_stop, *fail != CleanupStep::ServerStop);
    }
}
#[test]
fn completion_requires_exact_counts_cleanup_and_no_late_signal() {
    for concurrent in [false, true] {
        for count in [0, 2, 3, 5, 6, 7] {
            let mut report = blank_report();
            report.cleanup = Finalized {
                client_cancel: true,
                server_cancel: true,
                client_idle: true,
                server_idle: true,
                client_stop: true,
                server_stop: true,
            };
            report.observations = (0..count)
                .map(|_| {
                    Observation::captured(
                        "raw",
                        b"",
                        &successful_capture(&"ab".repeat(32)),
                        Instant::now(),
                    )
                })
                .collect();
            finish_report(&mut report, &Ok(()), "stream_capture", concurrent, false);
            assert_eq!(report.passed, count == if concurrent { 6 } else { 3 });
            finish_report(&mut report, &Ok(()), "stream_capture", concurrent, true);
            assert!(!report.passed);
            assert_eq!(report.failure.as_deref(), Some("interrupted"));
        }
    }
    let mut report = blank_report();
    finish_report(&mut report, &Ok(()), "stream_capture", false, false);
    assert_eq!(report.failure.as_deref(), Some("cleanup_unconfirmed"));
    finish_report(
        &mut report,
        &Err(Error::Invalid("setup_failed")),
        "setup",
        false,
        false,
    );
    assert_eq!(report.failure.as_deref(), Some("setup_failed"));
}
#[test]
fn post_hardlink_error_always_publishes_failure_marker() {
    for name in ["report.json", "complete.json"] {
        let (root, _) = fixture();
        let mut report = blank_report();
        report.passed = true;
        let _fault =
            records::PublicationFault::install(name, records::PublicationFaultStage::AfterLinkErr);
        assert!(publish(root.path(), &report).is_err());
        assert!(root.path().join(name).exists());
        assert!(root.path().join("failure.json").exists());
    }
}
#[test]
fn receipt_missing_malformed_mismatched_and_exact_are_distinguished() {
    let (root, environment) = fixture();
    let pins = valid_pins();
    let n = "ab".repeat(32);
    directory(&root.path().join("fixture")).unwrap();
    directory(&root.path().join("fixture/receipts")).unwrap();
    let state = State {
        client: None,
        server: None,
        environment,
        accounts: vec![],
        fixture: root.path().join("fixture/main"),
    };
    let path = streaming_fixture::receipt_path(&state.fixture, &n).unwrap();
    let observation = || Observation::captured("raw", b"", &successful_capture(&n), Instant::now());
    let mut o = observation();
    check_receipt(&mut o, &state, &pins, &n).unwrap();
    assert_eq!(o.failure.as_deref(), Some("receipt_absent"));
    assert!(!o.receipt_present);
    write_private(&path, b"malformed", 0o600).unwrap();
    let mut o = observation();
    check_receipt(&mut o, &state, &pins, &n).unwrap();
    assert_eq!(o.failure.as_deref(), Some("receipt_malformed"));
    assert!(o.receipt_present);
    fs::remove_file(&path).unwrap();
    let expected =
        streaming_fixture::receipt(&state.fixture, &pins.fixture.sha256, &n, "macos-arm64")
            .unwrap();
    let mut wrong = expected.clone();
    wrong.stdin_bytes += 1;
    records::write(&path, &wrong).unwrap();
    let mut o = observation();
    check_receipt(&mut o, &state, &pins, &n).unwrap();
    assert_eq!(o.failure.as_deref(), Some("receipt_malformed"));
    fs::remove_file(&path).unwrap();
    records::write(&path, &expected).unwrap();
    let mut o = observation();
    check_receipt(&mut o, &state, &pins, &n).unwrap();
    assert!(o.receipt_present && o.receipt_valid);
    assert!(o.failure.is_none());
    assert!(!o.offsets_available);
}
