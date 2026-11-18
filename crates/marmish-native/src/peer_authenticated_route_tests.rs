use crate::{
    mdk_adapter::terminal_relay_fixture::RecipientFault,
    peer_control_version::ControlVersion,
    peer_invitation::{
        Paths, record,
        store::{
            HardenedPeerConnection, Page as CursorPage, ScanStart, TrustedOutcome,
            consent_checkpoint::{
                Checkpoint, Policy, Route, Selector, Status,
                wire::{Codec, MAX_OUTER_BYTES, Offer},
            },
        },
    },
    peer_relay_trust::qualified_recipient_fixture,
};
use cgka_traits::app_components::NostrRoutingV1;
use std::{
    task::Context,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::*;

fn time_limit() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn wall_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn offer(
    signer: &Keys,
    recipient: PublicKey,
    route: &NostrRoutingV1,
    version: ControlVersion,
) -> Prepared<Offer> {
    Codec::for_version(MAX_OUTER_BYTES, version)
        .unwrap()
        .prepare_offer(signer, recipient, [0x8d; 16], route, wall_now())
        .unwrap()
}

fn permit<'a>(
    port: &PeerRecipientPort,
    prepared: &'a Prepared<Offer>,
    current: &'a mut dyn FnMut(Operation, u64) -> bool,
) -> ControlSendPermit<'a> {
    let flight = prepared.flight.flight();
    assert_eq!(prepared.flight.version(), ControlVersion::V2);
    assert_eq!(flight.author.to_bytes(), port.0.scope.recipient);
    assert_eq!(
        prepared.flight.routing().relays,
        [port.0.scope.origin.as_str()]
    );
    ControlSendPermit {
        scope: port.0.scope.clone(),
        remote: flight.recipient,
        carrier: flight.carrier_id.to_bytes(),
        digest: sealed_digest(prepared.frame()),
        operation: Operation {
            id: flight.revision.to_bytes(),
            generation: 1,
        },
        deadline: flight.expires_at,
        bytes: prepared.frame(),
        current,
    }
}

fn open_checkpoint(paths: &Paths<'_>, signer: &Keys, url: &str) -> Checkpoint {
    let checked = paths.cursor().unwrap();
    let conn = HardenedPeerConnection::open(checked.path(), *paths.cursor_key()).unwrap();
    checked.unchanged().unwrap();
    // SAFETY: reads the effective UID of this Unix process.
    let owner_uid = unsafe { libc::geteuid() };
    Checkpoint::attach_installed_v2(conn, signer.clone(), owner_uid, url, Policy::default())
        .unwrap()
}

#[tokio::test]
#[ignore = "uses a disposable private WSS relay and real SQLCipher"]
async fn peer_authenticated_route_private_wss_installed_marker_and_exact_send() {
    let started = Instant::now();
    let founder = Keys::generate();
    let live_founder = Keys::generate();
    let (mut relay, _temp, lease, material) =
        qualified_recipient_fixture(founder.public_key(), live_founder.public_key()).await;
    let qualified = Instant::now();
    assert_eq!(relay.qualification_connections(), 4);
    let recipient_keys = material.signer();
    let recipient = material.public_key();
    let url = relay.url.as_str();
    let route = Route::configured(url).unwrap();
    let routing = NostrRoutingV1::new([3; 32], vec![url.to_owned()]).unwrap();
    let paths = Paths::new(&lease, &material).unwrap();
    paths.initialize().unwrap();

    let connector = relay.connector(lease.root(), true, None);
    let mut sender = Owner::open(
        &connector,
        url,
        founder.public_key(),
        &founder,
        time_limit(),
    )
    .await
    .unwrap();
    let history = offer(&founder, recipient, &routing, ControlVersion::V2);
    assert!(
        sender
            .publish(&history, history.flight.flight(), time_limit())
            .await
            .unwrap()
    );

    let open_port = || ReceiverRouteAdmission::fixture(lease.clone(), &material).unwrap();
    let mut port = PeerRecipientPort::open(open_port(), time_limit())
        .await
        .unwrap();
    let attached = Instant::now();
    assert!(relay.authenticated(recipient));
    assert!(relay.authenticated(founder.public_key()));

    let mut checkpoint = open_checkpoint(&paths, &recipient_keys, url);
    let source = checkpoint.bind_source(url).unwrap();
    let wall = wall_now();
    checkpoint.cursor.require_available(route.id, wall).unwrap();
    let cursor = checkpoint
        .cursor
        .scan(route.id, ScanStart::Recovery, wall)
        .unwrap()
        .unwrap();
    let page = port
        .page([cursor.lower, cursor.until], time_limit())
        .await
        .unwrap();
    port.use_page(&page, |view| {
        assert_eq!(view.account, recipient);
        assert_eq!(view.origin, url);
        assert!(view.request.ends_with("/p/1"));
        assert_eq!(view.bounds, [cursor.lower, cursor.until]);
        let [incoming] = view.records else {
            panic!("one history record expected")
        };
        assert_eq!(incoming.id, history.flight.flight().carrier_id.to_bytes());
        let raw = [record(incoming)];
        let result = checkpoint.admit(&source, raw[0], view.wall).unwrap();
        assert_eq!(result.status, Status::Accepted(TrustedOutcome::Applied));
        assert!(
            checkpoint
                .cursor
                .commit_page(
                    &cursor,
                    &CursorPage {
                        origin: route.id,
                        since: view.bounds[0],
                        until: view.bounds[1],
                        records: &raw,
                    },
                    &[result.receipt],
                    view.wall
                )
                .unwrap()
                .is_none()
        );
    })
    .unwrap();

    let legacy = offer(&founder, recipient, &routing, ControlVersion::V1);
    let live = offer(&live_founder, recipient, &routing, ControlVersion::V2);
    let unregistered = Keys::generate();
    let misaddressed = offer(
        &founder,
        unregistered.public_key(),
        &routing,
        ControlVersion::V2,
    );
    assert!(
        !sender
            .publish(&misaddressed, misaddressed.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    // Existing installed policy retains at most one live Offer per founder.
    // A distinct founder proves the live path can create a second marker.
    let live_connector = relay.connector(lease.root(), true, None);
    let mut live_sender = Owner::open(
        &live_connector,
        url,
        live_founder.public_key(),
        &live_founder,
        time_limit(),
    )
    .await
    .unwrap();
    for (sender, event, expected) in [
        (&mut sender, &legacy, Status::Invalid),
        (
            &mut live_sender,
            &live,
            Status::Accepted(TrustedOutcome::Applied),
        ),
    ] {
        assert!(
            sender
                .publish(event, event.flight.flight(), time_limit())
                .await
                .unwrap()
        );
        let record = port.wait_live(time_limit()).await.unwrap().unwrap();
        port.use_live(&record, |view| {
            assert_eq!((view.account, view.origin), (recipient, url));
            assert_eq!(view.record.id, event.flight.flight().carrier_id.to_bytes());
            let outcome = checkpoint
                .admit(
                    &source,
                    crate::peer_invitation::record(view.record),
                    view.wall,
                )
                .unwrap();
            assert_eq!(outcome.status, expected);
        })
        .unwrap();
    }

    drop(checkpoint);
    let mut reopened = open_checkpoint(&paths, &recipient_keys, url);
    let cold_source = reopened.bind_source(url).unwrap();
    for received in [&history, &live] {
        let selector = Selector::from(&received.flight);
        assert!(
            reopened
                .installed_live_offer(&cold_source, selector, wall_now())
                .is_ok()
        );
    }
    for rejected in [&legacy, &misaddressed] {
        assert!(matches!(
            reopened.installed_live_offer(
                &cold_source,
                Selector::from(&rejected.flight),
                wall_now()
            ),
            Err(crate::peer_invitation::store::consent_checkpoint::Error::Missing)
        ));
    }
    assert!(
        reopened
            .cursor
            .scan(route.id, ScanStart::Periodic, wall_now())
            .unwrap()
            .is_none()
    );
    let admitted = Instant::now();

    let outgoing = offer(
        &recipient_keys,
        founder.public_key(),
        &routing,
        ControlVersion::V2,
    );
    let flight = outgoing.flight.flight();
    let mut exact = |operation: Operation, _: u64| {
        operation.id == flight.revision.to_bytes() && operation.generation == 1
    };
    let outbound = permit(&port, &outgoing, &mut exact);
    let ControlOutcome::Accepted(ack) = port.publish_control(outbound, time_limit()).await else {
        panic!("expected exact positive ACK");
    };
    port.use_accepted(&ack, |view| {
        assert_eq!((view.account, view.origin), (recipient, url));
        assert!(view.wall >= flight.created_at);
        assert_eq!(view.carrier, flight.carrier_id.to_bytes());
        assert_eq!(
            view.operation,
            Operation {
                id: flight.revision.to_bytes(),
                generation: 1
            }
        );
    })
    .unwrap();

    let mut other = PeerRecipientPort::open(open_port(), time_limit())
        .await
        .unwrap();
    assert_eq!(
        other.use_accepted(&ack, |_| panic!("foreign ACK exposed")),
        Err(Failure::Binding)
    );
    assert_eq!(
        other.use_page(&page, |_| panic!("foreign page exposed")),
        Err(Failure::Binding)
    );
    let wrong_scope = permit(&port, &outgoing, &mut exact);
    assert!(matches!(
        other.publish_control(wrong_scope, time_limit()).await,
        ControlOutcome::NotSent(Failure::Binding)
    ));

    let lost = offer(
        &recipient_keys,
        founder.public_key(),
        &routing,
        ControlVersion::V2,
    );
    relay.recipient_lose_ack(lost.flight.flight().carrier_id);
    let mut current = |_, _| true;
    let uncertain = permit(&port, &lost, &mut current);
    assert!(matches!(
        port.publish_control(uncertain, Instant::now() + Duration::from_millis(600))
            .await,
        ControlOutcome::PossiblyExposed(Failure::Deadline)
    ));
    assert_eq!(
        port.use_accepted(&ack, |_| panic!("retired ACK exposed")),
        Err(Failure::Binding)
    );

    let mut withdrawn = |_, _| false;
    let refused = permit(&other, &lost, &mut withdrawn);
    assert!(matches!(
        other.publish_control(refused, time_limit()).await,
        ControlOutcome::NotSent(Failure::Retired)
    ));
    assert_eq!(
        other.use_page(&page, |_| panic!("retired page exposed")),
        Err(Failure::Binding)
    );
    let writes = relay.peer_transcript();
    let mut bad_digest = PeerRecipientPort::open(open_port(), time_limit())
        .await
        .unwrap();
    let mut now_current = |_, _| true;
    let mut forged = permit(&bad_digest, &lost, &mut now_current);
    forged.digest[0] ^= 1;
    assert!(matches!(
        bad_digest.publish_control(forged, time_limit()).await,
        ControlOutcome::NotSent(Failure::Binding)
    ));
    let mut invalid_generation = PeerRecipientPort::open(open_port(), time_limit())
        .await
        .unwrap();
    let mut forged = permit(&invalid_generation, &outgoing, &mut exact);
    forged.operation.generation += 1;
    assert!(matches!(
        invalid_generation
            .publish_control(forged, time_limit())
            .await,
        ControlOutcome::NotSent(Failure::Retired)
    ));
    assert_eq!(relay.publication_attempts(wire::CARRIER), 6);

    assert_eq!(writes.len(), 5);
    for (written, offered, peer) in [
        (&writes[0], &history, recipient),
        (&writes[1], &legacy, recipient),
        (&writes[2], &live, recipient),
        (&writes[3], &outgoing, founder.public_key()),
        (&writes[4], &lost, founder.public_key()),
    ] {
        assert_eq!(written.recipient, peer);
        assert_eq!(written.sender, offered.flight.flight().author);
        assert_eq!(written.event, offered.flight.flight().carrier_id);
        assert_eq!(written.bytes_sha256, sealed_digest(offered.frame()));
    }
    assert_eq!(relay.qualification_connections(), 10);
    sender.stop(time_limit()).await.unwrap();
    live_sender.stop(time_limit()).await.unwrap();
    for (fault, expected, requests) in [
        (RecipientFault::MissingAuthAck, Failure::Deadline, 0),
        (RecipientFault::NegativeAuthAck, Failure::Grammar, 0),
        (RecipientFault::WrongAuthAck, Failure::Grammar, 0),
        (RecipientFault::WrongLiveEose, Failure::Grammar, 1),
    ] {
        let before = relay.recipient_requests(recipient);
        relay.recipient_fault(fault);
        let limit = Instant::now() + Duration::from_secs(2);
        assert_eq!(
            PeerRecipientPort::open(open_port(), limit).await.err(),
            Some(expected)
        );
        assert_eq!(relay.recipient_requests(recipient), before + requests);
    }
    assert_eq!(relay.qualification_connections(), 14);
    assert_eq!(relay.peer_transcript().len(), 5);
    relay.stop().await;
    lease.cleanup_runtime().unwrap();
    eprintln!(
        "Peer qualified WSS ms: qualification={} checked_auth={} intake_cold_read={} sends={} total={}",
        (qualified - started).as_millis(),
        (attached - qualified).as_millis(),
        (admitted - attached).as_millis(),
        admitted.elapsed().as_millis(),
        started.elapsed().as_millis(),
    );
}

#[derive(Default)]
struct CountingSink {
    ready_polls: usize,
    starts: usize,
    flush_polls: usize,
    ready_pending: bool,
    flush_pending: bool,
}

impl Sink<Message> for CountingSink {
    type Error = ();

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        self.ready_polls += 1;
        if std::mem::take(&mut self.ready_pending) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, _: Message) -> std::result::Result<(), Self::Error> {
        self.starts += 1;
        Ok(())
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        self.flush_polls += 1;
        if std::mem::take(&mut self.flush_pending) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        panic!("retirement must drop without flushing");
    }
}

#[tokio::test(start_paused = true)]
async fn peer_authenticated_route_send_fence_pending_boundaries() {
    for (ready_pending, flush_pending, cut_on_check, expected) in [
        (true, false, 2, (1, 0, 0, false)), // readiness yielded before handoff
        (false, false, 3, (1, 1, 0, false)), // staged locally, no underlying flush
        (false, true, 4, (1, 1, 1, true)),  // first flush could have sent, no retry
    ] {
        let mut sink = CountingSink {
            ready_pending,
            flush_pending,
            ..Default::default()
        };
        let mut checks = 0;
        let mut exposed = false;
        let result = fenced_sink(
            &mut sink,
            Message::Text("fixture".into()),
            time_limit(),
            &mut exposed,
            |_| {
                checks += 1;
                if checks == cut_on_check {
                    Err(Failure::Retired)
                } else {
                    Ok(())
                }
            },
        )
        .await;
        assert_eq!(result, Err(Failure::Retired));
        assert_eq!(
            (sink.ready_polls, sink.starts, sink.flush_polls, exposed),
            expected
        );
    }
}

#[tokio::test(start_paused = true)]
async fn peer_authenticated_route_expiry_prevents_first_write_poll() {
    let mut sink = CountingSink::default();
    let mut exposed = false;
    assert_eq!(
        fenced_sink(
            &mut sink,
            Message::Text("fixture".into()),
            Instant::now(),
            &mut exposed,
            |_| Ok(()),
        )
        .await,
        Err(Failure::Deadline)
    );
    assert_eq!(
        (sink.ready_polls, sink.starts, sink.flush_polls, exposed),
        (0, 0, 0, false)
    );
}
