//! Exact native artifacts carried by the Peer Consent and Delivery flights.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use cgka_engine::{DEFAULT_CIPHERSUITE, KeyPackageMetadata, key_package_metadata};
use cgka_traits::app_components::PRIVATE_USE_APP_COMPONENT_ID_START;
use cgka_traits::engine::KeyPackage;
use cgka_traits::error::EngineError;
use cgka_traits::group::ProtocolProfile;
use cgka_traits::{MemberId, MessageId, TransportEndpoint};
use nostr::event::{EventId, Kind};
use nostr::key::PublicKey;
use sha2::{Digest, Sha256};
use transport_nostr_adapter::NostrKeyPackagePublication;
use zeroize::Zeroizing;

use super::seal::{fixed_hex, is_exact_recipient, verify_signed};
use super::{Error, Result};

pub(super) const ARTIFACT_CAP: usize = 20 * 1024;
const ENCODED_ARTIFACT_CAP: usize = ARTIFACT_CAP.div_ceil(3) * 4;
const PACKAGE_KIND: Kind = Kind::Custom(30443);

pub(super) fn decode(encoded: &str, error: Error) -> Result<Zeroizing<Vec<u8>>> {
    if encoded.len() > ENCODED_ARTIFACT_CAP {
        return Err(Error::Size);
    }
    let raw = Zeroizing::new(STANDARD.decode(encoded).map_err(|_| error)?);
    if raw.len() > ARTIFACT_CAP {
        return Err(Error::Size);
    }
    Ok(raw)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct PackageBinding {
    pub(crate) event_id: [u8; 32],
    pub(crate) reference: [u8; 32],
    pub(crate) slot: [u8; 16],
}

pub(super) struct VerifiedPackage {
    pub(super) binding: PackageBinding,
    pub(super) package: KeyPackage,
    pub(super) not_before: u64,
    pub(super) not_after: u64,
}

impl VerifiedPackage {
    pub(super) fn lifetime(&self) -> std::ops::Range<u64> {
        self.not_before..self.not_after
    }
}

pub(super) fn verify_package(
    raw: &[u8],
    author: PublicKey,
    consent_time: u64,
    relays: &[String],
) -> Result<VerifiedPackage> {
    if raw.len() > ARTIFACT_CAP {
        return Err(Error::Size);
    }
    let event = verify_signed(raw, Error::Package)?;
    if event.kind != PACKAGE_KIND
        || event.pubkey != author
        || event.created_at.as_secs() > consent_time
        || serde_json::to_vec(&event).map_err(|_| Error::Package)? != raw
    {
        return Err(Error::Package);
    }
    let slot = match event.tags.iter().next().map(|tag| tag.as_slice()) {
        Some([label, value]) if label == "d" => fixed_hex(value, Error::Package)?,
        _ => return Err(Error::Package),
    };
    let payload = decode(&event.content, Error::Package)?;
    let package = KeyPackage::with_source_event_id(
        payload.to_vec(),
        MessageId::new(event.id.to_bytes().to_vec()),
    )
    .with_protocol_profile(ProtocolProfile::Current);
    let meta = key_package_metadata(&package).map_err(|error| match error {
        EngineError::InvalidKeyPackageLifetime { .. } => Error::NativePackageLifetime,
        _ => Error::Package,
    })?;
    if meta.credential_identity_hex != author.to_hex()
        || meta.protocol_profile != ProtocolProfile::Current
        || meta.ciphersuite != u16::from(DEFAULT_CIPHERSUITE)
        || !meta.last_resort
    {
        return Err(Error::Package);
    }
    let expected = publication(&package, &meta, author, slot, relays)
        .to_event_at(event.created_at.as_secs())
        .map_err(|_| Error::Package)?;
    if event.content != expected.content
        || !event
            .tags
            .iter()
            .map(|tag| tag.as_slice())
            .eq(expected.tags.iter().map(Vec::as_slice))
    {
        return Err(Error::Package);
    }
    Ok(VerifiedPackage {
        binding: PackageBinding {
            event_id: event.id.to_bytes(),
            reference: fixed_hex(&meta.key_package_ref_hex, Error::Package)?,
            slot,
        },
        package,
        not_before: meta.not_before,
        not_after: meta.not_after,
    })
}

pub(super) fn publication(
    package: &KeyPackage,
    meta: &KeyPackageMetadata,
    author: PublicKey,
    slot: [u8; 16],
    relays: &[String],
) -> NostrKeyPackagePublication {
    NostrKeyPackagePublication {
        client_name: None,
        account_id: MemberId::new(author.to_bytes().to_vec()),
        key_package: package.clone(),
        key_package_slot_id: hex::encode(slot),
        key_package_ref: meta.key_package_ref_hex.clone(),
        mls_ciphersuite: format!("0x{:04x}", meta.ciphersuite),
        mls_extensions: meta
            .mls_extensions
            .iter()
            .map(|value| format!("0x{value:04x}"))
            .collect(),
        mls_proposals: meta
            .mls_proposals
            .iter()
            .map(|value| format!("0x{value:04x}"))
            .collect(),
        app_components: meta
            .app_components
            .iter()
            .filter(|value| **value >= PRIVATE_USE_APP_COMPONENT_ID_START)
            .map(|value| format!("0x{value:04x}"))
            .collect(),
        publish_endpoints: relays.iter().cloned().map(TransportEndpoint).collect(),
    }
}

pub(super) fn verify_welcome(raw: &[u8], client: PublicKey) -> Result<(EventId, [u8; 32])> {
    if raw.len() > ARTIFACT_CAP {
        return Err(Error::Size);
    }
    let event = verify_signed(raw, Error::Welcome)?;
    if event.kind != Kind::GiftWrap
        || !is_exact_recipient(&event.tags, client)
        || event.content.is_empty()
    {
        return Err(Error::Welcome);
    }
    Ok((event.id, Sha256::digest(raw).into()))
}
