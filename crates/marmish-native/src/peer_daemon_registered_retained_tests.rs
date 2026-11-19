//! Installed Both shutdown and exact ordinary stock cold readback.
use super::*;
use crate::{
    command_wire::{Outcome, OutcomeKind},
    journal::WorkerState,
    peer_relay_trust::qualified_both_command_fixture,
};

const STOCK: &str = "MARMISH_TEST_REGISTERED_STOCK_LIBTEST";
const STOCK_DAEMON: &str = "MARMISH_TEST_REGISTERED_STOCK_DAEMON";
const STOCK_SELECTOR: &str = "peer_invitation::daemon_wss_tests::registered_stock_reopen_child";
const FIRST: &[u8] = b"one\0; '\"*?[]\xff\r\n";
const SECOND: &[u8] = b"two\0$HOME \\ &\xfe\n";

fn assert_marker(path: &Path) {
    assert!(std::fs::read(path).unwrap() == [FIRST, SECOND].concat());
}

async fn quiet_command_relay(relay: &Relay) {
    timeout(Duration::from_secs(5), async {
        let mut last = relay.publication_attempts(445);
        let mut stable = Instant::now();
        while stable.elapsed() < Duration::from_millis(500) {
            tokio::time::sleep(Duration::from_millis(25)).await;
            let now = relay.publication_attempts(445);
            if now != last {
                stable = Instant::now();
                last = now;
            }
        }
    })
    .await
    .expect("bounded ordinary relay quiet");
}

fn launch_stock(daemon: &Path, root: &Path, relay: &Relay, ca: &Path) -> OwnedChild {
    let stock = PathBuf::from(std::env::var_os(STOCK).expect("exact stock libtest"));
    assert_eq!(stock.canonicalize().unwrap(), stock);
    let layout = PathBuf::from(std::env::var_os(STOCK_DAEMON).expect("installed stock daemon"));
    assert_eq!(layout.canonicalize().unwrap(), layout);
    assert_eq!(layout.file_name().unwrap(), "marmishd");
    let prefix = |path: &Path| path.ancestors().nth(3).unwrap().to_owned();
    assert_ne!(prefix(&layout), prefix(daemon), "separate installs");
    OwnedChild(
        Command::new(stock)
            .args(["--ignored", "--exact", STOCK_SELECTOR, "--nocapture"])
            .env_clear()
            .env("MARMISH_TEST_STOCK_REOPEN_ROOT", root)
            .env("MARMISH_TEST_STOCK_REOPEN_DAEMON", layout)
            .env("MARMISH_TEST_STOCK_REOPEN_CANDIDATE", daemon)
            .env("MARMISH_TEST_STOCK_REOPEN_RELAY", relay.url.as_str())
            .env("MARMISH_TEST_STOCK_REOPEN_CA", ca)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

async fn stock_evidence(socket: &Path, after: Option<u64>) -> serde_json::Value {
    let response = ipc::request(socket, &Request::CommandEvidence { after, limit: 2 })
        .await
        .expect("ordinary Command evidence IPC");
    assert!(response.ok, "ordinary Command evidence accepted");
    response.result.expect("ordinary Command evidence page")
}

async fn assert_stock_rows(socket: &Path, sender: PublicKey, invocations: [String; 2]) {
    let page = stock_evidence(socket, None).await;
    let mut rows: Vec<_> = page["rows"].as_array().unwrap().iter().collect();
    rows.sort_by_key(|row| row["cursor"].as_u64().unwrap());
    assert_eq!(rows.len(), 2);
    let mut cursor = 0;
    let outcome = serde_json::to_value(Outcome {
        kind: OutcomeKind::Code,
        value: Some(0),
    })
    .unwrap();
    let worker = serde_json::to_value(WorkerState::Exited).unwrap();
    for ((row, invocation), bytes) in rows.into_iter().zip(invocations).zip([FIRST, SECOND]) {
        let next = row["cursor"].as_u64().unwrap();
        assert!(next > cursor);
        cursor = next;
        assert!(row["invocation"] == invocation && row["principal"] == sender.to_hex());
        assert_eq!(row["engine_admitted"], 1);
        assert!(row["refusal"].is_null());
        assert_eq!(row["outcome"], outcome);
        assert_eq!(row["worker_state"], worker);
        assert_eq!(row["authority_valid"], true);
        assert!(
            row["observed_seq"]
                .as_u64()
                .is_some_and(|sequence| sequence > 0)
        );
        for (stream, expected) in [("stdout", bytes.len()), ("stderr", 0)] {
            assert_eq!(row[stream]["produced"], expected);
            assert_eq!(row[stream]["closed"], true);
            assert_eq!(row[stream]["gaps"], 0);
        }
    }
    let next = stock_evidence(socket, Some(cursor)).await;
    assert!(next["rows"].as_array().unwrap().is_empty());
    assert!(next["next"].is_null());
}

async fn conclude_stock(stock: &mut OwnedChild) {
    stock.0.stdin.take().unwrap().write_all(b"S").unwrap();
    assert!(
        stock.finished().await.success(),
        "clean ordinary stock exit"
    );
    let mut output = String::new();
    stock
        .0
        .stdout
        .take()
        .unwrap()
        .take(4097)
        .read_to_string(&mut output)
        .unwrap();
    assert!(output.len() <= 4096);
    let passed = output
        .lines()
        .filter(|line| line.starts_with("test result: ok. 1 passed; 0 failed;"))
        .count();
    assert_eq!(passed, 1, "executed exactly one stock reopen selector");
}

async fn retained() {
    let example = PathBuf::from(std::env::var_os(EXAMPLE).expect("private example"));
    assert_eq!(example.canonicalize().unwrap(), example);
    let daemon = PathBuf::from(std::env::var_os(DAEMON).expect("installed candidate daemon"));
    assert_eq!(layout().canonicalize().unwrap(), daemon);
    crate::native_install::read_receipt(
        daemon.parent().unwrap().parent().unwrap().parent().unwrap(),
    )
    .expect("claimed installed candidate prefix");

    let parent = private();
    let sender = private();
    let mut sender_lease = ProfileLease::acquire(sender.path(), PROFILE).unwrap();
    let sender_key = material(&mut sender_lease, &daemon).public_key();
    let sender_socket = sender_lease.socket_path();
    let mut relay = Relay::start().await;
    let tee = std::fs::canonicalize("/usr/bin/tee").unwrap();
    let third = nostr::key::Keys::generate().public_key();
    let (mut peer_relay, root, lease, identity) =
        qualified_both_command_fixture(sender_key, third, parent.path(), relay.url.as_str(), &tee)
            .await;
    assert_ne!(peer_relay.url, relay.url);
    let signer = identity.public_key();
    relay.ordinary(signer);
    relay.ordinary(sender_key);
    configure_off(&sender_lease, &relay, signer);
    let peer_connections = peer_relay.qualification_connections();
    let peer_subscriptions = peer_relay.recipient_requests(signer);
    let socket = lease.socket_path();
    drop((lease, identity, sender_lease));

    let sender_ca = sender.path().join("fixture-root.der");
    write_private(&sender_ca, relay.qualification_ca());
    let sender_args = arguments(&daemon, sender.path(), &relay, &sender_ca);
    let log = tempfile::tempfile_in(sender.path()).unwrap();
    let mut sender_child = OwnedChild(fresh_process(&example, &sender_args, log).spawn().unwrap());
    println!("retained stage=sender");
    wait_active(&mut sender_child, &sender_socket).await;

    let ca = root.path().join("fixture-root.der");
    write_private(&ca, relay.qualification_ca());
    let args = arguments(&daemon, root.path(), &relay, &ca);
    let log = tempfile::tempfile_in(root.path()).unwrap();
    let clock = Instant::now();
    let (mut child, report) = private_parent(example, args, log).await;
    report.expect("private registered Both R/A/C report");
    println!("retained stage=registered");
    wait_active(&mut child, &socket).await;
    let startup = clock.elapsed();

    let marker = root.path().join("command-marker");
    let argv = vec![
        tee.as_os_str().as_encoded_bytes().to_vec(),
        b"-a".to_vec(),
        marker.as_os_str().as_encoded_bytes().to_vec(),
    ];
    let clock = Instant::now();
    let first =
        ordinary_command_bytes(&sender_socket, signer, &relay, argv.clone(), FIRST, Some(1)).await;
    let first_time = clock.elapsed();
    let clock = Instant::now();
    let second =
        ordinary_command_bytes(&sender_socket, signer, &relay, argv, SECOND, Some(2)).await;
    let second_time = clock.elapsed();
    assert_ne!(first, second);
    assert_marker(&marker);

    let clock = Instant::now();
    quiet_command_relay(&relay).await;
    let quiet = clock.elapsed();
    let clock = Instant::now();
    child.stop().await;
    let stop = clock.elapsed();
    assert!(!socket.exists());
    drop(ProfileLease::acquire_ready(root.path(), PROFILE).unwrap());
    assert_marker(&marker);

    let clock = Instant::now();
    let mut stock = launch_stock(&daemon, root.path(), &relay, &ca);
    println!("retained stage=stock-reopen");
    wait_active(&mut stock, &socket).await;
    assert_stock_rows(&socket, sender_key, [first, second]).await;
    let cold = clock.elapsed();
    conclude_stock(&mut stock).await;
    assert!(!socket.exists());
    drop(ProfileLease::acquire_ready(root.path(), PROFILE).unwrap());
    assert_marker(&marker);
    sender_child.stop().await;

    assert!(relay.publication_attempts(445) > 0);
    assert_eq!(relay.publication_attempts(7435), 0);
    assert_eq!(peer_relay.publication_attempts(445), 0);
    assert_eq!(peer_relay.publication_attempts(7435), 0);
    assert_eq!(peer_relay.qualification_connections(), peer_connections);
    assert_eq!(peer_relay.recipient_requests(signer), peer_subscriptions);
    peer_relay.stop().await;
    relay.stop().await;
    println!(
        "retained rows=2 startup_ms={} first_ms={} second_ms={} quiet_ms={} stop_ms={} cold_reopen_ms={}",
        startup.as_millis(),
        first_time.as_millis(),
        second_time.as_millis(),
        quiet.as_millis(),
        stop.as_millis(),
        cold.as_millis()
    );
}

#[test]
#[ignore = "requires exact installed Linux AMD64 candidate and source-matched stock libtest"]
fn installed_registered_both_retained_results() {
    run_native(LIMIT, retained);
}
