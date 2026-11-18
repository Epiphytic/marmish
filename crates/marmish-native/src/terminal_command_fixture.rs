//! Real durable Command control on the same relay as the experimental lanes.
use super::*;
use crate::command_config::CommandConfig;
use crate::command_engine::{CommandEngine, EngineSettings};
use crate::command_service::CommandService;
use crate::command_wire::{self as wire, Id, RequestOp, ResponseResult, Submit};
use crate::identity::BackendChoice;
use crate::journal::{ClientDelivery, CommandRoom, Owner};
use rusqlite::{Connection, OpenFlags};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

struct CommandPeer {
    material: IdentityMaterial,
    lease: ProfileLease,
}

impl CommandPeer {
    fn new(base: &Path, name: &str) -> Self {
        let mut lease = ProfileLease::acquire(base, name).unwrap();
        let material = lease
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + STEP,
            )
            .unwrap();
        Self { material, lease }
    }

    fn public(&self) -> PublicKey {
        self.material.public_key()
    }

    async fn open(&self, relay: &Relay, remote: PublicKey, server: bool, cat: &Path) -> MdkOwner {
        let mut owner = MdkOwner::open(
            &self.lease,
            &self.material,
            relay.connector(self.lease.root(), true, None),
            vec![remote.to_hex()],
        )
        .unwrap();
        let config = CommandConfig::from_bytes(
            &serde_json::to_vec(&json!({
                "version": 1,
                "role": if server { "server" } else { "client" },
                "relays": [relay.url.as_str()], "tor": "off", "cwd": "/",
                "limits": {}, "aliases": {},
                "trusted_remotes": if server { vec![] } else { vec![remote.to_hex()] },
                "execution_clients": if server { vec![remote.to_hex()] } else { vec![] },
                "command_policy": {"version":1, "defaults":{"allow_exec":[cat]}},
            }))
            .unwrap(),
        )
        .unwrap();
        let worker = worker();
        if server {
            let account = self.public().to_bytes();
            let policy = config
                .engine_policy(self.lease.root().join("commands"), worker, account, &[])
                .unwrap()
                .unwrap();
            owner.journal.command_configure(config.limits()).unwrap();
            let identity = crate::command_identity::derive(
                self.lease.profile_id(),
                account,
                self.material.sqlcipher_root(),
            )
            .unwrap();
            owner
                .journal
                .command_bind_engine(
                    crate::command_identity::DERIVATION_VERSION,
                    &identity.root_id,
                    &identity.key_id,
                )
                .unwrap();
            let engine = CommandEngine::start(
                EngineSettings {
                    policy,
                    binding: identity.binding,
                    master: identity.master,
                    // The unit-test executable has no installed native layout.
                    // The real worker is used, with copy-helper registration disabled.
                    canonical_native_image: None,
                },
                owner.journal.command_recovery().unwrap(),
            )
            .unwrap();
            owner.commands = Some(CommandService::new(config, Some(engine)));
        } else {
            owner
                .enable_commands(&self.lease, &self.material, config, worker)
                .unwrap();
        }
        owner.activate(Instant::now() + STEP).await.unwrap();
        owner.relay_proof_ingress = Some(Vec::new());
        owner
    }

    fn mdk_key(&self) -> Zeroizing<[u8; 32]> {
        let mut key = Zeroizing::new([0; 32]);
        Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), self.material.sqlcipher_root())
            .expand(b"mdk", &mut *key)
            .unwrap();
        key
    }

    fn inspect_mdk(&self) -> Connection {
        let key = self.mdk_key();
        let path = self.lease.mdk_database();
        let reader = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let key = Zeroizing::new(format!("x'{}'", hex::encode(*key)));
        reader.pragma_update(None, "key", key.as_str()).unwrap();
        reader.execute_batch("PRAGMA cipher_compatibility=4; PRAGMA cipher_memory_security=ON; PRAGMA cipher_page_size=4096;").unwrap();
        reader
    }
}

fn worker() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let worker = exe
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("marmish-worker");
    assert!(
        worker.is_file(),
        "build marmish-native --bins before this opt-in proof"
    );
    worker
}

fn fresh_id() -> Id {
    hex::decode(foundation::fresh_id(now().unwrap()).unwrap())
        .unwrap()
        .try_into()
        .unwrap()
}

async fn response(
    client: &mut MdkOwner,
    server: &mut MdkOwner,
    owner: &Owner,
    id: Id,
    cursor: Option<ClientDelivery>,
    deadline: Instant,
) -> ResponseResult {
    loop {
        assert!(Instant::now() < deadline, "bounded Command response");
        client.tick(deadline).await.unwrap();
        server.tick(deadline).await.unwrap();
        if let Some((reply, digest)) = client.journal.command_client_response(owner, id).unwrap() {
            client
                .journal
                .command_consume_response(owner, id, digest, cursor)
                .unwrap();
            return reply.result.unwrap_or_else(|error| {
                panic!(
                    "Command response failed: {} {:?}",
                    error.diagnostic_category(),
                    error.detail
                )
            });
        }
    }
}

async fn run_command(client: &mut MdkOwner, server: &mut MdkOwner, room: &CommandRoom, cat: &Path) {
    let deadline = Instant::now() + STEP;
    let owner = Owner::from_context(
        &client
            .command_context(&GroupId::new(room.group.clone()))
            .unwrap(),
    );
    let invocation = fresh_id();
    let bytes = b"\0$HOME; '\"*?[]\xff\r\n";
    client
        .command_request(
            room.session,
            invocation,
            RequestOp::Submit(Submit {
                argv: vec![cat.as_os_str().as_encoded_bytes().to_vec()],
                invocation,
                stdin: bytes.to_vec(),
            }),
        )
        .unwrap();
    assert!(matches!(
        response(client, server, &owner, invocation, None, deadline).await,
        ResponseResult::Submit { .. }
    ));
    let mut stdout_cursor = 0;
    let mut result = Vec::new();
    loop {
        let id = fresh_id();
        let poll = RequestOp::Poll {
            invocation,
            stdout_cursor,
            stderr_cursor: 0,
            max_bytes: 1024,
        };
        loop {
            match client.command_request(room.session, id, poll.clone()) {
                Ok(_) => break,
                // The prior consumed Poll may still own its native obligation.
                Err(Error::Invalid("command_poll_busy")) => {
                    assert!(Instant::now() < deadline, "bounded Poll admission");
                    client.tick(deadline).await.unwrap();
                    server.tick(deadline).await.unwrap();
                }
                Err(error) => panic!("Command Poll admission failed: {error}"),
            }
        }
        let ResponseResult::Poll {
            status,
            stdout,
            stderr,
        } = response(
            client,
            server,
            &owner,
            id,
            Some(ClientDelivery {
                stdout: stdout_cursor,
                stderr: 0,
            }),
            deadline,
        )
        .await
        else {
            panic!("expected Command Poll");
        };
        assert!(stderr.bytes.is_empty());
        result.extend_from_slice(&stdout.bytes);
        stdout_cursor = stdout.next_cursor;
        assert!(result.len() <= bytes.len());
        if let Some(outcome) = status
            .outcome
            .filter(|_| status.stdout.closed && status.stderr.closed)
        {
            assert!(outcome.kind == wire::OutcomeKind::Code);
            assert_eq!(outcome.value, Some(0));
            assert_eq!(result, bytes);
            break;
        }
    }
}

fn snapshot(reader: &Connection) -> (i64, i64) {
    let count = |sql| reader.query_row(sql, [], |row| row.get(0)).unwrap();
    (
        count("SELECT COUNT(*) FROM cgka_ingress_dedup"),
        count("SELECT COUNT(*) FROM cgka_processed_transport_ids"),
    )
}

fn assert_excluded(storage: &SqliteAccountStorage, events: &CapturedRoom) {
    assert!(
        !storage
            .has_ingress_dedup_marker(&MessageId::new(events.welcome.id.to_bytes().to_vec()))
            .unwrap()
    );
    assert!(
        !storage
            .has_processed_transport_id(&MessageId::new(events.frame.id.to_bytes().to_vec()))
            .unwrap()
    );
}

#[tokio::test]
#[ignore = "opt-in two durable native Command profiles with a real worker and loopback relay"]
async fn terminal_copies_never_ingest_in_durable_command_profiles() {
    let temp = tempfile::tempdir().unwrap();
    let relay = Relay::start().await;
    relay.assert_inbox_read_policy().await;
    let commands = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(temp.path())
        .unwrap();
    let client_peer = CommandPeer::new(commands.path(), "client");
    let server_peer = CommandPeer::new(commands.path(), "server");
    let cat = [Path::new("/usr/bin/cat"), Path::new("/bin/cat")]
        .into_iter()
        .find_map(|path| std::fs::canonicalize(path).ok())
        .unwrap();
    for key in [client_peer.public(), server_peer.public()] {
        relay.ordinary(key);
    }
    let mut client = client_peer
        .open(&relay, server_peer.public(), false, &cat)
        .await;
    let mut server = server_peer
        .open(&relay, client_peer.public(), true, &cat)
        .await;
    let room = client
        .open_command_room(
            server_peer.public().to_bytes(),
            false,
            Instant::now() + STEP,
        )
        .await
        .unwrap();
    run_command(&mut client, &mut server, &room, &cat).await;
    let events = prove(0, relay.url.as_str()).await;
    relay.mode(Mode::KnownAccountsOnly);
    let unknown = Lane::open(&relay, temp.path(), events.founder.clone()).await;
    unknown.activate(None).await;
    relay.observed(|entry| matches!(entry, Observation::Authentication(key, false) if *key == unknown.public)).await;
    unknown.close().await;
    run_command(&mut client, &mut server, &room, &cat).await;
    relay.mode(Mode::AuthorsMustAuthenticate);
    let experimental = Lane::open(&relay, temp.path(), events.founder.clone()).await;
    experimental.activate(None).await;
    relay.observed(|entry| matches!(entry, Observation::Authentication(key, true) if *key == experimental.public)).await;
    for event in [&events.welcome, &events.frame] {
        assert_ne!(event.pubkey, experimental.public);
        assert!(!experimental.publish(event).await);
        relay
            .observed(|entry| matches!(entry, Observation::Write(id, _, false) if id == &event.id))
            .await;
    }
    experimental.close().await;
    run_command(&mut client, &mut server, &room, &cat).await;

    let inspect = server_peer.inspect_mdk();
    let baseline = snapshot(&inspect);
    let actual_arrivals = server.relay_proof_ingress.as_ref().unwrap();
    assert!(
        actual_arrivals
            .iter()
            .any(|id| server.storage.has_ingress_dedup_marker(id).unwrap())
    );
    assert!(
        actual_arrivals
            .iter()
            .any(|id| server.storage.has_processed_transport_id(id).unwrap())
    );
    for (owner, account) in [
        (&mut client, client_peer.public()),
        (&mut server, server_peer.public()),
    ] {
        let inbox_id = relay
            .subscription(account, 1059, "#p", json!(account))
            .await;
        let room_id = relay
            .subscription(account, 445, "#h", json!(hex::encode(&room.transport)))
            .await;
        let previously = owner.relay_proof_ingress.as_ref().unwrap().len();
        for (id, event) in [(&inbox_id, &events.welcome), (&room_id, &events.frame)] {
            relay.inject(account, id, event);
            let copy = NostrRelayEvent {
                endpoint: TransportEndpoint(relay.url.to_string()),
                subscription_id: Some(id.clone()),
                event: NostrTransportEvent::from_nostr_event(event).unwrap(),
            };
            assert_eq!(
                owner
                    .adapter
                    .handle_relay_event(copy.clone())
                    .await
                    .unwrap(),
                0
            );
            if event.id == events.frame.id {
                assert_eq!(
                    owner
                        .adapter
                        .handle_group_relay_copy(copy, Some(&owner.account))
                        .await
                        .unwrap(),
                    0
                );
            }
        }
        let deadline = Instant::now() + STEP;
        for _ in 0..4 {
            owner.receive_one(deadline).await.unwrap();
        }
        assert_eq!(
            owner.relay_proof_ingress.as_ref().unwrap().len(),
            previously
        );
        assert_excluded(&owner.storage, &events);
    }
    assert_eq!(snapshot(&inspect), baseline);
    events.assert_no_package_publication();
    run_command(&mut client, &mut server, &room, &cat).await;
    client.shutdown(Instant::now() + STEP).await.unwrap();
    server.shutdown(Instant::now() + STEP).await.unwrap();
    drop((client, server, inspect));
    let reopened = server_peer.inspect_mdk();
    let (dedup, processed) = snapshot(&reopened);
    assert_eq!(dedup, baseline.0);
    assert!(processed > baseline.1);
    eprintln!(
        "terminal_route_proof command_baseline_dedup={} command_baseline_processed={} reopened_dedup={dedup} reopened_processed={processed}",
        baseline.0, baseline.1
    );
    for peer in [&client_peer, &server_peer] {
        let reopened = SqliteAccountStorage::open_encrypted(
            peer.lease.mdk_database(),
            &SqlCipherKey::raw(*peer.mdk_key()),
        )
        .unwrap();
        assert_excluded(&reopened, &events);
    }
    relay.assert_healthy();
}
