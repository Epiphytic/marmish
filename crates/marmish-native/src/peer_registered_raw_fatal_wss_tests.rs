use super::super::{ControlOutcome, Failure, Operation, Owner, offer, permit, wall_now};
use super::{Keys, PeerRecipientPort, ReceiverRouteAdmission, Trip, run_cases, time_limit};
use crate::{
    Error,
    identity::{IdentityMaterial, ProfileLease},
    mdk_adapter::terminal_relay_fixture::{RecipientFault, Relay, SocketEnd},
    peer_control_version::ControlVersion,
    transport::PeerSocketDrain,
};
use cgka_traits::app_components::NostrRoutingV1;
use std::{future::poll_fn, path::Path, time::Duration};
use tokio::time::{Instant, sleep, timeout};

const RUNTIME_SOCKET: usize = 5;
const PROMPT: Duration = Duration::from_secs(2);

#[test]
#[ignore = "uses fresh subprocesses, private WSS relays and real SQLCipher"]
fn peer_registered_raw_fatal_private_wss() {
    run_cases(&[
        "fatal_before_open",
        "fatal_startup",
        "fatal_drop_open",
        "fatal_bad_eose",
        "fatal_idle",
        "fatal_stop",
        "fatal_page",
        "fatal_use_page",
        "fatal_use_ack",
        "fatal_buffered_live",
        "fatal_use_live",
        "fatal_split_sink",
        "fatal_split_stream",
    ]);
}

pub(super) async fn run(
    case: &str,
    relay: &Relay,
    lease: &ProfileLease,
    material: &IdentityMaterial,
    founder: &Keys,
    trip: Trip,
    (admission, drain): (ReceiverRouteAdmission, PeerSocketDrain),
) {
    let account = material.public_key();
    if case == "before_open" {
        trip.trip();
        assert!(matches!(
            drain.reap(Instant::now() + Duration::from_millis(25)).await,
            Err(Error::Timeout)
        ));
        assert!(matches!(
            PeerRecipientPort::open(admission, time_limit()).await,
            Err(Failure::Retired)
        ));
        drain.reap(time_limit()).await.unwrap();
        sleep(Duration::from_millis(60)).await;
        assert_eq!(relay.qualification_connections(), RUNTIME_SOCKET - 1);
        assert_eq!(drain.tcp_attempts(), 0);
        assert!(!relay.authenticated(account));
        assert_eq!(relay.recipient_requests(account), 0);
        assert_eq!(relay.recipient_closes(account), 0);
        assert_eq!(relay.all_publication_attempts(), 0);
        return;
    }

    let close_count = match case {
        "startup" | "drop_open" => {
            relay.recipient_fault(RecipientFault::MissingAuthAck);
            {
                let open = PeerRecipientPort::open(admission, time_limit());
                tokio::pin!(open);
                tokio::select! {
                    output = &mut open => panic!("open finished while waiting for withheld AUTH ACK: {:?}", output.err()),
                    _ = eventually(|| relay.authenticated(account)) => {}
                }
                assert_eq!(relay.recipient_requests(account), 0);
                assert!(relay.runtime_socket_active(RUNTIME_SOCKET));
                if case == "startup" {
                    trip.trip();
                    assert!(matches!(
                        timeout(PROMPT, open.as_mut()).await.unwrap(),
                        Err(Failure::Retired)
                    ));
                }
            }
            assert_eq!(relay.recipient_requests(account), 0);
            0
        }
        "bad_eose" => {
            relay.recipient_fault(RecipientFault::WrongLiveEose);
            assert!(matches!(
                PeerRecipientPort::open(admission, time_limit()).await,
                Err(Failure::Grammar)
            ));
            assert!(relay.authenticated(account));
            assert_eq!(relay.recipient_requests(account), 1);
            0
        }
        "split_sink" | "split_stream" => {
            let (route, _, sink, stream) = admission
                .open(super::super::super::wire::MESSAGE_LIMIT, time_limit())
                .await
                .unwrap();
            drop(route);
            if case == "split_sink" {
                drop(stream);
                assert_split_held(&drain, relay).await;
                drop(sink);
            } else {
                drop(sink);
                assert_split_held(&drain, relay).await;
                drop(stream);
            }
            0
        }
        _ => {
            let mut port = PeerRecipientPort::open(admission, time_limit())
                .await
                .unwrap();
            assert!(relay.runtime_socket_active(RUNTIME_SOCKET));
            assert!(matches!(
                drain.reap(time_limit()).await,
                Err(Error::Transport("peer_socket_revoke_required"))
            ));
            let wall = port.use_current_recipient(|_, _, wall| wall).unwrap();
            let bounds = [wall - 1, wall];
            match case {
                "idle" => {
                    let pending = port.wait_live(time_limit());
                    tokio::pin!(pending);
                    assert!(
                        poll_fn(|cx| std::task::Poll::Ready(
                            pending.as_mut().poll(cx).is_pending()
                        ))
                        .await
                    );
                    trip.trip();
                    assert!(matches!(
                        timeout(PROMPT, pending).await.unwrap(),
                        Err(Failure::Retired)
                    ));
                    0
                }
                "stop" => {
                    trip.trip();
                    assert!(matches!(
                        port.stop(time_limit()).await,
                        Err(Failure::Retired)
                    ));
                    0
                }
                "page" => {
                    trip.trip();
                    assert!(matches!(
                        port.page(bounds, time_limit()).await,
                        Err(Failure::Retired)
                    ));
                    assert_eq!(relay.recipient_requests(account), 1);
                    0
                }
                "use_page" => {
                    let page = port.page(bounds, time_limit()).await.unwrap();
                    port.use_page(&page, |checked| assert!(checked.records.is_empty()))
                        .unwrap();
                    eventually(|| relay.recipient_closes(account) == 1).await;
                    trip.trip();
                    assert!(matches!(
                        port.use_page(&page, |_| panic!("retired page callback")),
                        Err(Failure::Retired)
                    ));
                    assert_eq!(relay.recipient_requests(account), 2);
                    1
                }
                "use_ack" => {
                    let routing =
                        NostrRoutingV1::new([3; 32], vec![relay.url.as_str().to_owned()]).unwrap();
                    let outgoing = offer(
                        &material.signer(),
                        founder.public_key(),
                        &routing,
                        ControlVersion::V2,
                    );
                    let revision = outgoing.flight.flight().revision.to_bytes();
                    let mut current = |operation: Operation, _| {
                        operation.id == revision && operation.generation == 1
                    };
                    let send = permit(&port, &outgoing, &mut current);
                    let ControlOutcome::Accepted(ack) =
                        port.publish_control(send, time_limit()).await
                    else {
                        panic!("exact successful acknowledgment required");
                    };
                    port.use_accepted(&ack, |_| ()).unwrap();
                    trip.trip();
                    assert!(matches!(
                        port.use_accepted(&ack, |_| panic!("retired acknowledgment callback")),
                        Err(Failure::Retired)
                    ));
                    assert_eq!(relay.all_publication_attempts(), 1);
                    0
                }
                "buffered_live" | "use_live" => {
                    let record =
                        reserve_live(&mut port, relay, lease.root(), material, founder).await;
                    assert_eq!(port.0.live_records.len(), 1);
                    trip.trip();
                    if case == "buffered_live" {
                        assert!(matches!(
                            port.wait_live(time_limit()).await,
                            Err(Failure::Retired)
                        ));
                    } else {
                        assert!(matches!(
                            port.use_live(&record, |_| panic!("retired live callback")),
                            Err(Failure::Retired)
                        ));
                    }
                    assert_eq!(relay.recipient_requests(account), 1);
                    assert_eq!(relay.all_publication_attempts(), 2);
                    0
                }
                _ => panic!("unrecognized registered fatal fixture case"),
            }
        }
    };
    drain.reap(Instant::now() + PROMPT).await.unwrap();
    assert_eq!(drain.tcp_attempts(), 1);
    eventually(|| relay.runtime_socket_end(RUNTIME_SOCKET).is_some()).await;
    assert_eq!(
        relay.runtime_socket_end(RUNTIME_SOCKET),
        Some(SocketEnd::Raw)
    );
    assert!(!relay.runtime_socket_active(RUNTIME_SOCKET));
    assert_eq!(relay.recipient_closes(account), close_count);
    if !matches!(case, "use_ack" | "buffered_live" | "use_live") {
        assert_eq!(relay.all_publication_attempts(), 0);
    }
}

async fn assert_split_held(drain: &PeerSocketDrain, relay: &Relay) {
    assert!(matches!(
        drain.reap(Instant::now() + Duration::from_millis(25)).await,
        Err(Error::Timeout)
    ));
    assert!(relay.runtime_socket_active(RUNTIME_SOCKET));
}

async fn reserve_live(
    port: &mut PeerRecipientPort,
    relay: &Relay,
    profile: &Path,
    material: &IdentityMaterial,
    founder: &Keys,
) -> super::super::LiveRecord {
    let routing = NostrRoutingV1::new([3; 32], vec![relay.url.as_str().to_owned()]).unwrap();
    let connector = relay.connector(profile, true, None);
    let mut sender = Owner::open(
        &connector,
        relay.url.as_str(),
        founder.public_key(),
        founder,
        time_limit(),
    )
    .await
    .unwrap();
    let account = material.public_key();
    let wall = wall_now();
    let codec = super::super::Codec::for_version(super::super::MAX_OUTER_BYTES, ControlVersion::V2)
        .unwrap();
    for id in [1, 2] {
        let offer = codec
            .prepare_offer(founder, account, [id; 16], &routing, wall)
            .unwrap();
        assert!(
            sender
                .publish(&offer, offer.flight.flight(), time_limit())
                .await
                .unwrap()
        );
    }
    timeout(PROMPT, async {
        while port.0.live_records.len() < 2 {
            port.0
                .predrain(&mut super::super::super::Budget::default(), time_limit())
                .await
                .unwrap();
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let record = port.wait_live(time_limit()).await.unwrap().unwrap();
    port.use_live(&record, |_| ()).unwrap();
    record
}

async fn eventually(mut condition: impl FnMut() -> bool) {
    timeout(PROMPT, async {
        while !condition() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("private relay observation before deadline");
}
