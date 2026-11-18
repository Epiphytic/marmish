//! Authenticated bootstrap over the bounded NIP-42 fixture with live Command owners.
use super::*;
use crate::terminal_bootstrap::state::{Advance, Exchange, Time};
use crate::terminal_bootstrap::{Codec, Role, Step};
use std::time::Duration;

fn clock() -> Time {
    Time {
        unix: now().unwrap(),
        monotonic: std::time::Instant::now(),
        continuous: None,
    }
}

async fn authenticated_lane(relay: &Relay, path: &Path, codec: &Codec) -> Lane {
    let lane = Lane::open(relay, path, Arc::new(codec.inbound_alias())).await;
    lane.activate(None).await;
    relay
        .observed(
            |entry| matches!(entry, Observation::Authentication(key, true) if *key == lane.public),
        )
        .await;
    relay
        .subscription(lane.public, 1059, "#p", json!(lane.public))
        .await;
    lane
}

async fn deliver(sender: &Lane, recipient: &Lane, frame: &[u8]) -> (MessageId, Vec<u8>) {
    let event: Event = serde_json::from_slice(frame).unwrap();
    assert!(sender.publish(&event).await);
    let delivery = timeout(STEP, recipient.adapter.receive())
        .await
        .expect("bounded alias delivery")
        .unwrap()
        .unwrap();
    assert_eq!(delivery.account_id, recipient.account);
    assert_eq!(delivery.message.id.as_slice(), event.id.to_bytes());
    recipient
        .adapter
        .ack_replay_delivery(&recipient.account, &delivery.message.id)
        .await;
    (delivery.message.id, delivery.message.payload)
}

async fn assert_no_command_ingress(
    relay: &Relay,
    owner: &mut MdkOwner,
    event: &Event,
    canonical: PublicKey,
) {
    let inbox = relay
        .subscription(canonical, 1059, "#p", json!(canonical))
        .await;
    let copy = NostrRelayEvent {
        endpoint: TransportEndpoint(relay.url.to_string()),
        subscription_id: Some(inbox),
        event: NostrTransportEvent::from_nostr_event(event).unwrap(),
    };
    assert_eq!(owner.adapter.handle_relay_event(copy).await.unwrap(), 0);
    let id = MessageId::new(event.id.to_bytes().to_vec());
    assert!(!owner.storage.has_ingress_dedup_marker(&id).unwrap());
    assert!(!owner.storage.has_processed_transport_id(&id).unwrap());
}

#[tokio::test]
#[ignore = "opt-in exact loopback WSS with disposable SQLCipher Command owners and a real worker"]
async fn bootstrap_stays_on_bound_alias_lanes() {
    let temp = tempfile::tempdir().unwrap();
    let relay = Relay::start().await;
    let commands = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(temp.path())
        .unwrap();
    let client_peer = CommandPeer::new(commands.path(), "client");
    let server_peer = CommandPeer::new(commands.path(), "server");
    let client_signer = client_peer.material.signer();
    let server_signer = server_peer.material.signer();
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
    let room = client
        .open_command_room(
            server_peer.public().to_bytes(),
            false,
            Instant::now() + STEP,
        )
        .await
        .unwrap();
    run_command(&mut client, &mut server, &room, &cat).await;
    let alternate = client
        .open_command_room(server_peer.public().to_bytes(), true, Instant::now() + STEP)
        .await
        .unwrap();
    run_command(&mut client, &mut server, &alternate, &cat).await;
    let group = GroupId::new(room.group.clone());
    let alternate_group = GroupId::new(alternate.group.clone());
    let client_context = client.command_context(&group).unwrap();
    let server_context = server.command_context(&group).unwrap();
    let client_codec = Codec::new(&client_context, &client_signer, &[]).unwrap();
    let server_codec = Codec::new(&server_context, &server_signer, &[]).unwrap();
    let other_client = Codec::new(
        &client.command_context(&alternate_group).unwrap(),
        &client_signer,
        &[],
    )
    .unwrap();
    let other_server = Codec::new(
        &server.command_context(&alternate_group).unwrap(),
        &server_signer,
        &[],
    )
    .unwrap();
    assert!(client_codec.binding_digest() == server_codec.binding_digest());
    assert!(other_client.binding_digest() == other_server.binding_digest());
    assert!(client_codec.binding_digest() != other_client.binding_digest());
    assert_eq!(
        client_codec.outbound_alias(),
        server_codec.inbound_alias().public_key()
    );
    assert_eq!(
        server_codec.outbound_alias(),
        client_codec.inbound_alias().public_key()
    );
    assert_ne!(client_codec.outbound_alias(), other_client.outbound_alias());
    assert_ne!(server_codec.outbound_alias(), other_server.outbound_alias());

    let client_read = client_peer.inspect_mdk();
    let server_read = server_peer.inspect_mdk();
    let baseline = (snapshot(&client_read), snapshot(&server_read));
    let package_writes = relay.package_writes();
    let client_lane = authenticated_lane(&relay, temp.path(), &client_codec).await;
    let server_lane = authenticated_lane(&relay, temp.path(), &server_codec).await;
    let alternate_client_lane = authenticated_lane(&relay, temp.path(), &other_client).await;
    let alternate_lane = authenticated_lane(&relay, temp.path(), &other_server).await;
    let mut client_flow = Exchange::new(client_codec);
    let mut server_flow = Exchange::new(server_codec);
    let mut wrong_flow = Exchange::new(other_client);

    wrong_flow.start(&client_signer, clock(), true).unwrap();
    let frame = &wrong_flow.retry(Step::Probe, clock(), true).unwrap().frame;
    let (wrong_id, wrong_frame) = deliver(&alternate_client_lane, &alternate_lane, frame).await;
    drop(wrong_flow);
    assert!(
        timeout(Duration::from_millis(100), server_lane.adapter.receive())
            .await
            .is_err()
    );
    assert!(matches!(
        server_flow.receive(&server_signer, &wrong_frame, clock(), true),
        Err(Error::Invalid("terminal_bootstrap_wrap"))
    ));
    assert!(server_flow.confirmed(clock(), true).is_err());
    assert_no_command_ingress(
        &relay,
        &mut server,
        &serde_json::from_slice(&wrong_frame).unwrap(),
        server_peer.public(),
    )
    .await;

    let began = std::time::Instant::now();
    let live = client.command_context(&group).is_ok();
    client_flow.start(&client_signer, clock(), live).unwrap();
    let mut step = Step::Probe;
    for expected in [
        Advance::Send(Step::Challenge),
        Advance::Send(Step::Confirm),
        Advance::Send(Step::Ready),
        Advance::Confirmed,
    ] {
        let (sender, receiver, signer, sending_lane, receiving_lane, origin, destination) =
            match step.sender() {
                Role::Client => (
                    &mut client_flow,
                    &mut server_flow,
                    &server_signer,
                    &client_lane,
                    &server_lane,
                    &mut client,
                    &mut server,
                ),
                Role::Server => (
                    &mut server_flow,
                    &mut client_flow,
                    &client_signer,
                    &server_lane,
                    &client_lane,
                    &mut server,
                    &mut client,
                ),
            };
        let live = origin.command_context(&group).is_ok();
        let frame = &sender.retry(step, clock(), live).unwrap().frame;
        let (id, received) = deliver(sending_lane, receiving_lane, frame).await;
        let remote_live = destination.command_context(&group).is_ok();
        assert_eq!(
            receiver
                .receive(signer, &received, clock(), remote_live)
                .unwrap(),
            expected
        );
        for owner in [&client, &server] {
            assert!(!owner.storage.has_ingress_dedup_marker(&id).unwrap());
            assert!(!owner.storage.has_processed_transport_id(&id).unwrap());
        }
        if let Advance::Send(next) = expected {
            step = next;
        }
    }
    let elapsed = began.elapsed();
    let (client_key, client_remote, client_id) = client_flow.confirmed(clock(), true).unwrap();
    let (server_key, server_remote, server_id) = server_flow.confirmed(clock(), true).unwrap();
    assert_eq!(client_key.public_key(), server_remote);
    assert_eq!(server_key.public_key(), client_remote);
    assert!(client_id == server_id);
    for _ in 0..4 {
        client.receive_one(Instant::now() + STEP).await.unwrap();
        server.receive_one(Instant::now() + STEP).await.unwrap();
    }
    assert!(!client.storage.has_ingress_dedup_marker(&wrong_id).unwrap());
    assert_eq!((snapshot(&client_read), snapshot(&server_read)), baseline);
    assert_eq!(relay.package_writes(), package_writes);

    client_lane.close().await;
    server_lane.close().await;
    alternate_client_lane.close().await;
    alternate_lane.close().await;
    run_command(&mut client, &mut server, &room, &cat).await;
    client.shutdown(Instant::now() + STEP).await.unwrap();
    server.shutdown(Instant::now() + STEP).await.unwrap();
    relay.assert_healthy();
    eprintln!(
        "terminal_bootstrap loopback_four_record_ms={:.3}",
        elapsed.as_secs_f64() * 1e3
    );
}
