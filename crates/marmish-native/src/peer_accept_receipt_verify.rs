//! Founder-side source verification for the isolated native activation proof.

use crate::{
    mdk_adapter::peer_staged_proof_tests::PeerFounderRuntime,
    peer_invitation::{
        accept_receipt_wire::{self, Receipt},
        store::consent_checkpoint::provisioning::prepared_consent::custody::publication::FounderAcceptance,
    },
};
use cgka_traits::{
    EpochPin, GroupEvent, GroupId, GroupLifecycleState, MessageId, MessageState, MessageStorage,
    StorageError, TransportAdapter,
    app_components::{NOSTR_ROUTING_COMPONENT_ID, NostrRoutingV1, decode_nostr_routing_v1},
    app_event::MarmotAppEvent,
};
use marmot_session::{
    AccountDeviceRuntime, KeyPackagePublisher, SessionError, TransportRoutingPolicy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

/// The founder constructs this from its own enrollment and confirmed MLS
/// state before receiving any receipt. None of the fields come from the
/// incoming application's claims.
pub(crate) struct Expectation<'a> {
    pub(crate) receipt: &'a Receipt,
    pub(crate) group: &'a GroupId,
    pub(crate) route: &'a NostrRoutingV1,
    pub(crate) recipient_leaf: u32,
    pub(crate) valid_from: u64,
    pub(crate) deadline: u64,
}

pub(crate) struct VerifiedReceiptSource {
    receipt: Receipt,
    group: GroupId,
    route: NostrRoutingV1,
    native_id: MessageId,
    app_id: [u8; 32],
    leaf: u32,
    created_at: u64,
    deadline: u64,
}

impl VerifiedReceiptSource {
    pub(crate) fn receipt(&self) -> &Receipt {
        &self.receipt
    }

    pub(crate) fn group(&self) -> &GroupId {
        &self.group
    }

    pub(crate) fn route(&self) -> &NostrRoutingV1 {
        &self.route
    }

    pub(crate) fn native_id(&self) -> &MessageId {
        &self.native_id
    }

    pub(crate) fn app_id(&self) -> [u8; 32] {
        self.app_id
    }

    pub(crate) fn leaf(&self) -> u32 {
        self.leaf
    }

    pub(crate) fn created_at(&self) -> u64 {
        self.created_at
    }

    pub(crate) fn deadline(&self) -> u64 {
        self.deadline
    }
}

pub(crate) fn pending<A, R, K>(
    founder: &AccountDeviceRuntime<A, R, K>,
    expected: &Expectation<'_>,
    wall: u64,
) -> Result<Option<VerifiedReceiptSource>, Refusal>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
{
    if wall < expected.valid_from {
        return Err(Refusal::Binding);
    }
    if wall >= expected.deadline {
        return Err(Refusal::Expired);
    }
    founder
        .group_authority_view()
        .with_group_authority_snapshot(expected.group, |storage, authority| {
            let pin = founder
                .epoch_pin(expected.group)
                .map_err(|_| Refusal::Binding)?;
            if pin != expected_pin(expected)
                || authority.lifecycle != GroupLifecycleState::Stable
                || authority.facts.epoch != pin.epoch
                || !authority.facts.is_member
                || !authority.facts.is_admin
                || authority.disbanding
                || founder.self_id().as_slice() != expected.receipt.founder.to_bytes()
            {
                return Err(Refusal::Binding);
            }
            let route_bytes = founder
                .app_component(expected.group, NOSTR_ROUTING_COMPONENT_ID)
                .map_err(|_| Refusal::Storage)?
                .ok_or(Refusal::Binding)?;
            if decode_nostr_routing_v1(&route_bytes).map_err(|_| Refusal::Binding)?
                != *expected.route
            {
                return Err(Refusal::Binding);
            }
            let membership = founder.canonical_group_membership(expected.group)?;
            let recipient = expected.receipt.recipient.to_bytes();
            if !membership.local_leaf_active
                || membership.member_leaves.len() != 2
                || !membership.member_leaves.iter().any(|leaf| {
                    leaf.leaf_index == expected.recipient_leaf
                        && leaf.member.as_slice() == recipient
                })
                || !membership.member_leaves.iter().any(|leaf| {
                    leaf.leaf_index != expected.recipient_leaf && leaf.member == founder.self_id()
                })
            {
                return Err(Refusal::Binding);
            }

            let events = storage.list_pending_application_events()?;
            let mut candidates = events
                .iter()
                .filter_map(|event| candidate(event, expected, wall));
            let Some(source) = candidates.next() else {
                return Ok(None);
            };
            if candidates.next().is_some()
                || events.iter().any(|event| {
                    matches!(event, GroupEvent::AppMessageInvalidated { group_id, message_id, .. }
                if group_id == expected.group && message_id == source.native_id())
                })
            {
                return Err(Refusal::Binding);
            }
            let stored = storage.get_message(source.native_id())?;
            if stored.group_id != *expected.group
                || stored.id != *source.native_id()
                || stored.epoch != pin.epoch
                || stored.state != MessageState::Processed
            {
                return Err(Refusal::Binding);
            }
            Ok(Some(source))
        })
}

fn expected_pin(expectation: &Expectation<'_>) -> EpochPin {
    EpochPin {
        epoch: cgka_traits::EpochId(expectation.receipt.epoch),
        identity: expectation.receipt.epoch_identity,
    }
}

fn candidate(
    event: &GroupEvent,
    expected: &Expectation<'_>,
    wall: u64,
) -> Option<VerifiedReceiptSource> {
    let GroupEvent::MessageReceived {
        group_id,
        message_id,
        sender,
        sender_leaf_index: Some(leaf),
        epoch,
        payload,
        ..
    } = event
    else {
        return None;
    };
    if group_id != expected.group
        || sender.as_slice() != expected.receipt.recipient.to_bytes()
        || *leaf != expected.recipient_leaf
        || *epoch != expected_pin(expected).epoch
    {
        return None;
    }
    let decoded = accept_receipt_wire::decode(payload).ok()?;
    if decoded.receipt != *expected.receipt
        || decoded.created_at < expected.valid_from
        || decoded.created_at > wall
        || MarmotAppEvent::decode(payload)
            .ok()?
            .validate_sender(&expected.receipt.recipient.to_hex())
            .is_err()
    {
        return None;
    }
    Some(VerifiedReceiptSource {
        receipt: decoded.receipt,
        group: group_id.clone(),
        route: expected.route.clone(),
        native_id: message_id.clone(),
        app_id: decoded.event_id,
        leaf: *leaf,
        created_at: decoded.created_at,
        deadline: expected.deadline,
    })
}

/// Retire precisely the already committed native source. The founder owner is
/// exclusive during readback and deletion. The original acceptance supplies
/// the epoch and time, so later expiry cannot undo this local cleanup.
pub(crate) fn retire(
    founder: &mut PeerFounderRuntime,
    accepted: &FounderAcceptance,
    expected: &Expectation<'_>,
    native_id: &MessageId,
) -> Result<bool, Refusal> {
    let source = accepted.source();
    let runtime = founder.runtime();
    if source.native_id != native_id.as_slice()
        || source.decoded.receipt != *expected.receipt
        || source.group != expected.group.as_slice()
        || source.route != expected.route.nostr_group_id
        || source.sender_leaf != expected.recipient_leaf
        || source.deadline != expected.deadline
        || runtime.self_id().as_slice() != source.decoded.receipt.founder.to_bytes()
    {
        return Err(Refusal::Binding);
    }
    let pending = runtime
        .group_authority_view()
        .with_group_authority_snapshot(expected.group, |storage, _| -> Result<bool, Refusal> {
            let message = storage.get_message(native_id)?;
            if message.id != *native_id
                || message.group_id != *expected.group
                || message.epoch.0 != source.decoded.receipt.epoch
                || message.state != MessageState::Processed
            {
                return Err(Refusal::Binding);
            }
            let group = expected.group;
            let events = storage.list_pending_application_events()?;
            let mut found = false;
            for event in &events {
                match event {
                    GroupEvent::AppMessageInvalidated {
                        group_id,
                        message_id,
                        ..
                    } if group_id == group && message_id == native_id => {
                        return Err(Refusal::Binding);
                    }
                    GroupEvent::MessageReceived { message_id, .. } if message_id == native_id => {
                        let receipt = candidate(event, expected, accepted.accepted_at())
                            .ok_or(Refusal::Binding)?;
                        if found
                            || receipt.app_id() != source.decoded.event_id
                            || receipt.created_at() != source.decoded.created_at
                        {
                            return Err(Refusal::Binding);
                        }
                        found = true;
                    }
                    _ => {}
                }
            }
            Ok(found)
        })?;
    if pending {
        founder.acknowledge_application_source(native_id)?;
    }
    Ok(pending)
}
