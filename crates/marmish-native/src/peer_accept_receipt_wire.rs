//! Canonical acceptance receipt carried by an authenticated MLS application message.

use cgka_traits::app_event::MarmotAppEvent;
use nostr::key::PublicKey;
use serde::{Deserialize, Serialize};

use crate::peer_control_version::{ControlVersion, ReceiptCoordinates};

const KIND: u64 = 30102;
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

/// A canonical v2 payload with a checked declared author. The authenticated MLS
/// sender, epoch, and group still have to be checked by the caller.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct V2Receipt(Decoded);

impl V2Receipt {
    pub(crate) fn decoded(&self) -> &Decoded {
        &self.0
    }

    pub(crate) fn matches_coordinates(&self, expected: &ReceiptCoordinates) -> bool {
        let receipt = &self.0.receipt;
        receipt.founder == expected.founder
            && receipt.recipient == expected.recipient
            && receipt.room_id == expected.room_id
            && receipt.offer_id == expected.offer_id
            && receipt.offer_revision == expected.offer_revision
            && receipt.welcome_id == expected.welcome_id
    }
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

impl Content {
    fn for_version(receipt: &Receipt, version: ControlVersion) -> Self {
        Self {
            v: version as u8,
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

    fn into_receipt(self, version: ControlVersion) -> Result<Receipt, InvalidReceipt> {
        if self.v != version as u8 {
            return Err(InvalidReceipt);
        }
        Ok(Receipt {
            founder: PublicKey::from_byte_array(parse_hex(&self.founder)?),
            recipient: PublicKey::from_byte_array(parse_hex(&self.recipient)?),
            room_id: parse_hex(&self.room_id)?,
            offer_id: parse_hex(&self.offer_id)?,
            offer_revision: parse_hex(&self.offer_revision)?,
            welcome_id: parse_hex(&self.welcome_id)?,
            epoch: self.epoch,
            epoch_identity: parse_hex(&self.epoch_identity)?,
        })
    }
}

fn parse_hex<const N: usize>(value: &str) -> Result<[u8; N], InvalidReceipt> {
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| InvalidReceipt)?;
    Ok(bytes)
}

fn canonical_content(receipt: &Receipt, version: ControlVersion) -> Result<String, InvalidReceipt> {
    if receipt.founder == receipt.recipient
        || receipt.founder.xonly().is_err()
        || receipt.recipient.xonly().is_err()
    {
        return Err(InvalidReceipt);
    }
    let content = serde_json::to_string(&Content::for_version(receipt, version))
        .map_err(|_| InvalidReceipt)?;
    if content.len() > MAX_CONTENT {
        return Err(InvalidReceipt);
    }
    Ok(content)
}

pub(crate) fn prepare(receipt: &Receipt, created_at: u64) -> Result<Prepared, InvalidReceipt> {
    prepare_version(receipt, created_at, ControlVersion::V1)
}

pub(crate) fn prepare_v2(receipt: &Receipt, created_at: u64) -> Result<Prepared, InvalidReceipt> {
    prepare_version(receipt, created_at, ControlVersion::V2)
}

fn prepare_version(
    receipt: &Receipt,
    created_at: u64,
    version: ControlVersion,
) -> Result<Prepared, InvalidReceipt> {
    let app = MarmotAppEvent::new(
        receipt.recipient.to_hex(),
        created_at,
        KIND,
        Vec::new(),
        canonical_content(receipt, version)?,
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
    decode_version(payload, ControlVersion::V1)
}

pub(crate) fn decode_v2(payload: &[u8]) -> Result<V2Receipt, InvalidReceipt> {
    decode_version(payload, ControlVersion::V2).map(V2Receipt)
}

fn decode_version(payload: &[u8], version: ControlVersion) -> Result<Decoded, InvalidReceipt> {
    if payload.len() > MAX_PAYLOAD {
        return Err(InvalidReceipt);
    }
    let app = MarmotAppEvent::decode(payload).map_err(|_| InvalidReceipt)?;
    if app.kind != KIND || !app.tags.is_empty() || app.content.len() > MAX_CONTENT {
        return Err(InvalidReceipt);
    }
    let content: Content = serde_json::from_str(&app.content).map_err(|_| InvalidReceipt)?;
    let receipt = content.into_receipt(version)?;
    if app.content != canonical_content(&receipt, version)? {
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
