//! Authenticate a native SelfRemove without letting this probe publish anything.

use super::{DeniedTransport, RELAY};
pub(super) use crate::mdk_adapter::peer_native_signer::self_remove::MAX_STORED_SIGNED_WIRE_BYTES;
use crate::mdk_adapter::{
    Publisher,
    peer_native_signer::self_remove::{
        Refusal, Scope, authenticate_proposal, retained_inner, verify,
    },
};
use cgka_engine::provider::EngineOpenMlsProvider;
use cgka_traits::engine::EpochPin;
use cgka_traits::storage::{LeaveRequestStorage, StorageError, StorageProvider};
use cgka_traits::{GroupId, MemberId, TransportPublishRequest, TransportPublishTarget};
use marmot_session::{AccountDeviceRuntime, StaticTransportRouting};
use openmls::prelude::tls_codec::Deserialize as _;
use openmls::prelude::{
    MlsGroup, MlsMessageBodyIn, MlsMessageIn, OpenMlsProvider, ProcessedMessageContent, Proposal,
    ProtocolMessage,
};
use openmls_rust_crypto::RustCrypto;
use storage_sqlite::SqliteAccountStorage;
use zeroize::Zeroizing;

type Runtime = AccountDeviceRuntime<DeniedTransport, StaticTransportRouting, Publisher>;

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
        origin: RELAY,
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
        origin: RELAY,
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
        origin: RELAY,
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
