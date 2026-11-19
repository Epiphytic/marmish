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

fn manual_failure_log(log: &File) -> String {
    let mut bytes = vec![0; 64 * 1024];
    let size = log.read_at(&mut bytes, 0).unwrap();
    String::from_utf8_lossy(&bytes[..size])
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ManualCase {
    Pending,
    NearCap,
    Approve,
}

async fn expect_no_pending(socket: &Path, selection: peer_read::Selection) {
    use peer_read::{Failure, Reply, Request, client_request};
    assert_eq!(
        client_request(socket, Request::List {}).await,
        Ok(Reply::List {
            entries: Vec::new()
        }),
    );
    assert_eq!(
        client_request(socket, Request::Show { selection }).await,
        Err(Failure::Unavailable),
    );
}

async fn mounted(case: ManualCase) {
    let measure_near_cap = case == ManualCase::NearCap;
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
    let intent = if case == ManualCase::Approve {
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

    if case == ManualCase::Approve {
        use crate::ipc::peer_approve::{self, Failure};
        let wrong = peer_approve::Selection {
            generation: exact.selection.generation.checked_add(1).unwrap(),
            ..exact.selection
        };
        assert_eq!(
            peer_approve::client_request(&socket, wrong).await,
            Err(Failure::Stale),
        );
        use nostr::nips::nip19::ToBech32 as _;

        let clock = Instant::now();
        let cli = timeout(
            STEP,
            tokio::process::Command::new(daemon.with_file_name("marmish"))
                .kill_on_drop(true)
                .arg("--state-dir")
                .arg(root.path())
                .arg("--identity")
                .arg(PROFILE)
                .arg("--socket")
                .arg(&socket)
                .args(["servers", "approve", "--server"])
                .arg(exact.selection.server.to_bech32().unwrap())
                .arg("--offer-id")
                .arg(hex::encode(exact.selection.offer_id))
                .arg("--revision")
                .arg(hex::encode(exact.selection.revision))
                .arg("--generation")
                .arg(exact.selection.generation.to_string())
                .output(),
        )
        .await
        .expect("installed approval CLI exits")
        .expect("installed approval CLI starts");
        assert!(cli.status.success(), "{:?}", cli.status);
        assert_eq!(cli.stdout, b"recorded local approval\n");
        assert!(cli.stderr.is_empty());
        println!(
            "manual_approval_decision_ms {}",
            clock.elapsed().as_millis()
        );
        let clock = Instant::now();
        assert_eq!(
            peer_approve::client_request(&socket, exact.selection).await,
            Ok(()),
        );
        println!("manual_approval_retry_ms {}", clock.elapsed().as_millis());
        timeout(STEP, async {
            loop {
                match peer_read::client_request(&socket, peer_read::Request::List {}).await {
                    Ok(peer_read::Reply::List { entries }) if entries.is_empty() => break,
                    Ok(peer_read::Reply::List { entries }) => assert_eq!(entries, [exact]),
                    Err(peer_read::Failure::Unavailable) => {}
                    response => panic!("unexpected List response after decision: {response:?}"),
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("published read view stops showing the approved invitation");
        expect_no_pending(&socket, exact.selection).await;
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
    let mut cold_lease = ProfileLease::acquire_ready(root.path(), PROFILE).unwrap();
    let committed_approval = (case == ManualCase::Approve).then(|| {
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
    if case == ManualCase::Approve {
        use crate::ipc::peer_approve::{self, Failure};
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
        assert_manual_requests(&peer, signer, reopened_recipient);
        assert_eq!(peer.qualification_connections(), reopened_recipient);
        expect_no_pending(&socket, exact.selection).await;
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
