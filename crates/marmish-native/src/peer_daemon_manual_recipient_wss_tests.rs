//! Installed Manual data plane with an independent signed Peer founder.
use super::{
    super::retained::{
        FIRST, SECOND, assert_stock_rows, conclude_stock, launch_stock, quiet_command_relay,
    },
    *,
};
use crate::{peer_invitation::near_cap_fixture, peer_relay_trust::qualified_both_command_fixture};
use nostr::key::Keys;
use std::{os::unix::fs::FileExt as _, sync::Arc};
use tokio::time::timeout_at;

#[path = "peer_daemon_manual_founder_fixture.rs"]
mod founder_fixture;
use founder_fixture::{Delivered, FounderFixture};

#[path = "peer_daemon_manual_refresh_benchmark.rs"]
mod refresh_benchmark;
use refresh_benchmark::RefreshLog;

#[path = "peer_daemon_manual_copy_observation.rs"]
mod exact_copy_observation;

#[path = "peer_daemon_manual_approval_reply_loss.rs"]
mod approval_reply_loss;

#[path = "peer_daemon_manual_later_offer.rs"]
mod later_offer;

async fn acknowledge(report: &mut UnixStream) {
    timeout(STEP, report.write_all(b"A"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        timeout(STEP, report.read_u8()).await.unwrap().unwrap(),
        b'C'
    );
}

async fn pending(
    socket: &Path,
    offered: &founder_fixture::Offered,
    delivered: &Delivered,
    deadline: Instant,
) -> Option<peer_read::Entry> {
    use peer_read::{Failure, Reply, Request, Selection, State, client_request};
    let selector = offered.selector();
    let entry = timeout_at(deadline, async {
        loop {
            match client_request(socket, Request::List {}).await {
                Ok(Reply::List { entries }) if entries.is_empty() => {}
                Ok(Reply::List { entries }) => {
                    let [entry] = entries.as_slice() else {
                        panic!("one pending foundation");
                    };
                    break *entry;
                }
                Err(Failure::Unavailable) => {}
                _ => panic!("unexpected Pending List response"),
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .ok()?;
    assert!(entry.selection.server == selector.server);
    assert!(
        entry.selection.offer_id == selector.id && entry.selection.revision == selector.revision
    );
    assert!(entry.selection.generation > 0);
    assert_eq!(entry.state, State::Pending);
    assert_eq!(entry.expires_at, delivered.pending_expires_at);
    assert!(entry.held_at < entry.expires_at);
    assert!(
        client_request(
            socket,
            Request::Show {
                selection: entry.selection
            }
        )
        .await
            == Ok(Reply::Show { entry }),
        "exact Pending Show",
    );
    let mut wrong = entry.selection.offer_id;
    wrong[0] ^= 1;
    let wrong = Selection {
        offer_id: wrong,
        ..entry.selection
    };
    assert!(
        client_request(socket, Request::Show { selection: wrong }).await
            == Err(Failure::Unavailable),
        "unrelated Pending Show refused",
    );
    Some(entry)
}

fn diagnostic_log(log: &File) -> Vec<u8> {
    let size = usize::try_from(log.metadata().unwrap().len()).unwrap();
    assert!(
        size <= 512 * 1024,
        "Manual recipient diagnostics exceeded log budget"
    );
    let mut bytes = vec![0; size];
    log.read_exact_at(&mut bytes, 0).unwrap();
    bytes
}

fn manual_failure_log(log: &File) -> String {
    String::from_utf8_lossy(&diagnostic_log(log))
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| {
            value["event"] == "diagnostic"
                && value["stage"]
                    .as_str()
                    .is_some_and(|stage| stage.starts_with("manual_"))
        })
        .map(|value| format!("{}: {}", value["stage"], value["category"]))
        .collect::<Vec<_>>()
        .join(", ")
}

fn approval_timings(log: &File) -> [u64; 2] {
    let readings: Vec<_> = String::from_utf8_lossy(&diagnostic_log(log))
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value["event"] == "manual_approval_timing")
        .map(|value| {
            assert_eq!(value["schema"], 1);
            value["decision_ns"].as_u64().expect("decision duration")
        })
        .collect();
    readings.try_into().expect("fresh approval and exact retry")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ManualCase {
    Pending,
    NearCap,
    Approve,
    ApproveReplyLost,
    LaterOffer,
}

async fn expect_no_pending_proof(socket: &Path, selection: peer_read::Selection) {
    use peer_read::{Failure, Request, client_request};
    // The private reader publishes only verified Pending entries. The rotor
    // refuses its ticket when no Pending entry remains after a decision.
    assert_eq!(
        client_request(socket, Request::List {}).await,
        Err(Failure::Unavailable),
    );
    assert_eq!(
        client_request(socket, Request::Show { selection }).await,
        Err(Failure::Unavailable),
    );
}

async fn mounted(case: ManualCase) {
    let measure_near_cap = case == ManualCase::NearCap;
    let approve = matches!(case, ManualCase::Approve | ManualCase::ApproveReplyLost);
    let example = PathBuf::from(std::env::var_os(EXAMPLE).expect("private example"));
    assert_eq!(example.canonicalize().unwrap(), example);
    let daemon = PathBuf::from(std::env::var_os(DAEMON).expect("installed candidate daemon"));
    assert_eq!(layout().canonicalize().unwrap(), daemon);
    crate::native_install::read_receipt(daemon.ancestors().nth(3).unwrap())
        .expect("claimed installed candidate prefix");

    let parent = private();
    let sender_dir = private();
    let founder_dir = private();
    let founder_key = Arc::new(Keys::generate());
    let mut sender_lease = ProfileLease::acquire(sender_dir.path(), PROFILE).unwrap();
    let sender = material(&mut sender_lease, &daemon).public_key();
    let sender_socket = sender_lease.socket_path();
    let mut ordinary = Relay::start().await;
    let cat = std::fs::canonicalize("/bin/cat").unwrap();
    let (mut peer, root, lease, identity) = qualified_both_command_fixture(
        sender,
        founder_key.public_key(),
        Keys::generate().public_key(),
        parent.path(),
        ordinary.url.as_str(),
        &cat,
    )
    .await;
    assert_ne!(peer.url, ordinary.url);
    let signer = identity.public_key();
    ordinary.ordinary(sender);
    ordinary.ordinary(signer);
    configure_off(&sender_lease, &ordinary, signer);
    let socket = lease.socket_path();
    let cursor = lease.root().join("peer/cursor.db");
    drop((lease, identity, sender_lease));

    let ca = root.path().join("fixture-root.der");
    write_private(&ca, ordinary.qualification_ca());
    let mut args = arguments(&daemon, root.path(), &ordinary, &ca);
    let intent = if approve || case == ManualCase::LaterOffer {
        "--manual-approval-recipient"
    } else {
        "--manual-recipient"
    };
    args.insert(0, intent.into());
    args.insert(0, "--debug".into());
    let recipient_log = tempfile::tempfile_in(root.path()).unwrap();
    let (mut child, mut report, _) =
        provisional_with_log(&example, &args, recipient_log.try_clone().unwrap()).await;
    unavailable(&socket).await;
    tokio::time::sleep(PROVISIONAL_HOLD).await;
    no_recipient_dial(&peer, signer);
    assert!(!cursor.try_exists().unwrap());
    assert!(matches!(
        ProfileLease::acquire(root.path(), PROFILE),
        Err(crate::Error::Identity("profile_in_use"))
    ));
    let startup = Instant::now();
    acknowledge(&mut report).await;
    drop(report);
    wait_active(&mut child, &socket).await;
    let recipient = PROBE_CONNECTIONS + 1;
    manual_recipient_live(&peer, signer, recipient).await;
    assert_eq!(peer.qualification_connections(), recipient);
    let startup = startup.elapsed();
    unavailable(&socket).await;
    if measure_near_cap {
        RefreshLog::new(recipient_log.try_clone().unwrap())
            .report_empty(&socket)
            .await;
    }

    let sender_ca = sender_dir.path().join("fixture-root.der");
    write_private(&sender_ca, ordinary.qualification_ca());
    let sender_args = arguments(&daemon, sender_dir.path(), &ordinary, &sender_ca);
    let log = tempfile::tempfile_in(sender_dir.path()).unwrap();
    let mut sender_child = OwnedChild(fresh_process(&example, &sender_args, log).spawn().unwrap());
    wait_active(&mut sender_child, &sender_socket).await;

    let deadline = Instant::now() + Duration::from_secs(65);
    let mut founder =
        FounderFixture::open(&peer, founder_key, signer, founder_dir.path(), deadline).await;
    let turn = Instant::now();
    let offered = founder.publish_offer(deadline).await;
    unavailable(&socket).await;
    let consented = {
        let proof = founder.await_verified_consent(&offered, deadline);
        tokio::pin!(proof);
        loop {
            tokio::select! {
                result = &mut proof => break result,
                () = tokio::time::sleep(Duration::from_millis(100)) => {
                    assert!(
                        peer.runtime_socket_active(recipient),
                        "Manual route closed before Consent; {}",
                        manual_failure_log(&recipient_log),
                    );
                    unavailable(&socket).await;
                }
            }
        }
    };
    unavailable(&socket).await;
    let delivered = founder.publish_delivery(&consented, deadline).await;
    let exact = pending(&socket, &offered, &delivered, deadline)
        .await
        .expect("installed listener projects verified Pending");
    let offer_to_pending = turn.elapsed();
    for id in [offered.carrier(), consented.carrier(), delivered.carrier] {
        assert_eq!(peer.accepted_manual_control_count(id), 1);
    }
    assert_manual_requests(&peer, signer, recipient);
    assert_eq!(peer.qualification_connections(), recipient + 1);
    let argv = vec![cat.as_os_str().as_encoded_bytes().to_vec()];
    let clock = Instant::now();
    let first = ordinary_command_bytes(
        &sender_socket,
        signer,
        &ordinary,
        argv.clone(),
        FIRST,
        Some(1),
    )
    .await;
    let first_time = clock.elapsed();
    assert!(peer.runtime_socket_active(recipient));
    pending(&socket, &offered, &delivered, Instant::now() + STEP)
        .await
        .expect("installed listener still projects verified Pending");

    if case == ManualCase::LaterOffer {
        let follow_up = later_offer::FollowUp {
            root: root.path(),
            socket: &socket,
            example: &example,
            daemon: &daemon,
            args: &args,
            signer,
            peer: &peer,
            ordinary: &ordinary,
            sender_socket: &sender_socket,
        };
        follow_up
            .run(&mut child, &mut founder, &offered, &delivered, exact, argv)
            .await;
        sender_child.stop().await;
        drop(founder);
        peer.stop().await;
        ordinary.stop().await;
        return;
    }

    if approve {
        use crate::ipc::peer_approve::{self, Failure};
        let wrong = peer_approve::Selection {
            generation: exact.selection.generation.checked_add(1).unwrap(),
            ..exact.selection
        };
        assert_eq!(
            peer_approve::client_request(&socket, wrong).await,
            Err(Failure::Stale),
        );
        let clock = Instant::now();
        if case == ManualCase::ApproveReplyLost {
            let cli = approval_reply_loss::test_lost_reply(
                &daemon,
                root.path(),
                PROFILE,
                &socket,
                exact.selection,
            )
            .await;
            assert_eq!(cli.status.code(), Some(1));
            assert!(cli.stdout.is_empty());
            assert_eq!(cli.stderr, b"marmish: operation_uncertain\n");
            println!(
                "manual_approval_lost_reply_ms {}",
                clock.elapsed().as_millis()
            );
        } else {
            let cli = timeout(
                STEP,
                approval_reply_loss::approval_cli(
                    &daemon,
                    root.path(),
                    PROFILE,
                    &socket,
                    exact.selection,
                )
                .output(),
            )
            .await
            .expect("installed approval CLI exits")
            .expect("installed approval CLI starts");
            assert!(cli.status.success(), "{:?}", cli.status);
            assert_eq!(cli.stdout, b"recorded local approval\n");
            assert!(cli.stderr.is_empty());
            println!("manual_approval_cli_ms {}", clock.elapsed().as_millis());
            let clock = Instant::now();
            assert_eq!(
                peer_approve::client_request(&socket, exact.selection).await,
                Ok(()),
            );
            println!("manual_approval_retry_ms {}", clock.elapsed().as_millis());
        }
        expect_no_pending_proof(&socket, exact.selection).await;
        assert!(peer.runtime_socket_active(recipient));
        assert_manual_requests(&peer, signer, recipient);
        assert_eq!(peer.all_publication_attempts(), 3);
    }

    let requests_before_loss = peer.recipient_requests(signer);
    let clock = Instant::now();
    peer.retire_recipient_runtime_socket(recipient, signer);
    eventually("relay closed exact Manual runtime", || {
        peer.runtime_socket_end(recipient).is_some()
    })
    .await;
    timeout(STEP, async {
        loop {
            use peer_read::{Failure, Reply, Request, client_request};
            match client_request(&socket, Request::List {}).await {
                Err(Failure::Unavailable) => break,
                Ok(Reply::List { entries }) => assert!(entries.len() <= 1),
                _ => panic!("unexpected read while closing Manual route"),
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Manual reader rejects after shutdown");
    let lost = clock.elapsed();
    let clock = Instant::now();
    let second =
        ordinary_command_bytes(&sender_socket, signer, &ordinary, argv, SECOND, Some(2)).await;
    let second_time = clock.elapsed();
    assert_ne!(first, second);
    unavailable(&socket).await;
    wait_active(&mut child, &socket).await;
    assert_eq!(peer.qualification_connections(), recipient + 1);
    assert_eq!(peer.recipient_requests(signer), requests_before_loss);
    assert_eq!(peer.all_publication_attempts(), 3);
    assert_eq!(ordinary.publication_attempts(7435), 0);

    quiet_command_relay(&ordinary).await;
    let clock = Instant::now();
    child.stop().await;
    let stopped = clock.elapsed();
    assert!(!socket.exists());
    if case == ManualCase::Approve {
        let [decision, retry] = approval_timings(&recipient_log);
        println!("manual_approval_native_decision_ns {decision}");
        println!("manual_approval_native_retry_ns {retry}");
    }
    let mut cold_lease = ProfileLease::acquire_ready(root.path(), PROFILE).unwrap();
    let committed_approval = approve.then(|| {
        let identity = material(&mut cold_lease, &daemon);
        near_cap_fixture::committed_approval(&cold_lease, &identity, peer.url.as_str(), &exact)
            .expect("exact Activating row, monotonic floors and retained keeper")
    });
    let near_cap = measure_near_cap.then(|| {
        let cold_identity = material(&mut cold_lease, &daemon);
        assert!(
            cold_identity.public_key() == signer,
            "same prepared identity"
        );
        let mut wrong = exact;
        wrong.selection.offer_id[0] ^= 1;
        assert!(matches!(
            near_cap_fixture::grow(&cold_lease, &cold_identity, peer.url.as_str(), &wrong),
            Err("pending_selection")
        ));
        let stage = near_cap_fixture::grow(&cold_lease, &cold_identity, peer.url.as_str(), &exact)
            .expect("near-cap SQLCipher fixture prepared from witnessed Pending");
        println!(
            "manual_near_cap_original {}",
            json!({
                "logical_bytes": stage.logical_bytes(),
                "allocated_bytes": stage.allocated_bytes(),
                "page_count": stage.page_count(),
                "page_size": stage.page_size(),
                "freelist_pages": stage.freelist_pages(),
            }),
        );
        stage
    });
    drop(cold_lease);

    // Reopen exactly the same profile with an independently started Manual
    // process before letting stock observe the ordinary retained results.
    let clock = Instant::now();
    let cold_log = tempfile::tempfile_in(root.path()).unwrap();
    let (mut reopened, mut report, _) =
        provisional_with_log(&example, &args, cold_log.try_clone().unwrap()).await;
    assert_eq!(peer.qualification_connections(), recipient + 1);
    unavailable(&socket).await;
    acknowledge(&mut report).await;
    drop(report);
    wait_active(&mut reopened, &socket).await;
    let reopened_recipient = recipient + 2;
    manual_recipient_live(&peer, signer, reopened_recipient).await;
    if approve {
        use crate::ipc::peer_approve::{self, Failure};
        if case == ManualCase::ApproveReplyLost {
            let clock = Instant::now();
            let cli = approval_reply_loss::retry_approval_cli(
                &daemon,
                root.path(),
                PROFILE,
                &socket,
                exact.selection,
                WAIT,
            )
            .await;
            assert_eq!(cli.status.code(), Some(0));
            assert_eq!(cli.stdout, b"recorded local approval\n");
            assert!(cli.stderr.is_empty());
            println!(
                "manual_approval_lost_reply_retry_ms {}",
                clock.elapsed().as_millis()
            );
        } else {
            let retry = timeout(WAIT, async {
                loop {
                    match peer_approve::client_request(&socket, exact.selection).await {
                        Err(Failure::Busy | Failure::Unavailable) => {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                        result => break result,
                    }
                }
            })
            .await
            .expect("reopened candidate replies to the original selection");
            assert_eq!(retry, Ok(()), "read-only exact approval retry after reopen");
        }
        assert_manual_requests(&peer, signer, reopened_recipient);
        assert_eq!(peer.qualification_connections(), reopened_recipient);
        expect_no_pending_proof(&socket, exact.selection).await;
        assert!(peer.runtime_socket_active(reopened_recipient));
    } else {
        let cold_entry = pending(&socket, &offered, &delivered, Instant::now() + WAIT).await;
        if measure_near_cap && cold_entry.is_none() {
            RefreshLog::new(cold_log.try_clone().unwrap()).report_cold_failure();
        }
        assert!(cold_entry == Some(exact), "exact cold Pending projection");
    }
    let cold_pending = clock.elapsed();
    let mut qualification_passed = true;
    if let Some(stage) = &near_cap {
        qualification_passed = RefreshLog::new(cold_log).qualify(&socket, exact).await;
        if let Err(reason) = exact_copy_observation::measure(
            cursor.parent().unwrap(),
            stage.identity(),
            stage.logical_bytes(),
            stage.allocated_bytes(),
        )
        .await
        {
            println!("manual_near_cap_copy_refused {reason}");
            qualification_passed = false;
        }
    }
    assert_eq!(peer.all_publication_attempts(), 3);
    reopened.stop().await;
    assert!(!socket.exists());
    let mut cold_lease = ProfileLease::acquire_ready(root.path(), PROFILE).unwrap();
    if let Some(committed) = committed_approval {
        let identity = material(&mut cold_lease, &daemon);
        let reopened =
            near_cap_fixture::committed_approval(&cold_lease, &identity, peer.url.as_str(), &exact)
                .expect("exact Activating state and keeper still retained");
        assert!(reopened.same_decision(&committed));
    }

    if let Some(stage) = near_cap {
        let cold_identity = material(&mut cold_lease, &daemon);
        stage
            .assert_unchanged(&cold_lease, &cold_identity)
            .expect("same closed encrypted original after installed cold validation");
    }
    drop(cold_lease);

    let clock = Instant::now();
    let mut stock = launch_stock(&daemon, root.path(), &ordinary, &ca);
    wait_active(&mut stock, &socket).await;
    assert_stock_rows(&socket, sender, [first, second]).await;
    let cold = clock.elapsed();
    conclude_stock(&mut stock).await;
    assert!(!socket.exists());
    drop(ProfileLease::acquire_ready(root.path(), PROFILE).unwrap());
    sender_child.stop().await;
    drop(founder);
    peer.stop().await;
    ordinary.stop().await;
    println!(
        "manual installed pending=1 command_rows=2 startup_ms={} offer_to_pending_ms={} active_command_ms={} lost_to_unavailable_ms={} post_loss_command_ms={} stop_ms={} cold_pending_ms={} cold_stock_ms={}",
        startup.as_millis(),
        offer_to_pending.as_millis(),
        first_time.as_millis(),
        lost.as_millis(),
        second_time.as_millis(),
        stopped.as_millis(),
        cold_pending.as_millis(),
        cold.as_millis(),
    );
    assert!(qualification_passed, "near-cap refresh or IPC gate failed");
}

#[test]
#[ignore = "requires matching installed Linux AMD64 candidate and stock builds"]
fn installed_registered_manual_pending_and_command() {
    run_native(LIMIT, || mounted(ManualCase::Pending));
}

#[test]
#[ignore = "opt-in near-cap benchmark with matching installed Linux AMD64 candidate and stock builds"]
fn installed_registered_manual_near_cap_refresh() {
    run_native(LIMIT, || mounted(ManualCase::NearCap));
}

#[test]
#[ignore = "opt-in local approval over matching installed Linux AMD64 packages and private WSS"]
fn installed_registered_manual_approval_and_command() {
    run_native(LIMIT, || mounted(ManualCase::Approve));
}

#[test]
#[ignore = "opt-in later Offer recovery with matching installed Linux AMD64 packages and private WSS"]
fn installed_registered_manual_later_offer_preserves_pending() {
    run_native(LIMIT, || mounted(ManualCase::LaterOffer));
}

struct ManualSchemaStartup<'a> {
    example: &'a Path,
    root: &'a Path,
    socket: &'a Path,
    peer: &'a Relay,
    signer: PublicKey,
}

impl ManualSchemaStartup<'_> {
    async fn matching(&self, args: &[OsString]) {
        let recipient = self.peer.qualification_connections() + 1;
        let (mut child, mut report, _) = provisional(self.example, args, self.root).await;
        acknowledge(&mut report).await;
        drop(report);
        wait_active(&mut child, self.socket).await;
        manual_recipient_live(self.peer, self.signer, recipient).await;
        assert_eq!(self.peer.qualification_connections(), recipient);
        child.stop().await;
        assert!(!self.socket.exists());
    }

    async fn refused(&self, args: &[OsString], selection: crate::ipc::peer_approve::Selection) {
        use crate::ipc::peer_approve::{self, Failure};

        let connections = self.peer.qualification_connections();
        let requests = self.peer.recipient_requests(self.signer);
        let log = tempfile::tempfile_in(self.root).unwrap();
        let (mut child, mut report, _) =
            provisional_with_log(self.example, args, log.try_clone().unwrap()).await;
        let task = recipient_task(child.0.id()).await;
        acknowledge(&mut report).await;
        drop(report);
        wait_active(&mut child, self.socket).await;
        eventually(
            "mismatched Manual worker refused before ordinary stop",
            || !task.try_exists().unwrap(),
        )
        .await;
        assert!(active(self.socket).await);
        unavailable(self.socket).await;
        assert!(
            matches!(
                peer_approve::client_request(self.socket, selection).await,
                Err(Failure::Unavailable | Failure::Uncertain)
            ),
            "the stopped Peer worker cannot acknowledge approval",
        );
        child.stop().await;
        assert!(!self.socket.exists());
        drop(ProfileLease::acquire_ready(self.root, PROFILE).unwrap());

        // Matching-mode startups and unchanged bytes establish the schema
        // boundary. This diagnostic confirms the point of local refusal.
        assert!(
            String::from_utf8_lossy(&diagnostic_log(&log))
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .any(|line| line["stage"] == "registered_recipient"
                    && line["category"] == "local_unavailable"),
            "expected local recipient refusal",
        );
        assert_eq!(self.peer.qualification_connections(), connections);
        assert_eq!(self.peer.recipient_requests(self.signer), requests);
        assert_eq!(self.peer.all_publication_attempts(), 0);
    }
}

async fn recipient_task(pid: u32) -> PathBuf {
    let tasks = PathBuf::from(format!("/proc/{pid}/task"));
    timeout(STEP, async {
        loop {
            for entry in std::fs::read_dir(&tasks).unwrap() {
                let Ok(entry) = entry else {
                    continue;
                };
                let task = entry.path();
                // Linux task comm truncates the installed worker name to 15 bytes.
                if std::fs::read(task.join("comm")).is_ok_and(|name| name == b"marmish-peer-re\n") {
                    return task;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned Manual worker waits for startup acknowledgement")
}

async fn incompatible_manual_schemas() {
    let example = PathBuf::from(std::env::var_os(EXAMPLE).expect("private example"));
    assert_eq!(example.canonicalize().unwrap(), example);
    let daemon = PathBuf::from(std::env::var_os(DAEMON).expect("installed candidate daemon"));
    assert_eq!(layout().canonicalize().unwrap(), daemon);
    crate::native_install::read_receipt(daemon.ancestors().nth(3).unwrap())
        .expect("claimed installed candidate prefix");

    for (matching, opposite) in [
        ("--manual-recipient", "--manual-approval-recipient"),
        ("--manual-approval-recipient", "--manual-recipient"),
    ] {
        reopen_manual_schema(&example, &daemon, matching, opposite).await;
    }
}

async fn reopen_manual_schema(example: &Path, daemon: &Path, matching: &str, opposite: &str) {
    use crate::ipc::peer_approve::Selection;

    let parent = private();
    let command = Keys::generate().public_key();
    let founder = Keys::generate().public_key();
    let mut ordinary = Relay::start().await;
    let (mut peer, root, lease, identity) = qualified_manual_recipient_fixture(
        founder,
        Keys::generate().public_key(),
        parent.path(),
        Some((command, ordinary.url.as_str())),
    )
    .await;
    assert_ne!(ordinary.url, peer.url);
    let signer = identity.public_key();
    ordinary.ordinary(signer);
    ordinary.ordinary(command);
    let socket = lease.socket_path();
    let cursor = lease.root().join("peer/cursor.db");
    drop((lease, identity));

    let ca = root.path().join("fixture-root.der");
    write_private(&ca, ordinary.qualification_ca());
    let args = arguments(daemon, root.path(), &ordinary, &ca);
    let startup = ManualSchemaStartup {
        example,
        root: root.path(),
        socket: &socket,
        peer: &peer,
        signer,
    };
    let mut matching_args = args.clone();
    matching_args.insert(0, matching.into());
    startup.matching(&matching_args).await;
    let cursor_bytes = std::fs::read(&cursor).unwrap();
    assert!(!cursor_bytes.is_empty());

    let mut opposite_args = args;
    opposite_args.insert(0, opposite.into());
    opposite_args.insert(0, "--debug".into());
    startup
        .refused(
            &opposite_args,
            Selection {
                server: founder,
                offer_id: [1; 16],
                revision: [2; 32],
                generation: 1,
            },
        )
        .await;
    assert!(
        std::fs::read(&cursor).unwrap() == cursor_bytes,
        "{opposite} changed the cursor created by {matching}",
    );
    startup.matching(&matching_args).await;
    assert_eq!(peer.all_publication_attempts(), 0);
    peer.stop().await;
    ordinary.stop().await;
}

#[test]
#[ignore = "opt-in Manual schema isolation over matching installed Linux AMD64 packages and private WSS"]
fn installed_registered_manual_rejects_incompatible_schema() {
    run_native(LIMIT, incompatible_manual_schemas);
}

#[test]
#[ignore = "opt-in committed local reply loss with matching installed packages and private WSS"]
fn installed_registered_manual_approval_lost_reply() {
    run_native(LIMIT, || mounted(ManualCase::ApproveReplyLost));
}
