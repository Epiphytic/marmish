//! Actual private example exec with independent qualified Peer and Command WSS.
use super::*;
use crate::{ipc::peer_read, mdk_adapter::terminal_relay_fixture::SocketEnd};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::UnixStream,
};

#[path = "peer_daemon_manual_recipient_wss_tests.rs"]
mod manual;

const PROBE_CONNECTIONS: usize = 4;
const PROVISIONAL_HOLD: Duration = Duration::from_secs(2);

pub(crate) async fn eventually(label: &'static str, predicate: impl Fn() -> bool) {
    timeout(WAIT, async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect(label);
}

async fn provisional(
    example: &Path,
    args: &[OsString],
    state: &Path,
) -> (OwnedChild, UnixStream, Duration) {
    let log = tempfile::tempfile_in(state).unwrap();
    let start = Instant::now();
    let (child, channel) =
        candidate_parent::spawn_observed(example, args, log.try_clone().unwrap()).unwrap();
    let mut child = OwnedChild(child);
    channel.set_nonblocking(true).unwrap();
    let mut channel = UnixStream::from_std(channel).unwrap();
    let byte = timeout(WAIT, channel.read_u8()).await;
    assert!(
        matches!(byte, Ok(Ok(b'R'))),
        "expected provisional readiness, got {byte:?}, child {:?}; {}",
        child.0.try_wait().unwrap(),
        diagnostic(log).trim_end(),
    );
    (child, channel, start.elapsed())
}

fn no_recipient_dial(peer: &Relay, signer: PublicKey) {
    assert_eq!(peer.qualification_connections(), PROBE_CONNECTIONS);
    assert!(!peer.authenticated(signer));
    assert_eq!(peer.recipient_requests(signer), 0);
    assert_eq!(peer.all_publication_attempts(), 0);
}

pub(crate) async fn recipient_live(peer: &Relay, signer: PublicKey, round: usize) -> usize {
    let socket = PROBE_CONNECTIONS + round;
    eventually("authenticated recipient subscription", || {
        peer.recipient_requests(signer) >= round
    })
    .await;
    assert!(peer.authenticated(signer));
    assert_eq!(peer.recipient_requests(signer), round);
    assert_eq!(peer.qualification_connections(), socket);
    assert!(peer.runtime_socket_active(socket));
    socket
}

async fn manual_recipient_live(peer: &Relay, signer: PublicKey, socket: usize) {
    eventually("authenticated live and recovery requests", || {
        peer.recipient_runtime_requests(socket, signer)
            .is_some_and(|requests| requests.live >= 1 && requests.history >= 1)
    })
    .await;
    assert_manual_requests(peer, signer, socket);
    assert!(peer.authenticated(signer));
    assert!(peer.runtime_socket_active(socket));
}

fn assert_manual_requests(peer: &Relay, signer: PublicKey, socket: usize) {
    let requests = peer.recipient_runtime_requests(socket, signer).unwrap();
    assert_eq!(requests.live, 1);
    assert!(requests.history >= 1);
    assert_eq!(requests.unexpected, 0);
}

async fn unavailable(socket: &Path) {
    let reply = ipc::request(
        socket,
        &ipc::Request::PeerRead {
            request: peer_read::Request::List {},
        },
    )
    .await
    .expect("same-UID Peer read response");
    assert!(!reply.ok);
    assert!(reply.result.is_none());
    assert_eq!(reply.error.as_deref(), Some("peer_unavailable"));
}

async fn command_after_loss(
    command: &CommandPeer,
    server: &mut MdkOwner,
    socket: &Path,
    relay: &Relay,
) {
    let cat = std::fs::canonicalize("/bin/cat").unwrap();
    let work = ordinary_command_with_inventory(socket, command.public(), relay, &cat, 2);
    tokio::pin!(work);
    timeout(Duration::from_secs(55), async {
        loop {
            tokio::select! {
                () = &mut work => break,
                result = server.tick(Instant::now() + STEP) => result.unwrap(),
            }
        }
    })
    .await
    .expect("ordinary command after the Peer socket closed");
}

async fn fresh_idle_recipient(mode: &'static str) {
    let example =
        PathBuf::from(std::env::var_os(EXAMPLE).expect("supply the built private example"));
    assert_eq!(example.canonicalize().unwrap(), example);
    let daemon =
        PathBuf::from(std::env::var_os(DAEMON).expect("supply the installed candidate daemon"));
    let install = daemon.parent().unwrap().parent().unwrap().parent().unwrap();
    crate::native_install::read_receipt(install).expect("claimed installed candidate prefix");
    assert_eq!(
        layout().canonicalize().unwrap(),
        daemon,
        "run the controller from the installed version's deps directory"
    );
    let private_dir = private();
    let command = CommandPeer::new(private_dir.path(), "command");
    let mut ordinary = Relay::start().await;
    let (mut peer, root, lease, identity) = qualified_manual_recipient_fixture(
        command.public(),
        nostr::key::Keys::generate().public_key(),
        private_dir.path(),
        Some((command.public(), ordinary.url.as_str())),
    )
    .await;
    assert_ne!(ordinary.url, peer.url);
    let signer = identity.public_key();
    assert_ne!(signer, command.public());
    ordinary.ordinary(signer);
    ordinary.ordinary(command.public());
    let socket = lease.socket_path();
    let cursor = lease.root().join("peer/cursor.db");
    let ca = root.path().join("fixture-root.der");
    write_private(&ca, ordinary.qualification_ca());
    let mut args = arguments(&daemon, root.path(), &ordinary, &ca);
    args.insert(0, mode.into());
    drop((lease, identity));

    let (mut refused, report, _) = provisional(&example, &args, root.path()).await;
    no_recipient_dial(&peer, signer);
    assert!(!cursor.try_exists().unwrap());
    drop(report);
    assert!(!refused.finished().await.success());
    assert!(!socket.exists());
    assert!(ProfileLease::acquire_ready(root.path(), PROFILE).is_ok());
    no_recipient_dial(&peer, signer);
    assert!(!cursor.try_exists().unwrap());

    let (mut child, mut report, readiness) = provisional(&example, &args, root.path()).await;
    no_recipient_dial(&peer, signer);
    unavailable(&socket).await;
    assert!(matches!(
        ProfileLease::acquire(root.path(), PROFILE),
        Err(crate::Error::Identity("profile_in_use"))
    ));
    // Give the real relay reactor time to observe an incorrectly early dial
    // while the inherited channel physically cannot have received A.
    let held = Instant::now();
    tokio::time::sleep(PROVISIONAL_HOLD).await;
    no_recipient_dial(&peer, signer);
    assert!(!cursor.try_exists().unwrap());
    let hold_time = held.elapsed();
    let begin = Instant::now();
    timeout(STEP, report.write_all(b"A"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        timeout(STEP, report.read_u8()).await.unwrap().unwrap(),
        b'C'
    );
    drop(report);
    wait_active(&mut child, &socket).await;
    let manual = mode == "--manual-recipient";
    let background_peer = if manual {
        let socket = PROBE_CONNECTIONS + 1;
        manual_recipient_live(&peer, signer, socket).await;
        socket
    } else {
        recipient_live(&peer, signer, 1).await
    };
    assert!(std::fs::metadata(&cursor).unwrap().len() > 0);
    let peer_startup = begin.elapsed();
    unavailable(&socket).await;

    let begin = Instant::now();
    let mut server = run_real_command(&command, &socket, &ordinary, signer).await;
    let first_command = begin.elapsed();
    assert!(ordinary.publication_attempts(445) > 0);
    assert!(peer.runtime_socket_active(background_peer));
    unavailable(&socket).await;

    let requests_before_loss = peer.recipient_requests(signer);
    peer.retire_recipient_runtime_socket(background_peer, signer);
    eventually("relay-initiated recipient socket closed", || {
        peer.runtime_socket_end(background_peer).is_some()
    })
    .await;
    assert!(!peer.runtime_socket_active(background_peer));
    let ordinary_before = ordinary.publication_attempts(445);
    let begin = Instant::now();
    command_after_loss(&command, &mut server, &socket, &ordinary).await;
    let second_command = begin.elapsed();
    assert!(ordinary.publication_attempts(445) > ordinary_before);
    assert_eq!(peer.qualification_connections(), background_peer);
    assert_eq!(peer.recipient_requests(signer), requests_before_loss);
    wait_active(&mut child, &socket).await;
    unavailable(&socket).await;
    child.stop().await;
    assert!(!socket.exists());
    command.stop_fixture_server(server).await;

    let log = tempfile::tempfile_in(root.path()).unwrap();
    let mut foreground = fresh_process(&example, &args, log.try_clone().unwrap());
    let null = File::open("/dev/null").unwrap();
    inherit_unrelated_three(&mut foreground, &null);
    let mut foreground = OwnedChild(foreground.spawn().unwrap());
    wait_active(&mut foreground, &socket).await;
    let foreground_peer = if manual {
        let socket = PROBE_CONNECTIONS + 2;
        manual_recipient_live(&peer, signer, socket).await;
        socket
    } else {
        recipient_live(&peer, signer, 2).await
    };
    assert_eq!(
        std::fs::read_link(format!("/proc/{}/fd/3", foreground.0.id())).unwrap(),
        Path::new("/dev/null"),
        "{}",
        diagnostic(log).trim_end()
    );
    unavailable(&socket).await;
    let begin = Instant::now();
    foreground.stop().await;
    eventually("graceful stop drained the exact Peer socket", || {
        peer.runtime_socket_end(foreground_peer) == Some(SocketEnd::Raw)
    })
    .await;
    let stop = begin.elapsed();
    assert!(!peer.runtime_socket_active(foreground_peer));
    assert_eq!(peer.qualification_connections(), foreground_peer);
    if !manual {
        assert_eq!(peer.recipient_closes(signer), 0);
    }
    assert!(!socket.exists());

    let mut stock = ProfileLease::acquire_ready(root.path(), PROFILE).unwrap();
    let identity = material(&mut stock, &daemon);
    assert_eq!(identity.public_key(), signer);
    let owner = MdkOwner::open(
        &stock,
        &identity,
        ordinary.connector(stock.root(), true, None),
        vec![],
    )
    .unwrap();
    stock.check_opened_binding(owner.opened_binding()).unwrap();
    daemon::stop_silent(stock, owner).await.unwrap();
    assert!(!socket.exists());
    assert_eq!(ordinary.publication_attempts(7435), 0);
    assert_eq!(peer.all_publication_attempts(), 0);
    peer.stop().await;
    ordinary.stop().await;

    println!(
        "{mode} provisional_ready_ms={} hold_ms={} ack_to_peer_ms={} active_command_ms={} post_disconnect_command_ms={} foreground_stop_ms={}",
        readiness.as_millis(),
        hold_time.as_millis(),
        peer_startup.as_millis(),
        first_command.as_millis(),
        second_command.as_millis(),
        stop.as_millis()
    );
}

#[test]
#[ignore = "requires exact installed Linux AMD64 candidate and built private example"]
fn installed_registered_passive_recipient_and_command() {
    run_native(LIMIT, || {
        Box::pin(fresh_idle_recipient("--passive-recipient"))
    });
}

#[test]
#[ignore = "requires exact installed Linux AMD64 candidate and built private example"]
fn installed_registered_idle_manual_recipient_and_command() {
    run_native(LIMIT, || {
        Box::pin(fresh_idle_recipient("--manual-recipient"))
    });
}
