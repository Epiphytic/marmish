use std::sync::Arc;
use std::time::{Duration, Instant};

use cgka_engine::EngineBuilder;
use cgka_engine::account_identity_proof::{
    AccountIdentityProofRequest, AccountIdentityProofSigner,
};
use cgka_traits::app_components::{NOSTR_ROUTING_COMPONENT_ID, default_group_components};
use cgka_traits::engine::CgkaEngine;
use nostr::event::Event;
use serde_json::{Value, json};
use storage_sqlite::{SqliteAccountStorage, SqliteStorageOptions};
use transport_nostr_peeler::NostrMlsPeeler;

use super::*;

const ROUTE: &str = "wss://peer.example/";
const CLOCK: u64 = 1_800_000_000;
const ROOM: [u8; 16] = [0x17; 16];

struct Fixture {
    codec: Codec,
    server: Keys,
    client: Keys,
    routing: NostrRoutingV1,
}

impl Fixture {
    fn new() -> Self {
        Self {
            codec: Codec::new(MAX_OUTER_BYTES).unwrap(),
            server: Keys::generate(),
            client: Keys::generate(),
            routing: NostrRoutingV1::new([0x23; 32], vec![ROUTE.to_owned()]).unwrap(),
        }
    }

    fn offer(&self, now: u64) -> Prepared<Offer> {
        self.codec
            .prepare_offer(
                &self.server,
                self.client.public_key(),
                ROOM,
                &self.routing,
                now,
            )
            .unwrap()
    }

    fn open_offer(&self, frame: &[u8], now: u64) -> Result<Offer> {
        self.codec
            .open_offer(&self.client, frame, None, &self.routing.relays, now)
    }

    fn consent(&self, offer: &Offer, package: &[u8], now: u64) -> Prepared<Consent> {
        self.codec
            .prepare_consent(&self.client, offer, package, now, now + 1200)
            .unwrap()
    }

    fn delivery(&self, consent: &Consent, welcome: &[u8], now: u64) -> Prepared<Delivery> {
        self.codec
            .prepare_delivery(&self.server, consent, welcome, now, now + 600)
            .unwrap()
    }
}

struct ProofSigner(Keys);
impl AccountIdentityProofSigner for ProofSigner {
    fn sign_account_identity_proof(
        &self,
        req: &AccountIdentityProofRequest,
    ) -> std::result::Result<[u8; 64], String> {
        assert_eq!(
            req.account_identity.as_slice(),
            self.0.public_key().to_bytes()
        );
        req.signature_from_signed_event(req.proof_event()?.finalize(&self.0).unwrap())
    }
}

async fn native_package(fixture: &Fixture) -> (Vec<u8>, u64) {
    marmish_engine::initialize_storage().unwrap();
    let storage = SqliteAccountStorage::in_memory_with_options(SqliteStorageOptions {
        cipher_memory_security: false,
        ..Default::default()
    })
    .unwrap();
    let client = &fixture.client;
    let mut engine = EngineBuilder::new(storage)
        .identity(client.public_key().to_bytes().to_vec())
        .account_identity_proof_signer(Arc::new(ProofSigner(client.clone())))
        .supported_app_components(
            default_group_components()
                .into_iter()
                .chain([NOSTR_ROUTING_COMPONENT_ID]),
        )
        .peeler(Box::new(NostrMlsPeeler::new()))
        .build()
        .unwrap();
    let package = engine.fresh_key_package().await.unwrap();
    let meta = cgka_engine::key_package_metadata(&package).unwrap();
    assert!(meta.last_resort);
    let now = Timestamp::now().as_secs();
    let dto = artifact::publication(
        &package,
        &meta,
        client.public_key(),
        [0x31; 16],
        &fixture.routing.relays,
    )
    .to_event_at(now)
    .unwrap();
    let tags = dto.tags.into_iter().map(|tag| Tag::parse(tag).unwrap());
    let event = EventBuilder::new(Kind::Custom(30443), dto.content)
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(now))
        .finalize(client)
        .unwrap();
    (serde_json::to_vec(&event).unwrap(), now)
}

fn welcome_for(client: PublicKey, content: String) -> Vec<u8> {
    let event = EventBuilder::new(Kind::GiftWrap, content)
        .tag(Tag::public_key(client))
        .custom_created_at(Timestamp::from_secs(CLOCK))
        .finalize(&Keys::generate())
        .unwrap();
    serde_json::to_vec(&event).unwrap()
}

fn open_layers(recipient: &Keys, frame: &[u8]) -> (Event, Event, UnsignedEvent) {
    let outer = seal::verify_signed(frame, Error::Outer).unwrap();
    let raw =
        nip44::decrypt_to_bytes(recipient.secret_key(), &outer.pubkey, &outer.content).unwrap();
    let seal = seal::verify_signed(&raw, Error::Seal).unwrap();
    let rumor = seal::open_signed_seal(&outer, recipient, None).unwrap();
    (outer, seal, rumor)
}

fn replace_body(
    codec: &Codec,
    frame: &[u8],
    signer: &Keys,
    recipient: &Keys,
    update: impl FnOnce(&mut Value, &mut UnsignedEvent),
) -> Vec<u8> {
    let (_, _, mut rumor) = open_layers(recipient, frame);
    let mut body = serde_json::from_str(&rumor.content).unwrap();
    update(&mut body, &mut rumor);
    rumor.content = serde_json::to_string(&body).unwrap();
    rumor.id = Some(rumor.compute_id());
    codec
        .prepare(
            signer,
            recipient.public_key(),
            &rumor,
            body["expires_at"].as_u64().unwrap(),
        )
        .unwrap()
        .0
}

#[tokio::test]
async fn peer_control_core_three_flights_preserve_exact_signed_bytes() {
    let f = Fixture::new();
    let (package, now) = native_package(&f).await;
    let initial = f.offer(now - FLIGHT_TTL + 10);
    let offer = f.open_offer(initial.frame(), now).unwrap();
    assert_eq!(offer.offer_id(), initial.flight.offer_id());
    assert_eq!(offer.room_id(), ROOM);
    assert_eq!(offer.routing(), &f.routing);
    assert_eq!(offer.flight().author, f.server.public_key());
    assert_eq!(offer.flight().recipient, f.client.public_key());
    assert_eq!(offer.flight().created_at, now - FLIGHT_TTL + 10);
    assert_eq!(offer.flight().expires_at, now + 10);
    assert_eq!(offer.flight().replay_safe_until, now + 910);

    let authored = f.consent(&offer, &package, now);
    let consent = f
        .codec
        .open_consent(&f.server, authored.frame(), &initial.flight, now)
        .unwrap();
    assert_eq!(consent.flight().revision, authored.flight.flight().revision);
    assert_eq!(consent.offer().offer_id(), offer.offer_id());
    assert_eq!(consent.package_bytes(), package);
    assert_eq!(consent.binding().slot, [0x31; 16]);
    assert_eq!(
        consent
            .key_package()
            .source
            .as_ref()
            .unwrap()
            .event_id
            .as_slice(),
        consent.binding().event_id
    );

    let mut welcome = welcome_for(f.client.public_key(), "opaque".to_owned());
    // The native Welcome custody binds exact bytes, including wire whitespace.
    welcome.push(b'\n');
    let authored_delivery = f.delivery(&consent, &welcome, now + 20);
    assert!(matches!(
        f.codec.classify_client(&f.client, initial.frame()).unwrap(),
        ClientCarrier::Offer
    ));
    let ClientCarrier::Delivery(incoming) = f
        .codec
        .classify_client(&f.client, authored_delivery.frame())
        .unwrap()
    else {
        panic!("expected authenticated Delivery");
    };
    let incoming: IncomingDelivery = *incoming;
    assert_eq!(incoming.offer().server, f.server.public_key());
    assert_eq!(incoming.offer().id, offer.offer_id());
    assert_eq!(
        incoming.offer().revision,
        offer.flight().revision.to_bytes()
    );
    let outer = seal::verify_signed(authored_delivery.frame(), Error::Outer).unwrap();
    assert!(incoming.matches_index(outer.id.to_bytes(), outer.created_at.as_secs()));
    let from_transcript = Transcript::from_consent(&authored.flight)
        .verify_delivery(
            &f.client,
            &offer,
            authored.frame(),
            &package,
            now + 340,
            incoming,
        )
        .unwrap();
    assert_eq!(
        from_transcript.consent_revision(),
        consent.flight().revision
    );
    assert_eq!(from_transcript.welcome_bytes(), welcome);

    let client_context = &authored.flight;
    let delivery = f
        .codec
        .open_delivery(
            &f.client,
            authored_delivery.frame(),
            client_context,
            now + 340,
        )
        .unwrap();
    assert_eq!(
        f.open_offer(initial.frame(), now + 340).err(),
        Some(Error::Time)
    );
    assert_eq!(delivery.flight().author, f.server.public_key());
    assert_eq!(delivery.flight().recipient, f.client.public_key());
    assert_eq!(
        delivery.flight().carrier_id,
        authored_delivery.flight.flight().carrier_id
    );
    assert_eq!(delivery.offer().routing(), &f.routing);
    assert_eq!(delivery.consent_revision(), consent.flight().revision);
    assert!(delivery.binding() == consent.binding());
    assert_eq!(delivery.welcome_bytes(), welcome);
    assert_eq!(delivery.welcome_id(), authored_delivery.flight.welcome_id());
    assert_eq!(
        delivery.welcome_digest(),
        authored_delivery.flight.welcome_digest()
    );
}

#[test]
fn peer_control_core_offer_uses_only_authenticated_time_and_configured_route() {
    let f = Fixture::new();
    let authored = f.offer(CLOCK);
    assert!(f.open_offer(authored.frame(), CLOCK - SKEW).is_ok());
    assert_eq!(
        f.open_offer(authored.frame(), CLOCK - SKEW - 1).err(),
        Some(Error::Time)
    );
    assert!(
        f.open_offer(authored.frame(), CLOCK + FLIGHT_TTL + SKEW - 1)
            .is_ok()
    );
    assert_eq!(
        f.open_offer(authored.frame(), CLOCK + FLIGHT_TTL + SKEW)
            .err(),
        Some(Error::Time)
    );
    assert_eq!(
        f.codec
            .open_offer(
                &f.client,
                authored.frame(),
                Some(Keys::generate().public_key()),
                &f.routing.relays,
                CLOCK
            )
            .err(),
        Some(Error::Seal)
    );
    assert_eq!(
        f.codec
            .open_offer(
                &f.client,
                authored.frame(),
                Some(f.server.public_key()),
                &["wss://other.example/".into()],
                CLOCK
            )
            .err(),
        Some(Error::Route)
    );
    assert_eq!(
        f.codec
            .open_offer(&f.server, authored.frame(), None, &f.routing.relays, CLOCK)
            .err(),
        Some(Error::Outer)
    );
    assert_eq!(
        f.codec
            .prepare_offer(&f.server, f.server.public_key(), ROOM, &f.routing, CLOCK)
            .err(),
        Some(Error::Binding)
    );
    assert_eq!(
        f.codec
            .prepare_offer(&f.server, f.client.public_key(), ROOM, &f.routing, u64::MAX)
            .err(),
        Some(Error::Time)
    );

    for relays in [
        vec!["wss://PEER.example/".to_owned()],
        vec!["wss://peer.example/?x=value".to_owned()],
        vec![ROUTE.to_owned(), ROUTE.to_owned()],
        vec!["wss://z.example/".to_owned(), ROUTE.to_owned()],
    ] {
        let routing = NostrRoutingV1 {
            nostr_group_id: [0; 32],
            relays,
        };
        assert_eq!(
            f.codec
                .prepare_offer(&f.server, f.client.public_key(), ROOM, &routing, CLOCK)
                .err(),
            Some(Error::Route)
        );
    }
}

#[test]
fn peer_control_core_randomized_layers_and_housekeeping_are_independent() {
    let f = Fixture::new();
    let original = f.offer(CLOCK);
    let (_, _, rumor) = open_layers(&f.client, original.frame());
    let expires_at = original.flight.flight().expires_at;
    let ephemeral_first = Keys::generate();
    let ephemeral_second = Keys::generate();
    let cover = Cover {
        outer_offset: 172_799,
        seal_offset: 17,
        retention_buckets: 2,
    };
    let (first, first_flight) = f
        .codec
        .prepare_with_cover(
            &f.server,
            &ephemeral_first,
            f.client.public_key(),
            &rumor,
            expires_at,
            cover,
        )
        .unwrap();
    let (second, second_flight) = f
        .codec
        .prepare_with_cover(
            &f.server,
            &ephemeral_second,
            f.client.public_key(),
            &rumor,
            expires_at,
            Cover {
                outer_offset: 17,
                seal_offset: 172_799,
                retention_buckets: 1,
            },
        )
        .unwrap();
    let (outer_a, seal_a, rumor_a) = open_layers(&f.client, &first);
    let (outer_b, seal_b, rumor_b) = open_layers(&f.client, &second);
    assert_eq!(outer_a.created_at.as_secs(), CLOCK - 172_799);
    assert_eq!(seal_a.created_at.as_secs(), CLOCK - 17);
    assert_eq!(outer_b.created_at.as_secs(), CLOCK - 17);
    assert_eq!(seal_b.created_at.as_secs(), CLOCK - 172_799);
    assert_eq!(outer_a.pubkey, ephemeral_first.public_key());
    assert_eq!(outer_b.pubkey, ephemeral_second.public_key());
    assert_ne!(outer_a.id, outer_b.id);
    assert_ne!(first_flight.carrier_id, second_flight.carrier_id);
    assert_eq!(first_flight.revision, second_flight.revision);
    assert_eq!(rumor_a.compute_id(), rumor_b.compute_id());
    let expiry_a: u64 = outer_a.tags.get(1).unwrap().as_slice()[1].parse().unwrap();
    let expiry_b: u64 = outer_b.tags.get(1).unwrap().as_slice()[1].parse().unwrap();
    assert_eq!(expiry_a - expiry_b, RETENTION_BUCKET);
    assert_eq!(expiry_b % RETENTION_BUCKET, 0);
    assert!(
        (expires_at + SKEW + RETENTION_BUCKET..expires_at + SKEW + 2 * RETENTION_BUCKET)
            .contains(&expiry_b)
    );
    assert!(f.open_offer(&first, CLOCK).is_ok() && f.open_offer(&second, CLOCK).is_ok());
    let revised = f.offer(CLOCK);
    assert_ne!(revised.flight.offer_id(), original.flight.offer_id());
    assert_ne!(
        open_layers(&f.client, revised.frame()).0.pubkey,
        open_layers(&f.client, original.frame()).0.pubkey
    );
}

#[test]
fn peer_control_core_rejects_open_schema_and_wrong_outer_layers() {
    let f = Fixture::new();
    let authored = f.offer(CLOCK);
    let (outer, _, rumor) = open_layers(&f.client, authored.frame());
    let mut obj: Value = serde_json::from_slice(authored.frame()).unwrap();
    obj["unused"] = json!(false);
    assert_eq!(
        f.open_offer(&serde_json::to_vec(&obj).unwrap(), CLOCK)
            .err(),
        Some(Error::Outer)
    );
    obj.as_object_mut().unwrap().remove("unused");
    obj["pubkey"] = json!(outer.pubkey.to_hex().to_uppercase());
    assert_eq!(
        f.open_offer(&serde_json::to_vec(&obj).unwrap(), CLOCK)
            .err(),
        Some(Error::Outer)
    );
    let duplicate = format!(
        "{{\"kind\":7435,{}",
        String::from_utf8_lossy(&authored.frame()[1..])
    );
    assert_eq!(
        f.open_offer(duplicate.as_bytes(), CLOCK).err(),
        Some(Error::Outer)
    );

    for (kind, tags) in [
        (
            Kind::GiftWrap,
            vec![
                Tag::public_key(f.client.public_key()),
                outer.tags.get(1).unwrap().clone(),
            ],
        ),
        (
            OUTER,
            vec![
                outer.tags.get(1).unwrap().clone(),
                Tag::public_key(f.client.public_key()),
            ],
        ),
        (
            OUTER,
            vec![
                Tag::public_key(f.client.public_key()),
                Tag::parse(["expiration", "0021600"]).unwrap(),
            ],
        ),
    ] {
        let frame = EventBuilder::new(kind, outer.content.clone())
            .tags(tags)
            .finalize(&Keys::generate())
            .unwrap();
        assert_eq!(
            f.open_offer(&serde_json::to_vec(&frame).unwrap(), CLOCK)
                .err(),
            Some(Error::Outer)
        );
    }

    let mut forgeries = Vec::new();
    let mut altered = rumor.clone();
    altered.id = Some(EventId::from_byte_array([0x11; 32]));
    forgeries.push(altered);
    let mut altered = rumor.clone();
    altered.pubkey = Keys::generate().public_key();
    altered.id = Some(altered.compute_id());
    forgeries.push(altered);
    let mut altered = rumor.clone();
    altered.tags = nostr::event::Tags::from_list(vec![Tag::public_key(f.server.public_key())]);
    altered.id = Some(altered.compute_id());
    forgeries.push(altered);

    for altered in forgeries {
        let frame = f
            .codec
            .prepare(
                &f.server,
                f.client.public_key(),
                &altered,
                CLOCK + FLIGHT_TTL,
            )
            .unwrap()
            .0;
        assert_eq!(f.open_offer(&frame, CLOCK).err(), Some(Error::Rumor));
    }

    for (field, value) in [
        ("extra", json!(true)),
        ("v", json!(2)),
        ("offer_id", json!("FF".repeat(16))),
    ] {
        let frame = replace_body(
            &f.codec,
            authored.frame(),
            &f.server,
            &f.client,
            |value_map, _| {
                value_map[field] = value;
            },
        );
        assert_eq!(f.open_offer(&frame, CLOCK).err(), Some(Error::Body));
    }
}

#[tokio::test]
async fn peer_control_core_checks_each_authenticated_stage_field() {
    let f = Fixture::new();
    let (package, now) = native_package(&f).await;
    let offer = f.offer(now);
    let consent = f.consent(&offer.flight, &package, now);
    let other = f.offer(now);
    assert_eq!(
        f.codec
            .open_consent(&f.server, consent.frame(), &other.flight, now)
            .err(),
        Some(Error::Binding),
    );
    assert_eq!(
        f.codec
            .prepare_consent(&f.server, &offer.flight, &package, now, now + 60)
            .err(),
        Some(Error::Binding),
    );

    for field in [
        "offer_id",
        "offer_revision",
        "package_event_id",
        "package_ref",
        "package_slot",
    ] {
        let frame = replace_body(
            &f.codec,
            consent.frame(),
            &f.client,
            &f.server,
            |body, _| {
                body[field] = json!("ef".repeat(body[field].as_str().unwrap().len() / 2));
            },
        );
        assert_eq!(
            f.codec
                .open_consent(&f.server, &frame, &offer.flight, now)
                .err(),
            Some(Error::Binding),
            "{field}"
        );
    }

    let mut noncanonical = package.clone();
    noncanonical.push(b' ');
    assert_eq!(
        f.codec
            .prepare_consent(&f.client, &offer.flight, &noncanonical, now, now + 20)
            .err(),
        Some(Error::Package)
    );
    let native = seal::verify_signed(&package, Error::Package).unwrap();
    let incorrect = EventBuilder::new(native.kind, native.content)
        .tags(
            native
                .tags
                .iter()
                .filter(|tag| tag.as_slice()[0] != "app_components")
                .cloned(),
        )
        .custom_created_at(native.created_at)
        .finalize(&f.client)
        .unwrap();
    let incorrect = serde_json::to_vec(&incorrect).unwrap();
    assert_eq!(
        f.codec
            .prepare_consent(&f.client, &offer.flight, &incorrect, now, now + 20)
            .err(),
        Some(Error::Package)
    );

    let welcome = welcome_for(f.client.public_key(), "opaque".to_owned());
    let delivery = f.delivery(&consent.flight, &welcome, now);
    for field in [
        "offer_id",
        "offer_revision",
        "consent_revision",
        "package_event_id",
        "package_ref",
        "package_slot",
        "welcome_event_id",
        "welcome_sha256",
    ] {
        let frame = replace_body(
            &f.codec,
            delivery.frame(),
            &f.server,
            &f.client,
            |body, _| {
                body[field] = json!("ef".repeat(body[field].as_str().unwrap().len() / 2));
            },
        );
        let expected = if field.starts_with("welcome_") {
            Error::Welcome
        } else {
            Error::Binding
        };
        assert_eq!(
            f.codec
                .open_delivery(&f.client, &frame, &consent.flight, now)
                .err(),
            Some(expected),
            "{field}"
        );
    }

    let wrong_welcome = welcome_for(f.server.public_key(), "opaque".to_owned());
    assert_eq!(
        f.codec
            .prepare_delivery(&f.server, &consent.flight, &wrong_welcome, now, now + 30)
            .err(),
        Some(Error::Welcome)
    );
    let mut wrong_signature: Value = serde_json::from_slice(&welcome).unwrap();
    wrong_signature["sig"] = json!("00".repeat(64));
    assert_eq!(
        f.codec
            .prepare_delivery(
                &f.server,
                &consent.flight,
                &serde_json::to_vec(&wrong_signature).unwrap(),
                now,
                now + 30
            )
            .err(),
        Some(Error::Welcome)
    );
}

#[tokio::test]
async fn peer_control_core_checks_causal_and_native_signed_windows() {
    let f = Fixture::new();
    let (package, now) = native_package(&f).await;
    let offer = f.offer(now + 300);
    let consent = f.consent(&offer.flight, &package, now);
    assert_eq!(consent.flight.flight().created_at, now + 300);
    let capped = f
        .codec
        .prepare_capped_fixture_consent(&f.client, &offer.flight, &package, now + 600, now)
        .unwrap();
    let transcript = Transcript::from_consent(&capped.flight);
    assert_eq!(transcript.expires, now + 600);
    transcript
        .verify(&f.client, &offer.flight, capped.frame(), &package, now)
        .unwrap();
    let received = f
        .codec
        .open_consent(&f.server, capped.frame(), &offer.flight, now)
        .unwrap();
    assert_eq!(Transcript::from_consent(&received), transcript);
    assert_eq!(
        transcript.verify(
            &f.client,
            &offer.flight,
            capped.frame(),
            &package,
            now + 600
        ),
        Err(Error::Binding)
    );
    assert_eq!(
        f.codec
            .prepare_consent(&f.client, &offer.flight, &package, now - 1, now + 600)
            .err(),
        Some(Error::Time)
    );
    let stale = replace_body(
        &f.codec,
        consent.frame(),
        &f.client,
        &f.server,
        |_, rumor| {
            rumor.created_at = Timestamp::from_secs(now + 299);
        },
    );
    assert_eq!(
        f.codec
            .open_consent(&f.server, &stale, &offer.flight, now)
            .err(),
        Some(Error::Time)
    );

    assert_eq!(
        f.codec
            .prepare_consent(
                &f.client,
                &offer.flight,
                &package,
                now,
                now + 300 + FLIGHT_TTL + 1
            )
            .err(),
        Some(Error::Time)
    );
    let meta =
        artifact::verify_package(&package, f.client.public_key(), now, &f.routing.relays).unwrap();
    let past = meta.not_before - 2;
    let before_package = f.offer(past);
    assert_eq!(
        f.codec
            .prepare_consent(&f.client, &before_package.flight, &package, past, past + 1)
            .err(),
        Some(Error::Package)
    );
    assert_eq!(
        package_window(&meta, meta.not_before, meta.not_after, meta.not_before),
        Ok(())
    );
    assert_eq!(
        package_window(&meta, meta.not_before - 1, meta.not_after, meta.not_before),
        Err(Error::PackageLifetime)
    );
    assert_eq!(
        package_window(&meta, meta.not_before, meta.not_after, meta.not_after),
        Err(Error::PackageLifetime)
    );

    let near_end = meta.not_after - 20;
    let offer = f.offer(near_end);
    assert_eq!(
        f.codec
            .prepare_consent(
                &f.client,
                &offer.flight,
                &package,
                near_end,
                meta.not_after + 1
            )
            .err(),
        Some(Error::PackageLifetime)
    );
    let consent = f
        .codec
        .prepare_consent(&f.client, &offer.flight, &package, near_end, meta.not_after)
        .unwrap();
    let welcome = welcome_for(f.client.public_key(), "opaque".to_owned());
    let delivery = f
        .codec
        .prepare_delivery(
            &f.server,
            &consent.flight,
            &welcome,
            near_end,
            meta.not_after,
        )
        .unwrap();
    assert!(
        f.codec
            .open_delivery(
                &f.client,
                delivery.frame(),
                &consent.flight,
                meta.not_after - 1
            )
            .is_ok()
    );
    assert_eq!(
        f.codec
            .open_delivery(&f.client, delivery.frame(), &consent.flight, meta.not_after)
            .err(),
        Some(Error::PackageLifetime)
    );
    assert_eq!(
        f.codec
            .prepare_delivery(
                &f.server,
                &consent.flight,
                &welcome,
                near_end,
                meta.not_after + 1
            )
            .err(),
        Some(Error::Time)
    );
}

#[tokio::test]
async fn peer_control_core_sender_receiver_agree_on_provisional_size_boundaries() {
    let f = Fixture::new();
    let (package, now) = native_package(&f).await;
    let offer = f.offer(now);
    let consent = f.consent(&offer.flight, &package, now);
    let smallest = welcome_for(f.client.public_key(), "x".into());
    let welcome = welcome_for(
        f.client.public_key(),
        "a".repeat(artifact::ARTIFACT_CAP - smallest.len() + 1),
    );
    assert_eq!(welcome.len(), artifact::ARTIFACT_CAP);
    let delivery = f.delivery(&consent.flight, &welcome, now);
    assert!(delivery.frame().len() < MAX_OUTER_BYTES);
    assert!(
        f.codec
            .open_delivery(&f.client, delivery.frame(), &consent.flight, now)
            .is_ok()
    );
    let remainder = Codec::new(delivery.frame().len() - 1).unwrap();
    assert_eq!(
        remainder
            .open_delivery(&f.client, delivery.frame(), &consent.flight, now)
            .err(),
        Some(Error::Size)
    );
    assert_eq!(
        remainder
            .prepare_delivery(&f.server, &consent.flight, &welcome, now, now + 600)
            .err(),
        Some(Error::Size)
    );
    let excessive = welcome_for(
        f.client.public_key(),
        "a".repeat(artifact::ARTIFACT_CAP - smallest.len() + 2),
    );
    assert_eq!(excessive.len(), artifact::ARTIFACT_CAP + 1);
    assert_eq!(
        f.codec
            .prepare_delivery(&f.server, &consent.flight, &excessive, now, now + 600)
            .err(),
        Some(Error::Size)
    );
    assert_eq!(Codec::new(0).err(), Some(Error::Size));
    assert_eq!(Codec::new(MAX_OUTER_BYTES + 1).err(), Some(Error::Size));
}

#[test]
fn peer_control_core_measures_pinned_nip44_padding_cliff() {
    let f = Fixture::new();
    let original = f.offer(CLOCK);
    let (_, _, mut rumor) = open_layers(&f.client, original.frame());
    rumor.content.clear();
    let overhead = serde_json::to_vec(&rumor).unwrap().len();
    for length in [28_672, 28_673] {
        rumor.content = "a".repeat(length - overhead);
        rumor.id = Some(rumor.compute_id());
        assert_eq!(serde_json::to_vec(&rumor).unwrap().len(), length);
        let prepared =
            f.codec
                .prepare(&f.server, f.client.public_key(), &rumor, CLOCK + FLIGHT_TTL);
        if length == 28_672 {
            assert!(prepared.unwrap().0.len() < MAX_OUTER_BYTES);
        } else {
            assert_eq!(prepared.err(), Some(Error::Size));
        }
    }
}

fn print_measurement(label: &str, mut times: Vec<Duration>, bytes: usize) {
    times.sort_unstable();
    println!(
        "{label}: p50={:.3}ms p95={:.3}ms bytes={bytes} samples={}",
        times[times.len() / 2].as_secs_f64() * 1_000.,
        times[(times.len() * 95).div_ceil(100) - 1].as_secs_f64() * 1_000.,
        times.len()
    );
}

#[tokio::test]
#[ignore = "offline benchmark"]
async fn peer_control_benchmark() {
    let f = Fixture::new();
    let (package, now) = native_package(&f).await;
    let welcome = welcome_for(f.client.public_key(), "x".repeat(2048));
    let mut measurements: [Vec<Duration>; 6] = std::array::from_fn(|_| Vec::with_capacity(100));
    let mut sizes = [0; 3];
    for iteration in 0..110 {
        let start = Instant::now();
        let offer = f.offer(now);
        let offer_prepared = start.elapsed();
        let start = Instant::now();
        let verified_offer = f.open_offer(offer.frame(), now).unwrap();
        let offer_verified = start.elapsed();
        let start = Instant::now();
        let consent = f.consent(&verified_offer, &package, now);
        let consent_prepared = start.elapsed();
        let start = Instant::now();
        let verified_consent = f
            .codec
            .open_consent(&f.server, consent.frame(), &offer.flight, now)
            .unwrap();
        let consent_verified = start.elapsed();
        let start = Instant::now();
        let delivery = f.delivery(&verified_consent, &welcome, now);
        let delivery_prepared = start.elapsed();
        let start = Instant::now();
        let verified = f
            .codec
            .open_delivery(&f.client, delivery.frame(), &consent.flight, now)
            .unwrap();
        let delivery_verified = start.elapsed();
        assert_eq!(verified.welcome_bytes(), welcome);
        if iteration >= 10 {
            for (samples, elapsed) in measurements.iter_mut().zip([
                offer_prepared,
                offer_verified,
                consent_prepared,
                consent_verified,
                delivery_prepared,
                delivery_verified,
            ]) {
                samples.push(elapsed);
            }
            sizes = [
                offer.frame().len(),
                consent.frame().len(),
                delivery.frame().len(),
            ];
        }
    }
    for (index, (label, times)) in [
        "offer prepare",
        "offer verify",
        "consent prepare",
        "consent verify",
        "delivery prepare",
        "delivery verify",
    ]
    .into_iter()
    .zip(measurements)
    .enumerate()
    {
        print_measurement(label, times, sizes[index / 2]);
    }
}
