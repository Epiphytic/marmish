//! Exact two-account NIP-42 scope for the native Peer enrollment fixture.
use super::*;

use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

const CARRIER: u16 = 7435;
const HISTORY_LIMIT: usize = 4;
const OUTER_BYTES: usize = 64 * 1024;
pub(super) const MESSAGE_LIMIT: usize = OUTER_BYTES + 256;

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Write {
    pub(crate) sender: PublicKey,
    pub(crate) recipient: PublicKey,
    pub(crate) event: EventId,
    pub(crate) bytes_sha256: [u8; 32],
}

pub(super) struct Scope {
    accounts: [PublicKey; 2],
    pub(super) writes: Vec<Write>,
}

impl Scope {
    pub(super) fn new(accounts: [PublicKey; 2]) -> Self {
        assert!(accounts[0] != accounts[1], "independent Peer accounts");
        Self {
            accounts,
            writes: Vec::new(),
        }
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
        if !self.includes(account)
            || filter.get("kinds") != Some(&json!([CARRIER]))
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
