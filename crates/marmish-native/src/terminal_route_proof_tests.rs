//! Opt-in proof against the native SDK, adapter and loopback NIP-42 relay.
use super::terminal_relay_fixture::{Mode, Observation, Relay, STEP};
use super::terminal_room_fixture::{CapturedRoom, prove};
use super::*;
use nostr::event::Event;
use serde_json::json;
use std::path::Path;
use tokio::time::timeout;
use transport_nostr_adapter::NostrRelayEvent;
#[path = "terminal_command_fixture.rs"]
mod terminal_command_fixture;
pub(crate) use terminal_command_fixture::{CommandPeer, open_cli_pty};

struct Lane {
    client: Client,
    connector: RelayConnector,
    relay_client: Arc<NostrSdkRelayClient>,
    adapter: NostrTransportAdapter,
    account: MemberId,
    public: PublicKey,
    endpoint: TransportEndpoint,
    forwarder: tokio::task::JoinHandle<()>,
}

impl Lane {
    async fn open(relay: &Relay, path: &Path, keys: Arc<Keys>) -> Self {
        let connector = relay.connector(path, true, None);
        connector.bootstrap(Instant::now() + STEP).await.unwrap();
        let client = connector.sdk_client(keys.clone()).unwrap();
        let public = keys.public_key();
        let account = MemberId::new(public.to_bytes().to_vec());
        let relay_client = Arc::new(NostrSdkRelayClient::with_account_signer(
            client.clone(),
            account.clone(),
            keys,
        ));
        let adapter = NostrTransportAdapter::new(relay_client.clone());
        let forwarder = relay_client.spawn_notification_forwarder(adapter.clone());
        Self {
            connector,
            client,
            relay_client,
            adapter,
            account,
            public,
            endpoint: TransportEndpoint(relay.url.to_string()),
            forwarder,
        }
    }

    async fn activate(&self, room: Option<&CapturedRoom>) {
        let groups = room
            .into_iter()
            .map(|room| TransportGroupSubscription {
                group_id: room.group.clone(),
                transport_group_id: room.route.to_vec(),
                endpoints: vec![self.endpoint.clone()],
                retained_since: None,
            })
            .collect();
        timeout(
            STEP,
            self.adapter.activate_account(TransportAccountActivation {
                account_id: self.account.clone(),
                inbox_endpoints: vec![self.endpoint.clone()],
                group_subscriptions: groups,
                since: None,
            }),
        )
        .await
        .unwrap()
        .unwrap();
    }

    async fn publish(&self, event: &Event) -> bool {
        timeout(
            STEP,
            self.relay_client.publish_event_for_account(
                &self.account,
                std::slice::from_ref(&self.endpoint),
                &NostrTransportEvent::from_nostr_event(event).unwrap(),
                1,
            ),
        )
        .await
        .expect("bounded publish")
        .is_ok_and(|outcome| outcome.accepted.len() == 1)
    }

    async fn receive_match(&self, event: &Event) -> TransportDelivery {
        self.receive_match_after(event, None).await
    }

    async fn receive_match_after(
        &self,
        event: &Event,
        mut prior: Option<&Event>,
    ) -> TransportDelivery {
        timeout(STEP, async {
            loop {
                let received = self.adapter.receive().await.unwrap().unwrap();
                assert_eq!(received.account_id, self.account);
                self.adapter
                    .ack_replay_delivery(&self.account, &received.message.id)
                    .await;
                if received.message.id.as_slice() == event.id.to_bytes() {
                    return received;
                }
                let replay = prior.take().expect("only one previous test publication");
                assert_eq!(received.message.id.as_slice(), replay.id.to_bytes());
            }
        })
        .await
        .unwrap()
    }

    fn relay_event(&self, id: String, event: &Event) -> NostrRelayEvent {
        NostrRelayEvent {
            endpoint: self.endpoint.clone(),
            subscription_id: Some(id),
            event: NostrTransportEvent::from_nostr_event(event).unwrap(),
        }
    }

    async fn close(self) {
        timeout(STEP, self.client.shutdown()).await.unwrap();
        self.connector.cancel();
        self.forwarder.abort();
    }
}

async fn subscriptions(relay: &Relay, lane: &Lane, route: &[u8; 32]) -> (String, String) {
    let inbox = relay
        .subscription(lane.public, 1059, "#p", json!(lane.public))
        .await;
    let room = relay
        .subscription(lane.public, 445, "#h", json!(hex::encode(route)))
        .await;
    (inbox, room)
}

async fn reject_other_room(
    relay: &Relay,
    lane: &Lane,
    ids: &(String, String),
    events: &CapturedRoom,
) {
    relay.inject(lane.public, &ids.0, &events.welcome);
    relay.inject(lane.public, &ids.1, &events.frame);
    assert_eq!(
        lane.adapter
            .handle_relay_event(lane.relay_event(ids.0.clone(), &events.welcome))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        lane.adapter
            .handle_relay_event(lane.relay_event(ids.1.clone(), &events.frame))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        lane.adapter
            .handle_group_relay_copy(
                lane.relay_event(ids.1.clone(), &events.frame),
                Some(&lane.account)
            )
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
#[ignore = "opt-in real native SDK and in-memory MDK relay test"]
async fn authenticated_relay_accepts_native_events_and_rejects_cross_room_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let relay = Relay::start().await;
    let first = prove(0, relay.url.as_str()).await;
    let second = prove(1, relay.url.as_str()).await;
    assert_ne!(first.group, second.group);
    assert_ne!(first.route, second.route);
    assert_ne!(first.joiner.public_key(), second.joiner.public_key());
    let sender = Lane::open(&relay, temp.path(), first.founder.clone()).await;
    sender.activate(None).await;
    let first_lane = Lane::open(&relay, temp.path(), first.joiner.clone()).await;
    let second_lane = Lane::open(&relay, temp.path(), second.joiner.clone()).await;
    first_lane.activate(Some(&first)).await;
    second_lane.activate(Some(&second)).await;
    let first_ids = subscriptions(&relay, &first_lane, &first.route).await;
    let second_ids = subscriptions(&relay, &second_lane, &second.route).await;
    reject_other_room(&relay, &first_lane, &first_ids, &second).await;
    reject_other_room(&relay, &second_lane, &second_ids, &first).await;

    assert_ne!(sender.public, first.welcome.pubkey);
    assert_ne!(sender.public, first.frame.pubkey);
    assert!(sender.publish(&first.welcome).await);
    first_lane.receive_match(&first.welcome).await;
    assert!(sender.publish(&first.frame).await);
    first_lane.receive_match(&first.frame).await;
    for event in [&first.welcome, &first.frame] {
        relay
            .observed(|entry| matches!(entry, Observation::Write(id, _, true) if id == &event.id))
            .await;
    }
    relay.observed(|entry| matches!(entry, Observation::Read(true, Some(key)) if *key == first_lane.public)).await;

    // The generic API admits a valid same-account Welcome for an unissued ID.
    // That case is independent of the zero-delivery checks across adapters.
    let foreign_id = "unissued-fixture-subscription".into();
    assert_eq!(
        second_lane
            .adapter
            .handle_relay_event(second_lane.relay_event(foreign_id, &second.welcome))
            .await
            .unwrap(),
        1
    );
    second_lane.receive_match(&second.welcome).await;
    // Positive group control on the second cross-room lane after its negative copies.
    assert!(sender.publish(&second.frame).await);
    second_lane.receive_match(&second.frame).await;
    first.assert_no_package_publication();
    second.assert_no_package_publication();
    relay.assert_zero_package_writes();
    sender.close().await;
    first_lane.close().await;
    second_lane.close().await;
}

#[tokio::test]
#[ignore = "opt-in native SDK TLS root and exact loopback endpoint check"]
async fn sdk_fixture_tls_trust_is_private_and_exact() {
    let temp = tempfile::tempdir().unwrap();
    let relay = Relay::start().await;
    let other = Relay::start().await;
    let untrusted = relay.connector(temp.path(), false, None);
    let wrong = relay.connector(temp.path(), true, Some(&other.url));
    for (connector, url) in [(untrusted, &relay.url), (wrong, &other.url)] {
        connector.bootstrap(Instant::now() + STEP).await.unwrap();
        let sdk = connector.sdk_client(Arc::new(Keys::generate())).unwrap();
        assert!(
            connect_configured_relay(&connector, &sdk, url, Instant::now() + STEP)
                .await
                .is_err()
        );
        timeout(STEP, sdk.shutdown()).await.unwrap();
    }
    let connector = relay.connector(temp.path(), true, None);
    connector.bootstrap(Instant::now() + STEP).await.unwrap();
    let sdk = connector.sdk_client(Arc::new(Keys::generate())).unwrap();
    connect_configured_relay(&connector, &sdk, &relay.url, Instant::now() + STEP)
        .await
        .unwrap();
    timeout(STEP, sdk.shutdown()).await.unwrap();
}

#[tokio::test]
#[ignore = "opt-in private WSS and native MDK acceptance for maximum Terminal v1 records"]
async fn maximum_terminal_records_round_trip_through_private_wss() {
    timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        let relay = Relay::start().await;
        let mut room = prove(0, relay.url.as_str()).await;
        let other = prove(1, relay.url.as_str()).await;
        let keys = [room.founder.clone(), room.joiner.clone()];
        let unrelated = Lane::open(&relay, temp.path(), other.joiner.clone()).await;
        unrelated.activate(Some(&other)).await;
        let (_, unrelated_group) = subscriptions(&relay, &unrelated, &other.route).await;

        let mut prior = None;
        for (sender, name) in [(0, "output"), (1, "input")] {
            let publisher = Lane::open(&relay, temp.path(), keys[sender].clone()).await;
            publisher.activate(None).await;
            let subscriber = Lane::open(&relay, temp.path(), keys[1 - sender].clone()).await;
            subscriber.activate(Some(&room)).await;
            subscriptions(&relay, &subscriber, &room.route).await;
            let frame = room.prepare_maximum_frame(sender).await;
            assert_eq!(
                unrelated.adapter.handle_relay_event(
                    unrelated.relay_event(unrelated_group.clone(), &frame.event)
                ).await.unwrap(),
                0
            );
            relay.inject(unrelated.public, &unrelated_group, &frame.event);
            let started = Instant::now();
            assert!(publisher.publish(&frame.event).await);
            let accepted = subscriber.receive_match_after(&frame.event, prior.as_ref()).await;
            room.accept_maximum_frame(&frame, Some(accepted)).await;
            let wss_bytes = relay.accepted_event_bytes(frame.event.id).await;
            assert!(wss_bytes <= 32 * 1024);
            println!(
                "terminal_frame_wss direction={name} payload_bytes=4096 app_bytes={} protected_content_bytes={} outer_bytes={} wss_request_bytes={wss_bytes} publish_receive_open_us={}",
                frame.app_bytes,
                frame.protected_bytes,
                frame.outer_bytes,
                started.elapsed().as_micros()
            );
            prior = Some(frame.event);
            subscriber.close().await;
            publisher.close().await;
        }
        assert!(timeout(Duration::from_millis(100), unrelated.adapter.receive()).await.is_err());
        unrelated.close().await;
        room.assert_no_package_publication();
        relay.assert_zero_package_writes();
        relay.assert_healthy();
    }).await.expect("bounded loopback qualification");
}
