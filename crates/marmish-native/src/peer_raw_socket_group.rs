//! Fixture-only exact Group turns on the same authenticated Peer socket.

use super::*;
use crate::mdk_adapter::terminal_relay_fixture::{
    GROUP_HISTORY_LIMIT, GROUP_KIND, GroupFingerprint,
};
use cgka_traits::{
    GroupId, MemberId, Timestamp, TransportDelivery, TransportDeliveryPlane,
    TransportDeliverySource, TransportEndpoint, TransportMessage, TransportWireMetadata,
};

#[derive(Default)]
pub(super) struct GroupLane {
    route: Option<[u8; 32]>,
    request: Option<String>,
    records: VecDeque<GroupRecord>,
}

impl GroupLane {
    pub(super) fn matches(&self, request: &str) -> bool {
        self.request.as_deref() == Some(request)
    }

    pub(super) fn push(
        &mut self,
        scope: Arc<Scope>,
        subscription: String,
        body: &RawValue,
    ) -> Result<()> {
        let route = self.route.as_ref().ok_or(Failure::Binding)?;
        if self.records.len() >= GROUP_HISTORY_LIMIT {
            return Err(Failure::Capacity);
        }
        self.records
            .push_back(GroupRecord::parse(scope, subscription, route, body)?);
        Ok(())
    }

    fn bind(&mut self, route: [u8; 32]) -> Result<()> {
        if self.route.is_some_and(|prior| prior != route) {
            return Err(Failure::Binding);
        }
        self.route = Some(route);
        Ok(())
    }
}

pub(in crate::peer_invitation) struct GroupPage {
    scope: Arc<Scope>,
    request: String,
    records: Vec<GroupRecord>,
}

impl GroupPage {
    pub(in crate::peer_invitation) fn request(&self) -> &str {
        &self.request
    }

    pub(in crate::peer_invitation) fn records(&self) -> &[GroupRecord] {
        &self.records
    }
}

pub(in crate::peer_invitation) struct GroupRecord {
    scope: Arc<Scope>,
    subscription: String,
    route: [u8; 32],
    fingerprint: GroupFingerprint,
    message: TransportMessage,
    wire_author: String,
}

impl GroupRecord {
    fn parse(scope: Arc<Scope>, sub: String, route: &[u8; 32], body: &RawValue) -> Result<Self> {
        if body.get().len() > RECORD_LIMIT_BYTES {
            return Err(Failure::Size);
        }
        let event: Event = serde_json::from_str(body.get()).map_err(|_| Failure::Grammar)?;
        let (fingerprint, message) =
            GroupFingerprint::from_signed(&event, route).ok_or(Failure::Binding)?;
        Ok(Self {
            scope,
            subscription: sub,
            route: *route,
            fingerprint,
            message,
            wire_author: event.pubkey.to_hex(),
        })
    }

    pub(in crate::peer_invitation) fn fingerprint(&self) -> GroupFingerprint {
        self.fingerprint
    }

    pub(in crate::peer_invitation) fn delivery(
        &self,
        group: &GroupId,
        wall: u64,
    ) -> TransportDelivery {
        TransportDelivery {
            account_id: MemberId::new(self.scope.recipient.to_vec()),
            group_id_hint: Some(group.clone()),
            received_at: Timestamp(wall),
            source: TransportDeliverySource {
                transport: self.message.source.clone(),
                plane: TransportDeliveryPlane::Group,
                endpoint: Some(TransportEndpoint(self.scope.origin.clone())),
                subscription_id: Some(self.subscription.clone()),
                wire: Some(TransportWireMetadata {
                    wire_id: Some(hex::encode(self.fingerprint.id)),
                    wire_kind: Some(u64::from(GROUP_KIND)),
                    wire_pubkey_hex: Some(self.wire_author.clone()),
                    transport_group_id: Some(hex::encode(self.route)),
                    gift_wrap_event_id: None,
                }),
            },
            message: self.message.clone(),
        }
    }
}

const RECORD_LIMIT_BYTES: usize = 65_536;

impl Owner {
    /// Install one exact route subscription and read a single bounded history cut.
    pub(in crate::peer_invitation) async fn group_history(
        &mut self,
        route: [u8; 32],
        deadline: Instant,
    ) -> Result<GroupPage> {
        self.group_history_after_install(route, deadline, |_| Ok(()))
            .await
    }

    pub(in crate::peer_invitation) async fn group_history_after_install(
        &mut self,
        route: [u8; 32],
        deadline: Instant,
        after_install: impl FnOnce(&str) -> Result<()>,
    ) -> Result<GroupPage> {
        self.socket()?;
        check_deadline(deadline)?;
        match self
            .active(async |owner| owner.scan_group(route, deadline, after_install).await)
            .await
        {
            Ok(page) => Ok(page),
            Err(error) => self.fail(error),
        }
    }

    async fn scan_group(
        &mut self,
        route: [u8; 32],
        deadline: Instant,
        after_install: impl FnOnce(&str) -> Result<()>,
    ) -> Result<GroupPage> {
        if self.groups.request.is_some() {
            return Err(Failure::Binding);
        }
        self.groups.bind(route)?;
        let request = format!("mp/{}/g", hex::encode(self.scope.namespace));
        let mut budget = Budget::default();
        self.predrain(&mut budget, deadline).await?;
        self.socket()?
            .send(
                json!(["REQ", request, {"kinds":[GROUP_KIND], "#h":[hex::encode(route)]}])
                    .to_string(),
                deadline,
            )
            .await?;
        after_install(&request)?;

        let mut records: Vec<GroupRecord> = Vec::with_capacity(GROUP_HISTORY_LIMIT);
        loop {
            let text = self.socket()?.text(&mut budget, deadline).await?;
            match wire::packet(&text)? {
                Packet::Event(subscription, body) if subscription == request => {
                    let record =
                        GroupRecord::parse(self.scope.clone(), subscription, &route, body)?;
                    let incoming = record.fingerprint();
                    if records
                        .last()
                        .is_some_and(|prior| prior.fingerprint().order() > incoming.order())
                    {
                        return Err(Failure::Grammar);
                    }
                    if let Some(prior) = records
                        .iter()
                        .find(|item| item.fingerprint().id == incoming.id)
                    {
                        if prior.fingerprint() != incoming {
                            return Err(Failure::Grammar);
                        }
                        continue;
                    }
                    if records.len() == GROUP_HISTORY_LIMIT {
                        return Err(Failure::Capacity);
                    }
                    records.push(record);
                }
                Packet::End(subscription) if subscription == request => break,
                packet => self.push_live(packet)?,
            }
        }
        self.groups.request = Some(request.clone());
        Ok(GroupPage {
            scope: self.scope.clone(),
            request,
            records,
        })
    }

    pub(in crate::peer_invitation) fn accepts_group_page(&self, page: &GroupPage) -> bool {
        self.current(&page.scope) && self.groups.matches(&page.request)
    }

    pub(in crate::peer_invitation) fn accepts_group_record(&self, record: &GroupRecord) -> bool {
        self.current(&record.scope)
            && self.groups.matches(&record.subscription)
            && self.groups.route == Some(record.route)
    }

    pub(in crate::peer_invitation) async fn next_group_record(
        &mut self,
        deadline: Instant,
    ) -> Result<GroupRecord> {
        self.socket()?;
        check_deadline(deadline)?;
        match self
            .active(async |owner| owner.read_group(deadline).await)
            .await
        {
            Ok(record) => Ok(record),
            Err(error) => self.fail(error),
        }
    }

    async fn read_group(&mut self, deadline: Instant) -> Result<GroupRecord> {
        if self.groups.request.is_none() {
            return Err(Failure::Binding);
        }
        let mut budget = Budget::default();
        loop {
            if let Some(record) = self.groups.records.pop_front() {
                return Ok(record);
            }
            let text = self.socket()?.text(&mut budget, deadline).await?;
            self.push_live(wire::packet(&text)?)?;
        }
    }

    #[cfg(test)]
    async fn prove_group_filter_refusals(&mut self, route: [u8; 32], deadline: Instant) {
        let exact = json!({"kinds": [GROUP_KIND], "#h": [hex::encode(route)]});
        for (index, filters) in [
            vec![exact.clone(), exact.clone()],
            vec![json!({"kinds": [GROUP_KIND, 30104], "#h": [hex::encode(route)]})],
        ]
        .into_iter()
        .enumerate()
        {
            let id = format!("mp-fixture-rejected-{index}");
            let mut frame = vec![json!("REQ"), json!(id)];
            frame.extend(filters);
            let socket = self.socket().unwrap();
            socket
                .send(json!(frame).to_string(), deadline)
                .await
                .unwrap();
            let response = socket.text(&mut Budget::default(), deadline).await.unwrap();
            let (verb, actual, _): (String, String, String) =
                serde_json::from_str(&response).unwrap();
            assert!(
                verb == "CLOSED" && actual == id,
                "unexpected private relay filter response"
            );
        }
    }

    #[cfg(test)]
    async fn prove_group_write_refusals(&mut self, route: [u8; 32], deadline: Instant) {
        let mut foreign = route;
        foreign[0] ^= 1;
        for (kind, tag) in [(GROUP_KIND, foreign), (30104, route)] {
            let event = EventBuilder::new(Kind::Custom(kind), "fixture-scope-probe")
                .tags([Tag::parse(["h", &hex::encode(tag)]).unwrap()])
                .finalize(&Keys::generate())
                .unwrap();
            assert!(
                !self
                    .publish_flight(
                        json!(["EVENT", event]).to_string(),
                        event.id.to_bytes(),
                        &mut Budget::default(),
                        deadline,
                    )
                    .await
                    .unwrap(),
                "private relay accepted out-of-scope event"
            );
        }
    }

    pub(in crate::peer_invitation) async fn push_group_exact(
        &mut self,
        route: [u8; 32],
        bytes: &[u8],
        id: [u8; 32],
        deadline: Instant,
    ) -> Result<Acknowledgement> {
        self.ready_to_publish(deadline)?;
        let outcome = self
            .active(async |owner| {
                owner.groups.bind(route)?;
                let frame = group_publication(route, bytes, id)?;
                let mut budget = Budget::default();
                owner.predrain(&mut budget, deadline).await?;
                check_deadline(deadline)?;
                owner
                    .publish_flight(frame, id, &mut budget, deadline)
                    .await
                    .map_err(|_| Failure::Ambiguous)
            })
            .await;
        match outcome {
            Ok(positive) => Ok(Acknowledgement {
                scope: self.scope.clone(),
                carrier: id,
                positive,
            }),
            Err(error) => self.fail(error),
        }
    }
}

fn group_publication(route: [u8; 32], bytes: &[u8], id: [u8; 32]) -> Result<String> {
    if bytes.len() > RECORD_LIMIT_BYTES {
        return Err(Failure::Size);
    }
    let raw: &RawValue = serde_json::from_slice(bytes).map_err(|_| Failure::Grammar)?;
    let event: Event = serde_json::from_str(raw.get()).map_err(|_| Failure::Grammar)?;
    let (fingerprint, _) = GroupFingerprint::from_signed(&event, &route).ok_or(Failure::Binding)?;
    if raw.get().as_bytes() != bytes || fingerprint.id != id {
        return Err(Failure::Binding);
    }
    let frame = format!(r#"["EVENT",{}]"#, raw.get());
    if frame.len() > wire::MESSAGE_LIMIT {
        return Err(Failure::Size);
    }
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdk_adapter::terminal_relay_fixture::{GroupFault, Relay};
    use std::{cell::Cell, time::Duration};

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }

    async fn connect(relay: &Relay, keys: &Keys, private: &std::path::Path) -> Owner {
        Owner::open(
            &relay.connector(private, true, None),
            relay.url.as_str(),
            keys.public_key(),
            keys,
            deadline(),
        )
        .await
        .unwrap()
    }

    fn group_event(route: [u8; 32], payload: &str) -> ([u8; 32], Vec<u8>) {
        use transport_nostr_peeler::NostrTransportEvent;

        let event = EventBuilder::new(Kind::Custom(GROUP_KIND), payload)
            .tags([Tag::parse(["h", &hex::encode(route)]).unwrap()])
            .finalize(&Keys::generate())
            .unwrap();
        let bytes = NostrTransportEvent::from_nostr_event(&event)
            .unwrap()
            .to_transport_message()
            .unwrap()
            .payload;
        (event.id.to_bytes(), bytes)
    }

    #[test]
    fn group_publication_checks_exact_bound_signed_event() {
        let route = [3; 32];
        let (id, bytes) = group_event(route, "local signed fixture");
        assert!(group_publication(route, &bytes, id).is_ok());
        assert!(matches!(
            group_publication([2; 32], &bytes, id),
            Err(Failure::Binding)
        ));
        assert!(matches!(
            group_publication(route, &bytes, [0; 32]),
            Err(Failure::Binding)
        ));
        assert!(matches!(
            group_publication(route, &[b'x'; RECORD_LIMIT_BYTES + 1], id),
            Err(Failure::Size)
        ));
    }

    #[tokio::test]
    #[ignore = "opt-in private authenticated WSS heartbeat relay scope"]
    async fn private_wss_heartbeat_relay_refuses_unregistered_and_out_of_scope_messages() {
        let route = [5; 32];
        let founder = Keys::generate();
        let recipient = Keys::generate();
        let outsider = Keys::generate();
        let mut relay =
            Relay::start_peer_group([founder.public_key(), recipient.public_key()], route).await;
        let private = tempfile::tempdir().unwrap();
        let connector = relay.connector(private.path(), true, None);
        assert!(matches!(
            Owner::open(
                &connector,
                relay.url.as_str(),
                outsider.public_key(),
                &outsider,
                deadline(),
            )
            .await,
            Err(Failure::Grammar)
        ));
        assert!(!relay.authenticated(outsider.public_key()));
        let mut registered = connect(&relay, &founder, private.path()).await;
        registered
            .prove_group_filter_refusals(route, deadline())
            .await;
        registered
            .prove_group_write_refusals(route, deadline())
            .await;
        assert!(relay.group_writes().is_empty());
        assert!(relay.peer_transcript().is_empty());
        assert_eq!(relay.publication_attempts(GROUP_KIND), 1);
        assert_eq!(relay.publication_attempts(30104), 1);
        registered.stop(deadline()).await.unwrap();
        relay.stop().await;
    }

    #[tokio::test]
    #[ignore = "opt-in private authenticated WSS Group turn and fault proof"]
    async fn exact_group_history_and_retry_remain_socket_scoped() {
        let route = [4; 32];
        let founder = Keys::generate();
        let recipient = Keys::generate();
        let mut relay =
            Relay::start_peer_group([founder.public_key(), recipient.public_key()], route).await;
        let private = tempfile::tempdir().unwrap();
        let mut writer = connect(&relay, &founder, private.path()).await;
        let mut reader = connect(&relay, &recipient, private.path()).await;

        let (first, first_bytes) = group_event(route, "fixture-time history evidence");
        let ack = writer
            .push_group_exact(route, &first_bytes, first, deadline())
            .await
            .unwrap();
        assert!(writer.accepted(&ack, first));
        assert!(!reader.accepted(&ack, first));

        let start = Cell::new(false);
        let page: super::super::GroupPage = reader
            .group_history_after_install(route, deadline(), |_| {
                start.set(true);
                Ok(())
            })
            .await
            .unwrap();
        assert!(start.get());
        assert!(reader.accepts_group_page(&page));
        assert!(!writer.accepts_group_page(&page));
        let [record]: &[super::super::GroupRecord] = page.records() else {
            panic!("one committed retained fixture event");
        };
        let cut = relay
            .group_history_cut(recipient.public_key(), page.request())
            .unwrap();
        assert_eq!(cut.records, [record.fingerprint()]);
        assert!(reader.accepts_group_record(record));
        assert_eq!(
            record
                .delivery(&GroupId::new(vec![1]), 1)
                .message
                .id
                .as_slice(),
            first
        );

        let (second, second_bytes) = group_event(route, "fixture-time live evidence");
        relay.group_fault(GroupFault::LoseNextAck);
        let withheld = writer.push_group_exact(
            route,
            &second_bytes,
            second,
            Instant::now() + Duration::from_millis(500),
        );
        let (ack, delivered) = tokio::join!(withheld, reader.next_group_record(deadline()));
        assert!(matches!(ack, Err(Failure::Ambiguous)));
        assert!(!writer.accepted(
            &Acknowledgement {
                scope: writer.scope.clone(),
                carrier: second,
                positive: true
            },
            second,
        ));
        let delivered = delivered.unwrap();
        assert_eq!(delivered.fingerprint().id, second);
        assert!(reader.accepts_group_record(&delivered));
        let mut retry = connect(&relay, &founder, private.path()).await;
        let ack = retry
            .push_group_exact(route, &second_bytes, second, deadline())
            .await
            .unwrap();
        assert!(retry.accepted(&ack, second));
        let writes = relay.group_writes();
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[1].record, writes[2].record);
        assert!(!writes[1].duplicate && !writes[1].acknowledged);
        assert!(writes[2].duplicate && writes[2].acknowledged);
        let own = retry.group_history(route, deadline()).await.unwrap();
        assert!(own.records().is_empty(), "own echo disabled on reconnect");

        let mut omitted = connect(&relay, &recipient, private.path()).await;
        relay.group_fault(GroupFault::Omit(first));
        let page = omitted.group_history(route, deadline()).await.unwrap();
        let cut = relay
            .group_history_cut(recipient.public_key(), page.request())
            .unwrap();
        assert_eq!(cut.records.len(), 2);
        assert_eq!(page.records().len(), 1);
        assert_eq!(page.records()[0].fingerprint(), delivered.fingerprint());

        reader
            .socket()
            .unwrap()
            .send(json!(["AUTH", {}]).to_string(), deadline())
            .await
            .unwrap();
        assert!(matches!(
            reader.next_group_record(deadline()).await,
            Err(Failure::Transport)
        ));
        assert!(!reader.accepts_group_record(&delivered));

        let mut no_eose = connect(&relay, &recipient, private.path()).await;
        relay.group_fault(GroupFault::LoseEose);
        assert!(matches!(
            no_eose
                .group_history(route, Instant::now() + Duration::from_millis(500))
                .await,
            Err(Failure::Deadline)
        ));
        let mut broken = connect(&relay, &recipient, private.path()).await;
        relay.group_fault(GroupFault::Disconnect);
        assert!(matches!(
            broken.group_history(route, deadline()).await,
            Err(Failure::Transport)
        ));
        relay.stop().await;
    }
}
