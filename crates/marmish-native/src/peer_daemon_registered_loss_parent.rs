//! Controller for isolated faults in the real registered libtest subprocess.
use super::{Image, PROFILE, Scenario, UnixStream, send};
use crate::{
    daemon_launch::{
        STARTUP_TIMEOUT,
        candidate_parent::{self, ReportEdge},
    },
    identity::ProfileLease,
    ipc::{self, Request},
    mdk_adapter::terminal_relay_fixture::Relay,
};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions, Permissions},
    io::{self, Read, Seek, Write},
    os::{
        fd::OwnedFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::{Child, ExitStatus},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::UnixStream as AsyncUnixStream,
    time::{Instant, sleep, timeout_at},
};

const DAEMON: &str = "MARMISH_TEST_REGISTERED_QUALIFICATION_DAEMON";
const STATUS: Duration = Duration::from_secs(15);
const STEP: Duration = Duration::from_secs(2);

struct OwnedChild(Option<Child>);

impl OwnedChild {
    fn pid(&self) -> libc::pid_t {
        self.0.as_ref().unwrap().id().try_into().unwrap()
    }

    async fn reap(&mut self) -> ExitStatus {
        timeout_at(Instant::now() + STEP, async {
            loop {
                if let Some(status) = self.0.as_mut().unwrap().try_wait().unwrap() {
                    self.0.take();
                    return status;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("bounded fatal reap")
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let pid = libc::pid_t::try_from(child.id()).unwrap();
            // SAFETY: this unreaped borrowed PID belongs to our private child.
            // The negative PID is used only when it is the session leader's group.
            unsafe {
                if libc::getpgid(pid) == pid {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn boot(scenario: Scenario, fields: [&OsStr; 4]) -> Vec<u8> {
    let mut body = vec![scenario as u8];
    for field in fields {
        let bytes = field.as_encoded_bytes();
        assert!(!bytes.is_empty() && bytes.len() <= 1024 && !bytes.contains(&0));
        body.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        body.extend_from_slice(bytes);
    }
    body
}

async fn frame(control: &mut AsyncUnixStream, tag: u8, cap: usize, deadline: Instant) -> Vec<u8> {
    timeout_at(deadline, async {
        let mut header = [0; 5];
        control
            .read_exact(&mut header)
            .await
            .expect("controller header");
        assert_eq!(header[0], tag, "exact controller frame ordering");
        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
        assert!(len <= cap, "controller frame bound");
        let mut body = vec![0; len];
        control
            .read_exact(&mut body)
            .await
            .expect("controller payload");
        body
    })
    .await
    .expect("bounded controller frame")
}

async fn wait_active(socket: &Path) {
    timeout_at(Instant::now() + STATUS, async {
        loop {
            if ipc::request(socket, &Request::Status {})
                .await
                .is_ok_and(|response| {
                    response.ok
                        && response
                            .result
                            .is_some_and(|result| result["state"] == "active")
                })
            {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("first active Status response");
}

async fn no_response(stream: &mut AsyncUnixStream, deadline: Instant) {
    let mut one = [0; 1];
    match timeout_at(deadline, stream.read(&mut one)).await {
        Ok(Ok(0)) => {}
        Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionReset => {}
        _ => panic!("expected EOF/reset with zero response bytes"),
    }
}

async fn fresh_refused(socket: &Path, deadline: Instant) {
    match timeout_at(deadline, AsyncUnixStream::connect(socket)).await {
        Ok(Err(error))
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) => {}
        Ok(Ok(mut stream)) => {
            let _ = timeout_at(deadline, ipc::send_fault_fixture_status(&mut stream)).await;
            no_response(&mut stream, deadline).await;
        }
        _ => panic!("fresh Status connection remains live"),
    }
}

fn assert_image(root: &Path, expected: &[u8]) {
    let actual = Image::capture(root).expect("bounded private file image");
    assert!(
        actual.bytes() == expected,
        "private authoritative file image changed"
    );
    let jobs = root.join("commands").join("jobs");
    assert!(
        fs::read_dir(jobs)
            .expect("engine jobs directory")
            .next()
            .is_none(),
        "engine job/spool unexpectedly present"
    );
}

fn checked_daemon(exe: &Path) -> PathBuf {
    let daemon =
        PathBuf::from(std::env::var_os(DAEMON).expect("supply installed candidate daemon"));
    let version = exe.parent().unwrap().parent().unwrap();
    assert_eq!(version.join("marmishd").canonicalize().unwrap(), daemon);
    crate::native_layout::resolve(&daemon).expect("candidate installed helper");
    daemon
}

fn log_has_fatal_stop(mut log: File) {
    log.rewind().unwrap();
    let mut output = String::new();
    log.take(128 * 1024).read_to_string(&mut output).unwrap();
    assert!(
        output
            .lines()
            .any(|line| serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .is_some_and(|entry| entry["stage"] == "registered_shutdown"
                    && entry["category"] == "owned_stop_failed")),
        "actual fatal owner category"
    );
}

const CHILD_SELECTOR: &str = super::SELECTOR;

async fn scenario(case: Scenario) {
    let started = Instant::now();
    let private = tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let daemon = checked_daemon(&executable);
    let ordinary = Relay::start().await;
    let principal = nostr::key::Keys::generate().public_key();
    let cat = Path::new("/bin/cat").canonicalize().unwrap();
    let (peer, state, lease, material) = crate::peer_relay_trust::qualified_both_command_fixture(
        principal,
        nostr::key::Keys::generate().public_key(),
        private.path(),
        ordinary.url.as_str(),
        &cat,
    )
    .await;
    assert_eq!(peer.qualification_connections(), 4);
    assert_ne!(peer.url, ordinary.url);
    assert!(peer.qualification_ca() != ordinary.qualification_ca());
    let account = material.public_key();
    ordinary.ordinary(account);
    assert!(lease.root().join("peer-relay-qualification.json").is_file());
    let socket = lease.socket_path();
    let root = lease.root().to_owned();
    drop(material);
    drop(lease);
    let ca = state.path().join("fixture-root.der");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&ca)
        .unwrap()
        .write_all(ordinary.qualification_ca())
        .unwrap();
    peer.record_dispatcher_connections().await;
    ordinary.record_dispatcher_connections().await;
    let (mut controller, controlled) = UnixStream::pair().unwrap();
    let log = tempfile::tempfile_in(private.path()).unwrap();
    let args = ["--ignored", "--exact", "--nocapture", CHILD_SELECTOR].map(OsString::from);
    let (child, report) = candidate_parent::spawn_fault_fixture(
        &executable,
        &args,
        log.try_clone().unwrap(),
        OwnedFd::from(controlled),
    )
    .unwrap();
    let mut child = OwnedChild(Some(child));
    let pid = child.pid();
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    controller.set_write_timeout(Some(super::BOOT)).unwrap();
    send(
        &mut controller,
        b'B',
        &boot(
            case,
            [
                daemon.as_os_str(),
                state.path().as_os_str(),
                OsStr::new(ordinary.url.as_str()),
                ca.as_os_str(),
            ],
        ),
    )
    .unwrap();
    controller.set_nonblocking(true).unwrap();
    let mut controller = AsyncUnixStream::from_std(controller).unwrap();
    let report = tokio::task::spawn_blocking(move || {
        let mut edges = Vec::with_capacity(3);
        let result = candidate_parent::observe_fault_report(report, deadline.into_std(), |edge| {
            edges.push(edge)
        });
        (result, edges)
    });

    let (result, edges) = report.await.expect("real startup parser joined");
    let (stale, cut_end) = match case {
        Scenario::Status => {
            assert!(result.is_ok());
            assert_eq!(
                edges,
                [ReportEdge::Ready, ReportEdge::Ack, ReportEdge::Commit]
            );
            wait_active(&socket).await;
            let end = Instant::now() + STATUS;
            let mut status = timeout_at(end, AsyncUnixStream::connect(&socket))
                .await
                .unwrap()
                .unwrap();
            timeout_at(end, ipc::send_fault_fixture_status(&mut status))
                .await
                .unwrap()
                .unwrap();
            (Some(status), end)
        }
        Scenario::Ready => {
            assert!(result.is_err());
            assert_eq!(
                edges,
                [
                    ReportEdge::Ready,
                    ReportEdge::Ack,
                    ReportEdge::PostAckClosed
                ]
            );
            (None, deadline)
        }
    };
    let sampled = frame(&mut controller, b'S', 1024, cut_end).await;
    assert_eq!(sampled.first().copied(), Some(case as u8));
    let observed = ordinary.dispatcher_connections();
    assert!(!observed.is_empty() && observed.values().all(|socket| socket.peer_port != 0));
    assert!(observed.values().any(|socket| socket.key_package));
    assert!(observed.values().any(|socket| socket.subscription));
    assert!(peer.dispatcher_connections().is_empty());
    if let Some(mut status) = stale {
        no_response(&mut status, cut_end).await;
        fresh_refused(&socket, cut_end).await;
    }
    let physical = frame(&mut controller, b'P', 32, cut_end).await;
    let pause = Instant::now() + STEP;
    assert_eq!(physical, [case as u8, 1, 1, 0, 1, 1, 1, 1, 1]);
    timeout_at(pause, async {
        while !observed
            .keys()
            .all(|&id| ordinary.runtime_socket_end(id).is_some())
        {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("every owned ordinary relay connection closed");
    assert_eq!(ordinary.dispatcher_connections(), observed);
    assert!(peer.dispatcher_connections().is_empty());
    assert!(matches!(
        ProfileLease::acquire_ready(state.path(), PROFILE),
        Err(crate::Error::Identity("profile_in_use"))
    ));
    assert_image(&root, &sampled[1..]);
    log_has_fatal_stop(log);
    timeout_at(pause, async {
        use tokio::io::AsyncWriteExt;
        controller.write_all(&[b'G', 0, 0, 0, 0]).await.unwrap();
        let mut one = [0; 1];
        assert_eq!(
            controller.read(&mut one).await.unwrap(),
            0,
            "control EOF after release"
        );
    })
    .await
    .expect("bounded control release");
    assert_eq!(child.reap().await.code(), Some(1));
    // SAFETY: zero-signal probe of the fresh private process group after reap.
    assert_eq!(unsafe { libc::kill(-pid, 0) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    assert!(peer.dispatcher_connections().is_empty());
    assert_eq!(ordinary.dispatcher_connections(), observed);
    assert_image(&root, &sampled[1..]);

    reopen(&ordinary, state.path(), &daemon, account).await;
    eprintln!(
        "registered dispatcher-loss case={} closed={} total_ms={}",
        case as u8,
        observed.len(),
        started.elapsed().as_millis()
    );
}

async fn reopen(relay: &Relay, state: &Path, daemon: &Path, account: nostr::key::PublicKey) {
    let mut lease = ProfileLease::acquire_ready(state, PROFILE).expect("reacquire after reap");
    let helper = crate::native_layout::resolve(daemon).unwrap().helper;
    let material = lease
        .prepare(
            crate::identity::BackendChoice::File,
            &helper,
            std::time::Instant::now() + STATUS,
        )
        .unwrap();
    assert!(lease.public_key() == Some(account) && material.public_key() == account);
    let config = crate::command_config::CommandConfig::load(lease.root()).unwrap();
    let mut native = crate::mdk_adapter::MdkOwner::open(
        &lease,
        &material,
        relay.connector(lease.root(), true, None),
        Vec::new(),
    )
    .expect("stock MDK and Journal reopen");
    assert!(native.status().unwrap().identity == account.to_hex());
    assert!(native.fixture_empty_command_recovery().unwrap());
    native
        .shutdown(Instant::now() + STATUS)
        .await
        .expect("stock owner close");
    let account = account.to_bytes();
    let policy = config
        .engine_policy(lease.root().join("commands"), helper, account, &[])
        .unwrap()
        .unwrap();
    let secret =
        crate::command_identity::derive(lease.profile_id(), account, material.sqlcipher_root())
            .unwrap();
    let engine = marmish_engine::Engine::open_with_master(
        policy,
        secret.binding,
        marmish_engine::ProvidedMaster::new(*secret.master),
    )
    .expect("stock engine reopen");
    let _closed = engine
        .into_cleanup()
        .expect("independent empty idle engine cleanup lock");
    verify_empty_engine(
        &lease.root().join("commands/engine.sqlite3"),
        &secret.master,
    );

    assert!(
        fs::read_dir(lease.root().join("commands").join("jobs"))
            .unwrap()
            .next()
            .is_none()
    );
}

// Independent read-only reference for the exact ordinary engine State domain.
// The actual Engine has already authenticated the binding and closed while
// retaining the frontend lock. No raw key bytes enter SQL or test output.
fn verify_empty_engine(path: &Path, master: &[u8; 32]) {
    use hkdf::Hkdf;
    use rusqlite::{Connection, OpenFlags, ffi};
    use sha2::Sha256;
    use zeroize::Zeroizing;

    let mut derived = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(b"marmish-storage-hkdf-v1"), master)
        .expand(b"marmish-storage-key-v1\0state\0", derived.as_mut())
        .unwrap();
    let raw = Zeroizing::new({
        let encoded = Zeroizing::new(hex::encode(derived.as_ref()));
        format!("x'{}'", encoded.as_str())
    });
    let database = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .expect("independent keyed engine view");
    database
        .execute_batch("PRAGMA cipher_compatibility=4; PRAGMA cipher_page_size=4096;")
        .unwrap();
    // SAFETY: the borrowed DB and key are live for this call; no other
    // thread uses this read-only connection.
    let result = unsafe { ffi::sqlite3_key(database.handle(), raw.as_ptr().cast(), 67) };
    assert_eq!(result, ffi::SQLITE_OK);
    let count: i64 = database
        .query_row("SELECT count(*) FROM jobs", [], |row| row.get(0))
        .expect("authenticate engine schema and recover exact invocation count");
    assert_eq!(count, 0);
}

#[test]
#[ignore = "private actual registered dispatcher-loss subprocess and installed helpers"]
fn registered_dispatcher_loss() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            scenario(Scenario::Ready).await;
            scenario(Scenario::Status).await;
        });
}
