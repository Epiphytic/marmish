//! Exact recipient-side readback of a frozen native acceptance receipt.

use std::path::Path;

use cgka_traits::{
    FanoutMlsState, FanoutTargetStatus, GroupId, GroupLifecycleState, OutboundFanout,
    OutboundFanoutStorage, StorageError, TransportAdapter, TransportPublishRequest,
    TransportPublishTarget,
    app_components::{NOSTR_ROUTING_COMPONENT_ID, decode_nostr_routing_v1},
};
use marmot_session::{
    AccountDeviceRuntime, KeyPackagePublisher, PublishedApplicationMessage, SessionError,
    TransportRoutingPolicy,
};
use sha2::{Digest, Sha256};
use transport_nostr_peeler::NostrTransportEvent;

use super::{
    postjoin_verify::check_image,
    store::consent_checkpoint::provisioning::prepared_consent::custody::publication::{
        CandidateBinding, Kind, Publication, Step,
    },
};

const MAX_SIGNED_BYTES: usize = 65_536;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Binding,
    Expired,
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

struct Evidence<'a> {
    publication: &'a Publication,
    request: TransportPublishRequest,
    binding: CandidateBinding,
    observed_at: u64,
}

pub(crate) struct CapturedReceipt<'a>(Evidence<'a>);

impl CapturedReceipt<'_> {
    pub(crate) fn publication(&self) -> &Publication {
        self.0.publication
    }

    pub(crate) fn observed_at(&self) -> u64 {
        self.0.observed_at
    }

    pub(crate) fn event_id(&self) -> [u8; 32] {
        self.0.binding.event_id
    }

    pub(crate) fn signed_digest(&self) -> [u8; 32] {
        self.0.binding.signed_digest
    }
}

pub(crate) struct RecoveredReceipt<'a>(Evidence<'a>);

impl RecoveredReceipt<'_> {
    pub(crate) fn publication(&self) -> &Publication {
        self.0.publication
    }

    pub(crate) fn request(&self) -> &TransportPublishRequest {
        &self.0.request
    }

    pub(crate) fn observed_at(&self) -> u64 {
        self.0.observed_at
    }

    pub(crate) fn event_id(&self) -> [u8; 32] {
        self.0.binding.event_id
    }

    pub(crate) fn signed_digest(&self) -> [u8; 32] {
        self.0.binding.signed_digest
    }
}

pub(crate) struct ConfirmedReceipt<'a>(Evidence<'a>);

impl ConfirmedReceipt<'_> {
    pub(crate) fn publication(&self) -> &Publication {
        self.0.publication
    }

    pub(crate) fn observed_at(&self) -> u64 {
        self.0.observed_at
    }

    pub(crate) fn event_id(&self) -> [u8; 32] {
        self.0.binding.event_id
    }

    pub(crate) fn signed_digest(&self) -> [u8; 32] {
        self.0.binding.signed_digest
    }
}

pub(crate) fn capture<'a>(
    recipient: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    image: &Path,
    publication: &'a Publication,
    wall: u64,
) -> Result<CapturedReceipt<'a>, Refusal> {
    if publication.step() != Step::NativeStarted || publication.candidate().is_some() {
        return Err(Refusal::Binding);
    }
    let (evidence, fanout) = read(recipient, image, publication, wall)?;
    require_closed_capture(&fanout)?;
    Ok(CapturedReceipt(evidence))
}

pub(crate) fn check_captured(
    recipient: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    image: &Path,
    publication: &Publication,
    wall: u64,
) -> Result<TransportPublishRequest, Refusal> {
    if publication.step() != Step::Captured {
        return Err(Refusal::Binding);
    }
    let (evidence, fanout) = read(recipient, image, publication, wall)?;
    require_closed_capture(&fanout)?;
    if publication.candidate() != Some(evidence.binding) {
        return Err(Refusal::Binding);
    }
    Ok(evidence.request)
}

fn require_closed_capture(fanout: &OutboundFanout) -> Result<(), Refusal> {
    if fanout.target_statuses() != [FanoutTargetStatus::RetryableUnavailable]
        || fanout.target_attempt_count(0) != 1
        || fanout
            .target_failure(0)
            .is_none_or(|failure| !failure.pre_send)
        || fanout.target_may_hold_event(0)
        || fanout.first_acceptance_observation().is_some()
    {
        return Err(Refusal::Binding);
    }
    Ok(())
}

pub(crate) fn recover<'a>(
    recipient: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    image: &Path,
    publication: &'a Publication,
    wall: u64,
) -> Result<RecoveredReceipt<'a>, Refusal> {
    if publication.step() != (Step::Uncertain { number: 1 }) {
        return Err(Refusal::Binding);
    }
    let (evidence, fanout) = read(recipient, image, publication, wall)?;
    let interrupted = match fanout.target_statuses() {
        [FanoutTargetStatus::Attempting] => true,
        [FanoutTargetStatus::PossiblyExposed] => fanout
            .target_failure(0)
            .is_some_and(|failure| !failure.pre_send),
        _ => false,
    };
    if publication.candidate() != Some(evidence.binding)
        || !interrupted
        || fanout.target_attempt_count(0) != 2
        || !ordered_attempt(publication, &fanout, wall)
        || !fanout.target_may_hold_event(0)
        || fanout.first_acceptance_observation().is_some()
    {
        return Err(Refusal::Binding);
    }
    Ok(RecoveredReceipt(evidence))
}

pub(crate) fn confirm<'a>(
    recipient: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    image: &Path,
    publication: &'a Publication,
    wall: u64,
) -> Result<ConfirmedReceipt<'a>, Refusal> {
    let (Step::Armed { number } | Step::Uncertain { number }) = publication.step() else {
        return Err(Refusal::Binding);
    };
    let (evidence, fanout) = read(recipient, image, publication, wall)?;
    accepted_application(&evidence, &fanout, number)?;
    Ok(ConfirmedReceipt(evidence))
}

pub(crate) fn acknowledge_published(
    recipient: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    image: &Path,
    publication: &Publication,
    wall: u64,
) -> Result<(), Refusal> {
    let Step::Published { number } = publication.step() else {
        return Err(Refusal::Binding);
    };
    if publication.candidate().is_none() || publication.attempt_wall().is_none() {
        return Err(Refusal::Binding);
    }
    // MDK deletes the fanout after local acknowledgement. The checked native
    // group and durable Published row still authorize an idempotent no-op.
    let Some((evidence, fanout)) = read_optional(recipient, image, publication, wall)? else {
        return Ok(());
    };
    let application = accepted_application(&evidence, &fanout, number)?;
    recipient.acknowledge_published_app_messages(&[application])?;
    Ok(())
}

fn accepted_application(
    evidence: &Evidence<'_>,
    fanout: &OutboundFanout,
    number: u8,
) -> Result<PublishedApplicationMessage, Refusal> {
    let publication = evidence.publication;
    let wall = evidence.observed_at;
    let outcome = fanout.outcome();
    if publication.candidate() != Some(evidence.binding)
        || fanout.target_statuses() != [FanoutTargetStatus::Accepted]
        || fanout.target_attempt_count(0) != u32::from(number) + 1
        || fanout.published_message_id() != Some(&evidence.request.message.id)
        || !outcome.fanout_complete
        || outcome.accepted_targets != 1
        || outcome.mls_confirmation_required
    {
        return Err(Refusal::Binding);
    }
    let accepted = fanout
        .first_acceptance_observation()
        .ok_or(Refusal::Binding)?;
    if !ordered_attempt(publication, fanout, wall)
        || fanout
            .target_last_attempt_at_ms(0)
            .is_none_or(|attempted| accepted.at_ms < attempted)
        || accepted.at_ms / 1_000 > wall
    {
        return Err(Refusal::Binding);
    }
    let application = fanout.application_message().ok_or(Refusal::Binding)?;
    Ok(PublishedApplicationMessage {
        publication_observation: Some(accepted),
        app_event_id: application.app_event_id.clone(),
        group_id: application.group_id.clone(),
        message_id: outcome.message_id,
        source_epoch: application.source_epoch,
        retention: application.retention,
        authority: application.authority,
    })
}

fn read<'a>(
    recipient: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    image: &Path,
    publication: &'a Publication,
    wall: u64,
) -> Result<(Evidence<'a>, OutboundFanout), Refusal> {
    read_optional(recipient, image, publication, wall)?.ok_or(Refusal::Binding)
}

fn read_optional<'a>(
    recipient: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    image: &Path,
    publication: &'a Publication,
    wall: u64,
) -> Result<Option<(Evidence<'a>, OutboundFanout)>, Refusal> {
    if wall >= publication.deadline() {
        return Err(Refusal::Expired);
    }
    if wall < publication.created_at()
        || publication.kind() != Kind::AcceptReceipt
        || publication.target_pin().is_some()
        || recipient.self_id().as_slice() != publication.recipient().to_bytes()
    {
        return Err(Refusal::Binding);
    }
    let (_, file) = publication.stage();
    let inode = (
        u64::from_be_bytes(file.device),
        u64::from_be_bytes(file.inode),
    );
    check_image(image, inode).map_err(|_| Refusal::Binding)?;
    let group = GroupId::new(publication.group());
    let fanout = recipient
        .group_authority_view()
        .with_group_authority_snapshot(
            &group,
            |storage, snapshot| -> Result<Option<OutboundFanout>, Refusal> {
                let pin = recipient.epoch_pin(&group).map_err(|_| Refusal::Storage)?;
                let expected = publication.source_pin();
                if snapshot.lifecycle != GroupLifecycleState::Stable
                    || snapshot.facts.epoch != pin.epoch
                    || !snapshot.facts.is_member
                    || snapshot.disbanding
                    || pin.epoch.0 != expected.ordinal
                    || pin.identity != expected.authenticator
                {
                    return Err(Refusal::Binding);
                }
                let membership = recipient.canonical_group_membership(&group)?;
                if !membership.local_leaf_active
                    || membership.member_leaves.len() != 2
                    || !membership.member_leaves.iter().any(|leaf| {
                        leaf.leaf_index == expected.recipient_leaf
                            && leaf.member.as_slice() == publication.recipient().to_bytes()
                    })
                    || !membership.member_leaves.iter().any(|leaf| {
                        leaf.leaf_index != expected.recipient_leaf
                            && leaf.member.as_slice() == publication.founder().to_bytes()
                    })
                {
                    return Err(Refusal::Binding);
                }
                let bytes = recipient
                    .app_component(&group, NOSTR_ROUTING_COMPONENT_ID)
                    .map_err(|_| Refusal::Storage)?
                    .ok_or(Refusal::Binding)?;
                let route = decode_nostr_routing_v1(&bytes).map_err(|_| Refusal::Binding)?;
                if route.nostr_group_id != publication.route()
                    || route.relays.as_slice() != [publication.origin()]
                {
                    return Err(Refusal::Binding);
                }
                let mut fanouts = storage.list_outbound_fanouts()?;
                fanouts.retain(|entry| entry.group_id() == Some(&group));
                if fanouts.len() > 1 {
                    return Err(Refusal::Binding);
                }
                Ok(fanouts.pop())
            },
        )?;
    check_image(image, inode).map_err(|_| Refusal::Binding)?;
    let Some(fanout) = fanout else {
        return Ok(None);
    };
    let (request, binding) = read_request(publication, &group, &fanout)?;
    Ok(Some((
        Evidence {
            publication,
            request,
            binding,
            observed_at: wall,
        },
        fanout,
    )))
}

fn ordered_attempt(publication: &Publication, fanout: &OutboundFanout, wall: u64) -> bool {
    publication.attempt_wall().is_some_and(|armed| {
        fanout
            .target_last_attempt_at_ms(0)
            .is_some_and(|attempted| armed <= attempted / 1_000 && attempted / 1_000 <= wall)
    })
}

fn read_request(
    publication: &Publication,
    group: &GroupId,
    fanout: &OutboundFanout,
) -> Result<(TransportPublishRequest, CandidateBinding), Refusal> {
    let request = fanout.request();
    let TransportPublishTarget::Group {
        group_id,
        transport_group_id,
        endpoints,
    } = &request.target
    else {
        return Err(Refusal::Binding);
    };
    let [endpoint] = endpoints.as_slice() else {
        return Err(Refusal::Binding);
    };
    let application = fanout.application_message().ok_or(Refusal::Binding)?;
    if request.account_id.as_slice() != publication.recipient().to_bytes()
        || group_id != group
        || transport_group_id.as_slice() != publication.route()
        || endpoint.as_str() != publication.origin()
        || request.required_acks > 1
        || request.validate_envelope_matches_target().is_err()
        || !(1..=MAX_SIGNED_BYTES).contains(&request.message.payload.len())
        || fanout.mls_state() != FanoutMlsState::NotApplicable
        || fanout.pending_ref().is_some()
        || fanout.pending_kind().is_some()
        || fanout.queued_intent_id().is_some()
        || fanout.host_change_kind().is_some()
        || application.group_id != *group
        || application.source_epoch.0 != publication.source_pin().ordinal
        || application.app_event_id != hex::encode(publication.semantic_id())
        || fanout
            .published_message_id()
            .is_some_and(|id| id != &request.message.id)
    {
        return Err(Refusal::Binding);
    }
    let event = NostrTransportEvent::from_transport_message(&request.message)
        .map_err(|_| Refusal::Binding)?;
    let signed = event
        .to_verified_nostr_event()
        .map_err(|_| Refusal::Binding)?;
    if event.to_transport_message().map_err(|_| Refusal::Binding)? != request.message
        || signed.id.to_bytes().as_slice() != request.message.id.as_slice()
    {
        return Err(Refusal::Binding);
    }
    let binding = CandidateBinding {
        event_id: signed.id.to_bytes(),
        signed_digest: Sha256::digest(&request.message.payload).into(),
    };
    // Native sends each target with one required acknowledgement even when the
    // durable wrapper uses the default acknowledgement count.
    let mut request = request.clone();
    request.required_acks = 1;
    Ok((request, binding))
}
