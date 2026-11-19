//! Fresh private example exec using a source-matched installed candidate layout.
use super::*;
use crate::{
    daemon_launch::candidate_parent, mdk_adapter::MdkOwner,
    peer_relay_trust::qualified_manual_recipient_fixture,
};
use std::{
    ffi::{OsString, c_int},
    fs::File,
    io::{Read, Seek},
    net::{Ipv4Addr, TcpListener},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, Stdio},
};

const PROFILE: &str = "test";
const EXAMPLE: &str = "MARMISH_TEST_REGISTERED_QUALIFICATION_EXAMPLE";
const DAEMON: &str = "MARMISH_TEST_REGISTERED_QUALIFICATION_DAEMON";
const STARTUP_MARKER: &str = "MARMISH_INTERNAL_STARTUP";
const LIMIT: Duration = Duration::from_secs(420);

struct OwnedChild(Child);

impl OwnedChild {
    async fn finished(&mut self) -> std::process::ExitStatus {
        timeout(WAIT, async {
            loop {
                if let Some(status) = self.0.try_wait().unwrap() {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("qualification child reaped before deadline")
    }

    async fn stop(&mut self) {
        assert!(self.0.try_wait().unwrap().is_none());
        let pid = c_int::try_from(self.0.id()).unwrap();
        // SAFETY: this is our still-owned child and its PID cannot be reused until reaped.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
        assert!(self.finished().await.success(), "graceful signal exit");
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn configure_off(lease: &ProfileLease, relay: &Relay, remote: PublicKey) {
    let config = json!({
        "version": 1, "role": "client", "relays": [relay.url.as_str()],
        "tor": "off", "cwd": null, "limits": {}, "aliases": {},
        "trusted_remotes": [remote.to_hex()], "execution_clients": [],
        "command_policy": {"version": 1}, "peer": {"receive":{"mode":"off"}}
    });
    write_private(
        &lease.root().join("config.json"),
        &serde_json::to_vec(&config).unwrap(),
    );
}

fn arguments(installed: &Path, root: &Path, relay: &Relay, ca: &Path) -> Vec<OsString> {
    [
        installed.as_os_str(),
        root.as_os_str(),
        PROFILE.as_ref(),
        relay.url.as_str().as_ref(),
        "off".as_ref(),
        ca.as_os_str(),
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

fn fresh_process(example: &Path, args: &[OsString], log: File) -> Command {
    let mut proc = Command::new(example);
    proc.args(args)
        .env_remove(STARTUP_MARKER)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    proc
}

fn inherit_unrelated_three(process: &mut Command, null: &File) {
    let fd = null.as_raw_fd();
    // SAFETY: the closure only uses raw async-signal-safe descriptor operations.
    unsafe {
        process.pre_exec(move || {
            if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn diagnostic(mut file: File) -> String {
    file.rewind().unwrap();
    let mut text = String::new();
    file.take(257).read_to_string(&mut text).unwrap();
    text
}

async fn wrong_marker_and_descriptor(
    example: &Path,
    args: &[OsString],
    root: &Path,
    socket: &Path,
) {
    for marker in ["marmish-daemon-v1", "marmish-registered-qualification-v1"] {
        let output = tempfile::tempfile_in(root).unwrap();
        let mut process = fresh_process(example, args, output.try_clone().unwrap());
        let null = File::open("/dev/null").unwrap();
        inherit_unrelated_three(&mut process, &null);
        let mut child = OwnedChild(process.env(STARTUP_MARKER, marker).spawn().unwrap());
        assert!(!child.finished().await.success());
        assert_eq!(
            diagnostic(output),
            "marmish qualification: invalid input: daemon_startup_context\n"
        );
        assert!(!socket.exists());
        assert!(ProfileLease::acquire_ready(root, PROFILE).is_ok());
    }
}

async fn wrong_fixture_route(example: &Path, args: &[OsString], root: &Path, socket: &Path) {
    let alternate = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    alternate.set_nonblocking(true).unwrap();
    let mut args = args.to_vec();
    args[3] = format!("wss://{}/", alternate.local_addr().unwrap()).into();
    let output = tempfile::tempfile_in(root).unwrap();
    let mut exec = fresh_process(example, &args, output.try_clone().unwrap());
    let mut child = OwnedChild(exec.spawn().unwrap());
    assert!(!child.finished().await.success());
    assert_eq!(
        diagnostic(output),
        "marmish qualification: invalid input: command_runtime_binding\n"
    );
    assert!(
        matches!(alternate.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    assert!(!socket.exists());
    assert!(ProfileLease::acquire_ready(root, PROFILE).is_ok());
}

async fn private_parent(
    example: PathBuf,
    args: Vec<OsString>,
    log: File,
) -> (OwnedChild, crate::Result<()>) {
    // The test's single-thread reactor must keep serving private WSS during R/A/C.
    tokio::task::spawn_blocking(move || {
        let launch = candidate_parent::launch(&example, &args, log).unwrap();
        (OwnedChild(launch.child), launch.report)
    })
    .await
    .unwrap()
}

async fn wait_active(child: &mut OwnedChild, socket: &Path) {
    timeout(WAIT, async {
        loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "qualification child stopped"
            );
            if active(socket).await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("active same-UID native socket");
}

async fn run_real_command(
    command: &CommandPeer,
    socket: &Path,
    relay: &Relay,
    signer: PublicKey,
) -> MdkOwner {
    let cat = std::fs::canonicalize("/bin/cat").unwrap();
    let mut server = command.open_fixture_server(relay, signer, &cat).await;
    let work = ordinary_command(socket, command.public(), relay, &cat);
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
    .expect("registered daemon finite command");
    server
}

async fn fresh_registered_startup() {
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
    let mut relay = Relay::start().await;
    let (mut peer_relay, root, lease, identity) = qualified_manual_recipient_fixture(
        command.public(),
        nostr::key::Keys::generate().public_key(),
        private_dir.path(),
        Some((command.public(), relay.url.as_str())),
    )
    .await;
    assert_eq!(peer_relay.qualification_connections(), 4);
    assert_ne!(relay.url, peer_relay.url);
    let signer = identity.public_key();
    relay.ordinary(signer);
    relay.ordinary(command.public());
    let socket = lease.socket_path();
    drop(lease);
    drop(identity);

    let ca = root.path().join("fixture-root.der");
    write_private(&ca, relay.qualification_ca());
    let args = arguments(&daemon, root.path(), &relay, &ca);

    wrong_marker_and_descriptor(&example, &args, root.path(), &socket).await;
    wrong_fixture_route(&example, &args, root.path(), &socket).await;
    let begin = Instant::now();
    let log = tempfile::tempfile_in(root.path()).unwrap();
    let (mut child, report) =
        private_parent(example.clone(), args.clone(), log.try_clone().unwrap()).await;
    if let Err(error) = report {
        let status = child.0.try_wait().unwrap();
        panic!(
            "private registered R/A/C report failed: {error}; child status {status:?}; {}",
            diagnostic(log).trim_end(),
        );
    }
    let startup = begin.elapsed();
    wait_active(&mut child, &socket).await;
    assert!(matches!(
        ProfileLease::acquire(root.path(), PROFILE),
        Err(crate::Error::Identity("profile_in_use"))
    ));
    use ipc::terminal_handshake::{Attach, Start, start};
    let terminal = Start {
        remote: command.public().to_hex(),
        session: "0123456789abcdef0123456789abcdef".into(),
        relays: vec![relay.url.as_str().into()],
        attach: Attach::Default,
        session_slot: None,
        offer: None,
        rows: 24,
        columns: 80,
    };
    assert!(matches!(
        start(&socket, terminal, Instant::now() + STEP).await,
        Err(crate::Error::Invalid("terminal_unavailable"))
    ));
    let began = Instant::now();
    let server = run_real_command(&command, &socket, &relay, signer).await;
    let command_time = began.elapsed();
    let remote = command.public();
    child.stop().await;
    assert!(!socket.exists());
    command.stop_fixture_server(server).await;
    assert!(relay.publication_attempts(445) > 0);
    assert_eq!(relay.publication_attempts(7435), 0);
    assert_eq!(peer_relay.publication_attempts(7435), 0);
    assert_eq!(peer_relay.recipient_requests(signer), 0);

    // Fresh no-marker example retains a deliberately unrelated inherited fd 3.
    let log = tempfile::tempfile_in(root.path()).unwrap();
    let mut foreground = fresh_process(&example, &args, log);
    let null = File::open("/dev/null").unwrap();
    inherit_unrelated_three(&mut foreground, &null);
    let mut foreground = OwnedChild(foreground.spawn().unwrap());
    wait_active(&mut foreground, &socket).await;
    assert_eq!(
        std::fs::read_link(format!("/proc/{}/fd/3", foreground.0.id())).unwrap(),
        Path::new("/dev/null")
    );
    foreground.stop().await;
    assert!(!socket.exists());

    // Independent ordinary owner can still reopen the existing Ready binding.
    let mut stock = ProfileLease::acquire_ready(root.path(), PROFILE).unwrap();
    let identity = material(&mut stock, &daemon);
    assert_eq!(identity.public_key(), signer);
    let reopened = MdkOwner::open(
        &stock,
        &identity,
        relay.connector(stock.root(), true, None),
        vec![],
    )
    .unwrap();
    stock
        .check_opened_binding(reopened.opened_binding())
        .unwrap();
    daemon::stop_silent(stock, reopened).await.unwrap();
    assert!(!socket.exists());
    let fallback = ProfileLease::acquire_ready(root.path(), PROFILE).unwrap();
    std::fs::remove_file(fallback.root().join("config.json")).unwrap();
    configure_off(&fallback, &relay, remote);
    drop(fallback);

    let log = tempfile::tempfile_in(root.path()).unwrap();
    let (mut refused, report) =
        private_parent(example.clone(), args.clone(), log.try_clone().unwrap()).await;
    assert!(
        report.is_err(),
        "stock fallback must never commit a registered report"
    );
    assert!(!refused.finished().await.success());
    assert_eq!(
        diagnostic(log),
        "marmish qualification: invalid input: candidate_startup_not_registered\n"
    );
    assert!(!socket.exists());
    assert!(ProfileLease::acquire_ready(root.path(), PROFILE).is_ok());

    let log = tempfile::tempfile_in(root.path()).unwrap();
    let mut fallback = OwnedChild(fresh_process(&example, &args, log).spawn().unwrap());
    wait_active(&mut fallback, &socket).await;
    fallback.stop().await;
    assert!(!socket.exists());
    assert!(ProfileLease::acquire_ready(root.path(), PROFILE).is_ok());
    peer_relay.stop().await;
    relay.stop().await;
    println!(
        "registered readiness ms={} finite command ms={}",
        startup.as_millis(),
        command_time.as_millis()
    );
}

#[test]
#[ignore = "requires an exact installed Linux AMD64 candidate and built private example"]
fn installed_registered_fresh_exec_and_command() {
    run_native(LIMIT, fresh_registered_startup);
}

#[path = "peer_daemon_passive_recipient_wss_tests.rs"]
mod passive_recipient;

#[path = "peer_daemon_registered_retained_tests.rs"]
mod retained;
