//! Run a real enrollment alongside the durable Command owner on one loopback relay.
use super::*;
use crate::mdk_adapter::terminal_enrollment::Owner as TerminalOwner;
use crate::mdk_adapter::terminal_enrollment_transport::{Authority, Lane as TerminalLane};
use crate::mdk_adapter::terminal_live_enrollment::{
    AttachedRoom, EnrollmentCeiling, InboundState, RoomPermit, admit,
};
use crate::terminal_bootstrap::state::{Advance, Exchange, Time};
use crate::terminal_bootstrap::{BoundSigning, Codec, Role, Step};
use crate::terminal_fence::AttachmentFence;
use crate::terminal_frames as frames;
use cgka_traits::TransportDeliveryPlane;
use std::num::NonZeroU64;
use std::sync::atomic::AtomicU64;
use tokio::sync::Semaphore;

fn clock() -> Time {
    Time {
        unix: now().unwrap(),
        monotonic: std::time::Instant::now(),
        continuous: None,
    }
}

async fn open_alias(
    relay: &Relay,
    path: &Path,
    codec: &Codec,
    active: Arc<AtomicU64>,
) -> TerminalLane {
    let keys = Arc::new(codec.inbound_alias());
    let public = keys.public_key();
    let authority = Authority::new(
        1,
        Arc::new(move |generation| active.load(Ordering::Acquire) == generation),
        Instant::now() + Duration::from_secs(30),
    );
    let endpoint = TransportEndpoint(relay.url.to_string());
    let connector = relay.connector(path, true, None);
    let mut lane = TerminalLane::open(connector, endpoint, keys, authority, 64 * 1024)
        .await
        .unwrap();
    lane.activate_inbox().await.unwrap();
    let actual_inbox = relay.subscription(public, 1059, "#p", json!(public)).await;
    assert_eq!(lane.inbox_id().unwrap(), actual_inbox);
    lane
}

async fn confirm(
    client: &mut TerminalLane,
    server: &mut TerminalLane,
    client_codec: Codec,
    server_codec: Codec,
    client_signer: &Keys,
    server_signer: &Keys,
) -> (Exchange, Exchange) {
    let mut client_exchange = Exchange::new(client_codec);
    let mut server_exchange = Exchange::new(server_codec);
    client_exchange.start(client_signer, clock(), true).unwrap();
    let mut step = Step::Probe;
    for expected in [
        Advance::Send(Step::Challenge),
        Advance::Send(Step::Confirm),
        Advance::Send(Step::Ready),
        Advance::Confirmed,
    ] {
        let (sender, receiver, lane_send, lane_receive, signer) = match step.sender() {
            Role::Client => (
                &mut client_exchange,
                &mut server_exchange,
                &mut *client,
                &mut *server,
                server_signer,
            ),
            Role::Server => (
                &mut server_exchange,
                &mut client_exchange,
                &mut *server,
                &mut *client,
                client_signer,
            ),
        };
        let sealed = sender.retry(step, clock(), true).unwrap();
        lane_send.publish_alias(&sealed.frame).await.unwrap();
        loop {
            let incoming = lane_receive
                .receive(TransportDeliveryPlane::AccountInbox, None, 31 * 1024)
                .await
                .unwrap();
            let advance = receiver
                .receive(signer, &incoming.message.payload, clock(), true)
                .unwrap();
            lane_receive.acknowledge(&incoming.message.id).await;
            if advance != Advance::Duplicate {
                assert_eq!(advance, expected);
                break;
            }
        }
        if let Advance::Send(next) = expected {
            step = next;
        }
    }
    (client_exchange, server_exchange)
}

fn open_journal(peer: &CommandPeer) -> Connection {
    let mut key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), peer.material.sqlcipher_root())
        .expand(b"journal", key.as_mut())
        .unwrap();
    let reader = Connection::open_with_flags(
        peer.lease.journal_database(),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let raw = Zeroizing::new(format!("x'{}'", hex::encode(*key)));
    reader.pragma_update(None, "key", raw.as_str()).unwrap();
    reader.execute_batch("PRAGMA cipher_compatibility=4; PRAGMA cipher_memory_security=ON; PRAGMA cipher_page_size=4096;").unwrap();
    reader
}

fn durable_state(peer: &CommandPeer) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    let md = peer.inspect_mdk();
    let journal = open_journal(peer);
    [
        (&md, "SELECT * FROM cgka_ingress_dedup ORDER BY 1"),
        (&md, "SELECT * FROM cgka_processed_transport_ids ORDER BY 1"),
        (&md, "SELECT * FROM cgka_groups ORDER BY id"),
        (
            &md,
            "SELECT * FROM cgka_key_package_lifecycle ORDER BY singleton",
        ),
        (&journal, "SELECT * FROM command_rooms ORDER BY 1"),
        (
            &journal,
            "SELECT * FROM command_invocations ORDER BY cursor",
        ),
        (
            &journal,
            "SELECT * FROM command_requests ORDER BY group_id,request_id",
        ),
        (
            &journal,
            "SELECT * FROM command_incoming_sources ORDER BY row_id",
        ),
    ]
    .into_iter()
    .map(|(reader, query)| {
        let mut statement = reader.prepare(query).unwrap();
        let columns = statement.column_count();
        statement
            .query_map([], |row| (0..columns).map(|index| row.get(index)).collect())
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    })
    .collect()
}

#[test]
#[ignore = "opt-in real loopback SDK MLS enrollment with an ordinary durable Command control"]
fn enrollment_routes_only_authenticated_terminal_frames() {
    // The unoptimized MDK futures and the underlying MLS codec use a larger stack.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(prove_enrollment(false, false));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "opt-in real loopback WSS NIP-42 readiness qualification"]
fn signed_auth_precedes_terminal_alias_admission() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(prove_enrollment(true, false));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "opt-in real loopback WSS terminal grant retirement before native join"]
fn signed_auth_then_grant_revocation_stops_native_join() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(prove_enrollment(true, true));
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn prove_enrollment(verified: bool, revoke_before_join: bool) {
    let temp = tempfile::tempdir().unwrap();
    let relay = Relay::start().await;
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
    for account in [client_peer.public(), server_peer.public()] {
        relay.ordinary(account);
    }
    let mut client = client_peer
        .open(&relay, server_peer.public(), false, &cat)
        .await;
    let mut server = server_peer
        .open_with_terminal(&relay, client_peer.public(), true, &cat, verified)
        .await;
    let command = client
        .open_command_room(
            server_peer.public().to_bytes(),
            false,
            Instant::now() + STEP,
        )
        .await
        .unwrap();
    run_command(&mut client, &mut server, &command, &cat).await;
    let command_group = GroupId::new(command.group.clone());
    let client_context = client.command_context(&command_group).unwrap();
    let server_context = server.command_context(&command_group).unwrap();
    let mut baseline = (durable_state(&client_peer), durable_state(&server_peer));
    let package_writes = relay.package_writes();
    let client_signer = client_peer.material.signer();
    let server_signer = server_peer.material.signer();
    let command_route = client_context.transport_group();
    let server_grant = Arc::new(AtomicU64::new(1));
    let client_grant = Arc::new(AtomicU64::new(1));
    let start = Instant::now();
    let client_start = EnrollmentCeiling::start(Duration::from_secs(25)).unwrap();
    let mut reserved = None;
    let mut live_fence = None;
    let mut admitted = None;
    let mut legacy = None;
    if verified {
        let relays = [relay.url.to_string()];
        let client_binding = client
            .terminal_client_binding(server_peer.public().to_bytes(), &relays, command.session)
            .unwrap()
            .unwrap();
        let mut server_bindings = server.terminal_server_bindings().unwrap();
        assert_eq!(server_bindings.len(), 1);
        let server_binding = server_bindings.pop().unwrap();
        let (client_signing, client_codec) =
            BoundSigning::bind(&client_binding.context, client_signer).unwrap();
        let (server_signing, server_codec) =
            BoundSigning::bind(&server_binding.context, server_signer).unwrap();
        assert!(
            client_signing
                .check_context(&server_binding.context, &client_codec)
                .is_err()
        );
        let endpoint = &relays[0];
        let client_connector = relay.connector(temp.path(), true, None);
        let server_connector = relay.connector(temp.path(), true, None);
        let client_carrier = admit(
            client_binding.context,
            client_binding.fence,
            client_connector.policy(),
            endpoint,
        )
        .unwrap();
        let server_carrier = admit(
            server_binding.context,
            server_binding.fence.clone(),
            server_connector.policy(),
            endpoint,
        )
        .unwrap();
        let server_alias = server_codec.inbound_alias().public_key();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut client_bootstrap = client_carrier
            .open(client_signing, client_codec, client_connector, deadline)
            .await
            .unwrap();
        relay.acknowledge_duplicates();
        client_bootstrap
            .constrain_client_start(client_start)
            .unwrap();
        client_bootstrap.start_client().await.unwrap();
        client_bootstrap.retry_probe().await.unwrap();
        assert_eq!(relay.duplicate_acks(), 1);
        assert_eq!(
            relay.subscription_counts(server_alias, 1059, "#p", &json!(server_alias)),
            (0, 0)
        );
        let server_bootstrap = server_carrier
            .open(server_signing, server_codec, server_connector, deadline)
            .await
            .unwrap();
        let client_rooms = Arc::new(Semaphore::new(1));
        let server_rooms = Arc::new(Semaphore::new(1));
        let mut inbound = InboundState::default();
        let granted = {
            let grant = server_grant.clone();
            Arc::new(move || grant.load(Ordering::Acquire) == 1)
        };
        let (_, (server_bootstrap, server_permit)) = tokio::try_join!(
            client_bootstrap.finish_client(),
            server_bootstrap.serve(&mut inbound, &server_rooms, granted),
        )
        .unwrap();
        assert_eq!(server_rooms.available_permits(), 0);
        let (client_admitted, server_admitted) = tokio::try_join!(
            client_bootstrap.enroll(
                relay.connector(temp.path(), true, None),
                RoomPermit::new(client_rooms.clone().try_acquire_owned().unwrap()),
                Some({
                    let grant = client_grant.clone();
                    Arc::new(move || grant.load(Ordering::Acquire) == 1)
                }),
            ),
            server_bootstrap.enroll(
                relay.connector(temp.path(), true, None),
                server_permit,
                None
            ),
        )
        .unwrap();
        reserved = Some((client_rooms, server_rooms));
        live_fence = Some(server_binding.fence);
        admitted = Some((client_admitted, server_admitted));
    } else {
        let client_codec = Codec::new(&client_context, &client_signer, &[]).unwrap();
        let server_codec = Codec::new(&server_context, &server_signer, &[]).unwrap();
        let client_grant = Arc::new(AtomicU64::new(1));
        let mut client_alias = open_alias(&relay, temp.path(), &client_codec, client_grant).await;
        let mut server_alias =
            open_alias(&relay, temp.path(), &server_codec, server_grant.clone()).await;
        let (client_exchange, server_exchange) = confirm(
            &mut client_alias,
            &mut server_alias,
            client_codec,
            server_codec,
            &client_signer,
            &server_signer,
        )
        .await;
        legacy = Some(
            tokio::try_join!(
                TerminalOwner::open(
                    client_exchange,
                    client_signer,
                    client_alias,
                    relay.connector(temp.path(), true, None)
                ),
                TerminalOwner::open(
                    server_exchange,
                    server_signer,
                    server_alias,
                    relay.connector(temp.path(), true, None)
                ),
            )
            .unwrap(),
        );
    }
    let (tclient, tserver) = match (&mut admitted, &mut legacy) {
        (Some((client, server)), None) => (client.owner_mut(), server.owner_mut()),
        (None, Some((client, server))) => (client, server),
        _ => unreachable!(),
    };
    let installation = start.elapsed();
    if verified {
        for owner in [&*tclient, &*tserver] {
            let alias = PublicKey::from_slice(owner.alias_adapter.account.as_slice()).unwrap();
            assert_eq!(relay.replay_alias_inbox(alias), 2);
        }
    }
    let group_conflicts = |id: &[u8; 32], route: &[u8; 32]| {
        [command.group.as_slice(), command_route]
            .iter()
            .all(|other| *other != id && *other != route)
    };
    let package_size = tclient
        .offer()
        .await
        .expect("prepare and send withheld package");
    tserver
        .found(group_conflicts)
        .await
        .expect("found and publish native Welcome");
    assert!(tclient.ready().await.is_err());
    assert!(tserver.ready().await.is_err());
    if revoke_before_join {
        client_grant.store(2, Ordering::Release);
        assert!(matches!(
            tclient.join(group_conflicts).await,
            Err(Error::Invalid("terminal_enrollment_authority"))
        ));
        assert!(tclient.room.checked_route().is_err());
        let (client_admitted, server_admitted) = admitted.take().unwrap();
        tokio::try_join!(client_admitted.close(), server_admitted.close()).unwrap();
        let (client_rooms, server_rooms) = reserved.unwrap();
        assert_eq!(client_rooms.available_permits(), 1);
        assert_eq!(server_rooms.available_permits(), 1);
        assert!((durable_state(&client_peer), durable_state(&server_peer)) == baseline);
        assert_eq!(relay.package_writes(), package_writes);
        client.shutdown(Instant::now() + STEP).await.unwrap();
        server.shutdown(Instant::now() + STEP).await.unwrap();
        relay.assert_healthy();
        return;
    }
    if !verified {
        tclient
            .hold_welcome()
            .await
            .expect("early authenticated native Welcome");
    }
    tclient
        .join(group_conflicts)
        .await
        .expect("Plan validation and native join");
    let mut subscriptions = vec![];
    for owner in [&*tclient, &*tserver] {
        owner.room.verify_mismatches().unwrap();
        let room_account =
            PublicKey::from_slice(owner.room.lane.adapter.account.as_slice()).unwrap();
        let alias_account = PublicKey::from_slice(owner.alias_adapter.account.as_slice()).unwrap();
        let (route, sub) = owner.room.checked_route().unwrap();
        let route = json!(hex::encode(route));
        assert_eq!(
            relay
                .subscription(room_account, 445, "#h", route.clone())
                .await,
            sub
        );
        for (account, kind, tag, value) in [
            (alias_account, 1059, "#p", json!(alias_account)),
            (room_account, 1059, "#p", json!(room_account)),
            (room_account, 445, "#h", route),
        ] {
            assert_eq!(
                relay.subscription_counts(account, kind, tag, &value),
                (1, 0)
            );
            let exact = relay.subscription(account, kind, tag, value.clone()).await;
            subscriptions.push((account, kind, tag, value, exact));
        }
    }
    tokio::try_join!(tclient.client_receipts(), tserver.server_receipts())
        .expect("ordered native receipts");
    let ready = start.elapsed();
    let digest = tclient.ready().await.unwrap();
    assert_eq!(tserver.ready().await.unwrap(), digest);

    let bytes: Vec<u8> = (0..=u8::MAX).cycle().take(frames::MAX_DATA).collect();
    let mut attached: Option<(AttachedRoom, AttachedRoom)> = None;
    let original_deadline = verified.then(|| {
        tclient
            .alias_adapter
            .authority
            .deadline()
            .unwrap()
            .max(tserver.alias_adapter.authority.deadline().unwrap())
    });
    let binding = if let Some((client, server)) = admitted.take() {
        let granted = || server_grant.load(Ordering::Acquire) == 1;
        let (mut client, mut server) =
            tokio::try_join!(client.ready(&|| true), server.ready(&granted))
                .expect("move-only continuations after native ClientHello");
        let lease = EnrollmentCeiling::start(Duration::from_secs(90)).unwrap();
        let expired = EnrollmentCeiling::start(Duration::ZERO).unwrap();
        assert_eq!(
            client.admission_ceiling().continuous(),
            client_start.continuous()
        );
        let writer_generation = NonZeroU64::new(17).unwrap();
        let client_binding = client.binding().clone();
        let server_binding = server.binding().clone();
        assert_eq!(
            client_binding.native.local_leaf,
            server_binding.native.peer_leaf
        );
        assert_eq!(
            server_binding.native.local_leaf,
            client_binding.native.peer_leaf
        );
        assert_ne!(
            client_binding.native.local_leaf,
            client_binding.native.peer_leaf
        );
        assert_eq!(client_binding.local_terminal, server_binding.peer_terminal);
        assert_eq!(client_binding.peer_terminal, server_binding.local_terminal);
        assert!(
            client
                .check_proof_binding(&server_binding, [3; 16], lease)
                .is_err()
        );
        assert!(
            server
                .check_proof_binding(&client_binding, [3; 16], lease)
                .is_err()
        );
        assert!(
            client
                .check_proof_binding(&client_binding, [3; 16], expired)
                .is_err()
        );
        assert!(
            client
                .check_proof_binding(&client_binding, [0; 16], lease)
                .is_err()
        );
        // An authenticated pending app must be visible to the selection
        // owner at its non-waiting frontier, before frame permission exists.
        assert!(
            server
                .try_receive_selection(&granted)
                .await
                .unwrap()
                .is_none()
        );
        let pending = frames::encode_app(
            &client_binding.local_terminal,
            now().unwrap(),
            frames::Binding {
                transcript: client_binding.transcript,
                attachment: [3; 16],
                generation: writer_generation.get(),
                role: frames::Role::Writer,
            },
            frames::Direction::ToServer,
            &frames::Record::Ordered(1, frames::Data::Input(vec![0x8f])),
        )
        .unwrap();
        client
            .send_selection(pending.clone(), &|| true)
            .await
            .unwrap();
        let pending_app = timeout(STEP, async {
            loop {
                if let Some(app) = server.try_receive_selection(&granted).await.unwrap() {
                    break app;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the exact queued application reaches selection");
        assert_eq!(
            pending_app.sender_leaf_index,
            server_binding.native.peer_leaf
        );
        assert_eq!(pending_app.bytes, pending);
        let (client, server) = tokio::try_join!(
            client.attach_enrollment_proof(
                &client_binding,
                [3; 16],
                writer_generation,
                lease,
                &|| true
            ),
            server.attach_enrollment_proof(
                &server_binding,
                [3; 16],
                writer_generation,
                lease,
                &granted
            )
        )
        .expect("isolated enrollment frame proof handoff");
        let binding = client.frame_binding();
        assert_eq!(server.frame_binding().transcript, binding.transcript);
        assert_eq!(server.frame_binding().attachment, binding.attachment);
        assert_eq!(server.frame_binding().generation, writer_generation.get());
        assert_eq!(server.frame_binding().generation, binding.generation);
        attached = Some((client, server));
        binding
    } else {
        frames::Binding {
            transcript: digest,
            attachment: [3; 16],
            generation: 1,
            role: frames::Role::Writer,
        }
    };
    let mut elapsed = vec![];
    for sequence in 1..=4 {
        for direction in [frames::Direction::ToServer, frames::Direction::FromServer] {
            if sequence == 4
                && direction == frames::Direction::FromServer
                && let Some(deadline) = original_deadline
            {
                tokio::time::sleep_until(deadline + Duration::from_millis(50)).await;
            }
            let to_server = direction == frames::Direction::ToServer;
            let data = if to_server {
                frames::Data::Input(bytes.clone())
            } else {
                frames::Data::Output(bytes.clone())
            };
            let author = match (&attached, &legacy) {
                (Some((client, server)), None) => {
                    if to_server { client } else { server }
                        .binding()
                        .local_terminal
                }
                (None, Some((client, server))) => {
                    let sender = if to_server { client } else { server };
                    sender.room.terminal_accounts().unwrap().0
                }
                _ => unreachable!(),
            };
            let payload = frames::encode_app(
                &author,
                now().unwrap(),
                binding,
                direction,
                &frames::Record::Ordered(sequence, data),
            )
            .unwrap();
            let started = Instant::now();
            let delivered = match (&mut attached, &mut legacy) {
                (Some((client, server)), None) => {
                    let (sender, receiver) = if to_server {
                        (client, server)
                    } else {
                        (server, client)
                    };
                    sender.send_frame(payload.clone()).await.unwrap();
                    let delivered = receiver.receive_frame().await.unwrap();
                    assert_eq!(
                        delivered.sender_leaf_index,
                        receiver.binding().native.peer_leaf
                    );
                    delivered
                }
                (None, Some((client, server))) => {
                    let (sender, receiver) = if to_server {
                        (client, server)
                    } else {
                        (server, client)
                    };
                    sender.send_frame(binding, payload.clone()).await.unwrap();
                    receiver.receive_frame(binding).await.unwrap()
                }
                _ => unreachable!(),
            };
            elapsed.push(started.elapsed().as_micros());
            assert_eq!(delivered.bytes, payload);
            assert!(
                frames::decode_app(
                    &delivered.bytes,
                    &author,
                    binding,
                    direction,
                    frames::Arrival {
                        retention: Some(delivered.retention),
                        wall: Some(now().unwrap()),
                        monotonic: Some(Duration::ZERO)
                    },
                )
                .is_some()
            );
        }
    }
    if verified {
        assert!(
            (durable_state(&client_peer), durable_state(&server_peer)) == baseline,
            "active Terminal traffic modified the durable Command or MDK state"
        );
        run_command(&mut client, &mut server, &command, &cat).await;
        baseline = (durable_state(&client_peer), durable_state(&server_peer));
        let (client, server) = attached.as_ref().unwrap();
        for endpoint in [client, server] {
            endpoint.check_live().await.unwrap();
            assert_eq!(endpoint.binding().transcript, digest);
        }
    }
    server_grant.store(2, Ordering::Release);
    if let Some(fence) = live_fence.as_ref() {
        revoke(fence);
    }
    match (&mut attached, &mut legacy) {
        (Some((_, server)), None) => {
            assert!(server.receive_frame().await.is_err());
            assert!(server.send_frame(vec![0]).await.is_err());
        }
        (None, Some((_, server))) => {
            assert!(server.receive_frame(binding).await.is_err());
            assert!(server.send_frame(binding, vec![0]).await.is_err());
        }
        _ => unreachable!(),
    }
    let mut exact_ids = Vec::new();
    for (account, kind, tag, value, id) in &subscriptions {
        assert_eq!(relay.subscription_counts(*account, *kind, tag, value).0, 1);
        exact_ids.push((*account, id.clone()));
    }
    match (attached, legacy) {
        (Some((client, server)), None) => tokio::try_join!(client.close(), server.close()).unwrap(),
        (None, Some((client, server))) => tokio::try_join!(client.close(), server.close()).unwrap(),
        _ => unreachable!(),
    };
    timeout(STEP, async {
        loop {
            if exact_ids
                .iter()
                .all(|(account, id)| !relay.has_live_subscription(*account, id))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("all exact Terminal subscriptions closed at relay");
    if let Some((client_rooms, server_rooms)) = reserved {
        assert_eq!(client_rooms.available_permits(), 1);
        assert_eq!(server_rooms.available_permits(), 1);
    }
    assert!(
        (durable_state(&client_peer), durable_state(&server_peer)) == baseline,
        "Terminal enrollment modified the durable Command or MDK state"
    );
    assert_eq!(relay.package_writes(), package_writes);
    run_command(&mut client, &mut server, &command, &cat).await;
    client.shutdown(Instant::now() + STEP).await.unwrap();
    server.shutdown(Instant::now() + STEP).await.unwrap();
    relay.assert_healthy();
    elapsed.sort_unstable();
    eprintln!(
        "terminal_enrollment installation_us={} ready_us={} frame_p50_us={} frame_p95_us={} samples={} package_bytes={}",
        installation.as_micros(),
        ready.as_micros(),
        elapsed[elapsed.len() / 2 - 1],
        elapsed[elapsed.len() - 1],
        elapsed.len(),
        package_size,
    );
}

#[tokio::test]
#[ignore = "opt-in real loopback WSS terminal AUTH refusal and source retirement"]
async fn refuses_incomplete_auth_and_retires_lost_exact_wss_source() {
    for (mode, category) in [
        (Mode::NoChallenge, "terminal_transport_unavailable"),
        (Mode::KnownAccountsOnly, "terminal_enrollment_auth"),
        (Mode::WrongAuthAcknowledgement, "timeout"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let relay = Relay::start().await;
        relay.mode(mode);
        let keys = Arc::new(Keys::generate());
        let generation = NonZeroU64::new(1).unwrap();
        let fence = AttachmentFence::new(generation, [1; 32]);
        let authority =
            Authority::from_fence(fence, Instant::now() + Duration::from_secs(8), None, None)
                .unwrap();
        let endpoint = TransportEndpoint(relay.url.to_string());
        let mut lane = TerminalLane::open_verified(
            relay.connector(temp.path(), true, None),
            endpoint,
            keys,
            authority,
            64 * 1024,
        )
        .await
        .unwrap();
        let error = lane.activate_inbox().await.unwrap_err();
        assert!(matches!(
            (category, error),
            (
                "terminal_transport_unavailable",
                Error::Transport("terminal_transport_unavailable")
            ) | (
                "terminal_enrollment_auth",
                Error::Transport("terminal_enrollment_auth")
            ) | ("timeout", Error::Timeout)
        ));
        lane.close().await.unwrap();
        relay.assert_healthy();
    }
    for disconnect in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let relay = Relay::start().await;
        let keys = Arc::new(Keys::generate());
        let public = keys.public_key();
        let publisher = Keys::generate();
        relay.seed_stored_events((0..41).map(|sequence| {
            let size = if sequence == 0 { 40 } else { 30 };
            nostr::event::EventBuilder::new(
                Kind::GiftWrap,
                format!("{}-{sequence}", "x".repeat(size * 1024)),
            )
            .tag(nostr::event::Tag::public_key(public))
            .finalize(&publisher)
            .unwrap()
        }));
        let generation = NonZeroU64::new(1).unwrap();
        let fence = AttachmentFence::new(generation, [1; 32]);
        let authority =
            Authority::from_fence(fence, Instant::now() + Duration::from_secs(8), None, None)
                .unwrap();
        let endpoint = TransportEndpoint(relay.url.to_string());
        let mut lane = TerminalLane::open_verified(
            relay.connector(temp.path(), true, None),
            endpoint,
            keys,
            authority,
            64 * 1024,
        )
        .await
        .unwrap();
        lane.activate_inbox().await.unwrap();
        let (stored, dropped) = lane.stored_backlog();
        assert!(stored > 0 && dropped > 0);
        assert_eq!(stored + dropped, 40, "40 KiB alias copy is inert");
        eprintln!("terminal_auth_backlog stored={stored} dropped={dropped}");
        let inbox = lane.inbox_id().unwrap().to_owned();
        if disconnect {
            relay.disconnect(public);
        } else {
            relay.refuse_exact_subscription(public, &inbox);
        }
        timeout(STEP, async {
            while lane.adapter.authority.check().is_ok() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("socket retirement observed");
        timeout(
            STEP,
            lane.receive(TransportDeliveryPlane::AccountInbox, None, 32 * 1024),
        )
        .await
        .unwrap()
        .expect_err("the verified source cannot deliver after loss");
        assert!(lane.publish_alias(b"inactive").await.is_err());
        lane.close().await.unwrap();
        timeout(STEP, async {
            while relay.has_live_subscription(public, &inbox) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("relay released retired source");
        relay.assert_healthy();
    }
}

fn revoke(fence: &AttachmentFence) {
    fence.retire();
}

#[path = "terminal_selection_wss_tests.rs"]
mod selection_proof;
