//! Durable single-carrier ownership in the existing SQLCipher checkpoint.

use super::super::super::super::HardenedPeerConnection;
use super::super::super::{Layout, Mode, Policy, wire};
use nostr::key::Keys;
use nostr::key::PublicKey;
#[cfg(test)]
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use rusqlite::params;
#[cfg(test)]
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;
#[cfg(test)]
use zeroize::Zeroizing;

use crate::peer_control_version::ControlVersion;
#[cfg(test)]
use crate::peer_invitation::prepared_consent::{JoinedReadback, NativeAttestation};

use super::super::super::super::read_seconds;
#[cfg(test)]
use super::super::super::wire::{
    Delivery, IncomingDelivery, PackageBinding, VerifiedDeliveryPackage,
};
#[cfg(test)]
use super::super::super::{Disposition, Local, RawRecord, Status, TrustedOutcome};
use super::*;

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
native_offer_ingress_fixture!();

#[path = "peer_pending_custody_schema.rs"]
mod custody_schema;
pub(crate) use custody_schema::Epoch;

#[cfg(test)]
#[path = "peer_pending_terminal_checkpoint.rs"]
pub(crate) mod terminal;
#[path = "peer_pending_terminal_schema.rs"]
mod terminal_schema;

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
native_installed_pending_fixture!();

#[cfg(test)]
#[path = "peer_terminal_leave_effect.rs"]
pub(crate) mod leave_effect;
#[cfg(test)]
#[path = "peer_terminal_leave_effect_schema.rs"]
mod leave_effect_schema;
#[cfg(test)]
#[path = "peer_terminal_leave_schema.rs"]
mod leave_schema;
#[cfg(test)]
#[path = "peer_terminal_leave_tests.rs"]
mod leave_tests;
#[cfg(test)]
#[path = "peer_terminal_leave_checkpoint.rs"]
mod terminal_leave;

#[cfg(test)]
#[path = "peer_activation_schema.rs"]
mod activation_schema;
#[cfg(test)]
#[path = "peer_activation_checkpoint.rs"]
pub(crate) mod publication;

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "peer_receiver_inventory_store.rs"]
pub(crate) mod receiver_inventory_store;

#[cfg(test)]
#[path = "peer_daemon_checkpoint.rs"]
pub(crate) mod daemon;

#[cfg(test)]
#[path = "peer_pending_custody_schema_tests.rs"]
mod tests;

// Installed policy is resolved for a single founder at the owner turn.
pub(crate) enum ReceivePolicy {
    Off,
    Manual,
    AutoAny,
    AutoFounders(BTreeSet<PublicKey>),
}

#[cfg(test)]
const MARGIN: u64 = 300;
#[cfg(test)]
const PENDING_WINDOW_SECONDS: u64 = 86_400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
#[cfg(test)]
pub(crate) enum CustodyState {
    Held,
    JoinStarted,
    Pending,
    Activating,
}

#[cfg(test)]
pub(crate) enum Replay {
    Absent,
    AlreadyApplied(Disposition),
    Invalid(Disposition),
}

#[cfg(test)]
pub(crate) enum Approval<'a> {
    Manual(&'a Local),
    Automatic,
}

#[cfg(test)]
enum HeldInput<'a> {
    Observed(RawRecord<'a>),
    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    InstalledObserved(RawRecord<'a>),
    Saved(&'a super::super::super::super::retry::Snapshot),
}

#[cfg(test)]
impl<'a> HeldInput<'a> {
    fn record(&self) -> RawRecord<'a> {
        match *self {
            Self::Observed(raw) => raw,
            #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
            Self::InstalledObserved(raw) => raw,
            Self::Saved(snapshot) => snapshot.record(),
        }
    }
}

#[cfg(test)]
pub(crate) struct CustodyConsent {
    #[cfg(test)]
    owner: Arc<()>,
    authority: ConsentAuthority,
}

#[cfg(test)]
enum ConsentAuthority {
    Fresh(Box<Queued>),
    Held(Box<HeldEvidence>),
}

#[cfg(test)]
impl CustodyConsent {
    pub(crate) fn carrier(&self) -> [u8; 32] {
        match &self.authority {
            ConsentAuthority::Fresh(queued) => queued.row.transcript.carrier,
            ConsentAuthority::Held(held) => held.row.transcript.carrier,
        }
    }

    pub(crate) fn stage(&self) -> (Binding, FileWitness) {
        match &self.authority {
            ConsentAuthority::Fresh(queued) => queued.stage(),
            ConsentAuthority::Held(held) => held.stage(),
        }
    }

    pub(crate) fn is_settled(&self) -> bool {
        match &self.authority {
            ConsentAuthority::Fresh(queued) => queued.row.ack_at.is_some(),
            ConsentAuthority::Held(_) => true,
        }
    }

    pub(crate) fn is_held(&self) -> bool {
        self.evidence().is_some()
    }

    pub(crate) fn evidence(&self) -> Option<&HeldEvidence> {
        match &self.authority {
            ConsentAuthority::Fresh(_) => None,
            ConsentAuthority::Held(held) => Some(held),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
#[cfg(test)]
struct JoinedCoordinates {
    group: Vec<u8>,
    epoch: u64,
    epoch_identity: [u8; 32],
    own_leaf: u32,
}

#[cfg(test)]
impl JoinedCoordinates {
    fn eq_readback(&self, proof: &JoinedReadback) -> bool {
        let (group, epoch, epoch_identity, own_leaf) = proof.coordinates();
        (
            self.group.as_slice(),
            self.epoch,
            self.epoch_identity,
            self.own_leaf,
        ) == (group, epoch, epoch_identity, own_leaf)
    }
}

#[derive(Debug, Eq, PartialEq)]
#[cfg(test)]
enum Progress {
    Held,
    JoinStarted {
        started_at: u64,
    },
    Pending {
        started_at: u64,
        joined: Qualified,
    },
    Activating {
        started_at: u64,
        joined: Qualified,
        automatic: bool,
        at: u64,
    },
}

#[derive(Debug, Eq, PartialEq)]
#[cfg(test)]
struct Qualified {
    native: JoinedCoordinates,
    qualified_at: u64,
    deadline: u64,
}

#[derive(Eq, PartialEq)]
#[cfg(test)]
struct Saved {
    carrier: [u8; 32],
    outer_stamp: u64,
    flight_created: u64,
    expires: u64,
    frame: Zeroizing<Vec<u8>>,
    replay_until: u64,
    welcome_id: [u8; 32],
    welcome_digest: [u8; 32],
    package: PackageBinding,
    package_digest: [u8; 32],
    package_not_before: u64,
    package_not_after: u64,
    automatic: bool,
    held_at: u64,
    held_deadline: u64,
    progress: Progress,
}

/// A saved authority witness, constructed only by the exact durable owner
/// reader. It carries no live socket, receipt or prepared-package capability.
#[cfg(test)]
pub(crate) struct HeldEvidence {
    owner: Arc<()>,
    offer: Offer,
    stage: (Binding, FileWitness),
    row: outbox::Outbox,
    saved: Saved,
    #[cfg(test)]
    active: bool,
}

#[cfg(test)]
impl HeldEvidence {
    pub(crate) fn offer(&self) -> &Offer {
        &self.offer
    }

    pub(crate) fn transcript(&self) -> Transcript {
        self.row.transcript
    }

    pub(crate) fn signed_consent(&self) -> &[u8] {
        self.row.frame.as_slice()
    }

    pub(crate) fn signed_delivery(&self) -> &[u8] {
        self.saved.frame.as_slice()
    }

    pub(crate) fn delivery_carrier(&self) -> [u8; 32] {
        self.saved.carrier
    }

    pub(crate) fn outer_stamp(&self) -> u64 {
        self.saved.outer_stamp
    }

    pub(crate) fn flight_created_at(&self) -> u64 {
        self.saved.flight_created
    }

    pub(crate) fn delivery_expires(&self) -> u64 {
        self.saved.expires
    }

    pub(crate) fn replay_until(&self) -> u64 {
        self.saved.replay_until
    }

    pub(crate) fn package(&self) -> PackageBinding {
        self.saved.package
    }

    pub(crate) fn package_bounds(&self) -> (u64, u64) {
        (self.saved.package_not_before, self.saved.package_not_after)
    }

    pub(crate) fn package_digest(&self) -> [u8; 32] {
        self.saved.package_digest
    }

    pub(crate) fn held_at(&self) -> u64 {
        self.saved.held_at
    }

    pub(crate) fn held_deadline(&self) -> u64 {
        self.saved.held_deadline
    }

    pub(crate) fn welcome_id(&self) -> [u8; 32] {
        self.saved.welcome_id
    }

    pub(crate) fn welcome_digest(&self) -> [u8; 32] {
        self.saved.welcome_digest
    }

    pub(crate) fn stage(&self) -> (Binding, FileWitness) {
        self.stage
    }

    pub(crate) fn state(&self) -> CustodyState {
        match self.saved.progress {
            Progress::Held => CustodyState::Held,
            Progress::JoinStarted { .. } => CustodyState::JoinStarted,
            Progress::Pending { .. } => CustodyState::Pending,
            Progress::Activating { .. } => CustodyState::Activating,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    pub(crate) fn pending_deadline(&self) -> Option<u64> {
        match &self.saved.progress {
            Progress::Pending { joined, .. } | Progress::Activating { joined, .. } => {
                Some(joined.deadline)
            }
            _ => None,
        }
    }

    pub(crate) fn matches_pending_readback(&self, proof: &JoinedReadback) -> bool {
        #[cfg(test)]
        if self.active {
            return false;
        }
        let joined = match &self.saved.progress {
            Progress::Pending { joined, .. } | Progress::Activating { joined, .. } => joined,
            _ => return false,
        };
        exact_readback(self, proof) && joined.native.eq_readback(proof)
    }

    fn same_turn(&self, other: &Self) -> bool {
        #[cfg(test)]
        if self.active != other.active {
            return false;
        }
        Arc::ptr_eq(&self.owner, &other.owner)
            && self.stage == other.stage
            && self.row == other.row
            && self.saved == other.saved
    }
}

#[cfg(test)]
pub(crate) struct JoinStarted {
    owner: Arc<()>,
    stage: (Binding, FileWitness),
    carrier: [u8; 32],
}

#[cfg(test)]
impl JoinStarted {
    pub(crate) fn matches(&self, held: &HeldEvidence) -> bool {
        Arc::ptr_eq(&self.owner, &held.owner)
            && self.stage == held.stage
            && self.carrier == held.saved.carrier
            && held.state() == CustodyState::Held
    }
}

pub(super) struct Context {
    account: [u8; 32],
    epoch: Epoch,
    origins: Vec<Origin>,
    origin: Origin,
    #[cfg(any(
        test,
        all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )
    ))]
    route: super::super::super::Route,
    #[cfg(any(
        test,
        all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )
    ))]
    signer: Keys,
    #[cfg(any(
        test,
        all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )
    ))]
    codec: wire::Codec,
    owner: Arc<()>,
}

impl Context {
    fn verify(&self, tx: &Transaction<'_>, wall: u64) -> std::result::Result<(), CursorError> {
        self.epoch.verify(tx, self.account, &self.origins)?;
        rows::check_wall(tx, wall)?;
        let quarantined: bool = tx.query_row(
            "SELECT quarantine_until IS NOT NULL FROM peer_cursor_origin WHERE origin=?1",
            [self.origin],
            |row| row.get(0),
        )?;
        if quarantined {
            return Err(CursorError::RelayContract);
        }
        Ok(())
    }

    #[cfg(any(
        test,
        all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )
    ))]
    pub(super) fn exact_consent(
        &self,
        tx: &Transaction<'_>,
        selector: Selector,
    ) -> Result<Option<Queued>> {
        if !outbox::exists(tx, selector, self.origin)? {
            return Ok(None);
        }
        let generation = generation(tx, selector, self.origin)?.ok_or(CursorError::Binding)?;
        let binding = Binding {
            key: selector,
            origin: self.origin,
            generation,
        };
        let invite = Invitation::load(tx, selector, self.origin)?;
        // This is historical verification of the original Offer, never an
        // extension of Offer eligibility or a use of its expiry as M2's clock.
        let signed_wall = invite.expires.checked_sub(1).ok_or(CursorError::Range)?;
        let offer = Checkpoint::check_signed_offer(
            &self.codec,
            &self.signer,
            &self.route,
            &invite,
            signed_wall,
        )?;
        let prepared = Candidate::read(tx, binding, self.account, offer)?;
        let row = outbox::load(tx, &prepared)?.ok_or(CursorError::Binding)?;
        Ok(Some(Queued { prepared, row }))
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    fn require_no_delivery_competitor(
        &self,
        tx: &Transaction<'_>,
        raw: RawRecord<'_>,
        prerequisite: [u8; 32],
    ) -> std::result::Result<(), CursorError> {
        let competitor: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM peer_cursor_deferred
                WHERE origin=?1 AND auth=?2 AND prerequisite=?3 AND id!=?4)",
            params![
                self.origin,
                super::super::super::super::Auth::FlightVerified as i64,
                prerequisite,
                raw.id
            ],
            |row| row.get(0),
        )?;
        if competitor {
            return Err(CursorError::Classification);
        }
        Ok(())
    }

    #[cfg(test)]
    fn read_evidence(&self, tx: &Transaction<'_>, queued: Queued) -> Result<Option<HeldEvidence>> {
        let Some(saved) = load(tx, &queued)? else {
            return Ok(None);
        };
        Ok(Some(HeldEvidence {
            #[cfg(test)]
            owner: self.owner.clone(),
            stage: queued.stage(),
            offer: queued.prepared.offer,
            row: queued.row,
            #[cfg(test)]
            active: self.epoch.is_publication()
                && activation_schema::active_exists(tx, saved.carrier)?,
            saved,
        }))
    }
}

pub(in super::super) fn owns(
    tx: &Transaction<'_>,
    selector: Selector,
    origin: Origin,
) -> std::result::Result<bool, CursorError> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM peer_pending_custody
            WHERE server=?1 AND offer_id=?2 AND offer_revision=?3 AND origin=?4)",
        params![
            selector.server.to_bytes(),
            selector.id,
            selector.revision,
            origin
        ],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
fn terminalized(tx: &Transaction<'_>, selector: Selector, origin: Origin) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_pending_terminal t JOIN peer_pending_custody c
            ON c.class=t.class AND c.generation=t.generation
            WHERE c.origin=?1 AND c.server=?2 AND c.offer_id=?3 AND c.offer_revision=?4)",
        params![
            origin,
            selector.server.to_bytes(),
            selector.id,
            selector.revision
        ],
        |row| row.get(0),
    )?)
}

impl Checkpoint {
    pub(crate) fn attach_installed_custody(
        conn: HardenedPeerConnection,
        signer: Keys,
        uid: u32,
        url: &str,
        policy: Policy,
        receive: ReceivePolicy,
    ) -> Result<Self> {
        let route = super::super::super::Route::configured(url)?;
        let codec = wire::Codec::for_version(wire::MAX_OUTER_BYTES, ControlVersion::V2)
            .map_err(|_| CursorError::Range)?;
        let (cursor, epoch) = custody_schema::open_installed(conn, &signer, &route, &codec)?;
        let mut checkpoint = Self::attach_with(cursor, signer, uid, &[url], policy, |_, _| Ok(()))?;
        checkpoint.codec = codec;
        checkpoint.layout = Layout::InstalledCustodyV2;
        checkpoint.installed_receive = Some(receive);
        checkpoint.provisional_schema = true;
        checkpoint.export_epoch = Some(super::Epoch::Custody(epoch));
        Ok(checkpoint)
    }

    #[cfg(test)]
    pub(crate) fn attach_pending_custody_fixture(
        conn: HardenedPeerConnection,
        signer: Keys,
        uid: u32,
        urls: &[&str],
        policy: Policy,
    ) -> Result<Self> {
        let origins = urls
            .iter()
            .map(|url| super::super::super::Route::configured(url).map(|route| route.id))
            .collect::<Result<Vec<_>>>()?;
        let (cursor, epoch) = custody_schema::open(conn, signer.public_key().to_bytes(), origins)?;
        let mut checkpoint = Self::attach_with(cursor, signer, uid, urls, policy, |_, _| Ok(()))?;
        checkpoint.provisional_schema = true;
        checkpoint.export_epoch = Some(super::Epoch::Custody(epoch));
        Ok(checkpoint)
    }

    pub(crate) fn is_installed_custody(&self) -> bool {
        self.layout == Layout::InstalledCustodyV2
    }

    pub(crate) fn installed_receive_policy(&self) -> Option<&ReceivePolicy> {
        self.installed_receive.as_ref()
    }

    #[cfg(any(
        test,
        all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )
    ))]
    pub(super) fn read_installed_consent<T>(
        &mut self,
        source: &Source,
        account: PublicKey,
        url: &str,
        wall: u64,
        read: impl FnOnce(&Transaction<'_>, &Context) -> Result<T>,
    ) -> Result<T> {
        if !self.is_installed_custody() {
            return Err(Error::Layout);
        }
        if !matches!(self.installed_receive_policy(), Some(ReceivePolicy::Manual)) {
            return Err(Error::Policy);
        }
        let cx = self.custody_context(source)?;
        if account.to_bytes() != cx.account
            || cx.route.urls.as_slice() != [url]
            || cx.origins.as_slice() != [cx.route.id]
        {
            return Err(Error::Source);
        }
        let tx = self.cursor.conn.transaction()?;
        cx.verify(&tx, wall)?;
        let floor = tx.query_row(
            "SELECT wall FROM peer_cursor_origin WHERE origin=?1",
            [cx.origin],
            |row| read_seconds(row, 0),
        )?;
        if wall < floor {
            return Err(CursorError::ClockRegressed.into());
        }
        read(&tx, &cx)
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(crate) fn issued_package_target(
        &mut self,
        source: &Source,
        account: PublicKey,
        url: &str,
        stage: &stage_journal::BoundStage,
        expected: (FileWitness, Artifact),
        wall: u64,
    ) -> Result<PackageTarget> {
        stage.require_package_owner(self)?;
        let binding = stage.binding();
        self.read_installed_consent(source, account, url, wall, |tx, cx| {
            if binding.generation.known
                || binding.origin != cx.origin
                || generation(tx, binding.key, cx.origin)? != Some(binding.generation)
                || outbox::exists(tx, binding.key, cx.origin)?
                || owns(tx, binding.key, cx.origin)?
                || stage_journal::load(tx, binding)?
                    != Some(State::Prepared(expected.0, expected.1))
                || wall >= stage.offer_expires().min(expected.1.expires)
            {
                return Err(CursorError::Binding.into());
            }
            let invite = Invitation::load(tx, binding.key, cx.origin)?;
            let offer =
                Checkpoint::check_signed_offer(&cx.codec, &cx.signer, &cx.route, &invite, wall)?;
            if invite.decision_at.is_some() || offer.flight().expires_at != stage.offer_expires() {
                return Err(CursorError::Stale.into());
            }
            Ok(PackageTarget {
                owner: cx.owner.clone(),
                binding,
                file: expected.0,
                artifact: expected.1,
                deadline: stage.offer_expires().min(expected.1.expires),
            })
        })
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(crate) fn read_managed_queued(
        &mut self,
        source: &Source,
        account: PublicKey,
        url: &str,
        selector: Selector,
        wall: u64,
    ) -> Result<Option<ManagedQueued>> {
        if !self.policy.permits(selector.server) {
            return Err(Error::Policy);
        }
        self.read_installed_consent(source, account, url, wall, |tx, cx| {
            // Terminal custody rows retain the matching custody owner.
            if owns(tx, selector, cx.origin)? {
                return Err(Error::Custodied);
            }
            let Some(queued) = cx.exact_consent(tx, selector)? else {
                return Ok(None);
            };
            let prepared = &queued.prepared;
            let deadline = prepared
                .offer
                .flight()
                .expires_at
                .min(prepared.artifact.expires)
                .min(queued.row.transcript.expires);
            if prepared.binding.generation.known || wall >= deadline {
                return Err(CursorError::Binding.into());
            }
            let target = PackageTarget {
                owner: cx.owner.clone(),
                binding: prepared.binding,
                file: prepared.witness,
                artifact: prepared.artifact,
                deadline,
            };
            Ok(Some(ManagedQueued { target, queued }))
        })
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(crate) fn confirm_managed_queued(
        &mut self,
        source: &Source,
        account: PublicKey,
        url: &str,
        prior: &ManagedQueued,
        wall: u64,
    ) -> Result<()> {
        self.require_managed_package_target(&prior.target)?;
        let current = self
            .read_managed_queued(source, account, url, prior.target.binding.key, wall)?
            .ok_or(CursorError::Stale)?;
        if current.target.stage() != prior.target.stage()
            || current.target.artifact != prior.target.artifact
            || current.target.deadline != prior.target.deadline
            || current.queued.prepared.offer.flight().carrier_id
                != prior.queued.prepared.offer.flight().carrier_id
            || current.queued.row != prior.queued.row
        {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn check_installed_consent_scope(
        &mut self,
        source: &Source,
        account: PublicKey,
        url: &str,
        wall: u64,
    ) -> Result<()> {
        self.read_installed_consent(source, account, url, wall, |_, _| Ok(()))
    }

    #[cfg(test)]
    pub(crate) fn custody_codec(&self) -> wire::Codec {
        self.codec
    }

    pub(in super::super::super) fn custody_mode(&self, founder: PublicKey) -> Result<Mode> {
        let Some(receive) = &self.installed_receive else {
            return self
                .policy
                .permits(founder)
                .then_some(self.policy.mode)
                .ok_or(Error::Policy);
        };
        if founder == self.signer.public_key() || self.policy.blocked.contains(&founder) {
            return Err(Error::Policy);
        }
        match receive {
            ReceivePolicy::Off => Err(Error::Policy),
            ReceivePolicy::Manual => Ok(Mode::Manual),
            ReceivePolicy::AutoAny => Ok(Mode::Automatic),
            ReceivePolicy::AutoFounders(founders) if founders.contains(&founder) => {
                Ok(Mode::Automatic)
            }
            ReceivePolicy::AutoFounders(_) => Ok(Mode::Manual),
        }
    }

    fn custody_context(&self, source: &Source) -> Result<Context> {
        let Some(super::Epoch::Custody(epoch)) = self.export_epoch else {
            return Err(CursorError::Binding.into());
        };
        let origin = self.origin(source)?;
        #[cfg(any(
            test,
            all(
                marmish_peer_receiver_candidate,
                target_os = "linux",
                target_arch = "x86_64"
            )
        ))]
        let route = super::super::super::Route::configured(&self.route(origin)?.urls[0])?;
        Ok(Context {
            account: self.signer.public_key().to_bytes(),
            epoch,
            origins: self.cursor.origins.clone(),
            origin,
            #[cfg(any(
                test,
                all(
                    marmish_peer_receiver_candidate,
                    target_os = "linux",
                    target_arch = "x86_64"
                )
            ))]
            route,
            #[cfg(any(
                test,
                all(
                    marmish_peer_receiver_candidate,
                    target_os = "linux",
                    target_arch = "x86_64"
                )
            ))]
            signer: self.signer.clone(),
            #[cfg(any(
                test,
                all(
                    marmish_peer_receiver_candidate,
                    target_os = "linux",
                    target_arch = "x86_64"
                )
            ))]
            codec: self.codec,
            owner: self.owner.clone(),
        })
    }

    #[cfg(test)]
    pub(crate) fn custody_owned(
        &mut self,
        source: &Source,
        selector: Selector,
        wall: u64,
    ) -> Result<bool> {
        if !matches!(self.export_epoch, Some(super::Epoch::Custody(_))) {
            return Ok(false);
        }
        let cx = self.custody_context(source)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        let owned = owns(&tx, selector, cx.origin)?;
        Ok(owned)
    }

    #[cfg(test)]
    pub(crate) fn custody_consent(
        &mut self,
        source: &Source,
        selector: Selector,
        wall: u64,
    ) -> Result<Option<CustodyConsent>> {
        let cx = self.custody_context(source)?;
        self.custody_mode(selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        if terminalized(&tx, selector, cx.origin)? {
            return Err(Error::Custodied);
        }
        let Some(queued) = cx.exact_consent(&tx, selector)? else {
            return Ok(None);
        };
        let authority = if owns(&tx, selector, cx.origin)? {
            ConsentAuthority::Held(Box::new(
                cx.read_evidence(&tx, queued)?.ok_or(CursorError::Binding)?,
            ))
        } else {
            if wall >= queued.row.transcript.expires || wall >= queued.prepared.artifact.expires {
                return Err(Error::Expired);
            }
            ConsentAuthority::Fresh(Box::new(queued))
        };
        Ok(Some(CustodyConsent {
            owner: cx.owner,
            authority,
        }))
    }

    #[cfg(test)]
    pub(crate) fn verify_custody_delivery(
        &self,
        recovered: &CustodyConsent,
        package: &NativeAttestation,
        incoming: IncomingDelivery,
        wall: u64,
    ) -> Result<Option<(Delivery, VerifiedDeliveryPackage)>> {
        self.check_local_owner(&recovered.owner)?;
        let ConsentAuthority::Fresh(queued) = &recovered.authority else {
            return Err(Error::Custodied);
        };
        let prepared = &queued.prepared;
        if Selector::from_delivery(&incoming) != prepared.binding.key {
            return Ok(None);
        }
        if wall >= queued.row.transcript.expires || wall >= queued.prepared.artifact.expires {
            return Err(Error::Expired);
        }
        let exact = package.require(prepared.stage(), prepared.artifact, wall)?;
        queued
            .row
            .transcript
            .verify_delivery_with_package(
                &self.signer,
                &prepared.offer,
                &queued.row.frame,
                exact,
                wall,
                incoming,
            )
            .map_err(|_| Error::Package)
    }

    #[cfg(test)]
    pub(crate) fn custody_replay(
        &mut self,
        source: &Source,
        raw: RawRecord<'_>,
        wall: u64,
    ) -> Result<Replay> {
        raw.check()?;
        let cx = self.custody_context(source)?;
        let digest = <[u8; 32]>::from(Sha256::digest(raw.bytes));
        let lookup = |tx: &Transaction<'_>| -> std::result::Result<Option<bool>, CursorError> {
            cx.verify(tx, wall)?;
            Ok(tx
                .query_row(
                    "SELECT origin=?2 AND outer_stamp=?3 AND frame=?4 AND replay_until>?5
                 FROM peer_pending_custody WHERE delivery_carrier=?1
                 UNION ALL
                 SELECT origin=?2 AND outer_stamp=?3 AND delivery_digest=?6 AND replay_until>?5
                 FROM peer_pending_terminal WHERE delivery_carrier=?1 AND step=3",
                    params![
                        raw.id,
                        cx.origin,
                        seconds(raw.stamp)?,
                        raw.bytes,
                        seconds(wall)?,
                        digest
                    ],
                    |r| r.get(0),
                )
                .optional()?)
        };
        let state = {
            let tx = self.cursor.with_wall(cx.origin, wall)?;
            lookup(&tx)?
        };
        if state == Some(true) {
            let (receipt, outcome) =
                self.cursor
                    .commit_trusted_exact(cx.origin, raw, wall, |tx| {
                        if lookup(tx)? != Some(true) {
                            return Err(CursorError::Stale);
                        }
                        rows::advance_wall(tx, wall)?;
                        Ok(TrustedOutcome::AlreadyApplied)
                    })?;
            return Ok(Replay::AlreadyApplied(Disposition {
                status: Status::Accepted(outcome),
                receipt,
            }));
        }
        if state == Some(false) {
            let receipt = self.cursor.commit_invalid(cx.origin, raw, wall)?;
            return Ok(Replay::Invalid(Disposition {
                status: Status::Invalid,
                receipt,
            }));
        }
        Ok(Replay::Absent)
    }

    #[cfg(test)]
    pub(crate) fn commit_custody_held(
        &mut self,
        source: &Source,
        raw: RawRecord<'_>,
        prior: &CustodyConsent,
        delivery: &Delivery,
        package: &VerifiedDeliveryPackage,
        wall: u64,
    ) -> Result<Disposition> {
        self.commit_custody_held_input(
            source,
            HeldInput::Observed(raw),
            prior,
            delivery,
            package,
            wall,
        )
    }

    #[cfg(test)]
    fn commit_custody_held_input(
        &mut self,
        source: &Source,
        input: HeldInput<'_>,
        prior: &CustodyConsent,
        delivery: &Delivery,
        package: &VerifiedDeliveryPackage,
        wall: u64,
    ) -> Result<Disposition> {
        let raw = input.record();
        raw.check()?;
        self.check_local_owner(&prior.owner)?;
        let cx = self.custody_context(source)?;
        let ConsentAuthority::Fresh(prior) = &prior.authority else {
            return Err(Error::Custodied);
        };
        let key = prior.prepared.binding.key;
        let expiry = delivery.flight().expires_at.min(package.bounds().1);
        let deadline = expiry
            .checked_sub(MARGIN)
            .filter(|&value| value > wall)
            .ok_or(Error::Expired)?;
        let mode = self.custody_mode(key.server)?;
        let frame_digest: [u8; 32] = Sha256::digest(raw.bytes).into();
        if package.frame_digest() != frame_digest || !package.matches_index(raw.id, raw.stamp) {
            return Err(Error::Index);
        }
        let (receipt, outcome) = self
            .cursor
            .commit_trusted_exact(cx.origin, raw, wall, |tx| {
                let write = || -> Result<()> {
                    cx.verify(tx, wall)?;
                    let current = cx.exact_consent(tx, key)?.ok_or(CursorError::Stale)?;
                    if owns(tx, key, cx.origin)?
                        || prior.stage() != current.stage()
                        || prior.row != current.row
                    {
                        return Err(CursorError::Stale.into());
                    }
                    match input {
                        HeldInput::Observed(_) => {}
                        #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
                        HeldInput::InstalledObserved(_) => {
                            cx.require_no_delivery_competitor(
                                tx,
                                raw,
                                current.row.transcript.carrier,
                            )?;
                        }
                        HeldInput::Saved(snapshot) => {
                            snapshot.require_current_flight(
                                tx,
                                cx.origin,
                                current.row.transcript.carrier,
                            )?;
                        }
                    }
                    HeldWrite {
                        raw,
                        delivery,
                        package,
                        mode,
                        wall,
                        deadline,
                    }
                    .insert(tx, &current)?;
                    cx.verify(tx, wall)?;
                    rows::advance_wall(tx, wall)?;
                    Ok(())
                };
                write().map_err(schema::cursor_error)?;
                Ok(TrustedOutcome::Held)
            })?;
        Ok(Disposition {
            status: Status::Accepted(outcome),
            receipt,
        })
    }

    #[cfg(test)]
    pub(crate) fn read_custody(
        &mut self,
        source: &Source,
        selector: Selector,
        generation: u64,
        wall: u64,
    ) -> Result<Option<HeldEvidence>> {
        let cx = self.custody_context(source)?;
        self.custody_mode(selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        if terminalized(&tx, selector, cx.origin)? {
            return Ok(None);
        }
        let Some(queued) = cx.exact_consent(&tx, selector)? else {
            return Ok(None);
        };
        if queued.prepared.binding.generation.count != generation {
            return Ok(None);
        }
        cx.read_evidence(&tx, queued)
    }

    #[cfg(test)]
    pub(crate) fn start_custody_join(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        wall: u64,
    ) -> Result<JoinStarted> {
        if self.is_installed_custody() {
            return Err(Error::Layout);
        }
        self.transition_custody(source, prior, wall, Change::Start)?;
        Ok(JoinStarted {
            owner: self.owner.clone(),
            stage: prior.stage,
            carrier: prior.saved.carrier,
        })
    }

    #[cfg(test)]
    pub(crate) fn commit_custody_pending(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        proof: &JoinedReadback,
        wall: u64,
    ) -> Result<()> {
        if self.is_installed_custody() {
            return Err(Error::Layout);
        }
        self.transition_custody(source, prior, wall, Change::Qualify(proof))
    }

    #[cfg(test)]
    pub(crate) fn commit_custody_activating(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        proof: &JoinedReadback,
        approval: Approval<'_>,
        wall: u64,
    ) -> Result<()> {
        if self.layout == Layout::InstalledCustodyV2 {
            return Err(Error::Layout);
        }
        let automatic = match approval {
            Approval::Manual(local) => {
                self.check_local(local)?;
                false
            }
            Approval::Automatic if matches!(self.policy.mode, Mode::Automatic) => true,
            Approval::Automatic => return Err(Error::Policy),
        };
        self.transition_custody(source, prior, wall, Change::Activate { proof, automatic })
    }

    #[cfg(test)]
    fn transition_custody(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        wall: u64,
        change: Change<'_>,
    ) -> Result<()> {
        self.transition_custody_guarded(source, prior, wall, change, || Ok(()))
            .map(|_| ())
    }

    #[cfg(test)]
    fn transition_custody_guarded(
        &mut self,
        source: &Source,
        prior: &HeldEvidence,
        wall: u64,
        change: Change<'_>,
        mut guard: impl FnMut() -> Result<()>,
    ) -> Result<usize> {
        self.check_local_owner(&prior.owner)?;
        let cx = self.custody_context(source)?;
        let selector = prior.stage.0.key;
        if prior.stage.0.origin != cx.origin {
            return Err(Error::Source);
        }
        self.custody_mode(selector.server)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        if terminalized(&tx, selector, cx.origin)? {
            return Err(CursorError::Stale.into());
        }
        let queued = cx.exact_consent(&tx, selector)?.ok_or(CursorError::Stale)?;
        let current = cx.read_evidence(&tx, queued)?.ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) {
            return Err(CursorError::Stale.into());
        }
        guard()?;
        let changed = change.apply(&tx, &current, wall)?;
        cx.verify(&tx, wall)?;
        rows::advance_wall(&tx, wall)?;
        guard()?;
        tx.commit()?;
        Ok(changed)
    }
}

#[cfg(test)]
enum Change<'a> {
    Start,
    Qualify(&'a JoinedReadback),
    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    QualifyInstalled(&'a InstalledJoinedReadback<'a>),
    Activate {
        proof: &'a JoinedReadback,
        automatic: bool,
    },
}

#[cfg(test)]
impl Change<'_> {
    fn apply(self, tx: &Transaction<'_>, prior: &HeldEvidence, wall: u64) -> Result<usize> {
        if wall < prior.held_at() {
            return Err(CursorError::ClockRegressed.into());
        }
        if wall >= prior.held_deadline() {
            return Err(Error::Expired);
        }
        let writer = custody_schema::Writer::Update;
        let id = prior.delivery_carrier();
        let generation = seconds(prior.stage.0.generation.count)?;
        let at = seconds(wall)?;
        let changed = match (self, &prior.saved.progress) {
            (Self::Start, Progress::Held) => {
                writer.guard(tx, false)?;
                tx.execute(
                    "UPDATE peer_pending_custody SET state=1,started_at=?3
                     WHERE delivery_carrier=?1 AND generation=?2 AND state=0",
                    params![id, generation, at],
                )?
            }
            (Self::Qualify(proof), Progress::JoinStarted { started_at })
                if exact_readback(prior, proof) && wall >= *started_at =>
            {
                qualify_pending(tx, prior, wall, proof.coordinates())?
            }
            #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
            (Self::QualifyInstalled(proof), Progress::JoinStarted { started_at })
                if wall >= *started_at =>
            {
                qualify_pending(tx, prior, wall, proof.coordinates())?
            }
            (Self::Activate { proof, automatic }, Progress::Pending { joined, .. }) => {
                if !prior.matches_pending_readback(proof) || wall < joined.qualified_at {
                    return Err(CursorError::Stale.into());
                }
                if wall >= joined.deadline {
                    return Err(Error::Expired);
                }
                if automatic && !prior.saved.automatic {
                    return Err(Error::Policy);
                }
                writer.guard(tx, false)?;
                tx.execute(
                    "UPDATE peer_pending_custody SET state=3,approval=?4,activating_at=?3
                     WHERE delivery_carrier=?1 AND generation=?2 AND state=2",
                    params![id, generation, at, automatic],
                )?
            }
            _ => return Err(CursorError::Stale.into()),
        };
        rows::require_write(changed)?;
        writer.guard(tx, true)?;
        Ok(changed)
    }
}

#[cfg(test)]
fn qualify_pending(
    tx: &Transaction<'_>,
    prior: &HeldEvidence,
    wall: u64,
    (group, epoch, identity, leaf): (&[u8], u64, [u8; 32], u32),
) -> Result<usize> {
    let deadline = prior.held_deadline().min(
        wall.checked_add(PENDING_WINDOW_SECONDS)
            .ok_or(CursorError::Range)?,
    );
    custody_schema::Writer::Update.guard(tx, false)?;
    Ok(tx.execute(
        "UPDATE peer_pending_custody
         SET state=2,group_id=?4,epoch_number=?5,epoch_identity=?6,own_leaf=?7,
             qualified_at=?3,pending_deadline=?8
         WHERE delivery_carrier=?1 AND generation=?2 AND state=1",
        params![
            prior.delivery_carrier(),
            seconds(prior.stage.0.generation.count)?,
            seconds(wall)?,
            group,
            seconds(epoch)?,
            identity,
            leaf,
            seconds(deadline)?
        ],
    )?)
}

#[cfg(test)]
fn exact_readback(prior: &HeldEvidence, proof: &JoinedReadback) -> bool {
    prior.stage == proof.stage() && prior.delivery_carrier() == proof.carrier()
}

#[cfg(test)]
struct HeldWrite<'a> {
    raw: RawRecord<'a>,
    delivery: &'a Delivery,
    package: &'a VerifiedDeliveryPackage,
    mode: Mode,
    wall: u64,
    deadline: u64,
}

#[cfg(test)]
impl HeldWrite<'_> {
    fn insert(self, tx: &Transaction<'_>, current: &Queued) -> Result<()> {
        let Self {
            raw,
            delivery,
            package,
            mode,
            wall,
            deadline,
        } = self;
        let prepared = &current.prepared;
        let binding = prepared.binding;
        let flight = delivery.flight();
        let transcript = current.row.transcript;
        let witness = package.binding();
        let (not_before, not_after) = package.bounds();
        let Some(acked) = current.row.ack_at else {
            return Err(CursorError::Stale.into());
        };
        if acked > wall
            || wall >= transcript.expires
            || wall < not_before
            || matches!(mode, Mode::Off)
            || raw.id != flight.carrier_id.to_bytes()
            || Selector::from(delivery.offer()) != binding.key
            || flight.author != binding.key.server
            || flight.recipient.to_bytes() != prepared.account
            || delivery.consent_revision().to_bytes() != transcript.revision
            || delivery.binding() != witness
            || witness.event_id != prepared.artifact.id
            || witness.reference != transcript.package_ref
            || witness.slot != transcript.package_slot
            || package.digest() != prepared.artifact.digest
            || prepared.artifact.expires != not_after
        {
            return Err(CursorError::Binding.into());
        }
        let writer = custody_schema::Writer::Insert;
        writer.guard(tx, false)?;
        rows::require_write(tx.execute(
        "INSERT INTO peer_pending_custody
         (class,generation,account,server,offer_id,offer_revision,origin,
          consent_carrier,consent_revision,delivery_carrier,outer_stamp,flight_created,
          expires,frame,replay_until,welcome_id,welcome_digest,package_id,package_digest,
          package_ref,package_slot,package_not_before,package_not_after,mode,held_at,held_deadline,state)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,
                 ?19,?20,?21,?22,?23,?24,?25,?26,0)",
        params![
            binding.generation.known, seconds(binding.generation.count)?, prepared.account,
            binding.key.server.to_bytes(), binding.key.id, binding.key.revision, binding.origin,
            transcript.carrier, transcript.revision, raw.id, seconds(raw.stamp)?,
            seconds(flight.created_at)?, seconds(flight.expires_at)?, raw.bytes,
            seconds(flight.replay_safe_until)?, delivery.welcome_id().to_bytes(),
            delivery.welcome_digest(), witness.event_id, package.digest(), witness.reference,
            witness.slot, seconds(not_before)?, seconds(not_after)?, matches!(mode, Mode::Automatic),
            seconds(wall)?, seconds(deadline)?,
        ],
    )?)?;
        writer.guard(tx, true)?;
        Ok(())
    }
}

#[cfg(test)]
fn load(tx: &Transaction<'_>, queued: &Queued) -> Result<Option<Saved>> {
    let binding = queued.prepared.binding;
    let transcript = queued.row.transcript;
    let result = tx.query_row(
        "SELECT delivery_carrier,outer_stamp,flight_created,expires,frame,replay_until,
                welcome_id,welcome_digest,package_id,package_digest,package_ref,package_slot,
                package_not_before,package_not_after,mode,held_at,held_deadline,state,
                started_at,group_id,epoch_number,epoch_identity,own_leaf,qualified_at,pending_deadline,approval,activating_at
         FROM peer_pending_custody
         WHERE class=?1 AND generation=?2 AND account=?3 AND server=?4 AND offer_id=?5
            AND offer_revision=?6 AND origin=?7 AND consent_carrier=?8 AND consent_revision=?9",
        params![
            binding.generation.known, seconds(binding.generation.count)?, queued.prepared.account,
            binding.key.server.to_bytes(), binding.key.id, binding.key.revision, binding.origin,
            transcript.carrier, transcript.revision,
        ],
        |row| {
            let joined = || -> rusqlite::Result<Qualified> {
                Ok(Qualified {
                    native: JoinedCoordinates {
                        group: row.get(19)?,
                        epoch: read_seconds(row, 20)?,
                        epoch_identity: row.get(21)?,
                        own_leaf: row.get(22)?,
                    },
                    qualified_at: read_seconds(row, 23)?,
                    deadline: read_seconds(row, 24)?,
                })
            };
            let started = || read_seconds(row, 18);
            let progress = match row.get::<_, u8>(17)? {
                0 => Progress::Held,
                1 => Progress::JoinStarted { started_at: started()? },
                2 => Progress::Pending { started_at: started()?, joined: joined()? },
                3 => Progress::Activating { started_at: started()?, joined: joined()?, automatic: row.get(25)?, at: read_seconds(row, 26)? },
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(Saved {
                carrier: row.get(0)?,
                outer_stamp: read_seconds(row,1)?,
                flight_created: read_seconds(row,2)?,
                expires: read_seconds(row,3)?,
                frame: Zeroizing::new(row.get(4)?),
                replay_until: read_seconds(row,5)?,
                welcome_id: row.get(6)?,
                welcome_digest: row.get(7)?,
                package: PackageBinding { event_id: row.get(8)?, reference: row.get(10)?, slot: row.get(11)? },
                package_digest: row.get(9)?,
                package_not_before: read_seconds(row,12)?,
                package_not_after: read_seconds(row,13)?,
                automatic: row.get(14)?,
                held_at: read_seconds(row,15)?,
                held_deadline: read_seconds(row,16)?,
                progress,
            })
        },
    ).optional()?;
    if let Some(ref saved) = result
        && (queued.row.ack_at.is_none_or(|ack| ack > saved.held_at)
            || saved.package.event_id != queued.prepared.artifact.id
            || saved.package_digest != queued.prepared.artifact.digest
            || saved.package.reference != transcript.package_ref
            || saved.package.slot != transcript.package_slot)
    {
        return Err(CursorError::Binding.into());
    }
    Ok(result)
}
