//! Native ingress and readback for the private pending Peer fixture.

use super::*;
use crate::peer_invitation::prepared_consent::Delivery;
use cgka_traits::app_components::decode_nostr_routing_v1;
use cgka_traits::engine::GroupEvent;
use cgka_traits::ingest::IngestOutcome;
use cgka_traits::{
    EpochPin, EventSizeClass, GroupId, TransportDeliveryPlane, TransportDeliverySource,
    TransportEndpointFailure, TransportEnvelope, TransportPublishTarget, TransportReadBack,
};
use marmot_session::{AccountDeviceEffects, KeyPackageEventDeletion, KeyPackageSlotWithdrawal};
use sha2::Digest as _;
use std::{result::Result, sync::Mutex};

const SUBSCRIPTION: &str = "marmish-peer-m2-inbox";
const PURPOSE_PREFIX: &str = "marmish/peer/1/";
const DESCRIPTION: &str = "marmish-peer/1 peer transport";

type JoinRuntime = AccountDeviceRuntime<RefusingIo, StaticTransportRouting, RefusingIo>;

#[path = "peer_stage_leave_proof.rs"]
mod leave_proof;
pub(crate) use leave_proof::{CapturedLeave, LeaveInspection};

/// Borrow the signed Delivery revalidated by the custody owner and its exact
/// package digest. The native half has no authority to advance custody.
pub(crate) struct JoinWitness<'a> {
    delivery: &'a Delivery,
    origin: &'a str,
    package_digest: [u8; 32],
    welcome: TransportMessage,
}

impl<'a> JoinWitness<'a> {
    pub(crate) fn new(
        delivery: &'a Delivery,
        origin: &'a str,
        package_digest: [u8; 32],
    ) -> StageResult<Self> {
        let flight = delivery.flight();
        let offer = delivery.offer();
        if offer.flight().author != flight.author
            || offer.flight().recipient != flight.recipient
            || offer.routing().relays != [origin]
            || flight.author == flight.recipient
        {
            return Err(StageError::Invalid);
        }
        let raw = delivery.welcome_bytes();
        let event: Event = serde_json::from_slice(raw).map_err(|_| StageError::Invalid)?;
        event.verify().map_err(|_| StageError::Invalid)?;
        let digest: [u8; 32] = Sha256::digest(raw).into();
        let dto: NostrTransportEvent =
            serde_json::from_slice(raw).map_err(|_| StageError::Invalid)?;
        // The DTO maps the native gift-wrap kind and recipient to Welcome.
        let mut welcome = dto
            .to_transport_message()
            .map_err(|_| StageError::Invalid)?;
        if event.id != delivery.welcome_id()
            || digest != delivery.welcome_digest()
            || welcome.id.as_slice() != event.id.to_bytes()
            || welcome.envelope
                != (TransportEnvelope::Welcome {
                    recipient: Self::member(flight.recipient),
                })
        {
            return Err(StageError::Invalid);
        }
        // The DTO derives routing and message ID. Preserve the authenticated
        // original JSON bytes in the native ingress payload.
        welcome.payload = raw.to_vec();
        Ok(Self {
            delivery,
            origin,
            package_digest,
            welcome,
        })
    }

    fn member(key: PublicKey) -> MemberId {
        MemberId::new(key.to_bytes().to_vec())
    }

    fn require_package_artifact(&self, package: NativePackage) -> StageResult<()> {
        let digest: [u8; 32] = Sha256::digest(&package.bytes).into();
        if package.id != self.delivery.binding().event_id || digest != self.package_digest {
            return Err(StageError::Invalid);
        }
        Ok(())
    }

    fn group_from_receipt(&self, events: &[GroupEvent]) -> StageResult<GroupId> {
        let [
            GroupEvent::GroupJoined {
                group_id,
                via_welcome: welcome,
                welcomer: Some(welcomer),
                explicitly_confirmed: false,
                ..
            },
        ] = events
        else {
            return Err(StageError::Unavailable);
        };
        if welcome != &self.welcome.id
            || *welcomer != Self::member(self.delivery.flight().author)
            || group_id.as_slice().is_empty()
            || group_id.as_slice() == self.delivery.offer().room_id()
        {
            return Err(StageError::Invalid);
        }
        Ok(group_id.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JoinedAuthority {
    group: GroupId,
    epoch: EpochPin,
    own_leaf: u32,
}

impl JoinedAuthority {
    pub(crate) fn coordinates(&self) -> (&[u8], u64, [u8; 32], u32) {
        (
            self.group.as_slice(),
            self.epoch.epoch.0,
            self.epoch.identity,
            self.own_leaf,
        )
    }
}

impl OpenStage {
    fn join_runtime(&self, io: &RefusingIo) -> StageResult<JoinRuntime> {
        let config = SessionConfig::with_storage(
            self.storage.clone(),
            Duration::ZERO,
            self.signer.public_key().to_bytes().to_vec(),
            Box::new(NostrMlsPeeler::new().with_welcome_signer_arc(self.signer.clone())),
        )
        .account_identity_proof_signer(Arc::new(ProofSigner(self.signer.clone())))
        .supported_app_components(components());
        AccountDeviceRuntime::open(config, || {
            Ok::<_, marmot_session::SessionError>((
                io.clone(),
                StaticTransportRouting::new(vec![self.endpoint.clone()]),
                io.clone(),
            ))
        })
        .map_err(|_| StageError::Storage)
    }

    fn check_join_stage(&self, witness: &JoinWitness<'_>) -> StageResult<()> {
        if self.signer.public_key() != witness.delivery.flight().recipient
            || self.endpoint.0 != witness.origin
            || self.slot != hex::encode(witness.delivery.binding().slot)
        {
            return Err(StageError::Invalid);
        }
        Ok(())
    }

    pub(crate) fn require_prepared_join(
        &self,
        witness: &JoinWitness<'_>,
        sample: Duration,
    ) -> StageResult<()> {
        self.check_join_stage(witness)?;
        self.require_unjoined_provisional()?;
        let (_, pending) = self.load_provisional_pending(sample)?;
        if pending.key_package_ref != witness.delivery.binding().reference {
            return Err(StageError::Invalid);
        }
        witness
            .require_package_artifact(self.exact_artifact(&pending, &self.publication(&pending))?)
    }

    /// Called once on the original image after the owner commits JoinStarted.
    /// A failed call conveys no authority and must never cause another ingest.
    pub(crate) async fn join_once(
        self,
        witness: &JoinWitness<'_>,
        sample: Duration,
    ) -> StageResult<()> {
        self.join_with_sample(witness, || Ok(sample.as_secs()))
            .await
    }

    #[cfg(test)]
    pub(crate) async fn join_once_fenced(
        self,
        witness: &JoinWitness<'_>,
        fence: &crate::peer_invitation::TurnFence<'_>,
    ) -> StageResult<()> {
        self.join_with_sample(witness, || fence.sample()).await
    }

    async fn join_with_sample(
        self,
        witness: &JoinWitness<'_>,
        sample: impl FnOnce() -> StageResult<u64>,
    ) -> StageResult<()> {
        self.check_join_stage(witness)?;
        let io = RefusingIo::default();
        let mut runtime = self.join_runtime(&io)?;
        let message = witness.welcome.clone();
        let delivery = TransportDelivery {
            account_id: JoinWitness::member(self.signer.public_key()),
            group_id_hint: None,
            received_at: Timestamp(sample()?),
            source: TransportDeliverySource {
                transport: witness.welcome.source.clone(),
                plane: TransportDeliveryPlane::AccountInbox,
                endpoint: Some(self.endpoint.clone()),
                subscription_id: Some(SUBSCRIPTION.into()),
                wire: None,
            },
            message,
        };
        let receipt = runtime.ingest_delivery(&delivery).await;
        drop(runtime);
        io.require_unused()?;
        let mut receipt = receipt.map_err(|_| StageError::Unavailable)?;
        if receipt.outcome != IngestOutcome::Processed || receipt.left_object_unpersisted {
            return Err(StageError::Invalid);
        }
        receipt.effects.local_accept_duration = None;
        receipt.effects.publish_duration = None;
        let events = std::mem::take(&mut receipt.effects.events);
        witness.group_from_receipt(&events)?;
        if receipt.effects != AccountDeviceEffects::default() {
            return Err(StageError::Invalid);
        }
        Ok(())
    }

    /// Open a native runtime on an independently guarded disposable copy. A
    /// persisted receipt is the only source of the native MLS group ID.
    pub(crate) fn read_joined(&self, witness: &JoinWitness<'_>) -> StageResult<JoinedAuthority> {
        self.check_join_stage(witness)?;
        let io = RefusingIo::default();
        let runtime = self.join_runtime(&io)?;
        let result = self.joined_authority(&runtime, witness);
        drop(runtime);
        io.require_unused()?;
        result
    }

    /// Native call only. The custody owner must first commit and consume its
    /// unique reservation. The proof installs no group route or connector.
    #[cfg(test)]
    pub(crate) async fn leave_for_custody(
        self,
        witness: &JoinWitness<'_>,
        expected: (&[u8], u64, [u8; 32], u32),
        fence: &crate::peer_invitation::TurnFence<'_>,
    ) -> StageResult<cgka_traits::MessageId> {
        use cgka_traits::{MessageId, engine::SendIntent, storage::LeaveRequestStorage};
        use marmot_session::AccountDeviceEffects;

        self.check_join_stage(witness)?;
        let io = RefusingIo::default();
        let mut runtime = self.join_runtime(&io)?;
        let authority = self.joined_authority(&runtime, witness)?;
        let deferred = self.check_leave_environment(&runtime, &authority, witness)?;
        let (leave, sent) = self.leave_rows(&authority)?;
        if leave.is_some() || !sent.is_empty() || authority.coordinates() != expected {
            return Err(StageError::Invalid);
        }
        fence.check()?;
        let JoinedAuthority { group, epoch, .. } = authority;
        let mut result = runtime
            .send_host_change_with_audit_context(
                SendIntent::Leave {
                    group_id: group.clone(),
                },
                Default::default(),
            )
            .await
            .map_err(|_| StageError::Unavailable)?;
        if !runtime
            .outbound_fanouts()
            .map_err(|_| StageError::Storage)?
            .is_empty()
        {
            return Err(StageError::Invalid);
        }
        let readback = self.joined_authority(&runtime, witness)?;
        if readback.coordinates() != expected
            || self.check_leave_environment(&runtime, &readback, witness)? != deferred
        {
            return Err(StageError::Invalid);
        }
        drop(runtime);
        io.require_unused()?;
        result.local_accept_duration = None;
        result.publish_duration = None;
        let [failure]: [marmot_session::PublishFailure; 1] = std::mem::take(&mut result.failures)
            .try_into()
            .map_err(|_| StageError::Invalid)?;
        // The session exposes the retained, unconsumed application receipt
        // again when it collects effects. Qualify that exact Welcome only.
        let receipts = std::mem::take(&mut result.events);
        if witness.group_from_receipt(&receipts)? != group {
            return Err(StageError::Invalid);
        }
        let event_id: MessageId = failure.message_id;
        if result != AccountDeviceEffects::default()
            || self
                .storage
                .leave_request(&group)
                .map_err(|_| StageError::Storage)?
                .is_none_or(|leave| {
                    leave.group_id != group
                        || leave.last_proposed_epoch != Some(epoch.epoch)
                        || leave.blocked.is_some()
                })
        {
            return Err(StageError::Invalid);
        }
        Ok(event_id)
    }

    fn joined_authority(
        &self,
        runtime: &JoinRuntime,
        witness: &JoinWitness<'_>,
    ) -> StageResult<JoinedAuthority> {
        let events = self
            .storage
            .list_pending_application_events()
            .map_err(|_| StageError::Storage)?;
        let group = witness.group_from_receipt(&events)?;
        if runtime.live_group_ids().map_err(|_| StageError::Storage)? != [group.clone()] {
            return Err(StageError::Invalid);
        }
        let offer = witness.delivery.offer();
        let record = runtime
            .group_record(&group)
            .map_err(|_| StageError::Unavailable)?;
        if record.protocol_profile != ProtocolProfile::Current
            || record.name != format!("{PURPOSE_PREFIX}{}", hex::encode(offer.room_id()))
            || record.description != DESCRIPTION
            || record.removed
        {
            return Err(StageError::Invalid);
        }
        let flight = witness.delivery.flight();
        let roster = runtime
            .canonical_group_membership(&group)
            .map_err(|_| StageError::Unavailable)?;
        let own_leaf = runtime
            .own_leaf_index(&group)
            .map_err(|_| StageError::Unavailable)?;
        let local = JoinWitness::member(flight.recipient);
        let founder = JoinWitness::member(flight.author);
        if !roster.local_leaf_active
            || roster.member_leaves.len() != 2
            || !roster
                .member_leaves
                .iter()
                .any(|leaf| leaf.member == local && leaf.leaf_index == own_leaf)
            || !roster
                .member_leaves
                .iter()
                .any(|leaf| leaf.member == founder)
            || runtime
                .admin_pubkeys(&group)
                .map_err(|_| StageError::Unavailable)?
                != [flight.author.to_bytes()]
        {
            return Err(StageError::Invalid);
        }
        let component = runtime
            .app_component(&group, NOSTR_ROUTING_COMPONENT_ID)
            .map_err(|_| StageError::Unavailable)?
            .ok_or(StageError::Invalid)?;
        if decode_nostr_routing_v1(&component).map_err(|_| StageError::Invalid)? != *offer.routing()
        {
            return Err(StageError::Invalid);
        }
        self.check_joined_package(witness)?;
        Ok(JoinedAuthority {
            epoch: runtime
                .epoch_pin(&group)
                .map_err(|_| StageError::Unavailable)?,
            group,
            own_leaf,
        })
    }

    fn check_joined_package(&self, witness: &JoinWitness<'_>) -> StageResult<()> {
        let mut baseline = self.load()?.ok_or(StageError::Unavailable)?;
        let pending = baseline
            .pending_replacement
            .take()
            .ok_or(StageError::Unavailable)?;
        let binding = witness.delivery.binding();
        if baseline.phase != MaintenancePhase::PendingPublication
            || pending.consumed_at.is_none()
            || pending.generation_revision != KEY_PACKAGE_GENERATION_REVISION
            || pending.key_package_ref != binding.reference
        {
            return Err(StageError::Invalid);
        }
        baseline.phase = MaintenancePhase::Complete;
        if baseline != KeyPackageLifecycleState::slot_only(self.slot.clone()) {
            return Err(StageError::Invalid);
        }
        witness
            .require_package_artifact(self.exact_artifact(&pending, &self.publication(&pending))?)
    }
}

#[derive(Clone, Copy)]
enum Blocked {
    Activate,
    Sync,
    Deactivate,
    Publish,
    Receive,
    PublishCommit,
    ReadBack,
    SizeCheck,
    CommitSizeCheck,
    PackageSlot,
    PreparePackage,
    PublishPackage,
    WithdrawSlot,
    DeletePackage,
}

#[derive(Clone, Default)]
pub(crate) struct RefusingIo {
    attempts: Arc<Mutex<Vec<Blocked>>>,
    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    absent_size_reports: bool,
}

impl RefusingIo {
    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(crate) fn no_size_report() -> Self {
        Self {
            absent_size_reports: true,
            ..Self::default()
        }
    }

    fn deny<T>(&self, operation: Blocked, refusal: T) -> T {
        self.attempts.lock().unwrap().push(operation);
        refusal
    }

    pub(crate) fn require_unused(&self) -> StageResult<()> {
        if self.attempts.lock().unwrap().is_empty() {
            Ok(())
        } else {
            Err(StageError::Unavailable)
        }
    }
}

#[async_trait]
impl TransportAdapter for RefusingIo {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        self.deny(Blocked::Activate, Err(TransportAdapterError::Closed))
    }
    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        self.deny(Blocked::Sync, Err(TransportAdapterError::Closed))
    }
    async fn deactivate_account(&self, _: &MemberId) -> Result<(), TransportAdapterError> {
        self.deny(Blocked::Deactivate, Err(TransportAdapterError::Closed))
    }
    async fn publish(
        &self,
        _: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        self.deny(Blocked::Publish, Err(TransportAdapterError::Closed))
    }
    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        self.deny(Blocked::Receive, Err(TransportAdapterError::Closed))
    }
    async fn publish_commit(
        &self,
        _: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        self.deny(Blocked::PublishCommit, Err(TransportAdapterError::Closed))
    }
    async fn read_back(
        &self,
        _: &MemberId,
        _: &TransportMessage,
        endpoints: &[TransportEndpoint],
    ) -> Vec<TransportReadBack> {
        self.deny(
            Blocked::ReadBack,
            vec![TransportReadBack::NoAnswer; endpoints.len()],
        )
    }
    async fn size_class_over_known_limits(
        &self,
        _: &TransportMessage,
        _: &TransportPublishTarget,
    ) -> Option<EventSizeClass> {
        #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
        if self.absent_size_reports {
            return None;
        }
        self.deny(Blocked::SizeCheck, None)
    }
    async fn commit_size_refusals(
        &self,
        _: &TransportMessage,
        _: &TransportPublishTarget,
    ) -> Vec<TransportEndpointFailure> {
        #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
        if self.absent_size_reports {
            return Vec::new();
        }
        self.deny(Blocked::CommitSizeCheck, Vec::new())
    }
}

#[async_trait]
impl KeyPackagePublisher for RefusingIo {
    fn legacy_slot_id(&self, _: &MemberId) -> Result<Option<String>, KeyPackagePublishError> {
        self.deny(Blocked::PackageSlot, Err(package_refused()))
    }
    async fn prepare_key_package(
        &self,
        _: KeyPackagePublication,
    ) -> Result<cgka_traits::maintenance::SignedPublicationArtifact, KeyPackagePublishError> {
        self.deny(Blocked::PreparePackage, Err(package_refused()))
    }
    async fn publish_prepared_key_package(
        &self,
        _: &KeyPackagePublication,
        _: &cgka_traits::maintenance::SignedPublicationArtifact,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        self.deny(Blocked::PublishPackage, Err(package_refused()))
    }
    async fn withdraw_key_package_slot(
        &self,
        _: &KeyPackageSlotWithdrawal,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        self.deny(Blocked::WithdrawSlot, Err(package_refused()))
    }
    async fn delete_key_package_event(
        &self,
        _: &KeyPackageEventDeletion,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        self.deny(Blocked::DeletePackage, Err(package_refused()))
    }
}

fn package_refused() -> KeyPackagePublishError {
    KeyPackagePublishError::unexposed("peer_join_fixture_refused")
}
