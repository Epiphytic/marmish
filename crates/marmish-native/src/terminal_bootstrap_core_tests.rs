use super::state::{Advance, Exchange, Time};
use super::*;
use nostr::event::EventBuilder;
use std::time::{Duration, Instant};

const NOW: u64 = 10_000;

struct Fixture {
    client: Keys,
    server: Keys,
}

impl Fixture {
    fn new() -> Self {
        Self {
            client: Keys::generate(),
            server: Keys::generate(),
        }
    }

    fn context(&self, role: Role, group: &[u8]) -> AuthenticatedCommandContext {
        let (local, remote, direction) = match role {
            Role::Client => (&self.client, &self.server, CommandDirection::Client),
            Role::Server => (&self.server, &self.client, CommandDirection::Server),
        };
        AuthenticatedCommandContext::test_fixture(
            local.public_key().to_bytes(),
            remote.public_key().to_bytes(),
            group.to_vec(),
            [1; 16],
            direction,
        )
    }

    fn codecs(&self, group: &[u8]) -> (Codec, Codec) {
        let client = Codec::new(&self.context(Role::Client, group), &self.client, &[]).unwrap();
        let server = Codec::new(&self.context(Role::Server, group), &self.server, &[]).unwrap();
        (client, server)
    }
}

fn clock() -> Time {
    Time {
        unix: NOW,
        monotonic: Instant::now(),
    }
}

fn created_body() -> Body {
    Body::Probe { client: [0xab; 16] }
}

fn rumor(signer: &Keys, receiver: PublicKey, content: String) -> UnsignedEvent {
    UnsignedEvent::new(
        signer.public_key(),
        Timestamp::from_secs(NOW),
        KIND,
        [Tag::public_key(receiver)],
        content,
    )
}

fn probe_rumor(client: &Keys, codec: &Codec) -> UnsignedEvent {
    let wire = Wire::from_body(codec.binding.digest, created_body(), NOW).unwrap();
    rumor(
        client,
        codec.outbound,
        serde_json::to_string(&wire).unwrap(),
    )
}

fn gift(signer: &Keys, receiver: PublicKey, rumor: UnsignedEvent) -> Vec<u8> {
    let event = GiftWrapBuilder::new(receiver, rumor)
        .finalize(signer)
        .unwrap();
    serde_json::to_vec(&event).unwrap()
}

fn finish_exchange(fixture: &Fixture, client: Codec, server: Codec, time: Time, duplicates: bool) {
    let mut client = Exchange::new(client);
    let mut server = Exchange::new(server);
    client.start(&fixture.client, time, true).unwrap();
    let mut next = Step::Probe;
    let mut sender = &mut client;
    let mut receiver = &mut server;
    for expected in [
        Advance::Send(Step::Challenge),
        Advance::Send(Step::Confirm),
        Advance::Send(Step::Ready),
        Advance::Confirmed,
    ] {
        let signer = match next.sender() {
            Role::Client => &fixture.server,
            Role::Server => &fixture.client,
        };
        let frame = &sender.retry(next, time, true).unwrap().frame;
        assert_eq!(
            receiver.receive(signer, frame, time, true).unwrap(),
            expected
        );
        if duplicates {
            assert_eq!(
                receiver.receive(signer, frame, time, true).unwrap(),
                Advance::Duplicate
            );
        }
        if let Advance::Send(step) = expected {
            next = step;
        }
        std::mem::swap(&mut sender, &mut receiver);
    }
    let (client_keys, client_peer, client_id) = client.confirmed(time, true).unwrap();
    let (server_keys, server_peer, server_id) = server.confirmed(time, true).unwrap();
    assert!(client_keys.public_key() == server_peer && server_keys.public_key() == client_peer);
    assert!(client_peer != server_peer && client_id == server_id);
}

#[test]
fn signed_exchange_binds_current_command_room() {
    let fixture = Fixture::new();
    let (client, server) = fixture.codecs(b"one Command room");
    assert!(client.binding.digest == server.binding.digest);
    assert!(client.inbound.public == server.outbound && client.outbound == server.inbound.public);
    assert!(client.inbound.public != server.inbound.public);
    assert!(!client.fresh_terminal_key(fixture.client.public_key()));
    let context = fixture.context(Role::Client, b"one Command room");
    assert!(matches!(
        Codec::new(&context, &fixture.server, &[]),
        Err(Error::Invalid("terminal_bootstrap_binding"))
    ));
    assert!(matches!(
        Codec::new(&context, &fixture.client, &[client.inbound.public]),
        Err(Error::Invalid("terminal_bootstrap_alias"))
    ));
    for group in [&[][..], &[0u8; GROUP_CAP + 1][..]] {
        assert!(Binding::new(&fixture.context(Role::Client, group), &fixture.client).is_err());
    }

    // A wrong Command digest remains wrong even inside a correctly routed,
    // canonically signed gift. Separate Command rooms also derive separate aliases.
    let (_, other) = fixture.codecs(b"another Command room");
    assert!(server.inbound.public != other.inbound.public);
    let wrong = Wire::from_body(other.binding.digest, created_body(), NOW).unwrap();
    let wrong = rumor(
        &fixture.client,
        client.outbound,
        serde_json::to_string(&wrong).unwrap(),
    );
    let frame = gift(&fixture.client, client.outbound, wrong);
    assert!(matches!(
        server.open(&frame, NOW),
        Err(Error::Invalid("terminal_bootstrap_binding"))
    ));
    let different = Fixture {
        client: fixture.client.clone(),
        server: Keys::generate(),
    };
    assert!(different.codecs(b"one Command room").0.inbound.public != client.inbound.public);
    finish_exchange(&fixture, client, server, clock(), true);
}

fn manual_seal(
    signer: &Keys,
    receiver: PublicKey,
    kind: Kind,
    tags: Vec<Tag>,
    plaintext: &[u8],
) -> Event {
    let encrypted = nip44::encrypt(
        signer.secret_key(),
        &receiver,
        plaintext,
        nip44::Version::V2,
    )
    .unwrap();
    EventBuilder::new(kind, encrypted)
        .tags(tags)
        .finalize(signer)
        .unwrap()
}

fn manual_outer(receiver: PublicKey, seal: &Event) -> Vec<u8> {
    let wrap = manual_seal(
        &Keys::generate(),
        receiver,
        Kind::GiftWrap,
        vec![Tag::public_key(receiver)],
        &serde_json::to_vec(seal).unwrap(),
    );
    serde_json::to_vec(&wrap).unwrap()
}

#[test]
fn signed_outer_seal_and_rumor_keep_recipient_and_sender_distinct() {
    let fixture = Fixture::new();
    let (client, server) = fixture.codecs(b"layers");
    let normal = || probe_rumor(&fixture.client, &client);
    let receiver = client.outbound;
    let foreign = Keys::generate();
    let wrong_recipient = fixture.server.public_key();
    let good = client.wrap(&fixture.client, created_body(), NOW).unwrap();
    assert!(server.open(&good.frame, NOW).is_ok());
    assert!(client.open(&good.frame, NOW).is_err());

    let mut bad_signature: Event = serde_json::from_slice(&good.frame).unwrap();
    let decorated = GiftWrapBuilder::new(receiver, normal())
        .expiration(Duration::from_secs(200_000))
        .finalize(&fixture.client)
        .unwrap();
    bad_signature.sig = decorated.sig;
    for outer in [bad_signature, decorated] {
        assert!(matches!(
            server.open(&serde_json::to_vec(&outer).unwrap(), NOW),
            Err(Error::Invalid("terminal_bootstrap_wrap"))
        ));
    }
    let routed_to_canonical = gift(&fixture.client, wrong_recipient, normal());
    assert!(server.open(&routed_to_canonical, NOW).is_err());
    assert!(server.open(&vec![b'x'; OUTER_CAP + 1], NOW).is_err());
    let oversized_outer = EventBuilder::new(Kind::GiftWrap, "x".repeat(OUTER_CONTENT_CAP + 1))
        .tags([Tag::public_key(receiver)])
        .finalize(&foreign)
        .unwrap();
    let oversized_frame = serde_json::to_vec(&oversized_outer).unwrap();
    assert!(oversized_frame.len() <= OUTER_CAP);
    assert!(server.open(&oversized_frame, NOW).is_err());

    let plaintext = serde_json::to_vec(&normal()).unwrap();
    for seal in [
        manual_seal(
            &fixture.client,
            receiver,
            Kind::TextNote,
            vec![],
            &plaintext,
        ),
        manual_seal(
            &fixture.client,
            receiver,
            Kind::Seal,
            vec![Tag::public_key(receiver)],
            &plaintext,
        ),
        manual_seal(&foreign, receiver, Kind::Seal, vec![], &plaintext),
        manual_seal(
            &fixture.client,
            receiver,
            Kind::Seal,
            vec![],
            &vec![b'x'; SEAL_CONTENT_CAP],
        ),
    ] {
        assert!(matches!(
            server.open(&manual_outer(receiver, &seal), NOW),
            Err(Error::Invalid("terminal_bootstrap_seal"))
        ));
    }
    let mut forged_seal = manual_seal(&fixture.client, receiver, Kind::Seal, vec![], &plaintext);
    forged_seal.sig = EventBuilder::new(Kind::TextNote, "signature from another event")
        .finalize(&fixture.client)
        .unwrap()
        .sig;
    assert!(
        server
            .open(&manual_outer(receiver, &forged_seal), NOW)
            .is_err()
    );

    let mut forged_id = normal();
    forged_id.id = Some(EventId::from_byte_array([0; 32]));
    let mut extra_recipient = normal();
    extra_recipient.tags = nostr::event::Tags::from_list(vec![
        Tag::public_key(receiver),
        Tag::public_key(wrong_recipient),
    ]);
    let mut wrong_author = normal();
    wrong_author.pubkey = foreign.public_key();
    let mut wrong_kind = normal();
    wrong_kind.kind = Kind::TextNote;
    let mut long_body = normal();
    long_body.content.push_str(&" ".repeat(RUMOR_CONTENT_CAP));
    for event in [
        forged_id,
        extra_recipient,
        wrong_author,
        wrong_kind,
        long_body,
    ] {
        let frame = gift(&fixture.client, receiver, event);
        assert!(matches!(
            server.open(&frame, NOW),
            Err(Error::Invalid("terminal_bootstrap_rumor"))
        ));
    }
}

#[test]
fn strict_record_shape_and_clock_bounds_are_authenticated() {
    let fixture = Fixture::new();
    let (client, server) = fixture.codecs(b"record shape");
    let raw = probe_rumor(&fixture.client, &client).content;
    for changed in [
        raw.replacen("{", "{\"version\":1,", 1),
        raw.replacen("{", "{\"ignored\":1,", 1),
        raw.replacen("{", "{\"previous\":null,", 1),
        raw.replacen("{", &format!("{{\"previous\":\"{}\",", "ab".repeat(32)), 1),
        raw.replace("lossless", "lossy"),
        raw.replace("ab".repeat(16).as_str(), "AB".repeat(16).as_str()),
        raw.replace("\"version\":1", "\"version\":1.0"),
        raw.replace("\"version\":1", "\"version\":2"),
        raw.replace("\"sender_role\":\"client\"", "\"sender_role\":\"server\""),
        raw.replace(
            &format!("\"expires_at\":{}", NOW + LIFETIME),
            "\"expires_at\":11",
        ),
    ] {
        assert!(changed != raw);
        let frame = gift(
            &fixture.client,
            client.outbound,
            rumor(&fixture.client, client.outbound, changed),
        );
        assert!(server.open(&frame, NOW).is_err());
    }
    let frame = &client
        .wrap(&fixture.client, created_body(), NOW)
        .unwrap()
        .frame;
    for accepted in [NOW - 5, NOW, NOW + 35] {
        assert!(server.open(frame, accepted).is_ok());
    }
    for rejected in [NOW - 6, NOW + 36] {
        assert!(server.open(frame, rejected).is_err());
    }
    assert!(
        client
            .wrap(&fixture.client, created_body(), u64::MAX)
            .is_err()
    );
}

#[test]
fn unlinked_skipped_restarted_and_revoked_exchanges_never_confirm() {
    let fixture = Fixture::new();
    let (client_codec, server_codec) = fixture.codecs(b"volatile liveness");
    let time = clock();
    let (_, fresh_server) = fixture.codecs(b"volatile liveness");
    let mut restarted = Exchange::new(fresh_server);
    let mut server = Exchange::new(server_codec);

    let send = |body| client_codec.wrap(&fixture.client, body, NOW).unwrap();
    let probe = send(created_body());
    let first = server
        .receive(&fixture.server, &probe.frame, time, true)
        .unwrap();
    assert_eq!(first, Advance::Send(Step::Challenge));
    assert!(server.confirmed(time, true).is_err());
    let rewrap = send(created_body());
    assert!(probe.frame != rewrap.frame && probe.id == rewrap.id);
    assert_eq!(
        server
            .receive(&fixture.server, &rewrap.frame, time, true)
            .unwrap(),
        Advance::Duplicate
    );
    let second_probe = send(Body::Probe { client: [9; 16] });
    assert!(matches!(
        server.receive(&fixture.server, &second_probe.frame, time, true),
        Err(Error::Invalid("terminal_bootstrap_rate"))
    ));
    let response = &server.retry(Step::Challenge, time, true).unwrap().frame;
    let challenge = client_codec.open(response, NOW).unwrap();
    let Body::Challenge { pair, .. } = challenge.body else {
        panic!("expected challenge")
    };

    let public = Keys::generate().public_key();
    let confirm = |pair, previous, client_key| {
        send(Body::Confirm {
            pair,
            previous,
            client_key,
        })
    };
    let authentic = confirm(pair, challenge.id, public);
    assert!(
        restarted
            .receive(&fixture.server, &authentic.frame, time, true)
            .is_err()
    );
    assert_eq!(
        restarted
            .receive(&fixture.server, &probe.frame, time, true)
            .unwrap(),
        Advance::Send(Step::Challenge)
    );
    assert!(
        restarted
            .receive(&fixture.server, &authentic.frame, time, true)
            .is_err()
    );
    assert!(restarted.confirmed(time, true).is_err());

    for invalid in [
        confirm(pair, probe.id, public),
        confirm(
            Pair {
                server: [0; 16],
                ..pair
            },
            challenge.id,
            public,
        ),
        confirm(pair, challenge.id, fixture.client.public_key()),
    ] {
        assert!(
            server
                .receive(&fixture.server, &invalid.frame, time, true)
                .is_err()
        );
        assert!(server.confirmed(time, true).is_err());
    }
    let expired = Time {
        unix: NOW + 30,
        monotonic: time.monotonic + Duration::from_secs(30),
    };
    assert!(
        server
            .receive(&fixture.server, &authentic.frame, expired, true)
            .is_err()
    );
    assert!(server.confirmed(expired, true).is_err());
    assert!(matches!(
        server.receive(&fixture.server, &probe.frame, expired, true),
        Err(Error::Invalid("terminal_bootstrap_rate"))
    ));

    assert!(matches!(
        restarted.retry(Step::Challenge, time, false),
        Err(Error::Invalid("terminal_bootstrap_authority"))
    ));
    assert!(restarted.retry(Step::Challenge, time, true).is_err());
    assert!(matches!(
        restarted.receive(&fixture.server, &probe.frame, time, true),
        Err(Error::Invalid("terminal_bootstrap_authority"))
    ));
    assert!(
        restarted
            .receive(&fixture.server, &authentic.frame, time, false)
            .is_err()
    );
    assert!(restarted.confirmed(time, true).is_err());

    // Revocation is checked even when a valid Confirm would otherwise
    // produce Ready and make the native bootstrap transcript available.
    let (_, revocable) = fixture.codecs(b"volatile liveness");
    let mut revocable = Exchange::new(revocable);
    revocable
        .receive(&fixture.server, &probe.frame, time, true)
        .unwrap();
    let response = revocable.retry(Step::Challenge, time, true).unwrap();
    let verified = client_codec.open(&response.frame, NOW).unwrap();
    let Body::Challenge { pair, .. } = verified.body else {
        panic!("expected Challenge");
    };
    let authentic = confirm(pair, verified.id, public);
    assert!(matches!(
        revocable.receive(&fixture.server, &authentic.frame, time, false),
        Err(Error::Invalid("terminal_bootstrap_authority"))
    ));
    assert!(revocable.retry(Step::Ready, time, true).is_err());
    assert!(revocable.confirmed(time, true).is_err());
}

#[test]
fn ready_requires_both_challenges_and_the_exact_confirmed_client_key() {
    let fixture = Fixture::new();
    let (client_codec, server_codec) = fixture.codecs(b"Ready liveness");
    let (_, signing_codec) = fixture.codecs(b"Ready liveness");
    let time = clock();
    let mut client = Exchange::new(client_codec);
    let mut server = Exchange::new(server_codec);
    client.start(&fixture.client, time, true).unwrap();
    let probe = &client.retry(Step::Probe, time, true).unwrap().frame;
    server.receive(&fixture.server, probe, time, true).unwrap();
    let challenge = &server.retry(Step::Challenge, time, true).unwrap().frame;
    client
        .receive(&fixture.client, challenge, time, true)
        .unwrap();
    let confirm = client.retry(Step::Confirm, time, true).unwrap();
    let decoded = signing_codec.open(&confirm.frame, NOW).unwrap();
    let Body::Confirm {
        pair, client_key, ..
    } = decoded.body
    else {
        panic!("expected Confirm");
    };

    let server_key = Keys::generate().public_key();
    for (pair, previous, client_key, server_key) in [
        (pair, decoded.id, server_key, Keys::generate().public_key()),
        (pair, decoded.id, client_key, fixture.server.public_key()),
        (pair, decoded.id, client_key, client_key),
        (pair, [0; 32], client_key, server_key),
        (
            Pair {
                server: [0; 16],
                ..pair
            },
            decoded.id,
            client_key,
            server_key,
        ),
    ] {
        let forged = signing_codec
            .wrap(
                &fixture.server,
                Body::Ready {
                    pair,
                    previous,
                    client_key,
                    server_key,
                },
                NOW,
            )
            .unwrap();
        assert!(
            client
                .receive(&fixture.client, &forged.frame, time, true)
                .is_err()
        );
        assert!(client.confirmed(time, true).is_err());
    }

    let confirm = &client.retry(Step::Confirm, time, true).unwrap().frame;
    assert_eq!(
        server
            .receive(&fixture.server, confirm, time, true)
            .unwrap(),
        Advance::Send(Step::Ready)
    );
    let ready = &server.retry(Step::Ready, time, true).unwrap().frame;
    assert_eq!(
        client.receive(&fixture.client, ready, time, true).unwrap(),
        Advance::Confirmed
    );
}

#[test]
fn duplicate_refused_confirm_never_prepares_ready() {
    let fixture = Fixture::new();
    let group = [13; 32];
    for (grant, capacity) in [(false, true), (true, false)] {
        let (client, server) = fixture.codecs(&group);
        let mut client = Exchange::new(client);
        let mut server = Exchange::new(server);
        let server_signing = BoundSigning::bind(
            &fixture.context(Role::Server, &group),
            Arc::new(fixture.server.clone()),
        )
        .unwrap()
        .0;
        let time = clock();
        client.start(&fixture.client, time, true).unwrap();
        let probe = &client.retry(Step::Probe, time, true).unwrap().frame;
        assert_eq!(
            server.receive(&fixture.server, probe, time, true).unwrap(),
            Advance::Send(Step::Challenge)
        );
        let challenge = &server.retry(Step::Challenge, time, true).unwrap().frame;
        assert_eq!(
            client
                .receive(&fixture.client, challenge, time, true)
                .unwrap(),
            Advance::Send(Step::Confirm)
        );
        let confirm = &client.retry(Step::Confirm, time, true).unwrap().frame;
        let mut gates = 0;
        let gate = |step, _| {
            assert_eq!(step, Step::Confirm);
            gates += 1;
            Ok(grant && capacity)
        };
        assert_eq!(
            server
                .receive_bound(&server_signing, confirm, time, gate)
                .unwrap(),
            Advance::Duplicate,
        );
        assert!(server.retry(Step::Ready, time, true).is_err());
        // The live owner retires the source on either admission error before
        // another delivery can reach the bootstrap state machine.
        assert!(
            server
                .receive_checked(&fixture.server, confirm, time, false, |_, _| {
                    panic!("retired authority ran the admission gate")
                })
                .is_err()
        );
        assert!(server.retry(Step::Ready, time, false).is_err());
        assert_eq!(gates, 1, "a denied Confirm stays terminal");
    }
}

#[test]
#[ignore = "opt-in warm cryptographic bootstrap benchmark"]
fn benchmark_signed_bootstrap() {
    use std::hint::black_box;
    let time = clock();
    let before = Instant::now();
    let fixture = Fixture::new();
    let (client, server) = fixture.codecs(b"benchmark");
    let cold_micros = before.elapsed().as_micros();
    // The first signing and decrypt routines have initialized their caches
    // before the warm sample begins.
    let frame = client
        .wrap(&fixture.client, created_body(), NOW)
        .unwrap()
        .frame;
    server.open(&frame, NOW).unwrap();
    let mut messages = Vec::new();
    let mut exchanges = Vec::new();
    for _ in 0..32 {
        let before = Instant::now();
        let frame = client
            .wrap(&fixture.client, created_body(), NOW)
            .unwrap()
            .frame;
        black_box(server.open(&frame, NOW).unwrap());
        messages.push(before.elapsed().as_micros());

        let (client, server) = fixture.codecs(b"benchmark");
        let before = Instant::now();
        finish_exchange(&fixture, client, server, time, false);
        exchanges.push(before.elapsed().as_micros());
    }
    for sample in [&mut messages, &mut exchanges] {
        sample.sort_unstable();
    }
    println!("bootstrap cold canonical key and alias setup={cold_micros}us");
    println!(
        "bootstrap warm wrap/check median={}us p95={}us samples=32",
        messages[16], messages[30]
    );
    println!(
        "bootstrap warm four records median={}us p95={}us samples=32",
        exchanges[16], exchanges[30]
    );
}
