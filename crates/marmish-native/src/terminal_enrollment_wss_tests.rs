//! Run a real enrollment alongside the durable Command owner on one loopback relay.
use super::*;
use crate::mdk_adapter::terminal_enrollment::Owner as TerminalOwner;
use crate::mdk_adapter::terminal_enrollment_transport::{Authority, Lane as TerminalLane};
use crate::terminal_bootstrap::state::{Advance, Exchange, Time};
use crate::terminal_bootstrap::{Codec, Role, Step};
use crate::terminal_frames as frames;
use cgka_traits::TransportDeliveryPlane;
use std::sync::atomic::AtomicU64;

fn clock() -> Time {
    Time {
        unix: now().unwrap(),
        monotonic: std::time::Instant::now(),
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
    let mut lane = TerminalLane::open(
        relay.connector(path, true, None),
        TransportEndpoint(relay.url.to_string()),
        keys,
        authority,
        64 * 1024,
    )
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
                .block_on(prove_enrollment());
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn prove_enrollment() {
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
        .open(&relay, client_peer.public(), true, &cat)
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
    let baseline = (durable_state(&client_peer), durable_state(&server_peer));
    let package_writes = relay.package_writes();
    let client_signer = client_peer.material.signer();
    let server_signer = server_peer.material.signer();
    let command_route = client_context.transport_group();
    let client_codec = Codec::new(&client_context, &client_signer, &[]).unwrap();
    let server_codec = Codec::new(&server_context, &server_signer, &[]).unwrap();
    let client_grant = Arc::new(AtomicU64::new(1));
    let server_grant = Arc::new(AtomicU64::new(1));
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
    let start = Instant::now();
    let (mut tclient, mut tserver) = tokio::try_join!(
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
    .unwrap();
    let installation = start.elapsed();
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
    tclient
        .hold_welcome()
        .await
        .expect("early authenticated native Welcome");
    tclient
        .join(group_conflicts)
        .await
        .expect("Plan validation and native join");
    let mut subscriptions = vec![];
    for owner in [&tclient, &tserver] {
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
            subscriptions.push((account, kind, tag, value));
        }
    }
    tokio::try_join!(tclient.client_receipts(), tserver.server_receipts())
        .expect("ordered native receipts");
    let ready = start.elapsed();
    let digest = tclient.ready().await.unwrap();
    assert_eq!(tserver.ready().await.unwrap(), digest);

    let bytes: Vec<u8> = (0..=u8::MAX).cycle().take(frames::MAX_DATA).collect();
    let binding = frames::Binding {
        transcript: digest,
        attachment: [3; 16],
        generation: 1,
        role: frames::Role::Writer,
    };
    let mut elapsed = vec![];
    for sequence in 1..=4 {
        for direction in [frames::Direction::ToServer, frames::Direction::FromServer] {
            let (sender, receiver, data) = match direction {
                frames::Direction::ToServer => (
                    &mut tclient,
                    &mut tserver,
                    frames::Data::Input(bytes.clone()),
                ),
                frames::Direction::FromServer => (
                    &mut tserver,
                    &mut tclient,
                    frames::Data::Output(bytes.clone()),
                ),
            };
            let author: [u8; 32] = sender
                .room
                .lane
                .adapter
                .account
                .as_slice()
                .try_into()
                .unwrap();
            let payload = frames::encode_app(
                &author,
                now().unwrap(),
                binding,
                direction,
                &frames::Record::Ordered(sequence, data),
            )
            .unwrap();
            let started = Instant::now();
            sender.send_frame(binding, payload.clone()).await.unwrap();
            let delivered = receiver.receive_frame(binding).await.unwrap();
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
    server_grant.store(2, Ordering::Release);
    assert!(tserver.receive_frame(binding).await.is_err());
    assert!(tserver.send_frame(binding, vec![0]).await.is_err());
    let mut exact_ids = Vec::new();
    for (account, kind, tag, value) in &subscriptions {
        assert_eq!(relay.subscription_counts(*account, *kind, tag, value).0, 1);
        exact_ids.push((
            *account,
            relay
                .subscription(*account, *kind, tag, value.clone())
                .await,
        ));
    }
    tokio::try_join!(tclient.close(), tserver.close()).unwrap();
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
