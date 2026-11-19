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
use std::time::Duration;

#[path = "terminal_bootstrap_wss_tests.rs"]
mod terminal_bootstrap_wss_tests;

#[path = "terminal_enrollment_wss_tests.rs"]
mod terminal_enrollment_wss_tests;

#[path = "terminal_installed_runtime_tests.rs"]
mod terminal_installed_runtime_tests;
pub(crate) use terminal_installed_runtime_tests::open_cli_pty;

pub(crate) struct CommandPeer {
    material: IdentityMaterial,
    lease: ProfileLease,
}

impl CommandPeer {
    pub(crate) fn new(base: &Path, name: &str) -> Self {
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

    pub(crate) fn public(&self) -> PublicKey {
        self.material.public_key()
    }

    async fn open(&self, relay: &Relay, remote: PublicKey, server: bool, cat: &Path) -> MdkOwner {
        self.open_with_engine(relay, remote, server, cat, false, None)
            .await
    }

    async fn open_with_terminal(
        &self,
        relay: &Relay,
        remote: PublicKey,
        server: bool,
        cat: &Path,
        terminal: bool,
    ) -> MdkOwner {
        self.open_with_engine(relay, remote, server, cat, terminal, None)
            .await
    }

    async fn open_with_engine(
        &self,
        relay: &Relay,
        remote: PublicKey,
        server: bool,
        cat: &Path,
        terminal: bool,
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))] admission: Option<
            crate::command_engine::Admission,
        >,
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))] _admission: Option<()>,
    ) -> MdkOwner {
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
                "terminal": {"clients":if terminal && server { vec![remote.to_hex()] } else { vec![] }},
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
            let settings = EngineSettings {
                policy,
                binding: identity.binding,
                master: identity.master,
                // The unit-test executable has no installed native layout.
                // The real worker is used, with copy-helper registration disabled.
                canonical_native_image: None,
            };
            let recovery = owner.journal.command_recovery().unwrap();
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            let engine = match admission {
                Some(admission) => CommandEngine::start_candidate(settings, recovery, admission),
                None => CommandEngine::start(settings, recovery),
            };
            #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
            let engine = CommandEngine::start(settings, recovery);
            let engine = engine.unwrap();
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

    pub(crate) async fn open_fixture_server(
        &self,
        relay: &Relay,
        remote: PublicKey,
        cat: &Path,
    ) -> MdkOwner {
        let mut owner = self.open(relay, remote, true, cat).await;
        owner.relay_proof_ingress = None;
        owner
    }

    pub(crate) async fn stop_fixture_server(self, owner: MdkOwner) {
        crate::daemon::stop_silent(self.lease, owner).await.unwrap();
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
) -> std::result::Result<ResponseResult, wire::WireError> {
    loop {
        assert!(Instant::now() < deadline, "bounded Command response");
        client.tick(deadline).await.unwrap();
        server.tick(deadline).await.unwrap();
        if let Some((reply, digest)) = client.journal.command_client_response(owner, id).unwrap() {
            let committed = client
                .journal
                .command_consume_response(owner, id, digest, cursor)
                .unwrap();
            if matches!(&reply.result, Err(error) if error.code == wire::ErrorCode::Busy) {
                assert!(cursor.is_none_or(|cursor| committed == cursor));
            }
            return reply.result;
        }
    }
}

async fn run_command(
    client: &mut MdkOwner,
    server: &mut MdkOwner,
    room: &CommandRoom,
    cat: &Path,
) -> Id {
    let deadline = Instant::now() + Duration::from_secs(15);
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
    let ack = response(client, server, &owner, invocation, None, deadline)
        .await
        .unwrap_or_else(|error| panic!("Command Submit failed: {}", error.diagnostic_category()));
    assert!(matches!(ack, ResponseResult::Submit { .. }));
    let mut stdout_cursor = 0;
    let mut result = Vec::new();
    let mut busy_replies = 0;
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
        let reply = response(
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
        .await;
        let (status, stdout, stderr) = match reply {
            Ok(ResponseResult::Poll {
                status,
                stdout,
                stderr,
            }) => (status, stdout, stderr),
            Err(error) if error.code == wire::ErrorCode::Busy => {
                busy_replies += 1;
                assert!(busy_replies <= 8, "bounded remote Poll retries");
                let retry_at = (Instant::now() + Duration::from_millis(25)).min(deadline);
                tokio::time::sleep_until(retry_at).await;
                continue;
            }
            Err(error) => panic!("Command Poll failed: {}", error.diagnostic_category()),
            Ok(_) => panic!("expected Command Poll"),
        };
        assert!(stderr.bytes.is_empty());
        result.extend_from_slice(&stdout.bytes);
        stdout_cursor = stdout.next_cursor;
        assert!(result.len() <= bytes.len());
        if let Some(outcome) = status
            .outcome
            .filter(|_| status.stdout.closed && status.stderr.closed)
        {
            assert!(
                outcome.kind == wire::OutcomeKind::Code,
                "Command fixture outcome {} ({:?})",
                serde_json::to_string(&outcome.kind).unwrap(),
                outcome.value,
            );
            assert_eq!(outcome.value, Some(0));
            assert_eq!(result, bytes);
            return invocation;
        }
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) async fn retained_candidate_rows(
    base: &Path,
    relay: &Relay,
    admission: crate::command_engine::Admission,
) -> (CommandPeer, MdkOwner, [Id; 2]) {
    let client_peer = CommandPeer::new(base, "client");
    let server_peer = CommandPeer::new(base, "server");
    let cat = std::fs::canonicalize("/bin/cat").unwrap();
    for key in [client_peer.public(), server_peer.public()] {
        relay.ordinary(key);
    }
    let mut client = client_peer
        .open(relay, server_peer.public(), false, &cat)
        .await;
    let mut server = server_peer
        .open_with_engine(
            relay,
            client_peer.public(),
            true,
            &cat,
            false,
            Some(admission),
        )
        .await;
    let room = client
        .open_command_room(
            server_peer.public().to_bytes(),
            false,
            Instant::now() + STEP,
        )
        .await
        .unwrap();
    let first = run_command(&mut client, &mut server, &room, &cat).await;
    let second = run_command(&mut client, &mut server, &room, &cat).await;
    assert_ne!(first, second);
    client_peer.stop_fixture_server(client).await;
    (server_peer, server, [first, second])
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
    let mut events = prove(0, relay.url.as_str()).await;
    let terminal_frame = events.prepare_maximum_frame(0).await;
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
        for (id, event) in [
            (&inbox_id, &events.welcome),
            (&room_id, &events.frame),
            (&room_id, &terminal_frame.event),
        ] {
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
            if id == &room_id {
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
        let terminal_id = MessageId::new(terminal_frame.event.id.to_bytes().to_vec());
        assert!(
            !owner
                .storage
                .has_ingress_dedup_marker(&terminal_id)
                .unwrap()
        );
        assert!(
            !owner
                .storage
                .has_processed_transport_id(&terminal_id)
                .unwrap()
        );
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
        let terminal_id = MessageId::new(terminal_frame.event.id.to_bytes().to_vec());
        assert!(!reopened.has_ingress_dedup_marker(&terminal_id).unwrap());
        assert!(!reopened.has_processed_transport_id(&terminal_id).unwrap());
    }
    relay.assert_healthy();
}

#[test]
#[ignore = "real loopback WSS with native Command workers"]
fn terminal_command_authority_native_wss() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    prove_terminal_command_authority(false).await;
                    prove_terminal_command_authority(true).await;
                });
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn prove_terminal_command_authority(granted: bool) {
    use crate::terminal_fence::FenceState;

    let temp = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let relay = Relay::start().await;
    let client_peer = CommandPeer::new(temp.path(), "authority-client");
    let server_peer = CommandPeer::new(temp.path(), "authority-server");
    let cat = [Path::new("/usr/bin/cat"), Path::new("/bin/cat")]
        .into_iter()
        .find_map(|path| std::fs::canonicalize(path).ok())
        .unwrap();
    for account in [client_peer.public(), server_peer.public()] {
        relay.ordinary(account);
    }
    let mut client = client_peer
        .open(&relay, server_peer.public(), false, &cat)
        .await;
    let mut server = server_peer
        .open_with_terminal(&relay, client_peer.public(), true, &cat, granted)
        .await;
    let remote = server_peer.public().to_bytes();
    let relays = [relay.url.to_string()];
    let room = client
        .open_command_room(remote, false, Instant::now() + STEP)
        .await
        .unwrap();
    run_command(&mut client, &mut server, &room, &cat).await;
    let before = Instant::now();
    client.tick(before + STEP).await.unwrap();
    let empty_turn = before.elapsed();
    let binding = client
        .terminal_client_binding(remote, &relays, room.session)
        .unwrap()
        .expect("exact ready Command room");
    assert_eq!(binding.context.relay_endpoints(), relays);
    assert!(
        client
            .terminal_client_binding(remote, &["wss://other.example/".into()], room.session)
            .is_err()
    );
    assert_eq!(binding.fence.state(), FenceState::Live);
    let server_bindings = server.terminal_server_bindings().unwrap();
    if granted {
        let [server_binding] = server_bindings.as_slice() else {
            panic!("exact granted server binding");
        };
        assert_eq!(server_binding.context.relay_endpoints(), relays);
        assert_eq!(
            server_binding.context.transport_group(),
            binding.context.transport_group()
        );
        assert_eq!(
            server_binding.context.authority_digest(),
            binding.context.authority_digest()
        );
        run_command(&mut client, &mut server, &room, &cat).await;
        assert_eq!(server_binding.fence.state(), FenceState::Live);
        client.terminal_authority.measure_pauses();
        server.terminal_authority.measure_pauses();
        let mut turns = Vec::new();
        for _ in 0..10 {
            let before = Instant::now();
            client.tick(before + STEP).await.unwrap();
            server.tick(Instant::now() + STEP).await.unwrap();
            turns.push(before.elapsed().as_micros());
            assert_eq!(binding.fence.state(), FenceState::Live);
            assert_eq!(server_binding.fence.state(), FenceState::Live);
        }
        turns.sort_unstable();
        let mut pauses = client.terminal_authority.measured_pauses();
        pauses.extend(server.terminal_authority.measured_pauses());
        pauses.sort_unstable();
        assert!(pauses.len() >= 20, "one measured drain per owner and turn");
        let p95 = pauses[(pauses.len() * 95).div_ceil(100) - 1];
        eprintln!(
            "loopback idle empty client tick={}us, fenced client/server pair p50={}us p95={}us",
            empty_turn.as_micros(),
            turns[4],
            turns[9],
        );
        eprintln!(
            "loopback fenced pause samples={} p50={}us p95={}us max={}us",
            pauses.len(),
            pauses[(pauses.len() - 1) / 2].as_micros(),
            p95.as_micros(),
            pauses.last().unwrap().as_micros(),
        );
        assert!(p95 < Duration::from_millis(50), "healthy pause budget");
        let group = GroupId::new(room.group);
        let deadline = Instant::now() + STEP;
        let updates = client.terminal_authority.stage(Some(&group));
        assert_eq!(binding.fence.state(), FenceState::Suspect);
        let effects = client
            .runtime
            .send(SendIntent::RemoveMembers {
                group_id: group.clone(),
                members: vec![MemberId::new(remote.to_vec())],
            })
            .await
            .unwrap();
        client.apply_effects(effects, deadline).await.unwrap();
        client.terminal_finish(updates);
        assert_eq!(binding.fence.state(), FenceState::Retired);
        assert_eq!(server_binding.fence.state(), FenceState::Live);
        // Deliberately exercise the receiver itself without rebuild_routes or
        // the tick's grant pruning being able to retire it earlier.
        while server_binding.fence.state() != FenceState::Retired {
            assert!(
                Instant::now() < deadline,
                "native membership retires receiver"
            );
            server.receive_one(deadline).await.unwrap();
        }

        let next = client
            .open_command_room(remote, true, Instant::now() + STEP)
            .await
            .unwrap();
        run_command(&mut client, &mut server, &next, &cat).await;
        let next_binding = client
            .terminal_client_binding(remote, &relays, next.session)
            .unwrap()
            .unwrap();
        let next_servers = server.terminal_server_bindings().unwrap();
        let [next_server] = next_servers.as_slice() else {
            panic!("single unaffected room after the membership change");
        };
        let group = GroupId::new(next.group);
        let deadline = Instant::now() + STEP;
        let updates = client.terminal_authority.stage(Some(&group));
        let effects = client
            .runtime
            .send(SendIntent::UpdateGroupData {
                group_id: group,
                name: None,
                description: Some("Marmish fixture changed description".into()),
            })
            .await
            .unwrap();
        client.apply_effects(effects, deadline).await.unwrap();
        client.terminal_finish(updates);
        assert_eq!(next_binding.fence.state(), FenceState::Retired);
        assert_eq!(next_server.fence.state(), FenceState::Live);
        while next_server.fence.state() != FenceState::Retired {
            assert!(
                Instant::now() < deadline,
                "native metadata retires receiver"
            );
            server.receive_one(deadline).await.unwrap();
        }
    } else {
        assert!(server_bindings.is_empty());
    }
    client.shutdown(Instant::now() + STEP).await.unwrap();
    assert_eq!(binding.fence.state(), FenceState::Retired);
    server.shutdown(Instant::now() + STEP).await.unwrap();
}
