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
    GroupStorage, MaintenanceStorage, MessageStorage, OutboundFanoutStorage, StorageProvider,
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

use super::store::consent_checkpoint::provisioning::prepared_consent::custody::publication::{
    Kind, Pin, Publication, Step, UncertainPostJoin,
};

const MAX_SIGNED_EVENT_BYTES: usize = 65_536;
const MAX_REFUSALS: u32 = 2;
const EXPOSED_NATIVE_ATTEMPTS: u32 = 2;

#[derive(Clone, Copy)]
enum Exposure {
    BeforeSend,
    PossiblyExposed,
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
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

pub(crate) struct Baseline {
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
    welcome: MessageId,
}

impl Baseline {
    pub(super) fn obligation_id(&self) -> &MessageId {
        &self.obligation
    }

    pub(super) fn group(&self) -> &GroupId {
        &self.group
    }

    pub(super) fn recipient(&self) -> &MemberId {
        &self.account
    }

    pub(super) fn founder(&self) -> &MemberId {
        &self.founder
    }

    pub(super) fn route(&self) -> [u8; 32] {
        self.authority.route.nostr_group_id
    }

    pub(super) fn origin(&self) -> &str {
        &self.authority.route.relays[0]
    }

    pub(super) fn inode(&self) -> (u64, u64) {
        self.inode
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn deadline(&self) -> u64 {
        self.deadline
    }

    pub(super) fn welcome(&self) -> &MessageId {
        &self.welcome
    }

    pub(super) fn source(&self) -> &EpochPin {
        &self.authority.pin
    }

    pub(super) fn recipient_leaf(&self) -> u32 {
        self.authority.local_leaf
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
pub(crate) struct Capture {
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

    pub(super) fn event_id(&self) -> [u8; 32] {
        self.request
            .message
            .id
            .as_slice()
            .try_into()
            .expect("verified signed Nostr event ID")
    }

    pub(super) fn signed_digest(&self) -> [u8; 32] {
        Sha256::digest(&self.request.message.payload).into()
    }

    pub(super) fn target(&self) -> &EpochPin {
        &self.target
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
        welcome: scope.welcome.clone(),
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
    let capture = verify_pending(runtime, baseline, candidate, Exposure::BeforeSend, wall)?;
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
    let capture = verify_pending(
        runtime,
        baseline,
        &prior.request,
        Exposure::BeforeSend,
        wall,
    )?;
    if !advances_exact_capture(prior, &capture)
        || !(prior.attempts..=prior.attempts + 1).contains(&capture.attempts)
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

fn advances_exact_capture(prior: &Capture, candidate: &Capture) -> bool {
    candidate.same_publication(prior)
        && candidate.last_attempt_ms >= prior.last_attempt_ms
        && (candidate.attempts != prior.attempts
            || candidate.last_attempt_ms == prior.last_attempt_ms)
}

pub(super) struct RecoveredPostJoin {
    baseline: Baseline,
    capture: Capture,
}

impl RecoveredPostJoin {
    pub(super) fn baseline(&self) -> &Baseline {
        &self.baseline
    }

    pub(super) fn capture(&self) -> &Capture {
        &self.capture
    }
}

/// Recover the frozen native request before the first armed publication.
/// A persisted capture must match the independently reopened native request.
pub(super) fn recover_unexposed<A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    image: &Path,
    publication: &Publication,
    wall: u64,
) -> Result<RecoveredPostJoin>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
    F: TransportAdapter,
    S: TransportRoutingPolicy,
    Q: KeyPackagePublisher,
{
    if wall < publication.created_at() || wall >= publication.deadline() {
        return Err(Refusal::Expired);
    }
    if publication.kind() != Kind::PostJoin
        || !matches!(publication.step(), Step::Reserved | Step::Captured)
        || publication.attempt_wall().is_some()
    {
        return Err(Refusal::Invalid);
    }
    let stage = JournalStage::from_publication(publication);
    check_image(image, stage.inode)?;
    let rows = read_rows(
        runtime,
        &stage.group,
        &stage.account,
        GroupLifecycleState::PendingPublish,
    )?;
    let baseline = stage.baseline(image, publication, &rows)?;
    let capture = bind_frozen_pending(&rows, &baseline, Exposure::BeforeSend, wall)?;
    if capture.attempts != 1 || capture.first_attempt_ms != capture.last_attempt_ms {
        return Err(Refusal::Invalid);
    }
    match publication.step() {
        Step::Reserved
            if publication.candidate().is_none() && publication.target_pin().is_none() => {}
        Step::Captured if matches_captured_publication(publication, &baseline, &capture) => {}
        _ => return Err(Refusal::Invalid),
    }
    verify_founder(
        founder,
        baseline.group(),
        baseline.founder(),
        &rows.authority,
    )?;
    baseline.check_image(wall)?;
    Ok(RecoveredPostJoin { baseline, capture })
}

/// Reconstitute one pending request only after the journal proves that this
/// exact capture was durably armed. The founder must have processed it at
/// the target pin before the owner can offer this capture to the retry gate.
pub(super) fn recover_exposed<A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    image: &Path,
    witness: &UncertainPostJoin,
    wall: u64,
) -> Result<RecoveredPostJoin>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
    F: TransportAdapter,
    S: TransportRoutingPolicy,
    Q: KeyPackagePublisher,
{
    let publication = witness.publication();
    let reserved_at = publication.created_at();
    if wall < reserved_at || wall >= publication.deadline() {
        return Err(Refusal::Expired);
    }

    let armed_at = publication.attempt_wall().ok_or(Refusal::Invalid)?;
    if publication.kind() != Kind::PostJoin
        || publication.step() != (Step::Uncertain { number: 1 })
        || !(reserved_at..=wall).contains(&armed_at)
    {
        return Err(Refusal::Invalid);
    }

    let stage = JournalStage::from_publication(publication);
    check_image(image, stage.inode)?;
    let rows = read_rows(
        runtime,
        &stage.group,
        &stage.account,
        GroupLifecycleState::PendingPublish,
    )?;
    let baseline = stage.baseline(image, publication, &rows)?;
    let capture = bind_frozen_pending(&rows, &baseline, Exposure::PossiblyExposed, wall)?;
    if !matches_captured_publication(publication, &baseline, &capture)
        // The first native invocation froze the request behind the closed
        // gate. Only the first persisted egress can be outstanding here.
        || capture.attempts != EXPOSED_NATIVE_ATTEMPTS
        || capture.first_attempt_ms / 1_000 > armed_at
        || capture.last_attempt_ms / 1_000 < armed_at
    {
        return Err(Refusal::Invalid);
    }
    verify_founder_cut(
        founder,
        baseline.group(),
        baseline.founder(),
        &baseline.authority,
        capture.target(),
        Some(&capture.request().message.id),
    )?;
    baseline.check_image(wall)?;
    Ok(RecoveredPostJoin { baseline, capture })
}

fn matches_captured_publication(
    publication: &Publication,
    baseline: &Baseline,
    capture: &Capture,
) -> bool {
    publication.candidate().is_some_and(|candidate| {
        candidate.event_id == capture.event_id()
            && candidate.signed_digest == capture.signed_digest()
    }) && publication.target_pin().is_some_and(|target| {
        target.ordinal == capture.target().epoch.0
            && target.authenticator == capture.target().identity
            && target.recipient_leaf == baseline.recipient_leaf()
    })
}

struct JournalStage {
    inode: (u64, u64),
    group: GroupId,
    account: MemberId,
    founder: MemberId,
    welcome: MessageId,
}

impl JournalStage {
    fn from_publication(publication: &Publication) -> Self {
        let (_, file) = publication.stage();
        Self {
            inode: (
                u64::from_be_bytes(file.device),
                u64::from_be_bytes(file.inode),
            ),
            group: GroupId::new(publication.group()),
            account: MemberId::new(publication.recipient().to_bytes()),
            founder: MemberId::new(publication.founder().to_bytes()),
            welcome: MessageId::new(publication.welcome_id()),
        }
    }

    fn verify_rows(&self, publication: &Publication, rows: &Rows, expected: Pin) -> Result<()> {
        let authority = &rows.authority;
        if self.account == self.founder
            || authority.admission.created_locally
            || authority.admission.via_welcome.as_ref() != Some(&self.welcome)
            || authority.route.nostr_group_id != publication.route()
            || authority.route.relays.as_slice() != [publication.origin()]
            || authority.pin.epoch.0 != expected.ordinal
            || authority.pin.identity != expected.authenticator
            || authority.local_leaf != expected.recipient_leaf
            || rows.obligation.id.as_slice() != publication.semantic_id()
        {
            return Err(Refusal::Invalid);
        }
        Ok(())
    }

    fn baseline(self, image: &Path, publication: &Publication, rows: &Rows) -> Result<Baseline> {
        self.verify_rows(publication, rows, publication.source_pin())?;
        Ok(Baseline {
            image: image.to_owned(),
            inode: self.inode,
            group: self.group,
            account: self.account,
            founder: self.founder,
            generation: publication.generation(),
            deadline: publication.deadline(),
            wall: publication.created_at(),
            authority: rows.authority.clone(),
            obligation: rows.obligation.id.clone(),
            welcome: self.welcome,
        })
    }
}

fn bind_frozen_pending(
    rows: &Rows,
    baseline: &Baseline,
    exposure: Exposure,
    wall: u64,
) -> Result<Capture> {
    let fanout = rows.fanout.as_ref().ok_or(Refusal::Invalid)?;
    let mut request = fanout.request().clone();
    // MDK's per-endpoint handoff requires one acknowledgment. The frozen
    // request still supplies the exact account, envelope, route and payload.
    request.required_acks = 1;
    bind_pending(rows, baseline, &request, exposure, wall)
}

pub(crate) struct ConfirmedRotation<'a> {
    baseline: &'a Baseline,
    capture: &'a Capture,
}

/// A completed native rotation bound to one exact durable journal turn.
/// Native confirmation discards the frozen request, so this cannot arm egress.
pub(crate) struct ColdConfirmedRotation<'a> {
    publication: &'a Publication,
    observed_at: u64,
}

impl<'a> ColdConfirmedRotation<'a> {
    pub(crate) fn publication(&self) -> &'a Publication {
        self.publication
    }

    pub(crate) fn observed_at(&self) -> u64 {
        self.observed_at
    }
}

pub(super) fn confirm_rotation_cold<'a, A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    image: &Path,
    publication: &'a Publication,
    wall: u64,
) -> Result<ColdConfirmedRotation<'a>>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
    F: TransportAdapter,
    S: TransportRoutingPolicy,
    Q: KeyPackagePublisher,
{
    if wall < publication.created_at() || wall >= publication.deadline() {
        return Err(Refusal::Expired);
    }
    if publication.kind() != Kind::PostJoin
        || !matches!(
            publication.step(),
            Step::Armed { number: 1 | 2 }
                | Step::Uncertain { number: 1 | 2 }
                | Step::Published { number: 1 | 2 }
        )
        || publication
            .attempt_wall()
            .is_none_or(|armed| !(publication.created_at()..=wall).contains(&armed))
    {
        return Err(Refusal::Invalid);
    }

    let source = publication.source_pin();
    let target = publication.target_pin().ok_or(Refusal::Invalid)?;
    let candidate = publication.candidate().ok_or(Refusal::Invalid)?;
    if source.ordinal.checked_add(1) != Some(target.ordinal)
        || source.authenticator == target.authenticator
        || source.recipient_leaf != target.recipient_leaf
    {
        return Err(Refusal::Invalid);
    }
    let stage = JournalStage::from_publication(publication);
    check_image(image, stage.inode)?;
    let rows = read_rows(
        runtime,
        &stage.group,
        &stage.account,
        GroupLifecycleState::Stable,
    )?;
    stage.verify_rows(publication, &rows, target)?;
    let obligation = &rows.obligation;
    let previous_leaf = obligation
        .own_leaf_baseline_hash
        .as_deref()
        .and_then(|hash| <&[u8; 32]>::try_from(hash).ok())
        .ok_or(Refusal::Invalid)?;
    let event_id = MessageId::new(candidate.event_id);
    let evolution = checked_evolution(
        rows.evolution.as_ref(),
        &stage.group,
        EpochId(source.ordinal),
        EpochId(target.ordinal),
        &obligation.id,
        previous_leaf,
    )?;
    if rows.authority.leaf_hash == *previous_leaf
        || obligation.phase != MaintenancePhase::Complete
        || obligation.semantic_rearm_count != 0
        || rows.fanout.is_some()
        || rows.pending.is_some()
        || evolution.phase != GroupEvolutionPhase::Confirmed
        || evolution.signed_message_id.as_ref() != Some(&event_id)
        || evolution.pending_ref.is_none()
    {
        return Err(Refusal::Invalid);
    }
    verify_founder_cut(
        founder,
        &stage.group,
        &stage.founder,
        &rows.authority,
        &rows.authority.pin,
        Some(&event_id),
    )?;
    check_image(image, stage.inode)?;
    Ok(ColdConfirmedRotation {
        publication,
        observed_at: wall,
    })
}

impl<'a> ConfirmedRotation<'a> {
    pub(super) fn baseline(&self) -> &'a Baseline {
        self.baseline
    }

    pub(super) fn capture(&self) -> &'a Capture {
        self.capture
    }
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
    verify_confirmation(runtime, founder, baseline, capture, None, wall)
}

pub(super) fn confirm_rotation<'a, A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    baseline: &'a Baseline,
    capture: &'a Capture,
    wall: u64,
) -> Result<ConfirmedRotation<'a>>
where
    A: TransportAdapter,
    R: TransportRoutingPolicy,
    K: KeyPackagePublisher,
    F: TransportAdapter,
    S: TransportRoutingPolicy,
    Q: KeyPackagePublisher,
{
    verify_confirmation(
        runtime,
        founder,
        baseline,
        capture,
        Some(&capture.request().message.id),
        wall,
    )?;
    Ok(ConfirmedRotation { baseline, capture })
}

fn verify_confirmation<A, R, K, F, S, Q>(
    runtime: &AccountDeviceRuntime<A, R, K>,
    founder: &AccountDeviceRuntime<F, S, Q>,
    baseline: &Baseline,
    capture: &Capture,
    founder_message: Option<&MessageId>,
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
    verify_founder_cut(
        founder,
        baseline.group(),
        baseline.founder(),
        &rows.authority,
        &rows.authority.pin,
        founder_message,
    )?;
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
    exposure: Exposure,
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
    let capture = bind_pending(&rows, baseline, candidate, exposure, wall)?;
    baseline.check_image(wall)?;
    Ok(capture)
}

fn bind_pending(
    rows: &Rows,
    baseline: &Baseline,
    candidate: &TransportPublishRequest,
    exposure: Exposure,
    wall: u64,
) -> Result<Capture> {
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
    let native = rows.native_pending.as_ref().ok_or(Refusal::Invalid)?;
    if native.reference != pending_ref || native.target.epoch != evolution.target_epoch {
        return Err(Refusal::Invalid);
    }
    let capture = verify_fanout(
        baseline,
        fanout,
        pending,
        pending_ref,
        candidate,
        exposure,
        wall,
    )?;
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
    checked_evolution(
        evolution,
        &baseline.group,
        baseline.authority.pin.epoch,
        baseline.next_epoch()?,
        &baseline.obligation,
        &baseline.authority.leaf_hash,
    )
}

fn checked_evolution<'a>(
    evolution: Option<&'a DurableGroupEvolution>,
    group: &GroupId,
    source: EpochId,
    target: EpochId,
    obligation: &MessageId,
    previous_leaf: &[u8; 32],
) -> Result<&'a DurableGroupEvolution> {
    let evolution = evolution.ok_or(Refusal::Invalid)?;
    if &evolution.group_id != group
        || evolution.source_epoch != source
        || evolution.target_epoch != target
        || evolution.own_leaf_before_hash.as_deref() != Some(previous_leaf.as_slice())
        || !evolution.removal_members.is_empty()
        || !matches!(
            &evolution.semantic,
            GroupEvolutionSemantic::SelfUpdate {
                trigger: MaintenanceTrigger::PostJoin,
                obligation_id: Some(id),
            } if id == obligation
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
    exposure: Exposure,
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
        || !exact_exposure(fanout, pending, endpoint, exposure)
        || !(1..=exposure.max_attempts()).contains(&attempts)
        || first > last
        || first / 1_000 < baseline.wall
        || last / 1_000 > wall
        || pending.message_id != durable.message.id
        || pending.origin.is_some()
        || pending.intent_kind.is_some()
        || pending.state != PendingCommitState::Sending
        || pending.endpoint_count != 1
        || !pending.refused_endpoints.is_empty()
    {
        return Err(Refusal::Invalid);
    }
    Ok((attempts, first, last))
}

impl Exposure {
    fn max_attempts(self) -> u32 {
        match self {
            Self::BeforeSend => MAX_REFUSALS,
            Self::PossiblyExposed => EXPOSED_NATIVE_ATTEMPTS,
        }
    }
}

fn exact_exposure(
    fanout: &OutboundFanout,
    pending: &PendingCommit,
    endpoint: &cgka_traits::TransportEndpoint,
    exposure: Exposure,
) -> bool {
    let failure = fanout.target_failure(0);
    let endpoints = std::slice::from_ref(endpoint);
    match exposure {
        Exposure::BeforeSend => {
            fanout.target_statuses() == [FanoutTargetStatus::RetryableUnavailable]
                && !fanout.possible_exposure()
                && failure.is_some_and(|failure| {
                    failure.pre_send
                        && failure.kind == TransportEndpointFailureKind::RetryableUnavailable
                        && &failure.endpoint == endpoint
                })
                && pending.owed_endpoints.as_slice() == endpoints
                && pending.unconfirmed_endpoints.is_empty()
        }
        Exposure::PossiblyExposed => {
            let exposure_recorded = match fanout.target_statuses() {
                [FanoutTargetStatus::PossiblyExposed] => failure.is_some_and(|failure| {
                    !failure.pre_send
                        && failure.kind == TransportEndpointFailureKind::PossiblyExposed
                        && &failure.endpoint == endpoint
                        && failure.rejection_category.is_none()
                        && failure.size_class.is_none()
                }),
                // A process cut can leave the frozen fanout durably Attempting.
                // Independent founder readback still has to prove receipt.
                [FanoutTargetStatus::Attempting] => failure.is_none(),
                _ => false,
            };
            exposure_recorded
                && fanout.possible_exposure()
                && pending.owed_endpoints.is_empty()
                && pending.unconfirmed_endpoints.as_slice() == endpoints
        }
    }
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
    verify_founder_cut(founder, group, account, recipient, &recipient.pin, None)
}

fn verify_founder_cut<A, R, K>(
    founder: &AccountDeviceRuntime<A, R, K>,
    group: &GroupId,
    account: &MemberId,
    recipient: &Authority,
    pin: &EpochPin,
    processed: Option<&MessageId>,
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
            if &authority.pin != pin
                || authority.leaves != recipient.leaves
                || authority.local_leaf == recipient.local_leaf
                || authority.route != recipient.route
                || !authority.admission.created_locally
                || authority.admission.via_welcome.is_some()
                || match processed {
                    Some(id) => !storage.has_processed_transport_id(id)?,
                    None => false,
                }
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

pub(super) fn check_image(image: &Path, inode: (u64, u64)) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use cgka_traits::{
        Timestamp, TransportEndpoint, TransportEndpointFailure, TransportMessage, TransportSource,
        transport::TransportEnvelope,
    };

    #[test]
    fn possible_exposure_needs_one_exact_unconfirmed_endpoint() {
        let endpoint = TransportEndpoint::from("wss://relay.invalid/");
        let group = GroupId::new([1; 32]);
        let id = MessageId::new([2; 32]);
        let transport_group_id = vec![3; 32];
        let request = TransportPublishRequest {
            account_id: MemberId::new([4; 32]),
            message: TransportMessage {
                id: id.clone(),
                payload: vec![0],
                timestamp: Timestamp(1),
                causal_deps: Vec::new(),
                source: TransportSource("marmot.transport.nostr".into()),
                envelope: TransportEnvelope::GroupMessage {
                    transport_group_id: transport_group_id.clone(),
                },
            },
            target: TransportPublishTarget::Group {
                group_id: group.clone(),
                transport_group_id,
                endpoints: vec![endpoint.clone()],
            },
            required_acks: 1,
        };
        let mut fanout =
            OutboundFanout::stage(request, Some(PendingStateRef::new(5)), Some(group), 1000)
                .unwrap();
        let mut pending = PendingCommit {
            message_id: id,
            origin: None,
            intent_kind: None,
            first_attempt_at: Some(Timestamp(1)),
            state: PendingCommitState::Sending,
            unconfirmed_endpoints: Vec::new(),
            owed_endpoints: vec![endpoint.clone()],
            refused_endpoints: Vec::new(),
            endpoint_count: 1,
        };
        let failure = |kind, pre_send| TransportEndpointFailure {
            endpoint: endpoint.clone(),
            reason: "fixture exposure".into(),
            kind,
            rejection_category: None,
            size_class: None,
            pre_send,
        };

        fanout.mark_attempt_started_at(0, 1000).unwrap();
        fanout
            .record_target_failure(
                0,
                failure(TransportEndpointFailureKind::RetryableUnavailable, true),
            )
            .unwrap();
        assert!(exact_exposure(
            &fanout,
            &pending,
            &endpoint,
            Exposure::BeforeSend
        ));
        assert!(!exact_exposure(
            &fanout,
            &pending,
            &endpoint,
            Exposure::PossiblyExposed
        ));

        fanout.mark_attempt_started_at(0, 2000).unwrap();
        assert!(!exact_exposure(
            &fanout,
            &pending,
            &endpoint,
            Exposure::PossiblyExposed
        ));
        pending.owed_endpoints.clear();
        pending.unconfirmed_endpoints.push(endpoint.clone());
        assert!(exact_exposure(
            &fanout,
            &pending,
            &endpoint,
            Exposure::PossiblyExposed
        ));

        fanout
            .record_target_failure(
                0,
                failure(TransportEndpointFailureKind::PossiblyExposed, false),
            )
            .unwrap();
        assert!(exact_exposure(
            &fanout,
            &pending,
            &endpoint,
            Exposure::PossiblyExposed
        ));
        assert!(!exact_exposure(
            &fanout,
            &pending,
            &endpoint,
            Exposure::BeforeSend
        ));
        pending.unconfirmed_endpoints[0] = TransportEndpoint::from("wss://other.invalid/");
        assert!(!exact_exposure(
            &fanout,
            &pending,
            &endpoint,
            Exposure::PossiblyExposed
        ));
    }
}
