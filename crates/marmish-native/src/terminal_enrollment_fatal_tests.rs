use super::*;
use crate::dispatcher_fatal::Trip;
use crate::mdk_adapter::terminal_enrollment_room::Room;
use crate::mdk_adapter::terminal_enrollment_transport::{Authority, Lane};
use crate::mdk_adapter::terminal_relay_fixture::{Relay, SocketEnd};
use crate::terminal_fence::AttachmentFence;
use crate::transport::{RouteMode, TransportLimits};
use cgka_traits::TransportEndpoint;
use std::num::NonZeroU64;
use std::path::Path;
use std::task::Poll;
use tokio::time::timeout;

const STEP: Duration = Duration::from_secs(10);
const INBOX: usize = 64 * 1024;

fn guarded_route(relay: &Relay, root: &Path) -> (Trip, Route) {
    let policy = RelayPolicy::parse(
        &[relay.url.to_string()],
        RouteMode::Off,
        TransportLimits::default(),
    )
    .unwrap();
    let (trip, stop) = Trip::new();
    let route = Route::physical_fixture(
        policy,
        ArtiDirs {
            cache: root.join("cache"),
            state: root.join("state"),
        },
        Arc::new(Keys::generate()),
        relay.roots(),
        stop,
    )
    .unwrap();
    (trip, route)
}

fn authority(reapers: &Arc<Reapers>) -> Arc<Authority> {
    Authority::from_fence(
        AttachmentFence::new(NonZeroU64::MIN, [1; 32]),
        Instant::now() + STEP,
        Some(reapers.clone()),
        None,
    )
    .unwrap()
}

async fn open_lane(route: &Route, relay: &Relay, reapers: &Arc<Reapers>, keys: Arc<Keys>) -> Lane {
    Lane::open_verified(
        route.connector(relay.url.as_str()).unwrap(),
        TransportEndpoint(relay.url.to_string()),
        keys,
        authority(reapers),
        INBOX,
    )
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "private TLS WSS and real signed alias/room SDK subscriptions"]
async fn terminal_fatal_closes_both_live_sdk_lanes_without_protocol_close() {
    timeout(STEP, async {
        let relay = Relay::start().await;
        relay.record_terminal_socket_ends();
        let root = tempfile::tempdir().unwrap();
        let (trip, route) = guarded_route(&relay, root.path());
        let alias_keys = Arc::new(Keys::generate());
        let room_keys = Arc::new(Keys::generate());
        let mut alias = open_lane(&route, &relay, &route.client_reapers, alias_keys.clone()).await;
        let mut room = open_lane(&route, &relay, &route.client_reapers, room_keys.clone()).await;
        alias.activate_inbox().await.unwrap();
        room.begin_active_enrollment();
        room.activate_room_fixture().await.unwrap();
        assert_eq!(route.client_reapers.custody(), (2, 0, false));

        let accounts = [alias_keys.public_key(), room_keys.public_key()];
        let sockets = accounts.map(|account| relay.terminal_connection_for(account));
        assert_ne!(sockets[0], sockets[1]);
        let requests = accounts.map(|account| {
            let count = relay.recipient_requests(account);
            assert!(count > 0);
            assert_eq!(relay.recipient_closes(account), 0);
            count
        });

        trip.trip();
        assert!(matches!(
            alias.activate_inbox().await,
            Err(Error::Transport("dispatcher_fatal"))
        ));
        assert!(matches!(
            room.activate_room_fixture().await,
            Err(Error::Transport("dispatcher_fatal"))
        ));
        let (alias_result, room_result) = tokio::join!(alias.close(), room.close());
        alias_result.unwrap();
        room_result.unwrap();
        route.finish().await.unwrap();
        for ((account, socket), requests) in accounts.into_iter().zip(sockets).zip(requests) {
            while relay.runtime_socket_active(socket) {
                tokio::task::yield_now().await;
            }
            assert_eq!(relay.runtime_socket_end(socket), Some(SocketEnd::Raw));
            assert_eq!(relay.recipient_requests(account), requests);
            assert_eq!(relay.recipient_closes(account), 0);
        }
        assert_eq!(route.client_reapers.custody(), (0, 0, false));
        assert_eq!(route.server_reapers.custody(), (0, 0, false));
        relay.assert_healthy();
    })
    .await
    .expect("bounded physical SDK-Lane shutdown");
}

#[tokio::test]
#[ignore = "private TLS WSS retains real fixture socket halves across a cut"]
async fn terminal_fatal_reap_waits_for_each_actual_socket_half() {
    timeout(STEP, async {
        let relay = Relay::start().await;
        let root = tempfile::tempdir().unwrap();
        let (trip, route) = guarded_route(&relay, root.path());
        let connector = route.connector(relay.url.as_str()).unwrap();
        connector.bootstrap(Instant::now() + STEP).await.unwrap();
        let (write, read) = connector.peer_test_open(&relay.url, None).await.unwrap();
        let mut lane = Lane::open_verified(
            connector,
            TransportEndpoint(relay.url.to_string()),
            Arc::new(Keys::generate()),
            authority(&route.server_reapers),
            INBOX,
        )
        .await
        .unwrap();
        assert_eq!(route.server_reapers.custody(), (1, 0, false));

        {
            let mut activation = std::pin::pin!(lane.activate_inbox());
            std::future::poll_fn(|cx| {
                assert!(activation.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            trip.trip();
            assert!(matches!(
                activation.await,
                Err(Error::Transport("dispatcher_fatal"))
            ));
        }
        drop(lane);

        let finish = route.finish();
        tokio::pin!(finish);
        assert!(futures_util::poll!(&mut finish).is_pending());
        drop(write);
        assert!(
            timeout(Duration::from_millis(50), &mut finish)
                .await
                .is_err()
        );
        assert_eq!(route.server_reapers.custody().0, 1);
        drop(read);
        finish.await.unwrap();
        assert_eq!(route.server_reapers.custody(), (0, 0, false));
        assert_eq!(route.client_reapers.custody(), (0, 0, false));
        relay.assert_healthy();
    })
    .await
    .expect("bounded retained-half proof");
}

#[tokio::test]
async fn terminal_fatal_route_cannot_open_native_room() {
    let relay = Relay::start().await;
    let root = tempfile::tempdir().unwrap();
    let (_, route) = guarded_route(&relay, root.path());
    let own = Arc::new(Keys::generate());
    let result = Room::open(
        route.connector(relay.url.as_str()).unwrap(),
        TransportEndpoint(relay.url.to_string()),
        own.clone(),
        own.public_key(),
        Keys::generate().public_key(),
        authority(&route.client_reapers),
        true,
    )
    .await;
    assert!(matches!(
        result,
        Err(error) if error.reaped
            && matches!(error.error, Error::Storage("terminal_guarded_room_unavailable"))
    ));
    assert_eq!(route.client_reapers.custody(), (0, 0, false));
    assert_eq!(relay.recipient_requests(own.public_key()), 0);
}

#[tokio::test]
async fn terminal_fatal_cancelled_close_keeps_the_counted_pump_join() {
    timeout(STEP, async {
        let relay = Relay::start().await;
        let root = tempfile::tempdir().unwrap();
        let (trip, route) = guarded_route(&relay, root.path());
        let mut lane = open_lane(
            &route,
            &relay,
            &route.client_reapers,
            Arc::new(Keys::generate()),
        )
        .await;
        let (started, entered) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let pump = tokio::task::spawn_blocking(move || {
            started.send(()).unwrap();
            assert_eq!(wait.recv_timeout(STEP), Ok(()));
        });
        lane.hold_fixture_pump(pump);
        entered.await.unwrap();
        trip.trip();
        {
            let close = lane.close();
            tokio::pin!(close);
            assert!(futures_util::poll!(&mut close).is_pending());
        }
        let finish = route.finish();
        tokio::pin!(finish);
        assert!(
            timeout(Duration::from_millis(50), &mut finish)
                .await
                .is_err()
        );
        assert_eq!(route.client_reapers.custody(), (1, 0, false));
        release.send(()).unwrap();
        finish.await.unwrap();
        assert_eq!(route.client_reapers.custody(), (0, 0, false));
    })
    .await
    .expect("bounded pump custody after close cancellation");
}
