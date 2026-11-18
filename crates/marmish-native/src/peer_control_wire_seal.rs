//! Bounded NIP-44 seal operations. The caller owns the outer kind and tag policy.

use nostr::event::{
    Event, EventBuilder, EventId, FinalizeEvent, Kind, Signature, Tag, Tags, UnsignedEvent,
};
use nostr::key::{Keys, PublicKey};
use nostr::nips::nip44::{self, Version};
use nostr::types::Timestamp;
use serde::Deserialize;
use zeroize::Zeroizing;

use super::{Error, Result};

pub(super) const OUTER_CONTENT_CAP: usize = 63 * 1024;
pub(super) const SEAL_CAP: usize = 49 * 1024;
pub(super) const SEAL_CONTENT_CAP: usize = 48 * 1024;
pub(super) const RUMOR_CAP: usize = 30 * 1024;
pub(super) const BODY_CAP: usize = 28 * 1024;

pub(super) fn fixed_hex<const N: usize>(value: &str, error: Error) -> Result<[u8; N]> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(error);
    }
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| error)?;
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedWire {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u16,
    tags: Vec<Vec<String>>,
    content: String,
    sig: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RumorWire {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u16,
    tags: Vec<Vec<String>>,
    content: String,
}

fn tags(values: Vec<Vec<String>>, error: Error) -> Result<Vec<Tag>> {
    if values.len() > 8 {
        return Err(error);
    }
    values
        .into_iter()
        .map(|tag| {
            if tag.len() > 128 || tag.iter().any(|value| value.len() > 512) {
                return Err(error);
            }
            Tag::parse(tag).map_err(|_| error)
        })
        .collect()
}

pub(super) fn verify_signed(bytes: &[u8], error: Error) -> Result<Event> {
    let wire: SignedWire = serde_json::from_slice(bytes).map_err(|_| error)?;
    let event = Event::new(
        EventId::from_byte_array(fixed_hex(&wire.id, error)?),
        PublicKey::from_byte_array(fixed_hex(&wire.pubkey, error)?),
        Timestamp::from_secs(wire.created_at),
        Kind::from(wire.kind),
        tags(wire.tags, error)?,
        wire.content,
        Signature::from_byte_array(fixed_hex(&wire.sig, error)?),
    );
    event.verify().map_err(|_| error)?;
    Ok(event)
}

pub(super) fn is_exact_recipient(tags: &Tags, recipient: PublicKey) -> bool {
    tags.len() == 1
        && tags
            .first()
            .is_some_and(|tag| is_recipient_tag(tag, recipient))
}

pub(super) fn is_recipient_tag(tag: &Tag, recipient: PublicKey) -> bool {
    matches!(tag.as_slice(), [name, target] if name == "p" && *target == recipient.to_hex())
}

pub(super) fn open_signed_seal(
    carrier: &Event,
    recipient: &Keys,
    expected_author: Option<PublicKey>,
) -> Result<UnsignedEvent> {
    let seal_bytes = Zeroizing::new(
        nip44::decrypt_to_bytes(recipient.secret_key(), &carrier.pubkey, &carrier.content)
            .map_err(|_| Error::Seal)?,
    );
    if seal_bytes.len() > SEAL_CAP {
        return Err(Error::Size);
    }
    let seal = verify_signed(&seal_bytes, Error::Seal)?;
    if seal.kind != Kind::Seal
        || !seal.tags.is_empty()
        || seal.pubkey == recipient.public_key()
        || seal.pubkey == carrier.pubkey
        || expected_author.is_some_and(|author| author != seal.pubkey)
    {
        return Err(Error::Seal);
    }
    if seal.content.len() > SEAL_CONTENT_CAP {
        return Err(Error::Size);
    }
    let rumor_bytes = Zeroizing::new(
        nip44::decrypt_to_bytes(recipient.secret_key(), &seal.pubkey, &seal.content)
            .map_err(|_| Error::Rumor)?,
    );
    if rumor_bytes.len() > RUMOR_CAP {
        return Err(Error::Size);
    }
    let wire: RumorWire = serde_json::from_slice(&rumor_bytes).map_err(|_| Error::Rumor)?;
    if wire.content.len() > BODY_CAP {
        return Err(Error::Size);
    }
    let rumor = UnsignedEvent {
        id: Some(EventId::from_byte_array(fixed_hex(&wire.id, Error::Rumor)?)),
        pubkey: PublicKey::from_byte_array(fixed_hex(&wire.pubkey, Error::Rumor)?),
        created_at: Timestamp::from_secs(wire.created_at),
        kind: Kind::from(wire.kind),
        tags: Tags::from_list(tags(wire.tags, Error::Rumor)?),
        content: wire.content,
    };
    if rumor.pubkey != seal.pubkey || rumor.id != Some(rumor.compute_id()) {
        return Err(Error::Rumor);
    }
    Ok(rumor)
}

pub(super) fn sign_seal(
    signer: &Keys,
    recipient: PublicKey,
    rumor: &UnsignedEvent,
    created_at: u64,
) -> Result<Zeroizing<Vec<u8>>> {
    let raw = Zeroizing::new(serde_json::to_vec(rumor).map_err(|_| Error::Rumor)?);
    if raw.len() > RUMOR_CAP {
        return Err(Error::Size);
    }
    let content = nip44::encrypt(signer.secret_key(), &recipient, &*raw, Version::V2)
        .map_err(|_| Error::Seal)?;
    if content.len() > SEAL_CONTENT_CAP {
        return Err(Error::Size);
    }
    let seal = EventBuilder::new(Kind::Seal, content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .finalize(signer)
        .map_err(|_| Error::Seal)?;
    let bytes = Zeroizing::new(serde_json::to_vec(&seal).map_err(|_| Error::Seal)?);
    if bytes.len() > SEAL_CAP {
        return Err(Error::Size);
    }
    Ok(bytes)
}
