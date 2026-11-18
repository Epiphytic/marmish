//! Canonical acceptance receipt carried by an authenticated MLS application message.

use cgka_traits::app_event::MarmotAppEvent;
use nostr::key::PublicKey;
use serde::{Deserialize, Serialize};

const KIND: u64 = 30102;
const VERSION: u8 = 1;
const MAX_CONTENT: usize = 1_024;
// The compact envelope adds a fixed header and at most one escape per body byte.
const MAX_PAYLOAD: usize = 2 * MAX_CONTENT + 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Receipt {
    pub(crate) founder: PublicKey,
    pub(crate) recipient: PublicKey,
    pub(crate) room_id: [u8; 16],
    pub(crate) offer_id: [u8; 16],
    pub(crate) offer_revision: [u8; 32],
    pub(crate) welcome_id: [u8; 32],
    pub(crate) epoch: u64,
    pub(crate) epoch_identity: [u8; 32],
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Prepared {
    event_id: [u8; 32],
    payload: Vec<u8>,
}

impl Prepared {
    pub(crate) fn event_id(&self) -> [u8; 32] {
        self.event_id
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Decoded {
    pub(crate) receipt: Receipt,
    pub(crate) event_id: [u8; 32],
    pub(crate) created_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidReceipt;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Content {
    v: u8,
    founder: String,
    recipient: String,
    room_id: String,
    offer_id: String,
    offer_revision: String,
    welcome_id: String,
    epoch: u64,
    epoch_identity: String,
}

impl From<&Receipt> for Content {
    fn from(receipt: &Receipt) -> Self {
        Self {
            v: VERSION,
            founder: receipt.founder.to_hex(),
            recipient: receipt.recipient.to_hex(),
            room_id: hex::encode(receipt.room_id),
            offer_id: hex::encode(receipt.offer_id),
            offer_revision: hex::encode(receipt.offer_revision),
            welcome_id: hex::encode(receipt.welcome_id),
            epoch: receipt.epoch,
            epoch_identity: hex::encode(receipt.epoch_identity),
        }
    }
}

impl TryFrom<Content> for Receipt {
    type Error = InvalidReceipt;

    fn try_from(content: Content) -> Result<Self, InvalidReceipt> {
        if content.v != VERSION {
            return Err(InvalidReceipt);
        }
        Ok(Self {
            founder: PublicKey::from_byte_array(parse_hex(&content.founder)?),
            recipient: PublicKey::from_byte_array(parse_hex(&content.recipient)?),
            room_id: parse_hex(&content.room_id)?,
            offer_id: parse_hex(&content.offer_id)?,
            offer_revision: parse_hex(&content.offer_revision)?,
            welcome_id: parse_hex(&content.welcome_id)?,
            epoch: content.epoch,
            epoch_identity: parse_hex(&content.epoch_identity)?,
        })
    }
}

fn parse_hex<const N: usize>(value: &str) -> Result<[u8; N], InvalidReceipt> {
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| InvalidReceipt)?;
    Ok(bytes)
}

fn canonical_content(receipt: &Receipt) -> Result<String, InvalidReceipt> {
    if receipt.founder == receipt.recipient
        || receipt.founder.xonly().is_err()
        || receipt.recipient.xonly().is_err()
    {
        return Err(InvalidReceipt);
    }
    let content = serde_json::to_string(&Content::from(receipt)).map_err(|_| InvalidReceipt)?;
    if content.len() > MAX_CONTENT {
        return Err(InvalidReceipt);
    }
    Ok(content)
}

pub(crate) fn prepare(receipt: &Receipt, created_at: u64) -> Result<Prepared, InvalidReceipt> {
    let app = MarmotAppEvent::new(
        receipt.recipient.to_hex(),
        created_at,
        KIND,
        Vec::new(),
        canonical_content(receipt)?,
    );
    Ok(Prepared {
        event_id: parse_hex(&app.id)?,
        payload: app.encode().map_err(|_| InvalidReceipt)?,
    })
}

/// Checks the inner format and its declared author. The caller verifies the
/// authenticated MLS sender and compares the returned ID and timestamp at the
/// reservation boundary.
pub(crate) fn decode(payload: &[u8]) -> Result<Decoded, InvalidReceipt> {
    if payload.len() > MAX_PAYLOAD {
        return Err(InvalidReceipt);
    }
    let app = MarmotAppEvent::decode(payload).map_err(|_| InvalidReceipt)?;
    if app.kind != KIND || !app.tags.is_empty() || app.content.len() > MAX_CONTENT {
        return Err(InvalidReceipt);
    }
    let content: Content = serde_json::from_str(&app.content).map_err(|_| InvalidReceipt)?;
    let receipt = Receipt::try_from(content)?;
    if app.content != canonical_content(&receipt)? {
        return Err(InvalidReceipt);
    }
    app.validate_sender(&receipt.recipient.to_hex())
        .map_err(|_| InvalidReceipt)?;
    Ok(Decoded {
        receipt,
        event_id: parse_hex(&app.id)?,
        created_at: app.created_at,
    })
}
