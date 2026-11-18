//! Authenticate a native SelfRemove without letting this probe publish anything.

use super::{DeniedTransport, RELAY};
use crate::mdk_adapter::Publisher;
use cgka_engine::provider::EngineOpenMlsProvider;
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
use marmot_session::{AccountDeviceRuntime, SessionError, StaticTransportRouting};
use openmls::prelude::tls_codec::Deserialize as _;
use openmls::prelude::{
    BasicCredential, ContentType, MlsGroup, MlsMessageBodyIn, MlsMessageIn, OpenMlsProvider,
    ProcessedMessageContent, Proposal, ProtocolMessage, Sender,
};
use openmls_rust_crypto::RustCrypto;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use storage_sqlite::SqliteAccountStorage;
use transport_nostr_peeler::{DEFAULT_EXPORTER_LABEL, NostrMlsPeeler, NostrTransportEvent};
use zeroize::Zeroizing;

type Runtime = AccountDeviceRuntime<DeniedTransport, StaticTransportRouting, Publisher>;

struct Scope<'a> {
    group: &'a GroupId,
    account: &'a MemberId,
    room_tag: &'a [u8],
}

#[derive(Debug, Eq, PartialEq)]
enum Refusal {
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
struct VerifiedLeaf(u32);

async fn verify(
    runtime: &mut Runtime,
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
        || endpoint.0 != RELAY
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

fn retained_inner(
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
        || !(1..=262_144).contains(&record.payload.len())
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

fn authenticate_proposal(
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
    let mut group = MlsGroup::load(provider.storage(), &group_id)
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
        || !routing.relays.iter().any(|relay| relay == RELAY)
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

pub(super) async fn check_positive(
    runtime: &mut Runtime,
    group: &GroupId,
    account: &MemberId,
    room_tag: &[u8],
    request: &TransportPublishRequest,
    benchmark: bool,
) {
    use cgka_traits::storage::AccountDeviceSignerStorage;
    let scope = Scope {
        group,
        account,
        room_tag,
    };
    let before = runtime
        .group_authority_view()
        .with_group_authority_snapshot(group, |storage, _| -> Result<_, Refusal> {
            let signer = storage
                .account_device_signer(account)?
                .ok_or(Refusal::Storage)?;
            // Try a genuine SQL mutation on the fixture handle while query-only.
            assert!(storage.put_account_device_signer(&signer).is_err());
            storage
                .group_replay_state_fingerprint(group)?
                .ok_or(Refusal::Storage)
        })
        .unwrap();
    let expected = verify(runtime, &scope, request)
        .await
        .expect("genuine native SelfRemove");

    let mut wrong_actor = request.clone();
    wrong_actor.account_id =
        MemberId::new(super::Keys::generate().public_key().to_bytes().to_vec());
    assert_eq!(
        verify(runtime, &scope, &wrong_actor).await,
        Err(Refusal::Invalid)
    );
    let bad_room_tag = [0_u8; 32];
    let wrong_route = Scope {
        room_tag: &bad_room_tag,
        ..scope
    };
    assert_eq!(
        verify(runtime, &wrong_route, request).await,
        Err(Refusal::Invalid)
    );

    let mut wrong_target = request.clone();
    wrong_target.target = TransportPublishTarget::Inbox {
        recipient: account.clone(),
        endpoints: super::endpoints(),
    };
    assert_eq!(
        verify(runtime, &scope, &wrong_target).await,
        Err(Refusal::Invalid)
    );
    let mut wrong_ack = request.clone();
    wrong_ack.required_acks = 0;
    assert_eq!(
        verify(runtime, &scope, &wrong_ack).await,
        Err(Refusal::Invalid)
    );
    let mut wrong_id = request.clone();
    wrong_id.message.id = cgka_traits::MessageId::new(vec![0; 32]);
    assert_eq!(
        verify(runtime, &scope, &wrong_id).await,
        Err(Refusal::Invalid)
    );
    let mut changed_wire = request.clone();
    changed_wire.message.payload.push(b' ');
    assert_eq!(
        verify(runtime, &scope, &changed_wire).await,
        Err(Refusal::Invalid)
    );

    let pin = runtime.epoch_pin(group).unwrap();
    let key = runtime.own_signature_key();
    runtime
        .group_authority_view()
        .with_group_authority_snapshot(group, |storage, authority| {
            let bytes = retained_inner(storage, authority, &scope, pin, request)?;
            let wrong_branch = EpochPin {
                identity: [0; 32],
                ..pin
            };
            assert_eq!(
                authenticate_proposal(storage, &scope, wrong_branch, &key, &bytes),
                Err(Refusal::Invalid)
            );
            let stale = EpochPin {
                epoch: cgka_traits::EpochId(pin.epoch.0 + 1),
                ..pin
            };
            assert_eq!(
                authenticate_proposal(storage, &scope, stale, &key, &bytes),
                Err(Refusal::Invalid)
            );
            assert_eq!(
                authenticate_proposal(storage, &scope, pin, &[0; 32], &bytes),
                Err(Refusal::Invalid)
            );
            assert!(storage.group_replay_state_fingerprint(group)? == Some(before));
            Ok::<_, Refusal>(())
        })
        .unwrap();

    if benchmark {
        let mut elapsed = Vec::with_capacity(20);
        for _ in 0..20 {
            let began = std::time::Instant::now();
            assert_eq!(verify(runtime, &scope, request).await, Ok(expected));
            elapsed.push(began.elapsed().as_micros());
        }
        elapsed.sort_unstable();
        eprintln!(
            "native_self_remove_rechecks={} p50_us={} p95_us={}",
            elapsed.len(),
            elapsed[9],
            elapsed[18]
        );
    }
}

pub(super) fn check_authenticated_negatives(
    runtime: &Runtime,
    local: &SqliteAccountStorage,
    remote: &SqliteAccountStorage,
    group: &GroupId,
    local_account: &MemberId,
    remote_account: &MemberId,
    room_tag: &[u8],
) {
    let scope = Scope {
        group,
        account: local_account,
        room_tag,
    };
    let remove = fixture_wire(local, group, local_account, Alternative::Remove);
    let private = fixture_wire(local, group, local_account, Alternative::Private);
    let wrong_leaf = fixture_wire(remote, group, remote_account, Alternative::OtherLeaf);
    let pin = runtime.epoch_pin(group).unwrap();
    let local_key = runtime.own_signature_key();
    runtime
        .group_authority_view()
        .with_group_authority_snapshot(group, |storage, _| {
            for bytes in [&remove, &private, &wrong_leaf] {
                assert_eq!(
                    authenticate_proposal(storage, &scope, pin, &local_key, bytes),
                    Err(Refusal::Invalid)
                );
            }
            Ok::<_, Refusal>(())
        })
        .unwrap();
}

#[derive(Clone, Copy)]
enum Alternative {
    Remove,
    Private,
    OtherLeaf,
}

// The fixture builds independent signed negative vectors and discards any
// signing-side writes. The verifier above accepts only query-only snapshots.
fn fixture_wire(
    storage: &SqliteAccountStorage,
    group_id: &GroupId,
    account: &MemberId,
    alternative: Alternative,
) -> Zeroizing<Vec<u8>> {
    use cgka_traits::storage::AccountDeviceSignerStorage;
    use openmls::prelude::tls_codec::Serialize as _;
    use openmls_basic_credential::SignatureKeyPair;
    let id = openmls::prelude::GroupId::from_slice(group_id.as_slice());
    enum GenerationExit {
        Storage,
        Fixture(Zeroizing<Vec<u8>>),
    }
    impl From<StorageError> for GenerationExit {
        fn from(_: StorageError) -> Self {
            Self::Storage
        }
    }

    let result =
        storage.with_group_savepoint(group_id, "peer-negative-proposal-fixture", |storage| {
            let crypto = RustCrypto::default();
            let provider =
                EngineOpenMlsProvider::<SqliteAccountStorage>::new(&crypto, storage.mls_storage());
            let mut group = MlsGroup::load(provider.storage(), &id).unwrap().unwrap();
            let binding = storage.account_device_signer(account)?.unwrap();
            let signer = SignatureKeyPair::read(
                provider.storage(),
                &binding.mls_signature_public_key,
                group.ciphersuite().signature_algorithm(),
            )
            .unwrap();

            let out = match alternative {
                Alternative::Remove => group
                    .leave_group(&provider, &signer)
                    .unwrap_or_else(|_| panic!("ordinary Remove")),
                Alternative::OtherLeaf => group
                    .leave_group_via_self_remove(&provider, &signer)
                    .unwrap_or_else(|_| panic!("other device's SelfRemove")),
                Alternative::Private => group
                    .create_message(&provider, &signer, b"fixture")
                    .unwrap_or_else(|_| panic!("private MLS application message")),
            };
            let bytes = Zeroizing::new(out.tls_serialize_detached().unwrap());

            let message = MlsMessageIn::tls_deserialize_exact(&bytes).unwrap();
            match alternative {
                Alternative::Remove | Alternative::OtherLeaf => {
                    let MlsMessageBodyIn::PublicMessage(public) = message.extract() else {
                        panic!("fixture expected a public proposal");
                    };
                    let processed = group
                        .process_message(&provider, ProtocolMessage::from(public))
                        .unwrap_or_else(|_| panic!("independently authentic native proposal"));
                    let ProcessedMessageContent::ProposalMessage(proposal) =
                        processed.into_content()
                    else {
                        panic!("fixture expected typed proposal");
                    };
                    match alternative {
                        Alternative::Remove => {
                            assert!(matches!(proposal.proposal(), Proposal::Remove(_)))
                        }
                        Alternative::OtherLeaf => {
                            assert!(matches!(proposal.proposal(), Proposal::SelfRemove))
                        }
                        Alternative::Private => unreachable!(),
                    }
                }
                Alternative::Private => assert!(matches!(
                    message.extract(),
                    MlsMessageBodyIn::PrivateMessage(_)
                )),
            }
            Err::<(), _>(GenerationExit::Fixture(bytes))
        });
    let Err(GenerationExit::Fixture(bytes)) = result else {
        panic!("could not generate and roll back signed negative fixture");
    };
    bytes
}

// The fixture is discarded after these direct negative storage mutations.
pub(super) async fn check_authoritative_denials(
    runtime: &mut Runtime,
    storage: &SqliteAccountStorage,
    group: &GroupId,
    room_tag: &[u8],
    request: &TransportPublishRequest,
) {
    use cgka_traits::storage::LeaveBlockReason;
    let account = runtime.self_id();
    let scope = Scope {
        group,
        account: &account,
        room_tag,
    };
    let pin = runtime
        .epoch_pin(group)
        .unwrap_or_else(|_| panic!("fresh native branch required"));
    let key = runtime.own_signature_key();
    let bytes = runtime
        .group_authority_view()
        .with_group_authority_snapshot(group, |exact, authority| {
            retained_inner(exact, authority, &scope, pin, request)
        })
        .expect("original exact native inner wire");
    let mut leave = storage.leave_request(group).unwrap().unwrap();
    storage.clear_leave_request(group).unwrap();
    assert_eq!(
        verify(runtime, &scope, request).await,
        Err(Refusal::Invalid)
    );
    leave.blocked = Some(LeaveBlockReason::LastActiveAdmin);
    storage.put_leave_request(&leave).unwrap();
    assert_eq!(
        verify(runtime, &scope, request).await,
        Err(Refusal::Invalid)
    );
    leave.blocked = None;
    let last_epoch = leave
        .last_proposed_epoch
        .expect("initial native leave epoch");
    leave.last_proposed_epoch = Some(cgka_traits::EpochId(
        last_epoch
            .0
            .checked_sub(1)
            .expect("fixture epoch is positive"),
    ));
    storage.put_leave_request(&leave).unwrap();
    assert_eq!(
        verify(runtime, &scope, request).await,
        Err(Refusal::Invalid)
    );
    storage
        .close()
        .unwrap_or_else(|_| panic!("fixture SQLCipher close failed"));
    assert_eq!(
        authenticate_proposal(storage, &scope, pin, &key, &bytes),
        Err(Refusal::Storage)
    );
    assert!(verify(runtime, &scope, request).await.is_err());
}
