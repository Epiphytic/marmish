//! Closed-stage authority for one exact native PostJoin publication.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use cgka_traits::app_components::{
    NOSTR_ROUTING_COMPONENT_ID, NostrRoutingV1, decode_nostr_routing_v1,
};
use cgka_traits::engine::{EpochPin, GroupMemberLeaf};
use cgka_traits::engine_state::PendingStateRef;
use cgka_traits::group::InvitationAdmission;
use cgka_traits::maintenance::{
    DurableGroupEvolution, GroupEvolutionPhase, GroupEvolutionSemantic, MaintenanceObligation,
    MaintenancePhase, MaintenanceTrigger, PostJoinHistoryFetchOutcome,
};
use cgka_traits::storage::{
    GroupStorage, MaintenanceStorage, OutboundFanoutStorage, StorageProvider,
};
use cgka_traits::transport_adapter::{FanoutPendingKind, TransportEndpointFailureKind};
use cgka_traits::{
    EpochId, EpochState, FanoutTargetStatus, GroupId, GroupLifecycleState, MemberId, MessageId,
    OutboundFanout, StorageError, TransportAdapter, TransportPublishRequest,
    TransportPublishTarget,
};
use marmot_session::{
    AccountDeviceRuntime, KeyPackagePublisher, PendingCommit, PendingCommitState, SessionError,
    TransportRoutingPolicy,
};
use nostr::key::PublicKey;
use openmls::prelude::{GroupId as MlsGroupId, MlsGroup};
use sha2::{Digest, Sha256};
use transport_nostr_peeler::NostrTransportEvent;

const MAX_SIGNED_EVENT_BYTES: usize = 65_536;
const MAX_REFUSALS: u32 = 2;

pub(super) struct Scope<'a> {
    pub(super) image: &'a Path,
    pub(super) inode: (u64, u64),
    pub(super) group: &'a GroupId,
    pub(super) account: PublicKey,
    pub(super) founder: PublicKey,
    pub(super) route: &'a NostrRoutingV1,
    pub(super) generation: u64,
    pub(super) deadline: u64,
    pub(super) welcome: &'a MessageId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Refusal {
    Invalid,
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

type Result<T> = std::result::Result<T, Refusal>;

#[derive(Debug, Eq, PartialEq)]
struct Authority {
    pin: EpochPin,
    leaves: Vec<GroupMemberLeaf>,
    local_leaf: u32,
    leaf_hash: [u8; 32],
    route: NostrRoutingV1,
    admission: InvitationAdmission,
}

struct Rows {
    authority: Authority,
    obligation: MaintenanceObligation,
    evolution: Option<DurableGroupEvolution>,
    fanout: Option<OutboundFanout>,
    pending: Option<PendingCommit>,
    native_pending: Option<NativePending>,
}

struct NativePending {
    target: EpochPin,
    reference: PendingStateRef,
}

pub(super) struct Baseline {
    image: PathBuf,
    inode: (u64, u64),
    group: GroupId,
    account: MemberId,
    founder: MemberId,
    generation: u64,
    deadline: u64,
    wall: u64,
    authority: Authority,
    obligation: MessageId,
}

impl Baseline {
    pub(super) fn obligation_id(&self) -> &MessageId {
        &self.obligation
    }

    fn check_image(&self, wall: u64) -> Result<()> {
        if wall < self.wall || wall >= self.deadline {
            return Err(Refusal::Expired);
        }
        check_image(&self.image, self.inode)
    }

    fn next_epoch(&self) -> Result<EpochId> {
        self.authority
            .pin
            .epoch
            .0
            .checked_add(1)
            .map(EpochId)
            .ok_or(Refusal::Invalid)
    }

    fn owns_authority(&self, authority: &Authority) -> bool {
        self.authority.leaves == authority.leaves
            && self.authority.local_leaf == authority.local_leaf
            && self.authority.route == authority.route
            && self.authority.admission == authority.admission
    }
}

/// Only a coherent retained native cut can produce this capability.
pub(super) struct Capture {
    request: TransportPublishRequest,
    evolution: MessageId,
    obligation: MessageId,
    pending: PendingStateRef,
    target: EpochPin,
    generation: u64,
    deadline: u64,
    attempts: u32,
    first_attempt_ms: u64,
    last_attempt_ms: u64,
}

impl Capture {
    pub(super) fn request(&self) -> &TransportPublishRequest {
        &self.request
    }

    pub(super) fn evolution_id(&self) -> &MessageId {
        &self.evolution
    }

    pub(super) fn obligation_id(&self) -> &MessageId {
        &self.obligation
    }

    pub(super) fn pending_ref(&self) -> &PendingStateRef {
        &self.pending
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn deadline(&self) -> u64 {
        self.deadline
    }

    pub(super) fn attempts(&self) -> u32 {
        self.attempts
    }

    fn same_publication(&self, other: &Self) -> bool {
        self.request == other.request
            && self.evolution == other.evolution
            && self.obligation == other.obligation
            && self.pending == other.pending
            && self.target == other.target
            && self.generation == other.generation
            && self.deadline == other.deadline
            && self.first_attempt_ms == other.first_attempt_ms
    }
}

pub(super) fn pre_capture<A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    scope: Scope<'_>,
    wall: u64,
) -> Result<Baseline>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
    F: TransportAdapter,
    S: TransportRoutingPolicy,
    Q: KeyPackagePublisher,
{
    if wall >= scope.deadline {
        return Err(Refusal::Expired);
    }
    check_image(scope.image, scope.inode)?;
    let account = MemberId::new(scope.account.to_bytes().to_vec());
    let founder_id = MemberId::new(scope.founder.to_bytes().to_vec());
    if account == founder_id || scope.route.relays.len() != 1 || scope.generation == 0 {
        return Err(Refusal::Invalid);
    }
    let rows = read_rows(runtime, scope.group, &account, GroupLifecycleState::Stable)?;
    let authority = rows.authority;
    let obligation = rows.obligation;
    if authority.admission.created_locally
        || authority.admission.via_welcome.as_ref() != Some(scope.welcome)
        || authority.route != *scope.route
        || obligation.phase != MaintenancePhase::Jitter
        || obligation.not_before.is_none_or(|time| time.0 > wall)
        || obligation.own_leaf_baseline_hash.as_deref() != Some(authority.leaf_hash.as_slice())
        || obligation.attempt_count != 0
        || obligation.semantic_rearm_count != 0
        || obligation.last_failure_code.is_some()
        || rows.evolution.is_some()
        || rows.fanout.is_some()
        || rows.pending.is_some()
    {
        return Err(Refusal::Invalid);
    }
    verify_founder(founder, scope.group, &founder_id, &authority)?;
    check_image(scope.image, scope.inode)?;
    Ok(Baseline {
        image: scope.image.to_owned(),
        inode: scope.inode,
        group: scope.group.clone(),
        account,
        founder: founder_id,
        generation: scope.generation,
        deadline: scope.deadline,
        wall,
        authority,
        obligation: obligation.id,
    })
}

pub(super) fn post_capture<A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    baseline: &Baseline,
    candidate: &TransportPublishRequest,
    refusals: u32,
    wall: u64,
) -> Result<Capture>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
    F: TransportAdapter,
    S: TransportRoutingPolicy,
    Q: KeyPackagePublisher,
{
    if refusals != 1 {
        return Err(Refusal::Invalid);
    }
    let capture = verify_pending(runtime, baseline, candidate, wall)?;
    if capture.attempts != refusals {
        return Err(Refusal::Invalid);
    }
    verify_founder(
        founder,
        &baseline.group,
        &baseline.founder,
        &baseline.authority,
    )?;
    Ok(capture)
}

pub(super) fn reopen<A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    baseline: &Baseline,
    prior: &Capture,
    wall: u64,
) -> Result<Capture>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
    F: TransportAdapter,
    S: TransportRoutingPolicy,
    Q: KeyPackagePublisher,
{
    let capture = verify_pending(runtime, baseline, &prior.request, wall)?;
    if !capture.same_publication(prior)
        || !(prior.attempts..=prior.attempts + 1).contains(&capture.attempts)
        || capture.last_attempt_ms < prior.last_attempt_ms
        || (capture.attempts == prior.attempts && capture.last_attempt_ms != prior.last_attempt_ms)
    {
        return Err(Refusal::Invalid);
    }
    verify_founder(
        founder,
        &baseline.group,
        &baseline.founder,
        &baseline.authority,
    )?;
    Ok(capture)
}

pub(super) fn post_confirm<A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    baseline: &Baseline,
    capture: &Capture,
    wall: u64,
) -> Result<()>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
    F: TransportAdapter,
    S: TransportRoutingPolicy,
    Q: KeyPackagePublisher,
{
    baseline.check_image(wall)?;
    let rows = read_rows(
        runtime,
        &baseline.group,
        &baseline.account,
        GroupLifecycleState::Stable,
    )?;
    let evolution = exact_evolution(rows.evolution.as_ref(), baseline)?;
    if !baseline.owns_authority(&rows.authority)
        || rows.authority.pin != capture.target
        || rows.authority.pin.epoch != baseline.next_epoch()?
        || rows.authority.leaf_hash == baseline.authority.leaf_hash
        || !baseline.obligation_matches(&rows.obligation, MaintenancePhase::Complete)
        || rows.fanout.is_some()
        || rows.pending.is_some()
        || evolution.phase != GroupEvolutionPhase::Confirmed
        || evolution.id != capture.evolution
        || evolution.signed_message_id.as_ref() != Some(&capture.request.message.id)
        || evolution.pending_ref != Some(capture.pending)
        || capture.obligation != baseline.obligation
        || capture.generation != baseline.generation
        || capture.deadline != baseline.deadline
    {
        return Err(Refusal::Invalid);
    }
    verify_founder(founder, &baseline.group, &baseline.founder, &rows.authority)?;
    baseline.check_image(wall)
}

impl Baseline {
    fn obligation_matches(
        &self,
        obligation: &MaintenanceObligation,
        phase: MaintenancePhase,
    ) -> bool {
        obligation.id == self.obligation
            && obligation.phase == phase
            && obligation.own_leaf_baseline_hash.as_deref()
                == Some(self.authority.leaf_hash.as_slice())
            && obligation.semantic_rearm_count == 0
    }
}

fn verify_pending<A, R, K>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    baseline: &Baseline,
    candidate: &TransportPublishRequest,
    wall: u64,
) -> Result<Capture>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
{
    baseline.check_image(wall)?;
    let rows = read_rows(
        runtime,
        &baseline.group,
        &baseline.account,
        GroupLifecycleState::PendingPublish,
    )?;
    let evolution = exact_evolution(rows.evolution.as_ref(), baseline)?;
    let fanout = rows.fanout.as_ref().ok_or(Refusal::Invalid)?;
    let pending = rows.pending.as_ref().ok_or(Refusal::Invalid)?;
    if rows.authority != baseline.authority
        || !baseline.obligation_matches(&rows.obligation, MaintenancePhase::PendingPublication)
        || !matches!(
            evolution.phase,
            GroupEvolutionPhase::Prepared | GroupEvolutionPhase::Attempting
        )
        || evolution.signed_message_id.as_ref() != Some(&candidate.message.id)
    {
        return Err(Refusal::Invalid);
    }
    let pending_ref = evolution.pending_ref.ok_or(Refusal::Invalid)?;
    let native = rows.native_pending.ok_or(Refusal::Invalid)?;
    if native.reference != pending_ref || native.target.epoch != evolution.target_epoch {
        return Err(Refusal::Invalid);
    }
    let capture = verify_fanout(baseline, fanout, pending, pending_ref, candidate, wall)?;
    baseline.check_image(wall)?;
    Ok(Capture {
        request: candidate.clone(),
        evolution: evolution.id.clone(),
        obligation: baseline.obligation.clone(),
        pending: pending_ref,
        target: native.target,
        generation: baseline.generation,
        deadline: baseline.deadline,
        attempts: capture.0,
        first_attempt_ms: capture.1,
        last_attempt_ms: capture.2,
    })
}

fn exact_evolution<'a>(
    evolution: Option<&'a DurableGroupEvolution>,
    baseline: &Baseline,
) -> Result<&'a DurableGroupEvolution> {
    let evolution = evolution.ok_or(Refusal::Invalid)?;
    if evolution.group_id != baseline.group
        || evolution.source_epoch != baseline.authority.pin.epoch
        || evolution.target_epoch != baseline.next_epoch()?
        || evolution.own_leaf_before_hash.as_deref()
            != Some(baseline.authority.leaf_hash.as_slice())
        || !evolution.removal_members.is_empty()
        || !matches!(
            &evolution.semantic,
            GroupEvolutionSemantic::SelfUpdate {
                trigger: MaintenanceTrigger::PostJoin,
                obligation_id: Some(id),
            } if id == &baseline.obligation
        )
    {
        return Err(Refusal::Invalid);
    }
    Ok(evolution)
}

fn verify_fanout(
    baseline: &Baseline,
    fanout: &OutboundFanout,
    pending: &PendingCommit,
    pending_ref: PendingStateRef,
    candidate: &TransportPublishRequest,
    wall: u64,
) -> Result<(u32, u64, u64)> {
    let durable = fanout.request();
    let TransportPublishTarget::Group {
        group_id,
        transport_group_id,
        endpoints,
    } = &durable.target
    else {
        return Err(Refusal::Invalid);
    };
    let [endpoint] = endpoints.as_slice() else {
        return Err(Refusal::Invalid);
    };
    let [relay] = baseline.authority.route.relays.as_slice() else {
        return Err(Refusal::Invalid);
    };
    if durable.account_id != baseline.account
        || group_id != &baseline.group
        || transport_group_id.as_slice() != baseline.authority.route.nostr_group_id
        || &endpoint.0 != relay
        || durable.required_acks > 1
        || !(1..=MAX_SIGNED_EVENT_BYTES).contains(&durable.message.payload.len())
        || !(1..=MAX_SIGNED_EVENT_BYTES).contains(&candidate.message.payload.len())
        || durable.account_id != candidate.account_id
        || durable.target != candidate.target
        || durable.message != candidate.message
        || candidate.required_acks != 1
    {
        return Err(Refusal::Invalid);
    }

    let event = NostrTransportEvent::from_transport_message(&durable.message)
        .map_err(|_| Refusal::Invalid)?;
    let signed = event
        .to_verified_nostr_event()
        .map_err(|_| Refusal::Invalid)?;
    let wire = event.to_transport_message().map_err(|_| Refusal::Invalid)?;
    if wire != durable.message
        || signed.id.to_bytes().as_slice() != durable.message.id.as_slice()
        || durable.validate_envelope_matches_target().is_err()
    {
        return Err(Refusal::Invalid);
    }

    let failure = fanout.target_failure(0).ok_or(Refusal::Invalid)?;
    let attempts = fanout.target_attempt_count(0);
    let first = fanout.first_attempt_at_ms().ok_or(Refusal::Invalid)?;
    let last = fanout
        .target_last_attempt_at_ms(0)
        .ok_or(Refusal::Invalid)?;
    if fanout.group_id() != Some(&baseline.group)
        || fanout.pending_ref() != Some(pending_ref)
        || fanout.pending_kind() != Some(FanoutPendingKind::GroupEvolution)
        || fanout.pending_origin_message_id() != Some(&durable.message.id)
        || fanout
            .published_message_id()
            .is_some_and(|id| id != &durable.message.id)
        || fanout.first_acceptance_observation().is_some()
        || fanout.queued_intent_id().is_some()
        || fanout.host_change_kind().is_some()
        || fanout.application_message().is_some()
        || !fanout.pending_post_confirmation_welcomes().is_empty()
        || fanout.target_statuses() != [FanoutTargetStatus::RetryableUnavailable]
        || fanout.possible_exposure()
        || !failure.pre_send
        || failure.kind != TransportEndpointFailureKind::RetryableUnavailable
        || &failure.endpoint != endpoint
        || !(1..=MAX_REFUSALS).contains(&attempts)
        || first > last
        || first / 1_000 < baseline.wall
        || last / 1_000 > wall
        || pending.message_id != durable.message.id
        || pending.origin.is_some()
        || pending.intent_kind.is_some()
        || pending.state != PendingCommitState::Sending
        || pending.endpoint_count != 1
        || pending.owed_endpoints.as_slice() != endpoints
        || !pending.unconfirmed_endpoints.is_empty()
        || !pending.refused_endpoints.is_empty()
    {
        return Err(Refusal::Invalid);
    }
    Ok((attempts, first, last))
}

fn read_pending_epoch<A, R, K>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    group: &GroupId,
    lifecycle: GroupLifecycleState,
) -> Result<Option<NativePending>>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
{
    if lifecycle != GroupLifecycleState::PendingPublish {
        return Ok(None);
    }
    let Some(EpochState::PendingPublish(state)) = runtime.epoch_state(group) else {
        return Err(Refusal::Invalid);
    };
    // MDK's projected pin names the staged branch while its live MLS group
    // still owns the canonical source. Bind both authorities independently.
    let target = runtime.epoch_pin(group).map_err(|_| Refusal::Invalid)?;
    if target.epoch != state.epoch() {
        return Err(Refusal::Invalid);
    }
    Ok(Some(NativePending {
        target,
        reference: state.pending_ref(),
    }))
}

fn canonical_pin(
    storage: &storage_sqlite::SqliteAccountStorage,
    group: &GroupId,
) -> Result<EpochPin> {
    let id = MlsGroupId::from_slice(group.as_slice());
    let native = MlsGroup::load(storage.mls_storage(), &id)
        .map_err(|_| Refusal::Storage)?
        .ok_or(Refusal::Invalid)?;
    if !native.is_active() || native.pending_commit().is_none() || native.group_id() != &id {
        return Err(Refusal::Invalid);
    }
    Ok(EpochPin {
        epoch: EpochId(native.epoch().as_u64()),
        identity: Sha256::digest(native.epoch_authenticator().as_slice()).into(),
    })
}

fn read_rows<A, R, K>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    group: &GroupId,
    account: &MemberId,
    lifecycle: GroupLifecycleState,
) -> Result<Rows>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
{
    runtime
        .group_authority_view()
        .with_group_authority_snapshot(group, |storage, snapshot| {
            let authority = read_authority(runtime, storage, group, account, snapshot, lifecycle)?;
            let obligations = storage.list_maintenance_obligations()?;
            let [obligation] = obligations.as_slice() else {
                return Err(Refusal::Invalid);
            };
            let history = storage
                .group_maintenance(group)?
                .and_then(|state| state.post_join_history)
                .ok_or(Refusal::Invalid)?;
            if obligation.group_id != *group
                || obligation.trigger != MaintenanceTrigger::PostJoin
                || history.obligation_id != obligation.id
                || !history
                    .completion
                    .is_some_and(|result| result.outcome == PostJoinHistoryFetchOutcome::Fetched)
                || !storage.list_transport_fanouts()?.is_empty()
            {
                return Err(Refusal::Invalid);
            }
            let mut evolutions = storage.list_group_evolutions()?;
            let mut fanouts = storage.list_outbound_fanouts()?;
            if evolutions.len() > 1 || fanouts.len() > 1 {
                return Err(Refusal::Invalid);
            }
            Ok(Rows {
                authority,
                obligation: obligation.clone(),
                evolution: evolutions.pop(),
                fanout: fanouts.pop(),
                pending: runtime
                    .pending_commit(group)
                    .map_err(|_| Refusal::Invalid)?,
                native_pending: read_pending_epoch(runtime, group, lifecycle)?,
            })
        })
}

fn verify_founder<A, R, K>(
    founder: &AccountDeviceRuntime<A, R, K>,
    group: &GroupId,
    account: &MemberId,
    recipient: &Authority,
) -> Result<()>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
{
    founder
        .group_authority_view()
        .with_group_authority_snapshot(group, |storage, snapshot| {
            let authority = read_authority(
                founder,
                storage,
                group,
                account,
                snapshot,
                GroupLifecycleState::Stable,
            )?;
            if authority.pin != recipient.pin
                || authority.leaves != recipient.leaves
                || authority.local_leaf == recipient.local_leaf
                || authority.route != recipient.route
                || !authority.admission.created_locally
                || authority.admission.via_welcome.is_some()
            {
                return Err(Refusal::Invalid);
            }
            Ok(())
        })
}

fn read_authority<A, R, K>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    storage: &storage_sqlite::SqliteAccountStorage,
    group: &GroupId,
    account: &MemberId,
    snapshot: marmot_session::GroupAuthoritySnapshot,
    lifecycle: GroupLifecycleState,
) -> Result<Authority>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
{
    let pin = match lifecycle {
        GroupLifecycleState::PendingPublish => canonical_pin(storage, group)?,
        _ => runtime.epoch_pin(group).map_err(|_| Refusal::Invalid)?,
    };
    let canonical = runtime
        .canonical_group_membership(group)
        .map_err(|_| Refusal::Invalid)?;
    if runtime.self_id() != *account
        || snapshot.lifecycle != lifecycle
        || snapshot.facts.epoch != pin.epoch
        || !snapshot.facts.is_member
        || snapshot.disbanding
        || !canonical.local_leaf_active
        || canonical.member_leaves.len() != 2
    {
        return Err(Refusal::Invalid);
    }
    let mut leaves = canonical.member_leaves;
    leaves.sort_by_key(|leaf| leaf.leaf_index);
    let mut local = leaves.iter().filter(|leaf| &leaf.member == account);
    let local_leaf = local.next().ok_or(Refusal::Invalid)?.leaf_index;
    if local.next().is_some() {
        return Err(Refusal::Invalid);
    }
    let leaf_hash = runtime
        .session()
        .own_leaf_hash(group)?
        .try_into()
        .map_err(|_| Refusal::Invalid)?;
    let component = runtime
        .app_component(group, NOSTR_ROUTING_COMPONENT_ID)
        .map_err(|_| Refusal::Invalid)?
        .ok_or(Refusal::Invalid)?;
    let route = decode_nostr_routing_v1(&component).map_err(|_| Refusal::Invalid)?;
    let admission = storage
        .invitation_admission(group)?
        .ok_or(Refusal::Invalid)?;
    Ok(Authority {
        pin,
        leaves,
        local_leaf,
        leaf_hash,
        route,
        admission,
    })
}

fn check_image(image: &Path, inode: (u64, u64)) -> Result<()> {
    let metadata = fs::symlink_metadata(image).map_err(|_| Refusal::Storage)?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
        || (metadata.dev(), metadata.ino()) != inode
        || !(1..=super::paths::MAX_STAGE_LEN).contains(&metadata.len())
    {
        return Err(Refusal::Invalid);
    }
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut path = image.as_os_str().to_owned();
        path.push(suffix);
        if !matches!(fs::symlink_metadata(path), Err(err) if err.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(Refusal::Invalid);
        }
    }
    Ok(())
}
