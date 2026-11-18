//! Exact checkpoint turns for the isolated Peer publication fixture.

use cgka_traits::engine::EpochPin;
use cgka_traits::{TransportPublishRequest, TransportPublishTarget};
use rusqlite::Row;

use crate::peer_invitation::accept_receipt_verify::VerifiedReceiptSource;
use crate::peer_invitation::accept_receipt_wire::{self as receipt_wire, Decoded, Receipt};
use crate::peer_invitation::postjoin_verify::{
    Baseline, Capture, ConfirmedRotation, RecoveredPostJoin,
};
use nostr::key::PublicKey;

use super::activation_schema::{self, Table};
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum Kind {
    PostJoin,
    AcceptReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CandidateBinding {
    pub(crate) event_id: [u8; 32],
    pub(crate) signed_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Pin {
    pub(crate) ordinal: u64,
    pub(crate) authenticator: [u8; 32],
    pub(crate) recipient_leaf: u32,
}

impl Pin {
    fn from_native(pin: &EpochPin, leaf: u32) -> Self {
        Self {
            ordinal: pin.epoch.0,
            authenticator: pin.identity,
            recipient_leaf: leaf,
        }
    }

    fn read(row: &Row<'_>, column: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            ordinal: u64::from_be_bytes(row.get(column)?),
            authenticator: row.get(column + 1)?,
            recipient_leaf: row.get(column + 2)?,
        })
    }

    fn is_next_after(self, source: Self) -> bool {
        source.ordinal.checked_add(1) == Some(self.ordinal)
            && source.authenticator != self.authenticator
            && source.recipient_leaf == self.recipient_leaf
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Scope {
    pub(crate) stage: (Binding, FileWitness),
    pub(crate) carrier: [u8; 32],
    pub(crate) group: Vec<u8>,
    pub(crate) route: [u8; 32],
}

impl Scope {
    fn generation(&self) -> u64 {
        self.stage.0.generation.count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Step {
    Reserved,
    NativeStarted,
    Captured,
    Armed { number: u8 },
    Uncertain { number: u8 },
    Published { number: u8 },
}

impl Step {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        match (row.get::<_, u8>(0)?, row.get::<_, u8>(1)?) {
            (0, 0) => Ok(Self::Reserved),
            (1, 0) => Ok(Self::NativeStarted),
            (2, 0) => Ok(Self::Captured),
            (3, number @ (1 | 2)) => Ok(Self::Armed { number }),
            (4, number @ (1 | 2)) => Ok(Self::Uncertain { number }),
            (5, number @ (1 | 2)) => Ok(Self::Published { number }),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }

    fn coordinates(self) -> (u8, u8) {
        match self {
            Self::Reserved => (0, 0),
            Self::NativeStarted => (1, 0),
            Self::Captured => (2, 0),
            Self::Armed { number } => (3, number),
            Self::Uncertain { number } => (4, number),
            Self::Published { number } => (5, number),
        }
    }
}

#[derive(Eq, PartialEq)]
struct SavedPublication {
    semantic_id: Vec<u8>,
    source_pin: Pin,
    target_pin: Option<Pin>,
    candidate: Option<CandidateBinding>,
    created_at: u64,
    changed_at: u64,
    armed_at: Option<u64>,
    step: Step,
}

pub(crate) struct Publication {
    owner: Arc<()>,
    kind: Kind,
    scope: Scope,
    recipient: PublicKey,
    origin: String,
    deadline: u64,
    welcome_id: [u8; 32],
    saved: SavedPublication,
}

impl Publication {
    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) fn generation(&self) -> u64 {
        self.scope.generation()
    }

    pub(crate) fn group(&self) -> &[u8] {
        &self.scope.group
    }

    pub(crate) fn route(&self) -> [u8; 32] {
        self.scope.route
    }

    pub(crate) fn stage(&self) -> (Binding, FileWitness) {
        self.scope.stage
    }

    pub(crate) fn founder(&self) -> PublicKey {
        self.scope.stage.0.key.server
    }

    pub(crate) fn attempt_wall(&self) -> Option<u64> {
        self.saved.armed_at
    }

    pub(crate) fn recipient(&self) -> PublicKey {
        self.recipient
    }

    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    pub(crate) fn deadline(&self) -> u64 {
        self.deadline
    }

    pub(crate) fn welcome_id(&self) -> [u8; 32] {
        self.welcome_id
    }

    pub(crate) fn semantic_id(&self) -> &[u8] {
        &self.saved.semantic_id
    }

    pub(crate) fn step(&self) -> Step {
        self.saved.step
    }

    pub(crate) fn candidate(&self) -> Option<CandidateBinding> {
        self.saved.candidate
    }

    pub(crate) fn source_pin(&self) -> Pin {
        self.saved.source_pin
    }

    pub(crate) fn target_pin(&self) -> Option<Pin> {
        self.saved.target_pin
    }

    pub(crate) fn created_at(&self) -> u64 {
        self.saved.created_at
    }

    fn same_turn(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
            && self.kind == other.kind
            && self.scope == other.scope
            && self.saved == other.saved
    }

    fn matches_rotation(&self, rotation: &RotationCoordinates) -> bool {
        self.kind == Kind::PostJoin
            && self.scope == rotation.scope
            && self.semantic_id() == rotation.obligation
            && self.source_pin() == rotation.source
    }
}

/// Minted only by a fresh durable arm. A cold reader never creates this turn.
pub(crate) struct EgressAttempt(Publication);

impl EgressAttempt {
    pub(crate) fn publication(&self) -> &Publication {
        &self.0
    }
}

/// Only a fresh durable NativeStarted turn can perform the one native send.
pub(crate) struct NativeReceiptStart(Publication);

impl NativeReceiptStart {
    pub(crate) fn publication(&self) -> &Publication {
        &self.0
    }
}

struct Enrollment {
    scope: Scope,
    receipt: Receipt,
    deadline: u64,
    activating_at: u64,
    original_leaf: u32,
}

fn matches_baseline(
    scope: &Scope,
    recipient: PublicKey,
    welcome: [u8; 32],
    origin: &str,
    deadline: u64,
    baseline: &Baseline,
) -> bool {
    let (_, file) = scope.stage;
    let (device, inode) = baseline.inode();
    file.device == device.to_be_bytes()
        && file.inode == inode.to_be_bytes()
        && scope.generation() == baseline.generation()
        && scope.group == baseline.group().as_slice()
        && scope.route == baseline.route()
        && scope.stage.0.key.server.to_bytes().as_slice() == baseline.founder().as_slice()
        && recipient.to_bytes().as_slice() == baseline.recipient().as_slice()
        && welcome.as_slice() == baseline.welcome().as_slice()
        && origin == baseline.origin()
        && deadline == baseline.deadline()
}

fn matches_request(baseline: &Baseline, request: &TransportPublishRequest) -> bool {
    let TransportPublishTarget::Group {
        group_id,
        transport_group_id,
        endpoints,
    } = &request.target
    else {
        return false;
    };
    matches!(endpoints.as_slice(), [endpoint] if endpoint.0 == baseline.origin())
        && request.account_id == *baseline.recipient()
        && group_id == baseline.group()
        && transport_group_id.as_slice() == baseline.route()
        && request.required_acks == 1
}

impl Enrollment {
    fn from_evidence(prior: &HeldEvidence, recipient: PublicKey) -> Result<Self> {
        let Progress::Activating { joined, at, .. } = &prior.saved.progress else {
            return Err(CursorError::Stale.into());
        };
        let key = prior.stage.0.key;
        Ok(Self {
            scope: Scope {
                stage: prior.stage,
                carrier: prior.delivery_carrier(),
                group: joined.native.group.clone(),
                route: prior.offer.routing().nostr_group_id,
            },
            receipt: Receipt {
                founder: key.server,
                recipient,
                room_id: prior.offer.room_id(),
                offer_id: key.id,
                offer_revision: key.revision,
                welcome_id: prior.welcome_id(),
                epoch: joined.native.epoch,
                epoch_identity: joined.native.epoch_identity,
            },
            deadline: joined.deadline,
            activating_at: *at,
            original_leaf: joined.native.own_leaf,
        })
    }

    fn at_pin(&self, pin: Pin) -> Receipt {
        Receipt {
            epoch: pin.ordinal,
            epoch_identity: pin.authenticator,
            ..self.receipt
        }
    }

    fn original_pin(&self) -> Pin {
        Pin {
            ordinal: self.receipt.epoch,
            authenticator: self.receipt.epoch_identity,
            recipient_leaf: self.original_leaf,
        }
    }

    fn verify_live(&self, wall: u64) -> Result<()> {
        if wall < self.activating_at {
            return Err(CursorError::ClockRegressed.into());
        }
        if wall >= self.deadline {
            return Err(Error::Expired);
        }
        Ok(())
    }
}

impl Context {
    fn publication_enrollment(
        &self,
        tx: &Transaction<'_>,
        selector: Selector,
        wall: u64,
    ) -> Result<(HeldEvidence, Enrollment)> {
        self.verify(tx, wall)?;
        if terminalized(tx, selector, self.origin)? {
            return Err(CursorError::Stale.into());
        }
        let queued = self
            .exact_consent(tx, selector)?
            .ok_or(CursorError::Stale)?;
        let evidence = self.read_evidence(tx, queued)?.ok_or(CursorError::Stale)?;
        let enrollment = Enrollment::from_evidence(&evidence, self.signer.public_key())?;
        Ok((evidence, enrollment))
    }

    fn load_publication(
        &self,
        tx: &Transaction<'_>,
        enrollment: &Enrollment,
        kind: Kind,
    ) -> Result<Option<Publication>> {
        let Some((generation, group, route, saved)) = tx
            .query_row(
                "SELECT phase,attempts,semantic_id,source_epoch,source_identity,source_leaf,
                        target_epoch,target_identity,target_leaf,event_id,signed_digest,created_at,
                        changed_at,generation,group_id,route,armed_at
                 FROM peer_activation_publication WHERE carrier=?1 AND kind=?2",
                params![enrollment.scope.carrier, kind as u8],
                |row| {
                    let event_id = row.get::<_, Option<[u8; 32]>>(9)?;
                    let candidate = event_id
                        .map(|event_id| {
                            Ok::<_, rusqlite::Error>(CandidateBinding {
                                event_id,
                                signed_digest: row.get(10)?,
                            })
                        })
                        .transpose()?;
                    let target_pin = row
                        .get::<_, Option<[u8; 8]>>(6)?
                        .map(|_| Pin::read(row, 6))
                        .transpose()?;
                    Ok((
                        read_seconds(row, 13)?,
                        row.get::<_, Vec<u8>>(14)?,
                        row.get::<_, [u8; 32]>(15)?,
                        SavedPublication {
                            semantic_id: row.get(2)?,
                            source_pin: Pin::read(row, 3)?,
                            target_pin,
                            candidate,
                            created_at: read_seconds(row, 11)?,
                            changed_at: read_seconds(row, 12)?,
                            armed_at: row
                                .get::<_, Option<i64>>(16)?
                                .map(u64::try_from)
                                .transpose()
                                .map_err(|_| rusqlite::Error::InvalidQuery)?,
                            step: Step::read(row)?,
                        },
                    ))
                },
            )
            .optional()?
        else {
            return Ok(None);
        };
        if generation != enrollment.scope.generation()
            || group != enrollment.scope.group
            || route != enrollment.scope.route
            || saved
                .target_pin
                .is_some_and(|pin| !pin.is_next_after(saved.source_pin))
        {
            return Err(CursorError::Binding.into());
        }
        if kind == Kind::PostJoin && !saved.source_pin.is_next_after(enrollment.original_pin()) {
            return Err(CursorError::Binding.into());
        }
        if kind == Kind::AcceptReceipt {
            let expected =
                receipt_wire::prepare(&enrollment.at_pin(saved.source_pin), saved.created_at)
                    .map_err(|_| CursorError::Binding)?;
            if expected.event_id().as_slice() != saved.semantic_id {
                return Err(CursorError::Binding.into());
            }
        }
        Ok(Some(Publication {
            owner: self.owner.clone(),
            kind,
            scope: enrollment.scope.clone(),
            recipient: enrollment.receipt.recipient,
            origin: self.route.urls[0].clone(),
            deadline: enrollment.deadline,
            welcome_id: enrollment.receipt.welcome_id,
            saved,
        }))
    }
}

fn insert_publication(
    tx: &Transaction<'_>,
    scope: &Scope,
    kind: Kind,
    semantic_id: &[u8],
    pin: Pin,
    wall: u64,
) -> Result<()> {
    Table::Publication.checked_write(tx, "INSERT", || {
        rows::require_write(tx.execute(
            "INSERT INTO peer_activation_publication
            (carrier,kind,generation,group_id,route,semantic_id,source_epoch,source_identity,
             source_leaf,created_at,changed_at,phase,attempts)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,0,0)",
            params![
                scope.carrier,
                kind as u8,
                seconds(scope.generation())?,
                scope.group,
                scope.route,
                semantic_id,
                pin.ordinal.to_be_bytes(),
                pin.authenticator,
                pin.recipient_leaf,
                seconds(wall)?,
            ],
        )?)?;
        Ok(())
    })
}

pub(crate) struct ReceiptReservation {
    publication: Publication,
    prepared: receipt_wire::Prepared,
}

impl ReceiptReservation {
    pub(crate) fn publication(&self) -> &Publication {
        &self.publication
    }

    pub(crate) fn prepared(&self) -> &receipt_wire::Prepared {
        &self.prepared
    }
}

enum Change {
    StartReceipt,
    Capture(CandidateBinding, Option<Pin>),
    Arm,
    Uncertain,
    Confirm(CandidateBinding),
}

impl Change {
    fn apply(self, prior: &Publication, tx: &Transaction<'_>, wall: u64) -> Result<()> {
        let (next, candidate, target) = match (self, prior.kind, prior.step()) {
            (Self::StartReceipt, Kind::AcceptReceipt, Step::Reserved) => {
                (Step::NativeStarted, None, None)
            }
            (Self::Capture(candidate, target), Kind::PostJoin, Step::Reserved)
                if target.is_some_and(|target| target.is_next_after(prior.source_pin())) =>
            {
                (Step::Captured, Some(candidate), target)
            }
            (Self::Capture(candidate, None), Kind::AcceptReceipt, Step::NativeStarted) => {
                (Step::Captured, Some(candidate), None)
            }
            (Self::Arm, _, Step::Captured) => (Step::Armed { number: 1 }, None, None),
            (Self::Arm, _, Step::Uncertain { number: 1 }) => {
                (Step::Armed { number: 2 }, None, None)
            }
            (Self::Uncertain, _, Step::Armed { number }) => {
                (Step::Uncertain { number }, None, None)
            }
            (Self::Confirm(candidate), _, Step::Armed { number } | Step::Uncertain { number })
                if prior.candidate() == Some(candidate) =>
            {
                (Step::Published { number }, None, None)
            }
            _ => return Err(CursorError::Stale.into()),
        };
        let armed_at = if matches!(next, Step::Armed { .. }) {
            Some(wall)
        } else {
            prior.attempt_wall()
        };
        let (previous, previous_attempts) = prior.step().coordinates();
        let (phase, attempts) = next.coordinates();
        let candidate = candidate.or(prior.candidate());
        let target = target.or(prior.target_pin());
        Table::Publication.checked_write(tx, "UPDATE", || {
            rows::require_write(tx.execute(
                "UPDATE peer_activation_publication SET phase=?3,attempts=?4,changed_at=?5,
                    event_id=?6,signed_digest=?7,target_epoch=?8,target_identity=?9,target_leaf=?10,
                    armed_at=?13
                 WHERE carrier=?1 AND kind=?2 AND phase=?11 AND attempts=?12",
                params![
                    prior.scope.carrier,
                    prior.kind as u8,
                    phase,
                    attempts,
                    seconds(wall)?,
                    candidate.map(|value| value.event_id),
                    candidate.map(|value| value.signed_digest),
                    target.map(|value| value.ordinal.to_be_bytes()),
                    target.map(|value| value.authenticator),
                    target.map(|value| value.recipient_leaf),
                    previous,
                    previous_attempts,
                    armed_at.map(seconds).transpose()?,
                ],
            )?)?;
            Ok(())
        })
    }
}

impl Checkpoint {
    pub(crate) fn attach_publication_fixture(
        conn: HardenedPeerConnection,
        signer: Keys,
        uid: u32,
        url: &str,
        policy: Policy,
    ) -> Result<Self> {
        let origin = super::super::super::super::Route::configured(url)?.id;
        let (cursor, epoch) =
            custody_schema::open_publication(conn, signer.public_key().to_bytes(), vec![origin])?;
        let mut checkpoint = Self::attach_with(cursor, signer, uid, &[url], policy, |_, _| Ok(()))?;
        checkpoint.provisional_schema = true;
        checkpoint.export_epoch = Some(super::super::Epoch::Custody(epoch));
        Ok(checkpoint)
    }

    fn publication_context(&self, source: &Source, other: PublicKey) -> Result<Context> {
        let cx = self.custody_context(source)?;
        if !matches!(cx.epoch, Epoch::Publication) {
            return Err(CursorError::Binding.into());
        }
        if !self.policy.permits(other) {
            return Err(Error::Policy);
        }
        Ok(cx)
    }

    pub(crate) fn publication(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        kind: Kind,
        wall: u64,
    ) -> Result<Option<Publication>> {
        let selector = prior.stage.0.key;
        let cx = self.publication_context(source, selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        let (current, enrollment) = cx.publication_enrollment(&tx, selector, wall)?;
        if !current.same_turn(prior) {
            return Err(CursorError::Stale.into());
        }
        cx.load_publication(&tx, &enrollment, kind)
    }

    fn update_publication(
        &mut self,
        source: &Source,
        prior: &Publication,
        change: Change,
        wall: u64,
    ) -> Result<Publication> {
        let selector = prior.scope.stage.0.key;
        let cx = self.publication_context(source, selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        let (_, enrollment) = cx.publication_enrollment(&tx, selector, wall)?;
        enrollment.verify_live(wall)?;
        let current = cx
            .load_publication(&tx, &enrollment, prior.kind)?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) {
            return Err(CursorError::Stale.into());
        }
        change.apply(prior, &tx, wall)?;
        cx.verify(&tx, wall)?;
        let saved = cx
            .load_publication(&tx, &enrollment, prior.kind)?
            .ok_or(CursorError::Binding)?;
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(saved)
    }

    pub(crate) fn start_native_receipt(
        &mut self,
        source: &Source,
        prior: &Publication,
        wall: u64,
    ) -> Result<NativeReceiptStart> {
        self.update_publication(source, prior, Change::StartReceipt, wall)
            .map(NativeReceiptStart)
    }

    pub(crate) fn arm_publication(
        &mut self,
        source: &Source,
        prior: &Publication,
        wall: u64,
    ) -> Result<EgressAttempt> {
        if prior.step() != Step::Captured {
            return Err(CursorError::Stale.into());
        }
        self.arm_exact(source, prior, wall)
    }

    fn arm_exact(
        &mut self,
        source: &Source,
        prior: &Publication,
        wall: u64,
    ) -> Result<EgressAttempt> {
        self.update_publication(source, prior, Change::Arm, wall)
            .map(EgressAttempt)
    }

    pub(crate) fn check_armed(
        &mut self,
        source: &Source,
        attempt: &EgressAttempt,
        wall: u64,
    ) -> Result<()> {
        let prior = attempt.publication();
        let selector = prior.scope.stage.0.key;
        let cx = self.publication_context(source, selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        let (_, enrollment) = cx.publication_enrollment(&tx, selector, wall)?;
        enrollment.verify_live(wall)?;
        let present = cx
            .load_publication(&tx, &enrollment, prior.kind)?
            .ok_or(CursorError::Stale)?;
        if !present.same_turn(prior) || !matches!(present.step(), Step::Armed { .. }) {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }

    pub(crate) fn record_publication_uncertain(
        &mut self,
        source: &Source,
        prior: &Publication,
        wall: u64,
    ) -> Result<Publication> {
        self.update_publication(source, prior, Change::Uncertain, wall)
    }

    fn reserve_postjoin_coordinates(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        scope: &Scope,
        semantic_id: &[u8],
        source_pin: Pin,
        wall: u64,
    ) -> Result<Publication> {
        let selector = prior.stage.0.key;
        let cx = self.publication_context(source, selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        let (current, enrollment) = cx.publication_enrollment(&tx, selector, wall)?;
        enrollment.verify_live(wall)?;
        if !current.same_turn(prior)
            || enrollment.scope != *scope
            || !source_pin.is_next_after(enrollment.original_pin())
            || activation_schema::active_exists(&tx, scope.carrier)?
            || activation_schema::postjoin_exists(&tx)?
        {
            return Err(CursorError::Stale.into());
        }
        insert_publication(&tx, scope, Kind::PostJoin, semantic_id, source_pin, wall)?;
        cx.verify(&tx, wall)?;
        let saved = cx
            .load_publication(&tx, &enrollment, Kind::PostJoin)?
            .ok_or(CursorError::Binding)?;
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(saved)
    }
}

pub(crate) struct UncertainPostJoin(Publication);

impl UncertainPostJoin {
    pub(crate) fn publication(&self) -> &Publication {
        &self.0
    }
}

struct RotationCoordinates {
    scope: Scope,
    obligation: Vec<u8>,
    candidate: CandidateBinding,
    source: Pin,
    confirmed: Pin,
}

impl RotationCoordinates {
    fn from_capture(scope: &Scope, baseline: &Baseline, capture: &Capture) -> Result<Self> {
        if capture.generation() != baseline.generation()
            || capture.deadline() != baseline.deadline()
            || capture.obligation_id() != baseline.obligation_id()
            || !matches_request(baseline, capture.request())
        {
            return Err(CursorError::Binding.into());
        }
        let leaf = baseline.recipient_leaf();
        let source = Pin::from_native(baseline.source(), leaf);
        let confirmed = Pin::from_native(capture.target(), leaf);
        if !confirmed.is_next_after(source) {
            return Err(CursorError::Binding.into());
        }
        Ok(Self {
            scope: scope.clone(),
            obligation: baseline.obligation_id().as_slice().to_vec(),
            candidate: CandidateBinding {
                event_id: capture.event_id(),
                signed_digest: capture.signed_digest(),
            },
            source,
            confirmed,
        })
    }
}

#[derive(Eq, PartialEq)]
pub(crate) struct ReceiptSourceCoordinates {
    pub(crate) decoded: Decoded,
    pub(crate) group: Vec<u8>,
    pub(crate) route: [u8; 32],
    pub(crate) sender_leaf: u32,
    pub(crate) native_id: Vec<u8>,
    pub(crate) deadline: u64,
}

pub(crate) struct FounderAcceptance {
    source: ReceiptSourceCoordinates,
    accepted_at: u64,
}

impl FounderAcceptance {
    pub(crate) fn source(&self) -> &ReceiptSourceCoordinates {
        &self.source
    }

    pub(crate) fn accepted_at(&self) -> u64 {
        self.accepted_at
    }
}

impl Checkpoint {
    pub(crate) fn reserve_postjoin(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        baseline: &Baseline,
        wall: u64,
    ) -> Result<Publication> {
        let enrollment = Enrollment::from_evidence(prior, self.signer.public_key())?;
        let cx = self.publication_context(source, prior.stage.0.key.server)?;
        if !matches_baseline(
            &enrollment.scope,
            enrollment.receipt.recipient,
            enrollment.receipt.welcome_id,
            &cx.route.urls[0],
            enrollment.deadline,
            baseline,
        ) {
            return Err(CursorError::Binding.into());
        }
        self.reserve_postjoin_coordinates(
            source,
            prior,
            &enrollment.scope,
            baseline.obligation_id().as_slice(),
            Pin::from_native(baseline.source(), baseline.recipient_leaf()),
            wall,
        )
    }

    pub(crate) fn capture_postjoin(
        &mut self,
        source: &Source,
        prior: &Publication,
        baseline: &Baseline,
        capture: &Capture,
        wall: u64,
    ) -> Result<Publication> {
        let coordinates = prior.rotation(baseline, capture)?;
        self.update_publication(
            source,
            prior,
            Change::Capture(coordinates.candidate, Some(coordinates.confirmed)),
            wall,
        )
    }

    pub(crate) fn confirm_postjoin(
        &mut self,
        source: &Source,
        prior: &Publication,
        confirmed: &ConfirmedRotation<'_>,
        wall: u64,
    ) -> Result<Publication> {
        let coordinates = prior.rotation(confirmed.baseline(), confirmed.capture())?;
        if prior.target_pin() != Some(coordinates.confirmed) {
            return Err(CursorError::Binding.into());
        }
        self.update_publication(source, prior, Change::Confirm(coordinates.candidate), wall)
    }

    pub(crate) fn activate_and_reserve_receipt(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        confirmed: &ConfirmedRotation<'_>,
        wall: u64,
    ) -> Result<ReceiptReservation> {
        let postjoin = self
            .publication(source, prior, Kind::PostJoin, wall)?
            .ok_or(CursorError::Stale)?;
        let coordinates = postjoin.rotation(confirmed.baseline(), confirmed.capture())?;
        self.activate_and_reserve_coordinates(source, prior, &coordinates, wall, |_| Ok(()))
    }

    pub(crate) fn read_uncertain_postjoin(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        wall: u64,
    ) -> Result<Option<UncertainPostJoin>> {
        Ok(self
            .publication(source, prior, Kind::PostJoin, wall)?
            .filter(|saved| {
                saved.deadline() > wall && saved.step() == (Step::Uncertain { number: 1 })
            })
            .map(UncertainPostJoin))
    }

    pub(crate) fn arm_recovered_postjoin(
        &mut self,
        source: &Source,
        prior: &UncertainPostJoin,
        baseline: &Baseline,
        recovered: &RecoveredPostJoin,
        wall: u64,
    ) -> Result<EgressAttempt> {
        let prior = prior.publication();
        let coordinates = prior.rotation(baseline, recovered.capture())?;
        if prior.step() != (Step::Uncertain { number: 1 })
            || prior.target_pin() != Some(coordinates.confirmed)
            || prior.candidate() != Some(coordinates.candidate)
        {
            return Err(CursorError::Binding.into());
        }
        self.arm_exact(source, prior, wall)
    }

    fn activate_and_reserve_coordinates(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        rotation: &RotationCoordinates,
        wall: u64,
        after_active: impl FnOnce(&Transaction<'_>) -> Result<()>,
    ) -> Result<ReceiptReservation> {
        let selector = prior.stage.0.key;
        let cx = self.publication_context(source, selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        let (current, enrollment) = cx.publication_enrollment(&tx, selector, wall)?;
        enrollment.verify_live(wall)?;
        let prepared = receipt_wire::prepare(&enrollment.at_pin(rotation.confirmed), wall)
            .map_err(|_| CursorError::Binding)?;
        let postjoin = cx
            .load_publication(&tx, &enrollment, Kind::PostJoin)?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior)
            || postjoin.scope != rotation.scope
            || !matches!(postjoin.step(), Step::Published { .. })
            || postjoin.semantic_id() != rotation.obligation
            || postjoin.candidate() != Some(rotation.candidate)
            || postjoin.source_pin() != rotation.source
            || postjoin.target_pin() != Some(rotation.confirmed)
        {
            return Err(CursorError::Stale.into());
        }
        Table::Active.checked_write(&tx, "INSERT", || {
            rows::require_write(tx.execute(
                "INSERT INTO peer_activation_active(carrier,generation,activated_at,epoch,identity,
                    recipient_leaf)
                 SELECT delivery_carrier,generation,?3,?4,?5,?6 FROM peer_pending_custody c
                 WHERE delivery_carrier=?1 AND generation=?2 AND state=3
                    AND NOT EXISTS(SELECT 1 FROM peer_activation_active a
                                   WHERE a.carrier=c.delivery_carrier)",
                params![
                    rotation.scope.carrier,
                    seconds(rotation.scope.generation())?,
                    seconds(wall)?,
                    rotation.confirmed.ordinal.to_be_bytes(),
                    rotation.confirmed.authenticator,
                    rotation.confirmed.recipient_leaf,
                ],
            )?)?;
            Ok(())
        })?;
        after_active(&tx)?;
        insert_publication(
            &tx,
            &rotation.scope,
            Kind::AcceptReceipt,
            &prepared.event_id(),
            rotation.confirmed,
            wall,
        )?;
        cx.verify(&tx, wall)?;
        let publication = cx
            .load_publication(&tx, &enrollment, Kind::AcceptReceipt)?
            .ok_or(CursorError::Binding)?;
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(ReceiptReservation {
            publication,
            prepared,
        })
    }

    pub(crate) fn read_receipt_reservation(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        wall: u64,
    ) -> Result<Option<ReceiptReservation>> {
        let selector = prior.stage.0.key;
        let cx = self.publication_context(source, selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        let (current, enrollment) = cx.publication_enrollment(&tx, selector, wall)?;
        if !current.same_turn(prior) {
            return Err(CursorError::Stale.into());
        }
        let receipt = enrollment.receipt;
        let Some(publication) = cx.load_publication(&tx, &enrollment, Kind::AcceptReceipt)? else {
            return Ok(None);
        };
        let pin = publication.source_pin();
        let receipt = Receipt {
            epoch: pin.ordinal,
            epoch_identity: pin.authenticator,
            ..receipt
        };
        let prepared = receipt_wire::prepare(&receipt, publication.created_at())
            .map_err(|_| CursorError::Binding)?;
        Ok(Some(ReceiptReservation {
            publication,
            prepared,
        }))
    }

    pub(crate) fn accept_verified_receipt(
        &mut self,
        source: &Source,
        verified: &VerifiedReceiptSource,
        wall: u64,
    ) -> Result<FounderAcceptance> {
        let cx = self.publication_context(source, verified.receipt().recipient)?;
        if verified.route().relays != cx.route.urls {
            return Err(CursorError::Binding.into());
        }
        let exact = ReceiptSourceCoordinates {
            decoded: Decoded {
                receipt: *verified.receipt(),
                event_id: verified.app_id(),
                created_at: verified.created_at(),
            },
            group: verified.group().as_slice().to_vec(),
            route: verified.route().nostr_group_id,
            sender_leaf: verified.leaf(),
            native_id: verified.native_id().as_slice().to_vec(),
            deadline: verified.deadline(),
        };
        self.accept_receipt_coordinates(source, &exact, wall)
    }

    fn accept_receipt_coordinates(
        &mut self,
        source: &Source,
        receipt_source: &ReceiptSourceCoordinates,
        wall: u64,
    ) -> Result<FounderAcceptance> {
        let receipt = receipt_source.decoded.receipt;
        let cx = self.publication_context(source, receipt.recipient)?;
        if receipt.founder != cx.signer.public_key() || receipt_source.decoded.created_at > wall {
            return Err(CursorError::Binding.into());
        }
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        if let Some(saved) = load_acceptance(&tx, cx.origin)? {
            return if saved.source == *receipt_source {
                Ok(saved)
            } else {
                Err(CursorError::Stale.into())
            };
        }
        if wall >= receipt_source.deadline {
            return Err(Error::Expired);
        }
        insert_acceptance(&tx, cx.origin, receipt_source, wall)?;
        cx.verify(&tx, wall)?;
        let saved = load_acceptance(&tx, cx.origin)?.ok_or(CursorError::Binding)?;
        if saved.source != *receipt_source {
            return Err(CursorError::Binding.into());
        }
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(saved)
    }
}

impl Publication {
    fn rotation(&self, baseline: &Baseline, capture: &Capture) -> Result<RotationCoordinates> {
        let coordinates = RotationCoordinates::from_capture(&self.scope, baseline, capture)?;
        if !self.matches_rotation(&coordinates)
            || !matches_baseline(
                &self.scope,
                self.recipient(),
                self.welcome_id(),
                self.origin(),
                self.deadline(),
                baseline,
            )
        {
            return Err(CursorError::Binding.into());
        }
        Ok(coordinates)
    }
}

fn insert_acceptance(
    tx: &Transaction<'_>,
    origin: Origin,
    source: &ReceiptSourceCoordinates,
    wall: u64,
) -> Result<()> {
    let decoded = &source.decoded;
    let receipt = decoded.receipt;
    Table::Acceptance.checked_write(tx, "INSERT", || {
        rows::require_write(tx.execute(
            "INSERT INTO peer_founder_acceptance
             (singleton,founder,recipient,room_id,offer_id,offer_revision,welcome_id,epoch,identity,
              origin,group_id,route,sender_leaf,native_id,event_id,authored_at,deadline,accepted_at)
             VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                receipt.founder.to_bytes(),
                receipt.recipient.to_bytes(),
                receipt.room_id,
                receipt.offer_id,
                receipt.offer_revision,
                receipt.welcome_id,
                receipt.epoch.to_be_bytes(),
                receipt.epoch_identity,
                origin,
                source.group,
                source.route,
                source.sender_leaf,
                source.native_id,
                decoded.event_id,
                seconds(decoded.created_at)?,
                seconds(source.deadline)?,
                seconds(wall)?,
            ],
        )?)?;
        Ok(())
    })
}

fn load_acceptance(tx: &Transaction<'_>, origin: Origin) -> Result<Option<FounderAcceptance>> {
    let saved = tx.query_row(
        "SELECT founder,recipient,room_id,offer_id,offer_revision,welcome_id,epoch,identity,group_id,
                route,sender_leaf,native_id,event_id,authored_at,deadline,accepted_at
         FROM peer_founder_acceptance WHERE singleton=1 AND origin=?1",
        [origin],
        |row| {
            Ok(FounderAcceptance {
                source: ReceiptSourceCoordinates {
                    decoded: Decoded {
                        receipt: Receipt {
                            founder: PublicKey::from_byte_array(row.get(0)?),
                            recipient: PublicKey::from_byte_array(row.get(1)?),
                            room_id: row.get(2)?,
                            offer_id: row.get(3)?,
                            offer_revision: row.get(4)?,
                            welcome_id: row.get(5)?,
                            epoch: u64::from_be_bytes(row.get(6)?),
                            epoch_identity: row.get(7)?,
                        },
                        event_id: row.get(12)?,
                        created_at: read_seconds(row, 13)?,
                    },
                    group: row.get(8)?,
                    route: row.get(9)?,
                    sender_leaf: row.get(10)?,
                    native_id: row.get(11)?,
                    deadline: read_seconds(row, 14)?,
                },
                accepted_at: read_seconds(row, 15)?,
            })
        },
    ).optional()?;
    if let Some(saved) = &saved {
        let decoded = &saved.source.decoded;
        let prepared = receipt_wire::prepare(&decoded.receipt, decoded.created_at)
            .map_err(|_| CursorError::Binding)?;
        if prepared.event_id() != decoded.event_id {
            return Err(CursorError::Binding.into());
        }
    }
    Ok(saved)
}

#[path = "peer_activation_checkpoint_tests.rs"]
mod tests;
