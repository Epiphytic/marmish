//! Installed Manual data plane with an independent signed Peer founder.
use super::{
    super::retained::{
        FIRST, SECOND, assert_stock_rows, conclude_stock, launch_stock, quiet_command_relay,
    },
    *,
};
use crate::peer_relay_trust::qualified_both_command_fixture;
use nostr::key::Keys;
use std::sync::Arc;
use tokio::time::timeout_at;

#[path = "peer_daemon_manual_founder_fixture.rs"]
mod founder_fixture;
use founder_fixture::{Delivered, FounderFixture};

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
) -> peer_read::Entry {
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
    .expect("installed listener projects verified Pending");
    assert!(entry.selection.server == selector.server);
    assert!(
        entry.selection.offer_id == selector.id && entry.selection.revision == selector.revision
    );
    assert!(entry.selection.generation > 0);
    assert_eq!(entry.state, State::Pending);
    assert_eq!(entry.expires_at, delivered.pending_expires_at);
    assert!(entry.held_at < entry.expires_at);
    assert_eq!(
        client_request(
            socket,
            Request::Show {
                selection: entry.selection
            }
        )
        .await,
        Ok(Reply::Show { entry }),
    );
    let mut wrong = entry.selection.offer_id;
    wrong[0] ^= 1;
    let wrong = Selection {
        offer_id: wrong,
        ..entry.selection
    };
    assert_eq!(
        client_request(socket, Request::Show { selection: wrong }).await,
        Err(Failure::Unavailable),
    );
    entry
}

async fn mounted() {
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
    args.insert(0, "--manual-recipient".into());
    let (mut child, mut report, _) = provisional(&example, &args, root.path()).await;
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
                () = tokio::time::sleep(Duration::from_millis(100)) => unavailable(&socket).await,
            }
        }
    };
    unavailable(&socket).await;
    let delivered = founder.publish_delivery(&consented, deadline).await;
    let exact = pending(&socket, &offered, &delivered, deadline).await;
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
    pending(&socket, &offered, &delivered, Instant::now() + STEP).await;

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
    drop(ProfileLease::acquire_ready(root.path(), PROFILE).unwrap());

    // Reopen exactly the same profile with an independently started Manual
    // process before letting stock observe the ordinary retained results.
    let clock = Instant::now();
    let (mut reopened, mut report, _) = provisional(&example, &args, root.path()).await;
    assert_eq!(peer.qualification_connections(), recipient + 1);
    unavailable(&socket).await;
    acknowledge(&mut report).await;
    drop(report);
    wait_active(&mut reopened, &socket).await;
    let reopened_recipient = recipient + 2;
    manual_recipient_live(&peer, signer, reopened_recipient).await;
    assert_eq!(
        pending(&socket, &offered, &delivered, Instant::now() + WAIT).await,
        exact,
    );
    let cold_pending = clock.elapsed();
    assert_eq!(peer.all_publication_attempts(), 3);
    reopened.stop().await;
    assert!(!socket.exists());
    drop(ProfileLease::acquire_ready(root.path(), PROFILE).unwrap());

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
}

#[test]
#[ignore = "requires matching installed Linux AMD64 candidate and stock builds"]
fn installed_registered_manual_pending_and_command() {
    run_native(LIMIT, mounted);
}
