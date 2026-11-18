use std::time::Duration;

use cgka_traits::app_components::NostrRoutingV1;
use nostr::{event::Event, key::Keys};
use serde_json::{json, value::RawValue};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::{
    Message,
    protocol::frame::{
        Frame,
        coding::{Data, OpCode},
    },
};

use crate::transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits};

use super::{
    fixture::{CHALLENGE, Peer, Relay, deadline, event_message},
    owner::{Owner, publication},
    peer_control_wire::{Codec, MAX_OUTER_BYTES, Offer, Prepared},
    wire::{self, Arrival, Budget, Failure, MESSAGE_LIMIT, Packet, Reserve},
};

const NOW: u64 = 1_800_000_000;
const QUICK: Duration = Duration::from_millis(80);

fn carrier(recipient: &str, id: u8, stamp: u64, content: &str) -> String {
    json!({
        "id": hex::encode([id; 32]),
        "pubkey": "ab".repeat(32),
        "created_at": stamp,
        "kind": 7435,
        "tags": [["p", recipient], ["expiration", "21600"]],
        "content": content,
        "sig": "db".repeat(64)
    })
    .to_string()
}

#[test]
fn peer_raw_socket_core_packet_and_header() {
    for packet in [
        r#"["EOSE"]"#,
        r#"["EOSE",7]"#,
        r#"["EOSE","live",null]"#,
        r#"["EVENT","live",{},"extra"]"#,
        r#"["NOTICE","private"]"#,
        r#"["AUTH","\t"]"#,
        r#"{"0":"EOSE","1":"live"}"#,
        r#"["EOSE","live"][]"#,
    ] {
        assert_eq!(
            wire::packet(packet).err(),
            Some(Failure::Grammar),
            "{packet}"
        );
    }
    let accepted = json!(["OK", "af".repeat(32), true, "not exposed"]);
    assert!(matches!(
        wire::packet(&accepted.to_string()),
        Ok(Packet::Acknowledged(_, true))
    ));
    for value in [json!("true"), json!(1), json!(null)] {
        let text = json!(["OK", "af".repeat(32), value, "detail"]).to_string();
        assert_eq!(wire::packet(&text).err(), Some(Failure::Grammar));
    }
    assert_eq!(
        wire::packet(&" ".repeat(MESSAGE_LIMIT + 1)).err(),
        Some(Failure::Size)
    );

    for challenge in ["z", &"z".repeat(256)] {
        let text = json!(["AUTH", challenge]).to_string();
        assert!(matches!(wire::packet(&text), Ok(Packet::Challenge(value)) if value == challenge));
    }

    let address = hex::encode([4; 32]);
    let body = carrier(&address, 5, 21600, "");
    let size = MAX_OUTER_BYTES - body.len();
    let boundary = carrier(&address, 5, 21600, &"x".repeat(size));
    assert_eq!(boundary.len(), MAX_OUTER_BYTES);
    let raw: &RawValue = serde_json::from_str(&boundary).unwrap();
    let received = Arrival::parse(raw, &[4; 32]).unwrap();
    assert_eq!(received.id, [5; 32]);
    assert_eq!(received.stamp, 21600);
    assert_eq!(received.body.get(), boundary);
    let over = carrier(&address, 5, 21600, &"x".repeat(size + 1));
    assert_eq!(
        Arrival::index(serde_json::from_str(&over).unwrap(), &[4; 32]),
        Err(Failure::Size)
    );
    assert_eq!(Arrival::index(raw, &[8; 32]), Err(Failure::Grammar));
    for bad in [
        body.replace("\"kind\":7435", "\"kind\":7435,\"kind\":7435"),
        body.replace("\"kind\":7435", "\"kind\":7435,\"unknown\":1"),
        body.replace("\"created_at\":21600", "\"created_at\":\"21600\""),
        body.replace("\"created_at\":21600", "\"created_at\":21600.0"),
        body.replace("\"created_at\":21600", "\"created_at\":-1"),
        body.replace("\"expiration\",\"21600\"", "\"expiration\",\"021600\""),
        body.replace("\"p\",", "\"x\","),
        body.replace(&"db".repeat(64), &"DB".repeat(64)),
    ] {
        let raw: &RawValue = serde_json::from_str(&bad).unwrap();
        assert_eq!(
            Arrival::index(raw, &[4; 32]),
            Err(Failure::Grammar),
            "{bad}"
        );
    }
}

#[test]
fn peer_raw_socket_core_rejects_noncanonical_header_fields() {
    let address = "fa".repeat(32);
    let body = carrier(&address, 0xaf, NOW, "");
    for (field, value) in [
        ("kind", json!(1059)),
        ("id", json!("AF".repeat(32))),
        ("pubkey", json!("AB".repeat(32))),
        ("pubkey", json!(address)),
        (
            "tags",
            json!([["p", address.to_ascii_uppercase()], ["expiration", "21600"]]),
        ),
        ("tags", json!([["expiration", "21600"], ["p", address]])),
        (
            "tags",
            json!([["p", address], ["expiration", "21600"], ["x", "extra"]]),
        ),
        (
            "tags",
            json!([["p", address, "extra"], ["expiration", "21600"]]),
        ),
        ("tags", json!([["p", address], ["expiration", "0"]])),
        ("tags", json!([["p", address], ["expiration", "21601"]])),
        ("tags", json!([["p", address], ["expiration", "+21600"]])),
    ] {
        let mut changed: serde_json::Value = serde_json::from_str(&body).unwrap();
        changed[field] = value;
        let raw = RawValue::from_string(changed.to_string()).unwrap();
        assert_eq!(
            Arrival::index(&raw, &[0xfa; 32]),
            Err(Failure::Grammar),
            "{field}"
        );
    }
    let mut missing: serde_json::Value = serde_json::from_str(&body).unwrap();
    missing.as_object_mut().unwrap().remove("content");
    let raw = RawValue::from_string(missing.to_string()).unwrap();
    assert_eq!(Arrival::index(&raw, &[0xfa; 32]), Err(Failure::Grammar));
}

#[test]
fn peer_raw_socket_core_reserves_and_budgets() {
    let mut live = Reserve::default();
    let mut page = Reserve::default();
    for _ in 0..4 {
        for reserve in [&mut live, &mut page] {
            reserve.charge(MAX_OUTER_BYTES).unwrap();
        }
    }
    assert_eq!(live.charge(1), Err(Failure::Capacity));
    assert_eq!(page.charge(1), Err(Failure::Capacity));
    live.release(MAX_OUTER_BYTES);
    assert_eq!(live.charge(MAX_OUTER_BYTES + 1), Err(Failure::Size));
    live.charge(MAX_OUTER_BYTES).unwrap();
    let mut budget = Budget::default();
    for index in 0..24 {
        budget.admit(index < 8).unwrap();
    }
    assert_eq!(budget.admit(false), Err(Failure::Capacity));
    let mut budget = Budget::default();
    for _ in 0..8 {
        budget.admit(true).unwrap();
    }
    assert_eq!(budget.admit(true), Err(Failure::Capacity));
}

#[tokio::test]
async fn peer_raw_socket_core_invalid_binding_never_connects() {
    let local = Keys::generate();
    let other = Keys::generate();
    let tmp = tempfile::tempdir().unwrap();
    let configured = "wss://configured.invalid/";
    for (origins, requested, recipient, until, error) in [
        (
            vec![configured],
            "ws://configured.invalid/",
            local.public_key(),
            deadline(),
            Failure::Binding,
        ),
        (
            vec![configured],
            "wss://configured.invalid",
            local.public_key(),
            deadline(),
            Failure::Binding,
        ),
        (
            vec![configured],
            configured,
            other.public_key(),
            deadline(),
            Failure::Binding,
        ),
        (
            vec![configured, "wss://other.invalid/"],
            configured,
            local.public_key(),
            deadline(),
            Failure::Binding,
        ),
        (
            vec![configured],
            configured,
            local.public_key(),
            Instant::now(),
            Failure::Deadline,
        ),
    ] {
        let origins: Vec<_> = origins.into_iter().map(str::to_owned).collect();
        let policy =
            RelayPolicy::parse(&origins, RouteMode::Off, TransportLimits::default()).unwrap();
        let dirs = ArtiDirs {
            state: tmp.path().join("state"),
            cache: tmp.path().join("cache"),
        };
        let connector = RelayConnector::new(policy, dirs).unwrap();
        assert_eq!(
            Owner::open(&connector, requested, recipient, &local, until)
                .await
                .err(),
            Some(error)
        );
        assert_eq!(connector.direct_attempt_count(), 0);
    }
}

async fn attached(
    relay: &Relay,
    connector: &RelayConnector,
    local: &Keys,
) -> (Owner, Peer, String) {
    let (owner, (peer, sub)) = tokio::join!(
        Owner::open(
            connector,
            relay.url.as_str(),
            local.public_key(),
            local,
            deadline()
        ),
        relay.ready(local.public_key()),
    );
    (owner.unwrap(), peer, sub)
}

fn prepared(
    origin: &str,
    author: &Keys,
    recipient: &Keys,
) -> (Codec, Prepared<Offer>, Vec<String>) {
    let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
    let relays = vec![origin.to_owned()];
    let route = NostrRoutingV1::new([35; 32], relays.clone()).unwrap();
    let prepared = codec
        .prepare_offer(author, recipient.public_key(), [47; 16], &route, NOW)
        .unwrap();
    (codec, prepared, relays)
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_signed_offer_and_slot() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let writer = Keys::generate();
    let reader = Keys::generate();
    let writer_connector = relay.connector(&tmp.path().join("writer"));
    let reader_connector = relay.connector(&tmp.path().join("reader"));
    let (mut source, mut sender, sender_id) = attached(&relay, &writer_connector, &writer).await;
    let (mut incoming, mut receiver, live_id) = attached(&relay, &reader_connector, &reader).await;
    assert_ne!(sender_id, live_id);
    assert_eq!(incoming.origin(), relay.url.as_str());
    let second = Owner::open(
        &reader_connector,
        relay.url.as_str(),
        reader.public_key(),
        &reader,
        Instant::now() + QUICK,
    )
    .await;
    assert_eq!(second.err(), Some(Failure::Deadline));
    assert_eq!(reader_connector.direct_attempt_count(), 1);
    assert!(incoming.wait_live(Instant::now()).await.unwrap().is_none());
    assert!(
        incoming
            .wait_live(Instant::now() + QUICK)
            .await
            .unwrap()
            .is_none()
    );

    let (codec, prepared, relays) = prepared(relay.url.as_str(), &writer, &reader);
    let flight = prepared.flight.flight();
    let original = prepared.frame().to_vec();
    let original_text = std::str::from_utf8(&original).unwrap();
    let event: Event = serde_json::from_slice(&original).unwrap();
    assert_eq!(event.id, flight.carrier_id);
    let relayed = serde_json::to_string_pretty(&event).unwrap();
    assert_ne!(relayed, original_text);
    assert_eq!(flight.recipient, reader.public_key());
    assert_ne!(flight.recipient, writer.public_key());
    let carrier = flight.carrier_id.to_bytes();
    let (ack, ()) = tokio::join!(
        source.publish_bytes(&original, flight.recipient, carrier, deadline()),
        async {
            let event = sender.publication(&original, writer.public_key()).await;
            sender.ack(&event, true).await;
        }
    );
    let ack = ack.unwrap();
    assert!(source.accepted(&ack, carrier));
    assert!(!source.accepted(&ack, [0; 32]));
    assert!(!incoming.accepted(&ack, carrier));
    let (live, ()) = tokio::join!(
        incoming.wait_live(deadline()),
        receiver.deliver(&live_id, &relayed)
    );
    let live = live.unwrap().unwrap();
    assert!(incoming.accepts_live(&live));
    assert!(!source.accepts_live(&live));
    assert_eq!(live.arrival().body.get(), relayed);
    let offer = codec
        .open_offer(
            &reader,
            live.arrival().body.get().as_bytes(),
            Some(writer.public_key()),
            &relays,
            NOW,
        )
        .unwrap();
    assert_eq!(offer.flight().carrier_id, flight.carrier_id);

    let stamp = Arrival::index(
        serde_json::from_str(&relayed).unwrap(),
        &reader.public_key().to_bytes(),
    )
    .unwrap()
    .1;
    let bounds = [stamp, stamp];
    let (page, page_id) = tokio::join!(incoming.page(bounds, deadline()), async {
        let page_id = receiver.request(reader.public_key(), Some(bounds)).await;
        assert_ne!(page_id, live_id);
        receiver.deliver(&page_id, &relayed).await;
        receiver.deliver(&live_id, &relayed).await;
        receiver.end(&page_id).await;
        receiver.close(&page_id).await;
        page_id
    });
    let page = page.unwrap();
    assert_eq!(page.request(), (page_id.as_str(), bounds));
    assert!(!page.is_full());
    assert!(incoming.accepts_page(&page));
    assert!(!source.accepts_page(&page));
    let [receipt] = page.records() else {
        panic!("expected one verified carrier")
    };
    assert_eq!(receipt.body.get(), relayed);
    let offer = codec
        .open_offer(
            &reader,
            receipt.body.get().as_bytes(),
            Some(writer.public_key()),
            &relays,
            NOW,
        )
        .unwrap();
    assert_eq!(offer.flight().carrier_id, flight.carrier_id);
    assert_eq!(
        incoming
            .wait_live(deadline())
            .await
            .unwrap()
            .unwrap()
            .arrival()
            .body
            .get(),
        relayed
    );

    let (refused, ()) = tokio::join!(source.publish(&prepared, flight, deadline()), async {
        let event = sender.publication(&original, writer.public_key()).await;
        sender.ack(&event, false).await;
    });
    assert!(!refused.unwrap());
    assert!(source.wait_live(Instant::now()).await.unwrap().is_none());
    assert_eq!(prepared.frame(), original);
    let (stopped, ()) = tokio::join!(incoming.stop(deadline()), async {
        receiver.close(&live_id).await;
        assert!(matches!(receiver.receive().await, Message::Close(_)));
    });
    stopped.unwrap();
    assert!(!incoming.accepts_page(&page));
    assert!(!incoming.accepts_live(&live));
    let (replacement, _, replacement_id) = attached(&relay, &reader_connector, &reader).await;
    assert_ne!(replacement_id, live_id);
    assert!(!replacement.accepts_page(&page));
    assert!(!replacement.accepts_live(&live));
    drop(replacement);
    let (_, _, third_id) = attached(&relay, &reader_connector, &reader).await;
    assert_ne!(third_id, replacement_id);
    assert_eq!(reader_connector.direct_attempt_count(), 3);
    let (stopped, ()) = tokio::join!(source.stop(deadline()), async {
        sender.close(&sender_id).await;
        assert!(matches!(sender.receive().await, Message::Close(_)));
    });
    stopped.unwrap();
    assert!(!source.accepted(&ack, carrier));
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_auth_transitions() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let local = Keys::generate();
    let connector = relay.connector(tmp.path());
    for first in [
        json!(["AUTH", ""]),
        json!(["AUTH", "x".repeat(257)]),
        json!(["AUTH", "x\n"]),
        json!(["OK", "ab".repeat(32), true, "too early"]),
    ] {
        let (owner, ()) = tokio::join!(
            Owner::open(
                &connector,
                relay.url.as_str(),
                local.public_key(),
                &local,
                deadline()
            ),
            async {
                let mut peer = relay.accept().await;
                peer.packet(first).await;
                peer.assert_no_request().await;
            },
        );
        assert_eq!(owner.err(), Some(Failure::Grammar));
    }
    for stage in [
        "wrong_ok",
        "negative_ok",
        "repeated_auth",
        "wrong_eose",
        "early_event",
    ] {
        let (owner, ()) = tokio::join!(
            Owner::open(
                &connector,
                relay.url.as_str(),
                local.public_key(),
                &local,
                deadline()
            ),
            async {
                let (mut peer, auth) = relay.authenticated(local.public_key()).await;
                match stage {
                    "wrong_ok" => {
                        peer.packet(json!(["OK", "ab".repeat(32), true, "wrong ID"]))
                            .await
                    }
                    "negative_ok" => peer.ack(&auth, false).await,
                    "repeated_auth" => peer.packet(json!(["AUTH", CHALLENGE])).await,
                    "wrong_eose" | "early_event" => {
                        peer.ack(&auth, true).await;
                        let sub = peer.request(local.public_key(), None).await;
                        if stage == "wrong_eose" {
                            peer.end("other subscription").await;
                        } else {
                            let body = carrier(&local.public_key().to_hex(), 5, NOW, "");
                            peer.deliver(&sub, &body).await;
                        }
                    }
                    _ => unreachable!(),
                }
                peer.assert_no_request().await;
            },
        );
        assert_eq!(owner.err(), Some(Failure::Grammar), "{stage}");
    }

    let (ready, old_auth) = tokio::join!(
        Owner::open(
            &connector,
            relay.url.as_str(),
            local.public_key(),
            &local,
            deadline()
        ),
        async {
            let (mut peer, auth) = relay.authenticated(local.public_key()).await;
            peer.ack(&auth, true).await;
            let id = peer.request(local.public_key(), None).await;
            peer.end(&id).await;
            (peer, auth)
        },
    );
    drop(ready.unwrap());
    drop(old_auth.0);
    let (replay, ()) = tokio::join!(
        Owner::open(
            &connector,
            relay.url.as_str(),
            local.public_key(),
            &local,
            deadline()
        ),
        async {
            let (mut peer, auth) = relay.authenticated(local.public_key()).await;
            assert_ne!(auth.id, old_auth.1.id);
            peer.ack(&old_auth.1, true).await;
            peer.assert_no_request().await;
        },
    );
    assert_eq!(replay.err(), Some(Failure::Grammar));
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_auth_and_eose_deadlines() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let local = Keys::generate();
    let connector = relay.connector(tmp.path());
    for stage in ["challenge", "auth_ok", "live_eose"] {
        let end = Instant::now() + Duration::from_millis(350);
        let (owner, _held_until_result) = tokio::join!(
            Owner::open(
                &connector,
                relay.url.as_str(),
                local.public_key(),
                &local,
                end
            ),
            async {
                if stage == "challenge" {
                    return relay.accept().await;
                }
                let (mut peer, auth) = relay.authenticated(local.public_key()).await;
                peer.assert_no_request().await;
                if stage == "live_eose" {
                    peer.ack(&auth, true).await;
                    peer.request(local.public_key(), None).await;
                }
                peer
            },
        );
        assert_eq!(owner.err(), Some(Failure::Deadline), "{stage}");
    }
    assert_eq!(connector.direct_attempt_count(), 3);
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_framing_and_pong() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let local = Keys::generate();
    let connector = relay.connector(tmp.path());
    let (owner, _) = tokio::join!(
        Owner::open(
            &connector,
            relay.url.as_str(),
            local.public_key(),
            &local,
            deadline()
        ),
        async {
            let mut peer = relay.accept().await;
            peer.frame(Message::Ping(vec![0x1e, 0xfe].into())).await;
            assert_eq!(peer.receive().await, Message::Pong(vec![0x1e, 0xfe].into()));
            peer.packet(json!(["AUTH", CHALLENGE])).await;
            let auth = peer
                .authentication(&relay.url, local.public_key(), CHALLENGE)
                .await;
            peer.ack(&auth, true).await;
            let live = peer.request(local.public_key(), None).await;
            peer.end(&live).await;
            peer
        },
    );
    drop(owner.unwrap());
    let (mut owner, mut peer, live) = attached(&relay, &connector, &local).await;
    let payload = vec![3; 125];
    let (receipt, ()) = tokio::join!(owner.wait_live(deadline()), async {
        peer.frame(Message::Ping(payload.clone().into())).await;
        assert_eq!(peer.receive().await, Message::Pong(payload.into()));
        let body = carrier(&local.public_key().to_hex(), 7, NOW, "");
        peer.deliver(&live, &body).await;
    });
    assert_eq!(receipt.unwrap().unwrap().arrival().id, [7; 32]);
    drop(owner);
    for mode in [
        "duplicate_eose",
        "closed",
        "notice",
        "binary",
        "invalid_utf8",
        "oversized_ping",
        "owner_cap",
        "connector_cap",
        "control_budget",
    ] {
        let (mut owner, mut peer, live) = attached(&relay, &connector, &local).await;
        let (received, expected) = tokio::join!(owner.wait_live(deadline()), async {
            match mode {
                "duplicate_eose" => peer.end(&live).await,
                "closed" => peer.packet(json!(["CLOSED", live, "fixture close"])).await,
                "notice" => peer.packet(json!(["NOTICE", "fixture notice"])).await,
                "binary" => peer.frame(Message::Binary(vec![1, 2].into())).await,
                "invalid_utf8" => {
                    peer.frame(Message::Frame(Frame::message(
                        vec![0xc3, 0x28],
                        OpCode::Data(Data::Text),
                        true,
                    )))
                    .await;
                    return Failure::Transport;
                }
                "oversized_ping" => {
                    peer.frame(Message::Frame(Frame::ping(vec![1; 126]))).await;
                    return Failure::Transport;
                }
                "owner_cap" => {
                    peer.frame(Message::Text("x".repeat(MESSAGE_LIMIT + 1).into()))
                        .await;
                    return Failure::Size;
                }
                "connector_cap" => {
                    peer.frame(Message::Text("x".repeat((1024 * 1024) + 1).into()))
                        .await;
                    return Failure::Transport;
                }
                "control_budget" => {
                    for _ in 0..9 {
                        peer.frame(Message::Pong(vec![7].into())).await;
                    }
                    return Failure::Capacity;
                }
                _ => unreachable!(),
            }
            Failure::Grammar
        });
        assert_eq!(received.err(), Some(expected), "{mode}");
        assert_eq!(
            owner.wait_live(deadline()).await.err(),
            Some(Failure::Retired),
            "{mode}"
        );
    }
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_history_and_late_page() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let local = Keys::generate();
    let connector = relay.connector(tmp.path());
    let stored = [
        (12, 38),
        (7, 40),
        (6, 40),
        (8, 39),
        (10, 37),
        (4, 38),
        (5, 41),
    ];
    let (mut owner, mut peer, live) = attached(&relay, &connector, &local).await;
    assert!(
        owner
            .wait_live(Instant::now() + QUICK)
            .await
            .unwrap()
            .is_none()
    );
    let bounds = [38, 40];
    let (page, page_id) = tokio::join!(owner.page(bounds, deadline()), async {
        let page_id = peer.request(local.public_key(), Some(bounds)).await;
        assert_ne!(page_id, live);
        let mut history = stored.to_vec();
        history.sort_by_key(|(_, stamp)| std::cmp::Reverse(*stamp));
        for (id, stamp) in history
            .into_iter()
            .filter(|(_, stamp)| (38..=40).contains(stamp))
            .take(4)
        {
            peer.deliver(
                &page_id,
                &carrier(&local.public_key().to_hex(), id, stamp, ""),
            )
            .await;
        }
        peer.end(&page_id).await;
        peer.close(&page_id).await;
        page_id
    });
    let page = page.unwrap();
    assert!(page.is_full());
    assert_eq!(page.request(), (page_id.as_str(), bounds));
    assert!(owner.accepts_page(&page));
    assert_eq!(
        page.records()
            .iter()
            .map(|row| (row.id[0], row.stamp))
            .collect::<Vec<_>>(),
        [(7, 40), (6, 40), (8, 39), (12, 38)]
    );

    let (result, ()) = tokio::join!(owner.wait_live(deadline()), async {
        peer.deliver(
            &page_id,
            &carrier(&local.public_key().to_hex(), 5, 38, "late"),
        )
        .await;
    });
    assert_eq!(result.err(), Some(Failure::Grammar));
    assert!(!owner.accepts_page(&page));
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_page_limits() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let local = Keys::generate();
    let connector = relay.connector(tmp.path());
    for (mode, expected) in [
        ("outside", Failure::Grammar),
        ("increasing", Failure::Grammar),
        ("duplicate", Failure::Grammar),
        ("wrong_eose", Failure::Grammar),
        ("page_full", Failure::Capacity),
        ("live_full", Failure::Capacity),
    ] {
        let (mut owner, mut peer, live) = attached(&relay, &connector, &local).await;
        let bounds = [38, 40];
        let (received, ()) = tokio::join!(owner.page(bounds, deadline()), async {
            let page = peer.request(local.public_key(), Some(bounds)).await;
            let rows: &[(u8, u64)] = match mode {
                "outside" => &[(1, 41)],
                "increasing" => &[(1, 39), (2, 40)],
                "duplicate" => &[(1, 39), (1, 38)],
                "page_full" | "live_full" => &[(1, 40), (2, 40), (3, 40), (4, 39), (5, 38)],
                "wrong_eose" => {
                    peer.end(&live).await;
                    return;
                }
                _ => unreachable!(),
            };
            let sub = if mode == "live_full" { &live } else { &page };
            for (id, stamp) in rows {
                let raw = carrier(&local.public_key().to_hex(), *id, *stamp, "");
                peer.deliver(sub, &raw).await;
            }
        });
        assert_eq!(received.err(), Some(expected), "{mode}");
        assert_eq!(
            owner.wait_live(deadline()).await.err(),
            Some(Failure::Retired),
            "{mode}"
        );
    }
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_predrain_budget() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let local = Keys::generate();
    let connector = relay.connector(tmp.path());
    let (mut owner, mut peer, live) = attached(&relay, &connector, &local).await;
    let raw = carrier(&local.public_key().to_hex(), 5, NOW, "");
    let mut frames = vec![event_message(&live, &raw), Message::Ping(vec![0xc4].into())];
    frames.extend((0..7).map(|_| Message::Pong(vec![7].into())));
    for id in 1..=4 {
        frames.push(event_message(
            &live,
            &carrier(&local.public_key().to_hex(), id, NOW, ""),
        ));
    }
    frames.push(Message::Text(
        json!(["NOTICE", "thirteenth frame"]).to_string().into(),
    ));
    let (initial, ()) = tokio::join!(owner.wait_live(deadline()), peer.batch(frames));
    assert_eq!(initial.unwrap().unwrap().arrival().id, [5; 32]);
    // The first live record and all following frames share one fixture flush.
    assert_eq!(
        owner.page([38, 40], deadline()).await.err(),
        Some(Failure::Capacity)
    );
    assert_eq!(peer.receive().await, Message::Pong(vec![0xc4].into()));
    peer.assert_no_request().await;
    assert_eq!(
        owner.wait_live(deadline()).await.err(),
        Some(Failure::Retired)
    );
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_elapsed_and_ambiguous() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let recipient = Keys::generate();
    let local = Keys::generate();
    let connector = relay.connector(tmp.path());
    let (_, prepared, _) = prepared(relay.url.as_str(), &local, &recipient);
    let original = prepared.frame().to_vec();
    let flight = prepared.flight.flight();
    let (mut owner, mut peer, _) = attached(&relay, &connector, &local).await;
    assert_eq!(
        owner.page([38, 40], Instant::now()).await.err(),
        Some(Failure::Deadline)
    );
    assert_eq!(
        owner.publish(&prepared, flight, Instant::now()).await.err(),
        Some(Failure::Deadline)
    );
    assert_eq!(owner.stop(Instant::now()).await, Err(Failure::Deadline));
    peer.assert_no_request().await;

    let bounds = [38, 40];
    let (page, _) = tokio::join!(
        owner.page(bounds, Instant::now() + QUICK),
        peer.request(local.public_key(), Some(bounds))
    );
    assert_eq!(page.err(), Some(Failure::Deadline));
    assert_eq!(
        owner.wait_live(deadline()).await.err(),
        Some(Failure::Retired)
    );
    for why in ["lost", "wrong_ok", "deadline", "live_overflow"] {
        let (mut owner, mut peer, live_id) = attached(&relay, &connector, &local).await;
        let (published, _held_until_result) = tokio::join!(
            owner.publish(
                &prepared,
                flight,
                Instant::now() + Duration::from_millis(250)
            ),
            async {
                peer.publication(&original, local.public_key()).await;
                match why {
                    "lost" => {
                        drop(peer);
                        None
                    }
                    "wrong_ok" => {
                        peer.packet(json!(["OK", "ab".repeat(32), true, "wrong flight"]))
                            .await;
                        Some(peer)
                    }
                    "live_overflow" => {
                        for id in 1..=5 {
                            let body = carrier(&local.public_key().to_hex(), id, NOW, "");
                            peer.deliver(&live_id, &body).await;
                        }
                        Some(peer)
                    }
                    "deadline" => Some(peer),
                    _ => unreachable!(),
                }
            }
        );
        assert_eq!(published, Err(Failure::Ambiguous), "{why}");
        assert_eq!(
            owner.publish(&prepared, flight, deadline()).await,
            Err(Failure::Retired),
            "{why}"
        );
        assert_eq!(prepared.frame(), original);
    }

    let mut bad_id = flight;
    bad_id.carrier_id = nostr::event::EventId::from_byte_array([0x19; 32]);
    let mut bad_recipient = flight;
    bad_recipient.recipient = local.public_key();
    for (invalid, expected) in [
        (bad_id, Failure::Binding),
        (bad_recipient, Failure::Grammar),
    ] {
        let (mut owner, mut peer, _) = attached(&relay, &connector, &local).await;
        assert_eq!(
            owner.publish(&prepared, invalid, deadline()).await,
            Err(expected)
        );
        peer.assert_no_request().await;
        assert_eq!(
            owner.publish(&prepared, flight, deadline()).await,
            Err(Failure::Retired)
        );
    }
}

#[test]
fn peer_raw_socket_core_publication_validation() {
    let origin = "wss://relay.invalid/";
    let signer = Keys::generate();
    let recipient = Keys::generate();
    let (_, offered, _) = prepared(origin, &signer, &recipient);
    let flight = offered.flight.flight();
    let event: serde_json::Value = serde_json::from_slice(offered.frame()).unwrap();
    let mut tampered = event.clone();
    let alternate = prepared(origin, &signer, &recipient).1;
    let other: serde_json::Value = serde_json::from_slice(alternate.frame()).unwrap();
    assert_ne!(event["sig"], other["sig"]);
    tampered["sig"] = other["sig"].clone();
    assert_eq!(
        publication(tampered.to_string().as_bytes(), flight),
        Err(Failure::Binding)
    );
    let spaced = format!(" {}\n", std::str::from_utf8(offered.frame()).unwrap());
    assert_eq!(
        publication(spaced.as_bytes(), flight),
        Err(Failure::Binding)
    );
    assert!(publication(offered.frame(), flight).is_ok());
}

#[tokio::test]
#[ignore = "private native WSS proof"]
async fn peer_raw_socket_native_wss_cancellation_retires() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let sender = Keys::generate();
    let recipient = Keys::generate();
    let connector = relay.connector(tmp.path());
    let (mut owner, mut peer, live_id) = attached(&relay, &connector, &sender).await;
    let raw = carrier(&sender.public_key().to_hex(), 5, 40, "");
    let (receipt, _) = tokio::join!(owner.wait_live(deadline()), peer.deliver(&live_id, &raw));
    let receipt = receipt.unwrap().unwrap();
    assert!(owner.accepts_live(&receipt));

    let bounds = [38, 40];
    let mut pending = Box::pin(owner.page(bounds, deadline()));
    tokio::select! {
        result = &mut pending => panic!("historical request should remain pending: {result:?}"),
        request = peer.request(sender.public_key(), Some(bounds)) => assert_ne!(request, live_id),
    }
    drop(pending);
    assert!(!owner.accepts_live(&receipt));
    assert_eq!(
        owner.page(bounds, deadline()).await.err(),
        Some(Failure::Retired)
    );

    let (mut owner, mut peer, replacement) = attached(&relay, &connector, &sender).await;
    assert_ne!(replacement, live_id);
    let (_, prepared, _) = prepared(relay.url.as_str(), &sender, &recipient);
    let original = prepared.frame().to_vec();
    let flight = prepared.flight.flight();
    let mut pending = Box::pin(owner.publish(&prepared, flight, deadline()));
    tokio::select! {
        result = &mut pending => panic!("publication should remain pending: {result:?}"),
        event = peer.publication(&original, sender.public_key()) => assert_eq!(event.id, flight.carrier_id),
    }
    drop(pending);
    assert_eq!(
        owner.publish(&prepared, flight, deadline()).await,
        Err(Failure::Retired)
    );
    assert_eq!(prepared.frame(), original);
    let (_, _, third_id) = attached(&relay, &connector, &sender).await;
    assert_ne!(third_id, replacement);
}

#[tokio::test]
#[ignore = "private native WSS microbenchmark"]
async fn peer_raw_socket_bench() {
    let relay = Relay::new().await;
    let tmp = tempfile::tempdir().unwrap();
    let writer = Keys::generate();
    let reader = Keys::generate();
    let writer_connector = relay.connector(&tmp.path().join("writer"));
    let reader_connector = relay.connector(&tmp.path().join("reader"));
    let (mut incoming, mut receiver, live) = attached(&relay, &reader_connector, &reader).await;
    let mut readiness = Vec::new();
    let mut acks = Vec::new();
    let mut deliveries = Vec::new();
    for run in 0..14 {
        let (codec, prepared, relays) = prepared(relay.url.as_str(), &writer, &reader);
        let flight = prepared.flight.flight();
        let start = Instant::now();
        let (mut source, mut sender, _) = attached(&relay, &writer_connector, &writer).await;
        let ready = start.elapsed();
        let start = Instant::now();
        let (accepted, ()) = tokio::join!(source.publish(&prepared, flight, deadline()), async {
            let event = sender
                .publication(prepared.frame(), writer.public_key())
                .await;
            sender.ack(&event, true).await;
        });
        assert!(accepted.unwrap());
        let ack = start.elapsed();
        let start = Instant::now();
        let (record, ()) = tokio::join!(
            incoming.wait_live(deadline()),
            receiver.deliver(&live, std::str::from_utf8(prepared.frame()).unwrap()),
        );
        let record = record.unwrap().unwrap();
        let signed = codec
            .open_offer(
                &reader,
                record.arrival().body.get().as_bytes(),
                Some(writer.public_key()),
                &relays,
                NOW,
            )
            .unwrap();
        assert_eq!(signed.flight().carrier_id, flight.carrier_id);
        let delivery = start.elapsed();
        if run >= 2 {
            readiness.push(ready);
            acks.push(ack);
            deliveries.push(delivery);
        }
    }
    for (name, mut values) in [
        ("authenticated_readiness", readiness),
        ("publication_ok", acks),
        ("authenticated_delivery", deliveries),
    ] {
        values.sort_unstable();
        let p50 = values[(values.len() - 1) / 2].as_secs_f64() * 1000.0;
        let p95 = values[(values.len() * 95).div_ceil(100) - 1].as_secs_f64() * 1000.0;
        println!(
            "{name}: count={} p50_ms={p50:.3} p95_ms={p95:.3}",
            values.len()
        );
    }
}
