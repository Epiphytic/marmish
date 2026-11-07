use super::super::tests::{entry, fixture, successful_capture, valid_pins};
use super::*;
#[test]
fn profile_identity_role_and_old_argv_literals_are_separate() {
    let w = Path::new("/owned/work");
    for name in ["client", "client-1", "client-2", "client-3"] {
        let args = setup_args(
            w,
            ProfileSpec {
                identity_name: name,
                role: "client",
            },
            None,
        )
        .unwrap();
        assert_eq!(
            &args[args.len() - 4..],
            ["setup", "--role", "client", "--no-grants"]
        );
        assert_eq!(args[3], name);
    }
    assert!(
        setup_args(
            w,
            ProfileSpec {
                identity_name: "../escape",
                role: "client"
            },
            None
        )
        .is_err()
    );
    assert!(
        setup_args(
            w,
            ProfileSpec {
                identity_name: "client-1",
                role: "both"
            },
            None
        )
        .is_err()
    );
    let account = "ab".repeat(32);
    let nonce = "cd".repeat(32);
    let socket = Path::new("/owned/socket");
    let old = stream_args(w, socket, &account, &nonce, true).unwrap();
    let named =
        stream_args_named(w, socket, "client", &account, &nonce, "macos-arm64", true).unwrap();
    assert_eq!(old, named);
    let cold = stream_args_named(
        w,
        socket,
        "client-2",
        &account,
        &nonce,
        records::ROLES[1],
        true,
    )
    .unwrap();
    assert_eq!(cold[3], "client-2");
    assert!(
        cold.iter()
            .any(|a| a == &OsString::from(records::public_destination(&account).unwrap()))
    );
    assert_eq!(
        &cold[cold.len() - 6..],
        streaming_fixture::arguments(&nonce, records::ROLES[1]).unwrap()
    );
}
#[test]
fn grants_are_exact_sorted_and_only_server_can_execute() {
    let peers = vec!["11".repeat(32), "22".repeat(32), "33".repeat(32)];
    let raw = serde_json::json!({"version":1,"role":"server","tor":"off","relays":[RELAY],"aliases":{},"command_policy":{"version":1},"execution_clients":peers,"trusted_remotes":[],"cwd":"/","limits":{}});
    let args = setup_args(
        Path::new("/w"),
        ProfileSpec {
            identity_name: "server",
            role: "server",
        },
        Some(&peers),
    )
    .unwrap();
    assert_eq!(
        args.iter()
            .filter(|a| a == &&OsString::from("--grant"))
            .count(),
        3
    );
    let value: serde_json::Value = serde_json::from_slice(
        &configure_peers(
            raw.clone(),
            "server",
            &peers,
            Path::new("/w"),
            Path::new("/w/fixture/main"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["execution_clients"], serde_json::json!(peers));
    assert_eq!(
        value["limits"],
        serde_json::json!({"max_uptime":1200,"active_per_principal":4})
    );
    let mut unsorted = peers.clone();
    unsorted.reverse();
    assert!(
        configure_peers(
            raw.clone(),
            "server",
            &unsorted,
            Path::new("/w"),
            Path::new("/f")
        )
        .is_err()
    );
    let mut duplicate = peers.clone();
    duplicate[1] = duplicate[0].clone();
    assert!(
        configure_peers(
            raw.clone(),
            "server",
            &duplicate,
            Path::new("/w"),
            Path::new("/f")
        )
        .is_err()
    );
    let mut extra = raw;
    extra["execution_clients"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("44".repeat(32)));
    assert!(configure_peers(extra, "server", &peers, Path::new("/w"), Path::new("/f")).is_err());
}
#[test]
fn duplicate_principals_and_sockets_are_refused() {
    let mut accounts = (1..=4).map(|i| format!("{i:064x}")).collect::<Vec<_>>();
    let mut sockets = (1..=4)
        .map(|i| PathBuf::from(format!("/s/{i}")))
        .collect::<Vec<_>>();
    assert!(topology(&accounts, &sockets).is_ok());
    accounts[1] = accounts[0].clone();
    assert!(topology(&accounts, &sockets).is_err());
    accounts[1] = format!("{:064x}", 2);
    sockets[2] = sockets[0].clone();
    assert!(topology(&accounts, &sockets).is_err());
}
fn complete_report() -> Report<'static> {
    let nonce = "ab".repeat(32);
    let capture = successful_capture(&nonce);
    let observations = ["cold-1", "cold-2", "cold-3", "direct-after"]
        .map(|name| Observation::captured(name, &[], &capture, Instant::now()))
        .into();
    Report {
        schema: 1,
        kind: KIND,
        qualification: false,
        same_host_only: true,
        relay: RELAY,
        pins: None,
        probe_sha256: "ab".repeat(32),
        stage: "fixture_direct".into(),
        failure: None,
        scenario: "cold-three-principals",
        cold_before_direct: true,
        local_outbound_fixture_traffic: false,
        principals: (0..3)
            .map(|i| Principal {
                label: LABELS[i],
                role: records::ROLES[i],
                account_sha256: format!("{i:064x}"),
                session_sha256: Some(format!("{:064x}", i + 3)),
                group_sha256: Some(format!("{:064x}", i + 6)),
                open_sha256: Some(format!("{:064x}", i + 9)),
                pair_valid: true,
            })
            .collect(),
        observations,
        cleanup: Cleanup {
            clients: std::array::from_fn(|_| ServiceProof {
                cancel: true,
                idle: true,
                stop: true,
            }),
            server: ServiceProof {
                cancel: true,
                idle: true,
                stop: true,
            },
        },
        passed: false,
    }
}
#[test]
fn failed_cold_case_cannot_be_masked_by_direct_success_or_warm_order() {
    let mut r = complete_report();
    assert!(complete(&r, &Ok(()), false));
    r.observations[0].fail("input_epipe");
    assert!(!complete(&r, &Ok(()), false));
    r.observations[0].failure = None;
    r.observations.swap(0, 3);
    assert!(!complete(&r, &Ok(()), false));
    r.observations.swap(0, 3);
    r.cold_before_direct = false;
    assert!(!complete(&r, &Ok(()), false));
    r.cold_before_direct = true;
    assert!(!complete(&r, &Ok(()), true));
    assert!(!complete(&r, &Err(Error::Timeout), false));
    r.principals[1].group_sha256 = r.principals[0].group_sha256.clone();
    assert!(!complete(&r, &Ok(()), false));
}
#[test]
fn direct_after_requires_every_owned_capture_collected_clean_and_no_signal() {
    let mut captures = std::array::from_fn(|_| Capture {
        cleanup: true,
        ..Default::default()
    });
    let end = Instant::now() + Duration::from_secs(5);
    assert!(can_direct(&captures, false, end));
    captures[2].cleanup = false;
    assert!(!can_direct(&captures, false, end));
    captures[2].cleanup = true;
    assert!(!can_direct(&captures, true, end));
    assert!(!can_direct(&captures, false, Instant::now()));
}
#[test]
fn principal_fallback_is_exact_unique_and_retains_prefix_failure() {
    let (root, _) = fixture();
    let pins = valid_pins();
    let fixture_path = root.path().join("main");
    let nonce = "ab".repeat(32);
    let c = successful_capture(&nonce);
    let case = StreamCase {
        case: "cold-1".into(),
        nonce: nonce.clone(),
        input: streaming_fixture::bytes(&nonce, 7, streaming_fixture::STDIN_BYTES).unwrap(),
        args: vec![],
    };
    let a = entry("client", &"a".repeat(32));
    let s = entry("server", &a.stream);
    let accounts = [a.client.clone(), a.server.clone()];
    let mut principal = Principal::new(0, &a.client);
    let obs = inspect_cold(
        &pins,
        &fixture_path,
        &mut principal,
        Inspection {
            case: &case,
            capture: &c,
            before: &[],
            client: std::slice::from_ref(&a),
            server: std::slice::from_ref(&s),
            accounts: &accounts,
            start: Instant::now(),
            wall_ms: 1,
            capture_failed: false,
        },
    );
    assert!(obs.binding_equal);
    assert!(!obs.stream_present);
    assert_eq!(obs.failure.as_deref(), Some("stream_id_absent"));
    assert!(principal.session_sha256.is_some());
    assert!(!principal.pair_valid);
    let mut second = a.clone();
    second.stream = "b".repeat(32);
    let mut ss = s.clone();
    ss.stream = second.stream.clone();
    assert!(choose(&[], &[a, second], &[s, ss], &accounts, None).is_err());
}
#[tokio::test]
async fn parallel_cleanup_has_fixed_phase_budgets_and_no_starvation() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    };
    struct Fake {
        index: usize,
        events: Arc<Mutex<Vec<(usize, Phase, Instant)>>>,
        gate: Arc<tokio::sync::Barrier>,
        clock: Arc<AtomicU64>,
        start: Instant,
    }
    impl ServiceActions for Fake {
        async fn perform(&mut self, phase: Phase, deadline: Instant) -> bool {
            let elapsed = self.clock.load(Ordering::SeqCst);
            let remaining = deadline.duration_since(self.start).as_secs() - elapsed;
            assert_eq!(remaining, if phase == Phase::Cancel { 75 } else { 45 });
            self.events
                .lock()
                .unwrap()
                .push((self.index, phase, deadline));
            // All operations must have started before the hung member may return.
            self.gate.wait().await;
            if self.index == 0 {
                self.clock.store(
                    match phase {
                        Phase::Cancel => 75,
                        Phase::Idle => 120,
                        Phase::Stop => 165,
                    },
                    Ordering::SeqCst,
                );
            }
            self.gate.wait().await;
            !(self.index == 0 && matches!(phase, Phase::Cancel | Phase::Idle))
        }
    }
    let start = Instant::now();
    let events = Arc::new(Mutex::new(Vec::new()));
    let gate = Arc::new(tokio::sync::Barrier::new(4));
    let clock = Arc::new(AtomicU64::new(0));
    let mut actions = std::array::from_fn(|index| Fake {
        index,
        events: events.clone(),
        gate: gate.clone(),
        clock: clock.clone(),
        start,
    });
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        finalize_actions(&mut actions, start),
    )
    .await
    .unwrap();
    assert!(!result.complete());
    assert!(result.clients.iter().all(|p| p.stop));
    assert!(result.server.stop);
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 12);
    for (phase, seconds) in [(Phase::Cancel, 75), (Phase::Idle, 120), (Phase::Stop, 165)] {
        let rows = events
            .iter()
            .filter(|(_, p, _)| *p == phase)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 4);
        assert!(
            rows.iter()
                .all(|(_, _, end)| *end == start + Duration::from_secs(seconds))
        );
    }
    assert!(events[..4].iter().all(|(_, p, _)| *p == Phase::Cancel));
    assert!(events[4..8].iter().all(|(_, p, _)| *p == Phase::Idle));
}
#[test]
fn new_publication_preserves_failure_marker_after_second_write_and_post_link_error() {
    for post_link in [false, true] {
        let (root, _) = fixture();
        let mut report = complete_report();
        report.passed = true;
        let _fault = post_link.then(|| {
            records::PublicationFault::install(
                "complete.json",
                records::PublicationFaultStage::AfterLinkErr,
            )
        });
        if !post_link {
            write_private(&root.path().join("complete.json"), b"occupied", 0o600).unwrap();
        }
        assert!(
            publish_report(
                PublicationContext {
                    output: root.path(),
                    report_kind: KIND
                },
                &records::canonical(&report).unwrap(),
                true
            )
            .is_err()
        );
        assert!(root.path().join("report.json").exists());
        assert!(root.path().join("failure.json").exists());
    }
}

#[test]
fn inspection_io_keeps_all_captures_and_capture_failure_is_not_epipe() {
    let (root, _) = fixture();
    // A non-directory receipt parent forces real receipt-path I/O refusal.
    write_private(&root.path().join("receipts"), b"occupied", 0o600).unwrap();
    let pins = valid_pins();
    let n = "ab".repeat(32);
    let a = entry("client", &"a".repeat(32));
    let server = entry("server", &a.stream);
    let accounts = [a.client.clone(), a.server.clone()];
    let mut good = successful_capture(&n);
    good.stderr.bytes = [
        format!("marmish stream: {}\n", a.stream).as_bytes(),
        good.stderr.bytes.as_slice(),
    ]
    .concat();
    let observations: Vec<_> = (0..3)
        .map(|i| {
            let c = if i == 2 { &Capture::default() } else { &good };
            let case = StreamCase {
                case: format!("cold-{}", i + 1),
                nonce: n.clone(),
                input: streaming_fixture::bytes(&n, 7, streaming_fixture::STDIN_BYTES).unwrap(),
                args: vec![],
            };
            let mut principal = Principal::new(i, &a.client);
            let mut s = server.clone();
            if i == 1 {
                s.active = true;
            }
            inspect_cold(
                &pins,
                &root.path().join("main"),
                &mut principal,
                Inspection {
                    case: &case,
                    capture: c,
                    before: &[],
                    client: std::slice::from_ref(&a),
                    server: &[s],
                    accounts: &accounts,
                    start: Instant::now(),
                    wall_ms: 1,
                    capture_failed: i == 2,
                },
            )
        })
        .collect();
    assert_eq!(observations.len(), 3);
    assert_eq!(
        observations[0].failure.as_deref(),
        Some("receipt_malformed")
    );
    assert_eq!(observations[0].stage, "receipt");
    assert_eq!(
        observations[0].stdout_bytes,
        streaming_fixture::STDOUT_BYTES
    );
    assert_eq!(
        observations[1].failure.as_deref(),
        Some("terminal_mismatch")
    );
    assert_eq!(observations[1].stage, "client_journal");
    assert_eq!(observations[2].failure.as_deref(), Some("capture_io"));
    assert!(!observations[2].input_epipe);
    assert!(!observations[2].owned_group_clean);
}

#[test]
fn unconfirmed_attachment_retains_private_state_even_after_four_daemons_stop() {
    let mut report = complete_report();
    assert!(may_remove_private_state(
        &report.cleanup,
        &report.observations
    ));
    report.observations[1].owned_group_clean = false;
    assert!(!may_remove_private_state(
        &report.cleanup,
        &report.observations
    ));
    report.observations[1].owned_group_clean = true;
    report.observations[3].owned_group_clean = false;
    assert!(!may_remove_private_state(
        &report.cleanup,
        &report.observations
    ));
    // No attachment attempted during an early setup refusal needs no child proof.
    assert!(may_remove_private_state(&report.cleanup, &[]));
}
