//! Test-only authentication of retained native SelfRemove bytes.

use cgka_engine::provider::EngineOpenMlsProvider;
use cgka_traits::TransportAdapter;
use cgka_traits::app_components::{NOSTR_ROUTING_COMPONENT_ID, decode_nostr_routing_v1};
use cgka_traits::engine::EpochPin;
use cgka_traits::group_context::GroupContextSnapshot;
use cgka_traits::ingest::PeeledContent;
use cgka_traits::message::{MessageState, StoredMessagePayload};
use cgka_traits::peeler::TransportPeeler;
use cgka_traits::storage::{LeaveRequestStorage, MessageStorage, StorageError, StorageProvider};
use cgka_traits::{
    GroupId, GroupLifecycleState, MemberId, TransportEnvelope, TransportPublishRequest,
    TransportPublishTarget,
};
use marmot_session::{
    AccountDeviceRuntime, KeyPackagePublisher, SessionError, StaticTransportRouting,
};
use openmls::prelude::tls_codec::Deserialize as _;
use openmls::prelude::{
    BasicCredential, ContentType, Credential, LeafNodeIndex, MlsGroup, MlsMessageBodyIn,
    MlsMessageIn, OpenMlsProvider, ProcessedMessageContent, Proposal, ProtocolMessage, Sender,
};
use openmls_rust_crypto::RustCrypto;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use storage_sqlite::SqliteAccountStorage;
use transport_nostr_peeler::{DEFAULT_EXPORTER_LABEL, NostrMlsPeeler, NostrTransportEvent};
use zeroize::Zeroizing;

pub(crate) const MAX_STORED_SIGNED_WIRE_BYTES: usize = 262_144;

pub(crate) struct Scope<'a> {
    pub(crate) group: &'a GroupId,
    pub(crate) account: &'a MemberId,
    pub(crate) room_tag: &'a [u8],
    pub(crate) origin: &'a str,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    Invalid,
    Storage,
}

impl From<StorageError> for Refusal {
    fn from(_: StorageError) -> Self {
        Self::Storage
    }
}

impl From<SessionError> for Refusal {
    fn from(_: SessionError) -> Self {
        Self::Storage
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedLeaf(pub(crate) u32);

pub(crate) async fn verify<A: TransportAdapter, K: KeyPackagePublisher>(
    runtime: &AccountDeviceRuntime<A, StaticTransportRouting, K>,
    scope: &Scope<'_>,
    request: &TransportPublishRequest,
) -> Result<VerifiedLeaf, Refusal> {
    let TransportPublishTarget::Group {
        group_id,
        transport_group_id: target_group,
        endpoints,
    } = &request.target
    else {
        return Err(Refusal::Invalid);
    };
    let [endpoint] = endpoints.as_slice() else {
        return Err(Refusal::Invalid);
    };
    let TransportEnvelope::GroupMessage { transport_group_id } = &request.message.envelope else {
        return Err(Refusal::Invalid);
    };
    if &request.account_id != scope.account
        || group_id != scope.group
        || target_group.as_slice() != scope.room_tag
        || endpoint.0 != scope.origin
        || request.required_acks != 1
        || transport_group_id.as_slice() != scope.room_tag
        || !(1..=65_536).contains(&request.message.payload.len())
    {
        return Err(Refusal::Invalid);
    }

    if runtime.self_id() != *scope.account {
        return Err(Refusal::Invalid);
    }
    let signature_key = runtime.own_signature_key();
    let pin = runtime
        .epoch_pin(scope.group)
        .map_err(|_| Refusal::Invalid)?;
    let inner = runtime
        .group_authority_view()
        .with_group_authority_snapshot(scope.group, |storage, authority| {
            retained_inner(storage, authority, scope, pin, request)
        })?;

    let event = NostrTransportEvent::from_transport_message(&request.message)
        .and_then(|event| event.to_verified_nostr_event())
        .map_err(|_| Refusal::Invalid)?;
    if event.id.to_bytes().as_slice() != request.message.id.as_slice() {
        return Err(Refusal::Invalid);
    }
    let (epoch, secret) = runtime
        .exporter_secret_with_epoch(scope.group, DEFAULT_EXPORTER_LABEL, 32)
        .map_err(|_| Refusal::Invalid)?;
    if epoch != pin.epoch {
        return Err(Refusal::Invalid);
    }
    let context = GroupContextSnapshot::new(
        epoch,
        HashMap::from([(DEFAULT_EXPORTER_LABEL.into(), secret.as_slice().to_vec())]),
        Some(scope.room_tag.to_vec()),
    );
    let peeler = NostrMlsPeeler::default();
    let prepared = peeler
        .prepare_group_message(&request.message)
        .map_err(|_| Refusal::Invalid)?;
    let peeled = peeler
        .peel_group_message(&prepared, &context)
        .await
        .map_err(|_| Refusal::Invalid)?;
    let PeeledContent::MlsMessage { bytes } = peeled.content else {
        return Err(Refusal::Invalid);
    };
    let bytes = Zeroizing::new(bytes);
    if bytes.as_slice() != inner.as_slice() {
        return Err(Refusal::Invalid);
    }

    let proof = runtime
        .group_authority_view()
        .with_group_authority_snapshot(scope.group, |storage, authority| {
            let retained = retained_inner(storage, authority, scope, pin, request)?;
            if retained.as_slice() != bytes.as_slice() {
                return Err(Refusal::Invalid);
            }
            let fingerprint = storage
                .group_replay_state_fingerprint(scope.group)?
                .ok_or(Refusal::Storage)?;
            let proof = authenticate_proposal(storage, scope, pin, &signature_key, &bytes)?;
            if storage.group_replay_state_fingerprint(scope.group)? != Some(fingerprint) {
                return Err(Refusal::Storage);
            }
            Ok(proof)
        })?;
    if runtime.epoch_pin(scope.group).ok() != Some(pin) {
        return Err(Refusal::Invalid);
    }
    Ok(proof)
}

pub(crate) fn retained_inner(
    storage: &SqliteAccountStorage,
    authority: cgka_engine::group_authority::GroupAuthoritySnapshot,
    scope: &Scope<'_>,
    pin: EpochPin,
    request: &TransportPublishRequest,
) -> Result<Zeroizing<Vec<u8>>, Refusal> {
    if authority.lifecycle != GroupLifecycleState::Stable
        || authority.disbanding
        || !authority.facts.is_member
        || authority.facts.is_admin
        || authority.facts.epoch != pin.epoch
    {
        return Err(Refusal::Invalid);
    }
    let Some(leave) = storage.leave_request(scope.group)? else {
        return Err(Refusal::Invalid);
    };
    if &leave.group_id != scope.group
        || leave.last_proposed_epoch != Some(pin.epoch)
        || leave.blocked.is_some()
    {
        return Err(Refusal::Invalid);
    }
    let record = match storage.get_message(&request.message.id) {
        Ok(record) => record,
        Err(StorageError::NotFound) => return Err(Refusal::Invalid),
        Err(err) => return Err(err.into()),
    };
    if record.id != request.message.id
        || &record.group_id != scope.group
        || record.epoch != pin.epoch
        || record.state != MessageState::Sent
        || !(1..=MAX_STORED_SIGNED_WIRE_BYTES).contains(&record.payload.len())
    {
        return Err(Refusal::Invalid);
    }
    let payload = Zeroizing::new(record.payload);
    let StoredMessagePayload::SignedOpenMlsWire {
        exact_message,
        openmls_message,
        ..
    } = StoredMessagePayload::decode(&payload).map_err(|_| Refusal::Invalid)?
    else {
        return Err(Refusal::Invalid);
    };
    let inner = Zeroizing::new(openmls_message.payload);
    if exact_message != request.message
        || openmls_message.id != record.id
        || !(1..=65_536).contains(&inner.len())
    {
        return Err(Refusal::Invalid);
    }
    Ok(inner)
}

pub(crate) fn authenticate_proposal(
    storage: &SqliteAccountStorage,
    scope: &Scope<'_>,
    pin: EpochPin,
    signature_key: &[u8],
    bytes: &[u8],
) -> Result<VerifiedLeaf, Refusal> {
    let message = MlsMessageIn::tls_deserialize_exact(bytes).map_err(|_| Refusal::Invalid)?;
    let MlsMessageBodyIn::PublicMessage(public) = message.extract() else {
        return Err(Refusal::Invalid);
    };
    let message = ProtocolMessage::from(public);
    let group_id = openmls::prelude::GroupId::from_slice(scope.group.as_slice());
    if message.content_type() != ContentType::Proposal
        || message.group_id() != &group_id
        || message.epoch().as_u64() != pin.epoch.0
    {
        return Err(Refusal::Invalid);
    }

    let crypto = RustCrypto::default();
    let provider =
        EngineOpenMlsProvider::<SqliteAccountStorage>::new(&crypto, storage.mls_storage());
    let (mut group, local_leaf, local_credential) =
        load_local_group(&provider, scope, pin, signature_key)?;
    let processed = group
        .process_message(&provider, message)
        .map_err(|_| Refusal::Invalid)?;
    if processed.sender() != &Sender::Member(local_leaf)
        || processed.group_id() != &group_id
        || processed.epoch().as_u64() != pin.epoch.0
        || processed.credential() != &local_credential
    {
        return Err(Refusal::Invalid);
    }
    let ProcessedMessageContent::ProposalMessage(proposal) = processed.into_content() else {
        return Err(Refusal::Invalid);
    };
    if !matches!(proposal.proposal(), Proposal::SelfRemove) {
        return Err(Refusal::Invalid);
    }
    Ok(VerifiedLeaf(local_leaf.u32()))
}

pub(crate) fn local_leaf(
    storage: &SqliteAccountStorage,
    scope: &Scope<'_>,
    pin: EpochPin,
    signature_key: &[u8],
) -> Result<VerifiedLeaf, Refusal> {
    let crypto = RustCrypto::default();
    let provider =
        EngineOpenMlsProvider::<SqliteAccountStorage>::new(&crypto, storage.mls_storage());
    let (_, leaf, _) = load_local_group(&provider, scope, pin, signature_key)?;
    Ok(VerifiedLeaf(leaf.u32()))
}

fn load_local_group(
    provider: &EngineOpenMlsProvider<'_, SqliteAccountStorage>,
    scope: &Scope<'_>,
    pin: EpochPin,
    signature_key: &[u8],
) -> Result<(MlsGroup, LeafNodeIndex, Credential), Refusal> {
    let group_id = openmls::prelude::GroupId::from_slice(scope.group.as_slice());
    let group = MlsGroup::load(provider.storage(), &group_id)
        .map_err(|_| Refusal::Storage)?
        .ok_or(Refusal::Invalid)?;
    let branch: [u8; 32] = Sha256::digest(group.epoch_authenticator().as_slice()).into();
    if !group.is_active()
        || group.pending_commit().is_some()
        || group.group_id() != &group_id
        || group.epoch().as_u64() != pin.epoch.0
        || branch != pin.identity
    {
        return Err(Refusal::Invalid);
    }
    let routing = group
        .extensions()
        .app_data_dictionary()
        .and_then(|extension| {
            extension
                .dictionary()
                .entries()
                .find(|entry| entry.id() == NOSTR_ROUTING_COMPONENT_ID)
        })
        .ok_or(Refusal::Invalid)?;
    let routing = decode_nostr_routing_v1(routing.data()).map_err(|_| Refusal::Invalid)?;
    if routing.nostr_group_id.as_slice() != scope.room_tag
        || !routing.relays.iter().any(|relay| relay == scope.origin)
    {
        return Err(Refusal::Invalid);
    }
    let local_leaf = group.own_leaf_index();
    let node = group.own_leaf_node().ok_or(Refusal::Invalid)?;
    let local_credential = node.credential().clone();
    let identity =
        BasicCredential::try_from(local_credential.clone()).map_err(|_| Refusal::Invalid)?;
    if identity.identity() != scope.account.as_slice()
        || node.signature_key().as_slice() != signature_key
    {
        return Err(Refusal::Invalid);
    }
    Ok((group, local_leaf, local_credential))
}
