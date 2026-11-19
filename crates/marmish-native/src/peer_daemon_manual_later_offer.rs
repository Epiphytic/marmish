//! A normal later signed Offer against already retained Pending on cold recovery.

use super::{founder_fixture::Offered, *};
use crate::ipc::peer_approve::{self, Failure};
use nostr::event::EventId;

pub(super) struct FollowUp<'a> {
    pub(super) root: &'a Path,
    pub(super) socket: &'a Path,
    pub(super) example: &'a Path,
    pub(super) daemon: &'a Path,
    pub(super) args: &'a [OsString],
    pub(super) signer: PublicKey,
    pub(super) peer: &'a Relay,
    pub(super) ordinary: &'a Relay,
    pub(super) sender_socket: &'a Path,
}

impl FollowUp<'_> {
    fn read_pending(
        &self,
        exact: &peer_read::Entry,
        originals: [EventId; 2],
        later: Option<EventId>,
    ) -> near_cap_fixture::ApprovalPending {
        let mut lease = ProfileLease::acquire_ready(self.root, PROFILE).unwrap();
        let identity = material(&mut lease, self.daemon);
        assert!(identity.public_key() == self.signer);
        near_cap_fixture::snapshot_approval_pending(
            &lease,
            &identity,
            self.peer.url.as_str(),
            exact,
            originals,
            later,
        )
        .expect("exact closed installed ApprovalV3 Pending and original Offer")
    }

    pub(super) async fn run(
        &self,
        first: &mut OwnedChild,
        founder: &mut FounderFixture<'_>,
        offered: &Offered,
        delivered: &Delivered,
        exact: peer_read::Entry,
        argv: Vec<Vec<u8>>,
    ) {
        let wrong = peer_approve::Selection {
            generation: exact.selection.generation.checked_add(1).unwrap(),
            ..exact.selection
        };
        assert_eq!(
            peer_approve::client_request(self.socket, wrong).await,
            Err(Failure::Stale),
        );
        first.stop().await;
        assert!(!self.socket.exists());
        let originals = [offered.carrier(), delivered.carrier];
        let before = self.read_pending(&exact, originals, None);

        let later = founder.publish_offer(Instant::now() + STEP).await;
        assert!(later.selector().server == offered.selector().server);
        assert_ne!(later.selector().id, offered.selector().id);
        let flight = later.flight();
        assert!(flight.author == offered.flight().author && flight.recipient == self.signer);
        assert!(flight.created_at > offered.flight().created_at);
        assert!(flight.created_at > delivered.created_at);
        assert!(!originals.contains(&later.carrier()));
        assert_eq!(self.peer.accepted_manual_control_count(later.carrier()), 1);
        assert_eq!(self.peer.all_publication_attempts(), 4);

        let expected_connection = self.peer.qualification_connections() + 1;
        self.peer
            .watch_recipient_history(expected_connection, self.signer, later.carrier());
        let log = tempfile::tempfile_in(self.root).unwrap();
        let (mut child, mut report, _) =
            provisional_with_log(self.example, self.args, log.try_clone().unwrap()).await;
        let task = recipient_task(child.0.id()).await;
        assert_eq!(
            self.peer.qualification_connections(),
            expected_connection - 1
        );
        acknowledge(&mut report).await;
        drop(report);
        wait_active(&mut child, self.socket).await;

        eventually(
            "later Offer and EOSE written on cold authenticated history",
            || self.peer.watched_recipient_history_written(),
        )
        .await;
        eventually("Manual recovery reports a cursor refusal", || {
            String::from_utf8_lossy(&diagnostic_log(&log))
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .any(|event| {
                    event["event"] == "diagnostic"
                        && event["stage"] == "manual_history"
                        && event["category"] == "cursor"
                })
        })
        .await;
        eventually("only the exact Manual worker and Peer route retire", || {
            self.peer.runtime_socket_end(expected_connection).is_some()
                && !task.try_exists().unwrap()
        })
        .await;
        expect_no_pending_proof(self.socket, exact.selection).await;
        assert!(
            matches!(
                peer_approve::client_request(self.socket, exact.selection).await,
                Err(Failure::Unavailable | Failure::Uncertain)
            ),
            "the stopped Peer worker cannot acknowledge the original approval",
        );

        ordinary_command_bytes(
            self.sender_socket,
            self.signer,
            self.ordinary,
            argv,
            SECOND,
            Some(2),
        )
        .await;
        assert!(active(self.socket).await, "ordinary daemon remains active");
        assert_eq!(self.peer.qualification_connections(), expected_connection);
        assert_eq!(self.peer.all_publication_attempts(), 4);
        assert_eq!(self.ordinary.publication_attempts(7435), 0);

        quiet_command_relay(self.ordinary).await;
        child.stop().await;
        assert!(!self.socket.exists());
        assert!(
            self.read_pending(&exact, originals, Some(later.carrier())) == before,
            "same Pending keeper and no accepted or deferred later Offer",
        );
    }
}
