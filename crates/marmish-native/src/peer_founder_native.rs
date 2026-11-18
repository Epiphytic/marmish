//! Isolated proof of one exact native create and an independently closed reopen.

use super::*;
use crate::mdk_adapter::{
    peer_native_signer::RefusingIo, peer_staged_proof_tests::postjoin_session_config,
};
use crate::peer_invitation::founder_ledger::Started;
use cgka_traits::{
    EpochId, EpochPin, GroupId, GroupLifecycleState, MemberId, MessageId, TransportEndpoint,
    TransportEnvelope,
    app_components::{
        AppComponentData, NOSTR_ROUTING_COMPONENT_ID, NostrRoutingV1, encode_nostr_routing_v1,
    },
    engine::{CreateGroupRequest, KeyPackage},
    maintenance::KeyPackageLifecycleState,
    storage::MaintenanceStorage,
    transport::TransportMessage,
};
use marmot_session::{
    AccountDeviceRuntime, PublishWork, StaticTransportRouting, WelcomeWithdrawal,
};
use nostr::key::{Keys, PublicKey};
use storage_sqlite::{SqlCipherKey, SqliteAccountStorage, SqliteJournalMode, SqliteStorageOptions};
use zeroize::Zeroizing;

const DESCRIPTION: &str = "marmish-peer/1 peer transport";

type Native = AccountDeviceRuntime<RefusingIo, StaticTransportRouting, RefusingIo>;

pub(crate) struct Expected<'a> {
    pub(crate) signer: Arc<Keys>,
    pub(crate) recipient: PublicKey,
    pub(crate) routing: &'a NostrRoutingV1,
    pub(crate) room: [u8; 16],
    pub(crate) reference: &'a [u8],
}

pub(crate) struct Image {
    pub(crate) group: GroupId,
    pub(crate) pin: EpochPin,
    pub(crate) founder_leaf: u32,
    pub(crate) recipient_leaf: u32,
    pub(crate) welcome_id: MessageId,
}

pub(crate) struct Created {
    pub(crate) image: Image,
    pub(crate) welcome: Zeroizing<Vec<u8>>,
}

impl FounderQuota<'_> {
    pub(crate) fn create(
        &mut self,
        started: Started,
        key: &Zeroizing<[u8; 32]>,
        expected: &Expected<'_>,
        package: KeyPackage,
        mut recheck: impl FnMut() -> Result<()>,
    ) -> Result<(Created, u64)> {
        let (generation, file) = started.coordinates();
        self.run(generation, file, |path| {
            recheck()?;
            let storage = open(path, key)?;
            if storage
                .key_package_lifecycle()
                .map_err(|_| "founder_lifecycle")?
                .is_some()
            {
                return Err("founder_lifecycle");
            }
            storage
                .put_key_package_lifecycle(&KeyPackageLifecycleState::slot_only(hex::encode(
                    expected.room,
                )))
                .map_err(|_| "founder_lifecycle")?;
            let (mut runtime, transport, publisher) = open_runtime(storage, expected)?;
            recheck()?;
            let route = encode_nostr_routing_v1(expected.routing).map_err(|_| "founder_route")?;
            let created = resolve_once(runtime.prepare_create_group_with_audit_context(
                CreateGroupRequest {
                    name: format!("marmish/peer/1/{}", hex::encode(expected.room)),
                    description: DESCRIPTION.into(),
                    members: vec![package],
                    required_features: Vec::new(),
                    initial_admins: vec![member(expected.signer.public_key())],
                    app_components: vec![AppComponentData {
                        component_id: NOSTR_ROUTING_COMPONENT_ID,
                        data: route,
                    }],
                },
                Default::default(),
            ))?
            .map_err(|_| "founder_create")?;
            let mut outputs = created.effects.publish.into_iter();
            let Some(PublishWork::FoundingGroupCreated { mut welcomes }) = outputs.next() else {
                return Err("founder_output");
            };
            if outputs.next().is_some() || welcomes.len() != 1 {
                return Err("founder_output");
            }
            let welcome = welcomes.pop().ok_or("founder_output")?;
            let group = created.group_id;
            require_welcome(&runtime, expected, &group, &welcome)?;
            recheck()?;
            if runtime
                .withdraw_welcome(&welcome.id)
                .map_err(|_| "founder_withdraw")?
                != WelcomeWithdrawal::Withdrawn
            {
                return Err("founder_withdraw");
            }
            let image = inspect(&runtime, expected, &group, &welcome.id)?;
            drop(runtime);
            require_unused(&transport, &publisher)?;
            Ok(Created {
                image,
                welcome: Zeroizing::new(welcome.payload),
            })
        })
    }

    pub(crate) fn inspect(
        &mut self,
        generation: NonZeroU64,
        file: (u64, u64),
        key: &Zeroizing<[u8; 32]>,
        expected: &Expected<'_>,
        original: &Image,
        mut recheck: impl FnMut() -> Result<()>,
    ) -> Result<(Image, u64)> {
        self.run(generation, file, |path| {
            recheck()?;
            let storage = open(path, key)?;
            if storage
                .key_package_lifecycle()
                .map_err(|_| "founder_lifecycle")?
                .is_none()
            {
                return Err("founder_lifecycle");
            }
            let (runtime, transport, publisher) = open_runtime(storage, expected)?;
            let observed = inspect(&runtime, expected, &original.group, &original.welcome_id)?;
            if observed.pin != original.pin
                || observed.founder_leaf != original.founder_leaf
                || observed.recipient_leaf != original.recipient_leaf
            {
                return Err("founder_changed");
            }
            drop(runtime);
            require_unused(&transport, &publisher)?;
            Ok(observed)
        })
    }
}

fn open(path: &Path, key: &Zeroizing<[u8; 32]>) -> Result<SqliteAccountStorage> {
    SqliteAccountStorage::open_encrypted_with_options(
        path,
        &SqlCipherKey::raw(**key),
        SqliteStorageOptions {
            journal_mode: SqliteJournalMode::Delete,
            ..Default::default()
        },
    )
    .map_err(|_| "founder_native_open")
}

fn open_runtime(
    storage: SqliteAccountStorage,
    expected: &Expected<'_>,
) -> Result<(Native, RefusingIo, RefusingIo)> {
    let config = postjoin_session_config(storage, expected.signer.clone());
    let transport = RefusingIo::no_size_report();
    let publisher = RefusingIo::default();
    let runtime = AccountDeviceRuntime::open(config, || {
        Ok::<_, marmot_session::SessionError>((
            transport.clone(),
            StaticTransportRouting::new(
                expected
                    .routing
                    .relays
                    .iter()
                    .cloned()
                    .map(TransportEndpoint)
                    .collect(),
            ),
            publisher.clone(),
        ))
    })
    .map_err(|_| "founder_runtime")?;
    Ok((runtime, transport, publisher))
}

fn require_unused(transport: &RefusingIo, publisher: &RefusingIo) -> Result<()> {
    transport
        .require_unused()
        .map_err(|_| "founder_transport_attempt")?;
    publisher
        .require_unused()
        .map_err(|_| "founder_package_attempt")
}

fn member(key: PublicKey) -> MemberId {
    MemberId::new(key.to_bytes().to_vec())
}

fn require_welcome(
    runtime: &Native,
    expected: &Expected<'_>,
    group: &GroupId,
    welcome: &TransportMessage,
) -> Result<()> {
    if welcome.envelope
        != (TransportEnvelope::Welcome {
            recipient: member(expected.recipient),
        })
        || runtime
            .welcomes_for_key_package(group, expected.reference)
            .map_err(|_| "founder_tracked")?
            != [welcome.id.clone()]
        || !matches!(
            runtime.outstanding_welcome_deliveries().map_err(|_| "founder_outstanding")?.as_slice(),
            [(owner, exact)] if owner == group && exact.id == welcome.id && exact.payload == welcome.payload
        )
    {
        return Err("founder_welcome");
    }
    Ok(())
}

fn inspect(
    runtime: &Native,
    expected: &Expected<'_>,
    group: &GroupId,
    welcome_id: &MessageId,
) -> Result<Image> {
    let pin = runtime.epoch_pin(group).map_err(|_| "founder_epoch")?;
    if pin.epoch != EpochId(1) {
        return Err("founder_epoch");
    }
    let founder = member(expected.signer.public_key());
    let recipient = member(expected.recipient);
    let membership = runtime
        .canonical_group_membership(group)
        .map_err(|_| "founder_membership")?;
    let founder_leaf = runtime
        .own_leaf_index(group)
        .map_err(|_| "founder_membership")?;
    let [first, second] = membership.member_leaves.as_slice() else {
        return Err("founder_membership");
    };
    let (local, remote) = if first.member == founder {
        (first, second)
    } else {
        (second, first)
    };
    if !membership.local_leaf_active
        || runtime.self_id() != founder
        || local.member != founder
        || local.leaf_index != founder_leaf
        || remote.member != recipient
        || remote.leaf_index == founder_leaf
        || runtime
            .welcomes_for_key_package(group, expected.reference)
            .map_err(|_| "founder_tracked")?
            != [welcome_id.clone()]
        || !runtime
            .outstanding_welcome_deliveries()
            .map_err(|_| "founder_outstanding")?
            .is_empty()
    {
        return Err("founder_image");
    }
    let exact = encode_nostr_routing_v1(expected.routing).map_err(|_| "founder_route")?;
    let routing = runtime
        .app_component(group, NOSTR_ROUTING_COMPONENT_ID)
        .map_err(|_| "founder_route")?;
    let locally_created = runtime
        .group_authority_view()
        .with_group_authority_snapshot::<_, marmot_session::SessionError>(
            group,
            |storage, snapshot| {
                use cgka_traits::storage::GroupStorage;
                let admission = storage.invitation_admission(group)?;
                Ok(admission.is_some_and(|admission| {
                    admission.created_locally && admission.via_welcome.is_none()
                }) && snapshot.lifecycle == GroupLifecycleState::Stable
                    && snapshot.facts.epoch == pin.epoch
                    && snapshot.facts.is_member
                    && !snapshot.disbanding)
            },
        )
        .map_err(|_| "founder_authority")?;
    if routing.as_deref() != Some(exact.as_slice()) || !locally_created {
        return Err("founder_authority");
    }
    Ok(Image {
        group: group.clone(),
        pin,
        founder_leaf,
        recipient_leaf: remote.leaf_index,
        welcome_id: welcome_id.clone(),
    })
}
