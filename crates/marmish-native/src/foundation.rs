//! Strict, deliberately non-executable phase-one application wire format.
//! Decoding does not authenticate a peer; only the MDK adapter can do that.
use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROTOCOL: &str = "marmish-foundation/2";
pub const KIND: u64 = 30100;
pub const MAX_CONTENT: usize = 32768;
pub const MAX_DATA: usize = 16384;

#[derive(Clone, PartialEq, Eq)]
pub enum Body {
    Message(Vec<u8>),
    Receipt {
        message_id: String,
        payload_sha256: String,
    },
}

/// Untrusted until the adapter verifies MLS author, purpose and conversation.
#[derive(Clone, PartialEq, Eq)]
pub struct Envelope {
    session: String,
    id: String,
    sender: String,
    recipient: String,
    body: Body,
}

// Declaration order is lexicographic, including nested body keys. Typed structs
// reject duplicate fields (including duplicates hidden inside nested objects).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire<T> {
    body: T,
    id: String,
    protocol: String,
    recipient: String,
    sender: String,
    session: String,
    #[serde(rename = "type")]
    kind: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DataBody {
    data: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptBody {
    message_id: String,
    payload_sha256: String,
}

pub(crate) fn valid_hex(value: &str, width: usize) -> bool {
    value.len() == width
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn payload_digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Fixed replay window; payload retention is independent.
pub(crate) const RETRY_HORIZON: u64 = 7 * 24 * 60 * 60;
pub(crate) const FUTURE_ALLOWANCE: u64 = 5 * 60;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Freshness {
    Eligible,
    Expired,
    Deferred,
    ExcessiveFuture,
}
pub(crate) fn id_timestamp(id: &str) -> Result<u64> {
    if !valid_hex(id, 32) {
        return Err(Error::Invalid("foundation_id"));
    }
    u64::from_str_radix(&id[..12], 16).map_err(|_| Error::Invalid("foundation_id"))
}
pub(crate) fn fresh_id(now: u64) -> Result<String> {
    if now >= 1u64 << 48 {
        return Err(Error::Invalid("clock"));
    }
    let mut nonce = [0u8; 10];
    getrandom::fill(&mut nonce).map_err(|_| Error::Identity("random"))?;
    Ok(format!("{now:012x}{}", hex::encode(nonce)))
}
pub(crate) fn classify_new_id(id: &str, now: u64, floor: u64) -> Result<Freshness> {
    let timestamp = id_timestamp(id)?;
    Ok(if timestamp < floor {
        Freshness::Expired
    } else if timestamp > now.max(floor).saturating_add(RETRY_HORIZON) {
        Freshness::ExcessiveFuture
    } else if timestamp > now.saturating_add(FUTURE_ALLOWANCE) {
        Freshness::Deferred
    } else {
        Freshness::Eligible
    })
}
/// Stable across MLS re-encryption and receipt retries. Length-prefix every
/// field so different bindings cannot share an ambiguous concatenation.
pub(crate) fn receipt_id(group: &[u8], original: &Envelope) -> Result<String> {
    let Body::Message(data) = original.body() else {
        return Err(Error::Invalid("receipt_loop"));
    };
    receipt_id_for(
        group,
        original.session(),
        original.sender(),
        original.recipient(),
        original.id(),
        &payload_digest(data),
    )
}
pub(crate) fn receipt_id_for(
    group: &[u8],
    session: &str,
    sender: &str,
    recipient: &str,
    id: &str,
    digest: &str,
) -> Result<String> {
    if !valid_hex(session, 32)
        || !valid_hex(sender, 64)
        || !valid_hex(recipient, 64)
        || !valid_hex(digest, 64)
    {
        return Err(Error::Invalid("receipt_binding"));
    }
    let stamp = id_timestamp(id)?;
    let mut hash = Sha256::new();
    hash.update(b"marmish/foundation/2/receipt-id");
    for field in [
        group,
        session.as_bytes(),
        sender.as_bytes(),
        recipient.as_bytes(),
        id.as_bytes(),
        digest.as_bytes(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    Ok(format!(
        "{stamp:012x}{}",
        &hex::encode(hash.finalize())[..20]
    ))
}

impl Envelope {
    pub fn message(
        session: String,
        id: String,
        sender: String,
        recipient: String,
        data: Vec<u8>,
    ) -> Result<Self> {
        Self::new(session, id, sender, recipient, Body::Message(data))
    }
    pub fn receipt(
        session: String,
        id: String,
        sender: String,
        recipient: String,
        message_id: String,
        payload_sha256: String,
    ) -> Result<Self> {
        Self::new(
            session,
            id,
            sender,
            recipient,
            Body::Receipt {
                message_id,
                payload_sha256,
            },
        )
    }
    fn new(
        session: String,
        id: String,
        sender: String,
        recipient: String,
        body: Body,
    ) -> Result<Self> {
        if !valid_hex(&session, 32)
            || !valid_hex(&id, 32)
            || !valid_hex(&sender, 64)
            || !valid_hex(&recipient, 64)
            || sender == recipient
        {
            return Err(Error::Invalid("foundation_binding"));
        }
        match &body {
            Body::Message(bytes) if bytes.len() > MAX_DATA => {
                return Err(Error::Invalid("foundation_payload_limit"));
            }
            Body::Receipt {
                message_id,
                payload_sha256,
            } if !valid_hex(message_id, 32) || !valid_hex(payload_sha256, 64) => {
                return Err(Error::Invalid("foundation_receipt"));
            }
            _ => {}
        }
        Ok(Self {
            session,
            id,
            sender,
            recipient,
            body,
        })
    }
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn sender(&self) -> &str {
        &self.sender
    }
    pub fn recipient(&self) -> &str {
        &self.recipient
    }
    pub fn body(&self) -> &Body {
        &self.body
    }
    fn wire<T>(&self, body: T, kind: &str) -> Wire<T> {
        Wire {
            body,
            id: self.id.clone(),
            protocol: PROTOCOL.into(),
            recipient: self.recipient.clone(),
            sender: self.sender.clone(),
            session: self.session.clone(),
            kind: kind.into(),
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let encoded = match &self.body {
            Body::Message(bytes) => serde_json::to_vec(&self.wire(
                DataBody {
                    data: STANDARD.encode(bytes),
                },
                "message",
            )),
            Body::Receipt {
                message_id,
                payload_sha256,
            } => serde_json::to_vec(&self.wire(
                ReceiptBody {
                    message_id: message_id.clone(),
                    payload_sha256: payload_sha256.clone(),
                },
                "receipt",
            )),
        }
        .map_err(|_| Error::Invalid("foundation_json"))?;
        if encoded.len() > MAX_CONTENT {
            return Err(Error::Invalid("foundation_content_limit"));
        }
        Ok(encoded)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_CONTENT {
            return Err(Error::Invalid("foundation_content_limit"));
        }
        let value = if let Ok(w) = serde_json::from_slice::<Wire<DataBody>>(bytes) {
            if w.protocol != PROTOCOL || w.kind != "message" {
                return Err(Error::Invalid("foundation_protocol"));
            }
            // Bound before base64 allocation; canonical re-encoding rejects
            // nonzero trailing bits, alternate alphabets or missing padding.
            if w.body.data.len() > MAX_DATA.div_ceil(3) * 4 {
                return Err(Error::Invalid("foundation_payload_limit"));
            }
            let data = STANDARD
                .decode(w.body.data)
                .map_err(|_| Error::Invalid("foundation_base64"))?;
            Self::new(w.session, w.id, w.sender, w.recipient, Body::Message(data))?
        } else {
            let w: Wire<ReceiptBody> =
                serde_json::from_slice(bytes).map_err(|_| Error::Invalid("foundation_json"))?;
            if w.protocol != PROTOCOL || w.kind != "receipt" {
                return Err(Error::Invalid("foundation_protocol"));
            }
            Self::new(
                w.session,
                w.id,
                w.sender,
                w.recipient,
                Body::Receipt {
                    message_id: w.body.message_id,
                    payload_sha256: w.body.payload_sha256,
                },
            )?
        };
        if value.encode()? != bytes {
            return Err(Error::Invalid("foundation_noncanonical"));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(data: Vec<u8>) -> Envelope {
        Envelope::message(
            "01".repeat(16),
            "02".repeat(16),
            "03".repeat(32),
            "04".repeat(32),
            data,
        )
        .unwrap()
    }
    #[test]
    fn binary_and_receipt_roundtrip() {
        for bytes in [vec![], (0..=255).collect(), vec![0xff; MAX_DATA]] {
            let msg = message(bytes);
            let encoded = msg.encode().unwrap();
            assert!(Envelope::decode(&encoded).unwrap() == msg);
            assert!(encoded.starts_with(b"{\"body\":{\"data\":"));
        }
        let receipt = Envelope::receipt(
            "01".repeat(16),
            "02".repeat(16),
            "03".repeat(32),
            "04".repeat(32),
            "05".repeat(16),
            payload_digest(b"\0\xff\n"),
        )
        .unwrap();
        assert!(Envelope::decode(&receipt.encode().unwrap()).unwrap() == receipt);
    }
    #[test]
    fn duplicate_unknown_and_noncanonical_are_rejected() {
        let canonical = String::from_utf8(message(vec![1]).encode().unwrap()).unwrap();
        for bad in [
            canonical.replacen("{", "{\"bogus\":0,", 1),
            canonical.replacen(
                "\"data\":\"AQ==\"",
                "\"data\":\"AQ==\",\"data\":\"Ag==\"",
                1,
            ),
            canonical.replacen(
                "\"id\":",
                "\"id\":\"00000000000000000000000000000000\",\"id\":",
                1,
            ),
            canonical.replace("AQ==", "AQ"),
            canonical.replace("AQ==", "AR=="),
            canonical.replace("\"message\"", "\"execute\""),
            format!("{canonical}\n"),
            canonical.replacen(':', ": ", 1),
        ] {
            assert!(
                Envelope::decode(bad.as_bytes()).is_err(),
                "bad vector accepted"
            );
        }
    }
    #[test]
    fn bounded_bindings_and_receipt_shape() {
        assert!(
            Envelope::message(
                "A".repeat(32),
                "02".repeat(16),
                "03".repeat(32),
                "04".repeat(32),
                vec![]
            )
            .is_err()
        );
        assert!(
            Envelope::message(
                "01".repeat(16),
                "02".repeat(16),
                "03".repeat(32),
                "04".repeat(32),
                vec![0; MAX_DATA + 1]
            )
            .is_err()
        );
        let mut encoded = message(vec![]).encode().unwrap();
        encoded.resize(MAX_CONTENT + 1, b' ');
        assert!(Envelope::decode(&encoded).is_err());
        let bad = String::from_utf8(message(vec![]).encode().unwrap())
            .unwrap()
            .replace("\"message\"", "\"receipt\"");
        assert!(Envelope::decode(bad.as_bytes()).is_err());
    }
    #[test]
    fn intrinsic_time_and_receipt_binding() {
        let now = 1_900_000_000;
        let floor = now - RETRY_HORIZON;
        let id = fresh_id(now).unwrap();
        assert_eq!(id.len(), 32);
        assert_eq!(id_timestamp(&id).unwrap(), now);
        assert_eq!(
            classify_new_id(&fresh_id(floor - 1).unwrap(), now, floor).unwrap(),
            Freshness::Expired
        );
        assert_eq!(
            classify_new_id(&fresh_id(floor).unwrap(), now, floor).unwrap(),
            Freshness::Eligible
        );
        assert_eq!(
            classify_new_id(&fresh_id(now + FUTURE_ALLOWANCE + 1).unwrap(), now, floor).unwrap(),
            Freshness::Deferred
        );
        assert_eq!(
            classify_new_id(&fresh_id(now + RETRY_HORIZON + 1).unwrap(), now, floor).unwrap(),
            Freshness::ExcessiveFuture
        );
        assert_eq!(
            classify_new_id(&fresh_id(floor - 1).unwrap(), floor - 100, floor).unwrap(),
            Freshness::Expired
        );
        assert!(id_timestamp(&"A".repeat(32)).is_err());
        let original = Envelope::message(
            fresh_id(now).unwrap(),
            id,
            "03".repeat(32),
            "04".repeat(32),
            vec![0, 255],
        )
        .unwrap();
        let receipt = receipt_id(b"group", &original).unwrap();
        assert_eq!(id_timestamp(&receipt).unwrap(), now);
        assert_eq!(receipt, receipt_id(b"group", &original).unwrap());
        assert_ne!(receipt, receipt_id(b"different", &original).unwrap());
        let different = Envelope::message(
            original.session().into(),
            original.id().into(),
            original.sender().into(),
            original.recipient().into(),
            vec![0, 254],
        )
        .unwrap();
        assert_ne!(receipt, receipt_id(b"group", &different).unwrap());
    }
}
