//! Same-user Unix socket tests with an on-disk SQLCipher journal.
use crate::client_inventory::{
    self, ClientInventoryEntry, ClientInventoryReply, InventoryRequest, InventoryResult,
};
use crate::command_config::CommandConfig;
use crate::command_wire::Account;
use crate::ipc::{self, Pending, Request, Response};
use crate::journal::{Journal, private_test_directory};
use crate::{Error, Result};
use nostr::{key::PublicKey, nips::nip19::ToBech32};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

struct Socket {
    path: PathBuf,
    listener: JoinHandle<Result<()>>,
}

impl Socket {
    fn start(directory: &Path, name: &str, queue: mpsc::Sender<Pending>) -> Self {
        let path = directory.join(name);
        let listener = ipc::bind(&path).unwrap();
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        let (_, snapshots) = watch::channel(Response::success(json!({
            "state": "transport_degraded",
        })));
        Self {
            path,
            listener: tokio::spawn(ipc::serve(listener, queue, snapshots)),
        }
    }

    async fn query(&self, request: InventoryRequest) -> Result<ClientInventoryReply> {
        ipc::client_inventory_request(&self.path, request).await
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

struct Fixture {
    socket: Socket,
    owner: JoinHandle<()>,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn start(name: &str, role: &str, keys: &[Account], large_policy: bool) -> Self {
        let deny_exec: Vec<_> = if large_policy {
            (0..12)
                .map(|index| format!("/private/{index}/{}", "x".repeat(985)))
                .collect()
        } else {
            Vec::new()
        };
        let config = CommandConfig::from_bytes(
            &serde_json::to_vec(&json!({
                "version": 1,
                "role": role,
                "relays": ["wss://127.0.0.1:9/"],
                "tor": "off",
                "trusted_remotes": [],
                "execution_clients": keys.iter().map(hex::encode).collect::<Vec<_>>(),
                "cwd": "/",
                "limits": {},
                "aliases": {},
                "command_policy": {"version": 1, "defaults": {"deny_exec": deny_exec}}
            }))
            .unwrap(),
        )
        .unwrap();
        let directory = private_test_directory();
        let account = [1; 32];
        let mut journal = Journal::open(
            &directory.path().join("j"),
            &[71; 32],
            &hex::encode(account),
        )
        .unwrap();
        journal
            .reconcile_client_inventory(config.execution_clients())
            .unwrap();
        let policy = config
            .engine_policy(
                directory.path().join("engine"),
                "/bin/true".into(),
                account,
                &[],
            )
            .unwrap()
            .and_then(|engine| engine.command_policy);
        let (queue, mut requests) = mpsc::channel::<Pending>(1);
        let owner = tokio::spawn(async move {
            while let Some(pending) = requests.recv().await {
                let Request::ClientInventory { request } = pending.request else {
                    panic!("inventory fixture received another queued operation");
                };
                let response = Response::inventory(client_inventory::query(
                    &journal,
                    &config,
                    policy.as_ref(),
                    request,
                ));
                let _ = pending.respond.send(response);
            }
        });
        Self {
            socket: Socket::start(directory.path(), name, queue),
            owner,
            _directory: directory,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.owner.abort();
    }
}

fn npub(key: Account) -> String {
    PublicKey::from_slice(&key).unwrap().to_bech32().unwrap()
}

fn list() -> InventoryRequest {
    InventoryRequest::List {
        after: None,
        limit: 64,
    }
}

fn show(npub: &str) -> InventoryRequest {
    InventoryRequest::Show {
        npub: npub.into(),
        policy_page: 0,
        policy_generation: None,
    }
}

#[tokio::test]
async fn named_inventory_sockets_isolate_clients_and_work_without_transport() {
    let alpha = [2; 32];
    let beta = [3; 32];
    let alpha_name = npub(alpha);
    let beta_name = npub(beta);
    let alpha_socket = Fixture::start("alpha.sock", "both", &[alpha], true);
    let beta_socket = Fixture::start("beta.sock", "server", &[beta], false);
    let client_only = Fixture::start("client.sock", "client", &[], false);
    for (fixture, expected, other) in [
        (&alpha_socket, &alpha_name, &beta_name),
        (&beta_socket, &beta_name, &alpha_name),
    ] {
        let socket = &fixture.socket;
        let status = ipc::request(&socket.path, &Request::Status {})
            .await
            .unwrap();
        assert_eq!(status.result, Some(json!({"state": "transport_degraded"})));
        let reply = socket.query(list()).await.unwrap();
        assert_eq!(reply.version, 1);
        let InventoryResult::List { clients, next } = reply.result else {
            panic!("expected a list response");
        };
        assert_eq!(clients.len(), 1);
        assert_eq!(&clients[0].npub, expected);
        assert!(clients[0].access.last_authenticated_submit_at.is_none());
        assert!(clients[0].access.last_engine_admitted_submit_at.is_none());
        assert!(next.is_none());
        assert!(matches!(
            socket.query(show(other)).await,
            Err(Error::Invalid("inventory_client_unknown"))
        ));
    }
    assert!(matches!(
        client_only.socket.query(list()).await,
        Err(Error::Invalid("inventory_server_unavailable"))
    ));

    let socket = &alpha_socket.socket;
    let InventoryResult::Show { client, policy } =
        socket.query(show(&alpha_name)).await.unwrap().result
    else {
        panic!("expected a show response");
    };
    assert_eq!(client.npub, alpha_name);
    let policy = serde_json::to_value(policy).unwrap();
    assert_eq!(policy["page"], 0);
    assert_eq!(policy["next_page"], 1);
    let generation = policy["generation"].as_str().unwrap();
    let continuation = |generation: String| InventoryRequest::Show {
        npub: alpha_name.clone(),
        policy_page: 1,
        policy_generation: Some(generation),
    };
    let InventoryResult::Show { policy, .. } = socket
        .query(continuation(generation.into()))
        .await
        .unwrap()
        .result
    else {
        panic!("expected continuation of policy");
    };
    assert_eq!(serde_json::to_value(policy).unwrap()["page"], 1);
    let changed = Fixture::start("changed.sock", "server", &[alpha], false);
    assert!(matches!(
        changed.socket.query(continuation(generation.into())).await,
        Err(Error::Invalid("inventory_policy_stale"))
    ));
}

#[tokio::test(start_paused = true)]
async fn stalled_inventory_queue_reports_busy_then_deadline() {
    let directory = private_test_directory();
    let (queue, mut requests) = mpsc::channel::<Pending>(1);
    let socket = Socket::start(directory.path(), "stalled.sock", queue.clone());
    let path = socket.path.clone();
    let caller = tokio::spawn(async move { ipc::client_inventory_request(&path, list()).await });
    let waiting_since = Instant::now();
    while queue.capacity() != 0 {
        assert!(!caller.is_finished());
        assert!(waiting_since.elapsed() < Duration::from_secs(5));
        tokio::task::yield_now().await;
    }
    let path = socket.path.clone();
    let competing = tokio::spawn(async move { ipc::client_inventory_request(&path, list()).await });
    // Keep the runtime runnable while real socket IO completes so paused time
    // cannot advance to the first caller's timeout by itself.
    let waiting_since = Instant::now();
    while !competing.is_finished() {
        assert!(waiting_since.elapsed() < Duration::from_secs(5));
        tokio::task::yield_now().await;
    }
    assert!(matches!(
        competing.await.unwrap(),
        Err(Error::Invalid("inventory_busy"))
    ));
    let pending = requests.recv().await.unwrap();
    assert!(matches!(
        pending.request,
        Request::ClientInventory {
            request: InventoryRequest::List { .. }
        }
    ));
    let remaining = pending.deadline - tokio::time::Instant::now();
    assert!(remaining > Duration::from_secs(14));
    assert!(remaining < Duration::from_secs(15));
    tokio::time::advance(remaining - Duration::from_millis(20)).await;
    assert!(!pending.respond.is_closed());
    tokio::time::advance(Duration::from_millis(20)).await;
    assert!(matches!(caller.await.unwrap(), Err(Error::Timeout)));
    assert!(pending.respond.is_closed());
}

#[tokio::test]
async fn oversized_inventory_reply_is_a_typed_socket_error() {
    let fixture = Fixture::start("source.sock", "server", &[[2; 32]], false);
    let reply = fixture.socket.query(list()).await.unwrap();
    let InventoryResult::List { clients, .. } = reply.result else {
        panic!("expected a list response");
    };
    let row = &clients[0];
    let clients = (0..1024)
        .map(|_| ClientInventoryEntry {
            npub: row.npub.clone(),
            terminal_shell_granted: row.terminal_shell_granted,
            access: row.access.clone(),
        })
        .collect();
    let oversized = ClientInventoryReply {
        version: reply.version,
        result: InventoryResult::List {
            clients,
            next: None,
        },
    };
    let directory = private_test_directory();
    let (queue, mut requests) = mpsc::channel::<Pending>(1);
    let socket = Socket::start(directory.path(), "oversize.sock", queue);
    let path = socket.path.clone();
    let call = tokio::spawn(async move { ipc::client_inventory_request(&path, list()).await });
    let pending = requests.recv().await.unwrap();
    assert!(
        pending
            .respond
            .send(Response::inventory(Ok(oversized)))
            .is_ok()
    );
    assert!(matches!(
        call.await.unwrap(),
        Err(Error::Invalid("inventory_response_too_large"))
    ));
}

#[tokio::test]
#[ignore = "local Unix socket and SQLCipher latency benchmark"]
async fn client_inventory_benchmark() {
    for count in [0_u64, 64, 4096] {
        let keys: Vec<Account> = (1..=count)
            .map(|index| {
                let mut key = [8; 32];
                key[24..].copy_from_slice(&index.to_be_bytes());
                key
            })
            .collect();
        let socket = &Fixture::start("bench.sock", "server", &keys, true).socket;
        sample_socket(socket, count, "list_64", list(), |result| {
            let InventoryResult::List { clients, next } = result.unwrap().result else {
                panic!("expected a list response");
            };
            assert_eq!(clients.len(), count.min(64) as usize);
            assert_eq!(next.is_some(), count > 64);
        })
        .await;
        let key = keys.first().copied().unwrap_or([8; 32]);
        sample_socket(socket, count, "show_page_0", show(&npub(key)), |result| {
            if count == 0 {
                assert!(matches!(
                    result,
                    Err(Error::Invalid("inventory_client_unknown"))
                ));
                return;
            }
            let InventoryResult::Show { policy, .. } = result.unwrap().result else {
                panic!("expected a show response");
            };
            let bytes = serde_json::to_vec(&policy).unwrap();
            assert!((7_400..=8192).contains(&bytes.len()));
        })
        .await;
    }
}

async fn sample_socket(
    socket: &Socket,
    count: u64,
    operation: &str,
    request: InventoryRequest,
    check: impl Fn(Result<ClientInventoryReply>),
) {
    const SAMPLES: usize = 200;
    let mut samples = Vec::with_capacity(SAMPLES + 1);
    for _ in 0..=SAMPLES {
        let request = request.clone();
        let started = Instant::now();
        let result = socket.query(request).await;
        samples.push(started.elapsed());
        check(result);
    }
    let first = samples.remove(0);
    let measured: Duration = samples.iter().sum();
    samples.sort_unstable();
    let percentile = |p: usize| samples[(SAMPLES * p).div_ceil(100) - 1].as_micros();
    eprintln!(
        "keys={count} operation={operation} samples={SAMPLES} first_us={} p50_us={} p95_us={} p99_us={} requests_per_s={:.1}",
        first.as_micros(),
        percentile(50),
        percentile(95),
        percentile(99),
        SAMPLES as f64 / measured.as_secs_f64(),
    );
}
