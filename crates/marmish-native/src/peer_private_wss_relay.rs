//! Exact two-account NIP-42 scope for the native Peer enrollment fixture.
use super::*;

use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

const CARRIER: u16 = 7435;
const HISTORY_LIMIT: usize = 4;
pub(crate) const GROUP_KIND: u16 = 445;
pub(crate) const GROUP_HISTORY_LIMIT: usize = 4;
const OUTER_BYTES: usize = 64 * 1024;
pub(super) const MESSAGE_LIMIT: usize = OUTER_BYTES + 256;

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Write {
    pub(crate) sender: PublicKey,
    pub(crate) recipient: PublicKey,
    pub(crate) event: EventId,
    pub(crate) bytes_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GroupFingerprint {
    pub(crate) id: [u8; 32],
    pub(crate) bytes_sha256: [u8; 32],
    pub(crate) created_at: u64,
}

impl GroupFingerprint {
    pub(crate) fn from_signed(
        event: &Event,
        route: &[u8; 32],
    ) -> Option<(Self, cgka_traits::TransportMessage)> {
        use cgka_traits::transport::TransportEnvelope;
        use transport_nostr_peeler::NostrTransportEvent;

        let wire = NostrTransportEvent::from_nostr_event(event).ok()?;
        wire.to_verified_nostr_event().ok()?;
        let message = wire.to_transport_message().ok()?;
        let TransportEnvelope::GroupMessage { transport_group_id } = &message.envelope else {
            return None;
        };
        if transport_group_id.as_slice() != route {
            return None;
        }
        Some((
            Self {
                id: event.id.to_bytes(),
                bytes_sha256: Sha256::digest(&message.payload).into(),
                created_at: event.created_at.as_secs(),
            },
            message,
        ))
    }

    pub(crate) fn order(&self) -> (std::cmp::Reverse<u64>, [u8; 32]) {
        (std::cmp::Reverse(self.created_at), self.id)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GroupHistoryCut {
    pub(crate) records: Vec<GroupFingerprint>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GroupWrite {
    pub(crate) sender: PublicKey,
    pub(crate) record: GroupFingerprint,
    pub(crate) duplicate: bool,
    pub(crate) acknowledged: bool,
}

pub(crate) enum GroupFault {
    Omit([u8; 32]),
    LoseEose,
    Disconnect,
    LoseNextAck,
}

struct GroupEvent {
    sender: PublicKey,
    record: GroupFingerprint,
    raw: Box<str>,
}

pub(super) struct GroupScope {
    pub(super) route: [u8; 32],
    stored: BTreeMap<[u8; 32], GroupEvent>,
    pub(super) writes: Vec<GroupWrite>,
    pub(super) cuts: BTreeMap<(PublicKey, String), GroupHistoryCut>,
    pub(super) fault: Option<GroupFault>,
}

impl GroupScope {
    fn new(route: [u8; 32]) -> Self {
        Self {
            route,
            stored: BTreeMap::new(),
            writes: Vec::new(),
            cuts: BTreeMap::new(),
            fault: None,
        }
    }

    fn filter(&self, filter: &serde_json::Map<String, Value>) -> bool {
        filter.len() == 2
            && filter.get("kinds") == Some(&json!([GROUP_KIND]))
            && filter.get("#h") == Some(&json!([hex::encode(self.route)]))
    }

    /// Return (duplicate, acknowledge) for the one bounded exact Group write.
    pub(super) fn accept(
        &mut self,
        sender: PublicKey,
        event: &Event,
        raw: &str,
    ) -> Option<(bool, bool)> {
        let ("EVENT", body) = serde_json::from_str::<(&str, &RawValue)>(raw).ok()? else {
            return None;
        };
        if body.get().len() > OUTER_BYTES || self.writes.len() == MAX_PUBLICATIONS {
            return None;
        }
        let (record, _) = GroupFingerprint::from_signed(event, &self.route)?;
        let duplicate = if let Some(existing) = self.stored.get(&record.id) {
            if existing.sender != sender || existing.raw.as_ref() != body.get() {
                return None;
            }
            true
        } else {
            if self.stored.len() == MAX_PUBLICATIONS {
                return None;
            }
            self.stored.insert(
                record.id,
                GroupEvent {
                    sender,
                    record,
                    raw: body.get().into(),
                },
            );
            false
        };
        let acknowledged = !matches!(self.fault, Some(GroupFault::LoseNextAck));
        if !acknowledged {
            self.fault = None;
        }
        self.writes.push(GroupWrite {
            sender,
            record,
            duplicate,
            acknowledged,
        });
        Some((duplicate, acknowledged))
    }

    pub(super) fn history(
        &mut self,
        account: PublicKey,
        request: &str,
    ) -> Option<(Vec<Value>, bool)> {
        if self.cuts.len() >= MAX_OBSERVATIONS || request.len() > 128 {
            return None;
        }
        let mut records: Vec<_> = self
            .stored
            .values()
            .filter(|event| event.sender != account)
            .map(|event| event.record)
            .collect();
        records.sort_by_key(GroupFingerprint::order);
        if self.cuts.contains_key(&(account, request.into())) {
            return None;
        }
        self.cuts.insert(
            (account, request.into()),
            GroupHistoryCut {
                records: records.clone(),
            },
        );
        if records.len() > GROUP_HISTORY_LIMIT {
            return None;
        }
        let fault = match self.fault {
            Some(GroupFault::LoseNextAck) => None,
            _ => self.fault.take(),
        };
        if matches!(fault, Some(GroupFault::Disconnect)) {
            return Some((Vec::new(), true));
        }
        let mut frames: Vec<_> = records
            .into_iter()
            .filter(|record| !matches!(fault, Some(GroupFault::Omit(id)) if record.id == id))
            .map(|record| {
                let body: Value = serde_json::from_str(&self.stored[&record.id].raw)
                    .expect("stored verified Group event");
                json!(["EVENT", request, body])
            })
            .collect();
        if !matches!(fault, Some(GroupFault::LoseEose)) {
            frames.push(json!(["EOSE", request]));
        }
        Some((frames, false))
    }
}

pub(super) struct Scope {
    accounts: [PublicKey; 2],
    pub(super) writes: Vec<Write>,
    pub(super) group: Option<GroupScope>,
}

impl Scope {
    pub(super) fn new(accounts: [PublicKey; 2]) -> Self {
        assert!(accounts[0] != accounts[1], "independent Peer accounts");
        Self {
            accounts,
            writes: Vec::new(),
            group: None,
        }
    }

    pub(super) fn with_group(mut self, route: Option<[u8; 32]>) -> Self {
        self.group = route.map(GroupScope::new);
        self
    }

    pub(super) fn includes(&self, account: PublicKey) -> bool {
        self.accounts.contains(&account)
    }

    pub(super) fn accept(&mut self, sender: PublicKey, event: &Event, raw: &str) -> bool {
        let Some(recipient) = self
            .includes(sender)
            .then(|| self.accounts.into_iter().find(|account| *account != sender))
            .flatten()
        else {
            return false;
        };
        if u16::from(event.kind) != CARRIER
            || self.includes(event.pubkey)
            || !auth_tag(event, "p", &recipient.to_hex())
            || event.tags.len() != 2
            || !event.tags.iter().any(|tag| {
                let parts = tag.as_slice();
                parts.len() == 2 && parts[0] == "expiration" && parts[1].parse::<u64>().is_ok()
            })
        {
            return false;
        }
        let Ok(("EVENT", body)) = serde_json::from_str::<(&str, &RawValue)>(raw) else {
            return false;
        };
        if body.get().len() > OUTER_BYTES {
            return false;
        }
        assert!(
            self.writes.len() < MAX_PUBLICATIONS,
            "Peer relay write bound"
        );
        self.writes.push(Write {
            sender,
            recipient,
            event: event.id,
            bytes_sha256: Sha256::digest(body.get()).into(),
        });
        true
    }

    pub(super) fn read_allowed(&self, account: Option<PublicKey>, filters: &[Value]) -> bool {
        let (Some(account), [filter]) = (account, filters) else {
            return false;
        };
        let Some(filter) = filter.as_object() else {
            return false;
        };
        if !self.includes(account) {
            return false;
        }
        if self
            .group
            .as_ref()
            .is_some_and(|group| group.filter(filter))
        {
            return true;
        }
        if filter.get("kinds") != Some(&json!([CARRIER]))
            || filter.get("#p") != Some(&json!([account]))
        {
            return false;
        }
        match filter.get("limit").and_then(Value::as_u64) {
            Some(0) => filter.len() == 3,
            Some(limit) if limit == HISTORY_LIMIT as u64 && filter.len() == 5 => {
                match (
                    filter.get("since").and_then(Value::as_u64),
                    filter.get("until").and_then(Value::as_u64),
                ) {
                    (Some(lower), Some(upper)) => lower <= upper,
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

pub(super) fn history_limit(filters: &[Value]) -> usize {
    if filters[0]["limit"] == json!(0) {
        0
    } else {
        HISTORY_LIMIT
    }
}

#[test]
fn peer_scope_refuses_unaddressed_oversize_and_native_traffic() {
    use nostr::event::{EventBuilder, FinalizeEvent, Kind, Tag};
    use nostr::key::Keys;

    let sender = Keys::generate();
    let receiver = Keys::generate();
    let outer = Keys::generate();
    let mut scope = Scope::new([sender.public_key(), receiver.public_key()]);
    let request = |kind, who| json!({"kinds":[kind], "#p":[who], "limit":0});
    assert!(scope.read_allowed(
        Some(receiver.public_key()),
        &[request(CARRIER, receiver.public_key())]
    ));
    assert!(!scope.read_allowed(None, &[request(CARRIER, receiver.public_key())]));
    assert!(!scope.read_allowed(
        Some(sender.public_key()),
        &[request(CARRIER, receiver.public_key())]
    ));

    let native_kinds = [1059, 30443, 445];
    for kind in native_kinds {
        assert!(!scope.read_allowed(
            Some(receiver.public_key()),
            &[request(kind, receiver.public_key())]
        ));
    }
    let mut submit = |kind, recipient, signer: &Keys, bytes| {
        let event = EventBuilder::new(Kind::Custom(kind), "x".repeat(bytes))
            .tags([
                Tag::public_key(recipient),
                Tag::parse(["expiration", "21600"]).unwrap(),
            ])
            .finalize(signer)
            .unwrap();
        let frame = json!(["EVENT", event]).to_string();
        scope.accept(sender.public_key(), &event, &frame)
    };
    for kind in native_kinds {
        assert!(!submit(kind, receiver.public_key(), &outer, 1));
    }
    assert!(!submit(CARRIER, sender.public_key(), &outer, 1));
    assert!(!submit(CARRIER, receiver.public_key(), &sender, 1));
    assert!(!submit(CARRIER, receiver.public_key(), &outer, OUTER_BYTES));
    assert!(submit(CARRIER, receiver.public_key(), &outer, 1));
    assert_eq!(scope.writes.len(), 1);
}

#[test]
fn group_scope_requires_an_exact_route_and_registered_principal() {
    let founder = nostr::key::Keys::generate().public_key();
    let recipient = nostr::key::Keys::generate().public_key();
    let outsider = nostr::key::Keys::generate().public_key();
    let route = [13; 32];
    let scope = Scope::new([founder, recipient]).with_group(Some(route));
    let exact = json!({"kinds": [GROUP_KIND], "#h": [hex::encode(route)]});
    for account in [founder, recipient] {
        assert!(scope.read_allowed(Some(account), std::slice::from_ref(&exact)));
    }
    for account in [None, Some(outsider)] {
        assert!(!scope.read_allowed(account, std::slice::from_ref(&exact)));
    }
    for malformed in [
        json!({"kinds": [GROUP_KIND], "#h": [hex::encode([0; 32])]}),
        json!({"kinds": [GROUP_KIND, CARRIER], "#h": [hex::encode(route)]}),
        json!({"kinds": [GROUP_KIND], "#h": [hex::encode(route)], "limit": 1}),
    ] {
        assert!(!scope.read_allowed(Some(founder), &[malformed]));
    }
    assert!(!scope.read_allowed(Some(founder), &[exact.clone(), exact]));
}
