//! Fixture-only ownership of Consent produced from a Prepared native stage.

#[cfg(test)]
use nostr::key::Keys;

#[cfg(test)]
use super::super::super::{CursorStore, HardenedPeerConnection};
use super::*;

#[path = "peer_prepared_consent_schema.rs"]
mod schema;

#[path = "peer_pending_custody_checkpoint.rs"]
pub(crate) mod custody;

#[derive(Clone, Copy)]
pub(in super::super) enum Epoch {
    #[cfg(test)]
    Prepared,
    Custody(custody::Epoch),
}

#[cfg(test)]
impl Checkpoint {
    pub(crate) fn attach_prepared_consent_fixture(
        conn: HardenedPeerConnection,
        signer: Keys,
        uid: u32,
        urls: &[&str],
        policy: super::super::Policy,
    ) -> Result<Self> {
        let origins = urls
            .iter()
            .map(|url| super::super::Route::configured(url).map(|route| route.id))
            .collect::<Result<Vec<_>>>()?;
        let cursor = schema::open(conn, signer.public_key().to_bytes(), origins)?;
        let mut checkpoint = Self::attach_with(cursor, signer, uid, urls, policy, |_, _| Ok(()))?;
        checkpoint.provisional_schema = true;
        checkpoint.export_epoch = Some(Epoch::Prepared);
        Ok(checkpoint)
    }
}

#[cfg(test)]
#[path = "peer_prepared_consent_schema_tests.rs"]
mod schema_tests;

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
use super::super::Offer;
#[cfg(test)]
use super::super::wire;
#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
use super::super::wire::Transcript;
#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
use rusqlite::Transaction;
#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
use stage_journal::{Artifact, FileWitness, State};

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
#[path = "peer_prepared_consent_rows.rs"]
mod outbox;

#[cfg(test)]
impl super::super::Selector {
    pub(crate) fn from_delivery(incoming: &wire::IncomingDelivery) -> Self {
        let offer = incoming.offer();
        Self {
            server: offer.server,
            id: offer.id,
            revision: offer.revision,
        }
    }
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
pub(crate) struct Candidate {
    account: [u8; 32],
    binding: Binding,
    witness: FileWitness,
    artifact: Artifact,
    offer: Offer,
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
impl Candidate {
    pub(crate) fn stage(&self) -> (Binding, FileWitness) {
        (self.binding, self.witness)
    }

    fn read(
        tx: &Transaction<'_>,
        binding: Binding,
        account: [u8; 32],
        offer: Offer,
    ) -> Result<Self> {
        let Some(State::Prepared(witness, artifact)) = stage_journal::load(tx, binding)? else {
            return Err(Error::Bound);
        };
        Ok(Self {
            account,
            binding,
            witness,
            artifact,
            offer,
        })
    }
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
pub(crate) struct Queued {
    prepared: Candidate,
    row: outbox::Outbox,
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
impl Queued {
    #[cfg(test)]
    pub(crate) fn frame(&self) -> &[u8] {
        self.row.frame.as_ref()
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(crate) fn publication_state(&self) -> (u64, Option<u64>, Option<u64>) {
        (self.row.attempt, self.row.retry_at, self.row.ack_at)
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    fn deadline(&self) -> u64 {
        self.row
            .transcript
            .expires
            .min(self.prepared.offer.flight().expires_at)
            .min(self.prepared.artifact.expires)
    }

    pub(crate) fn stage(&self) -> (Binding, FileWitness) {
        self.prepared.stage()
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(super) fn matches_exact(&self, prior: &Self) -> bool {
        self.stage() == prior.stage()
            && self.prepared.artifact == prior.prepared.artifact
            && self.prepared.offer.flight().carrier_id == prior.prepared.offer.flight().carrier_id
            && self.row == prior.row
    }
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
#[derive(Clone)]
pub(crate) struct PackageTarget {
    owner: std::sync::Arc<()>,
    binding: Binding,
    file: FileWitness,
    artifact: Artifact,
    deadline: u64,
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
impl PackageTarget {
    pub(crate) fn stage(&self) -> (Binding, FileWitness) {
        (self.binding, self.file)
    }

    pub(crate) fn artifact(&self) -> Artifact {
        self.artifact
    }

    pub(crate) fn deadline(&self) -> u64 {
        self.deadline
    }
}

/// Read-only challenge bound to the current checkpoint incarnation and exact
/// outbox bytes, including its attempt, retry and acknowledgment state.
#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(crate) struct ManagedQueued {
    target: PackageTarget,
    queued: Queued,
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
impl ManagedQueued {
    pub(crate) fn target(&self) -> &PackageTarget {
        &self.target
    }

    pub(crate) fn is_due(&self, wall: u64) -> bool {
        self.queued.row.ack_at.is_none()
            && self.queued.row.retry_at.is_none_or(|retry| retry <= wall)
    }

    #[cfg(test)]
    pub(crate) fn fixture_retry_at(&self) -> Option<u64> {
        self.queued.row.retry_at
    }

    #[cfg(test)]
    pub(crate) fn fixture_frame(&self) -> ([u8; 32], &[u8]) {
        (self.queued.row.transcript.carrier, &self.queued.row.frame)
    }

    #[cfg(test)]
    pub(crate) fn fixture_attempt(&self) -> (u64, Option<u64>) {
        (self.queued.row.attempt, self.queued.row.ack_at)
    }

    pub(crate) fn matches_saved(&self, saved: &Queued) -> bool {
        self.queued.matches_exact(saved)
    }
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
impl Checkpoint {
    pub(crate) fn require_managed_package_target(&self, target: &PackageTarget) -> Result<()> {
        self.check_local_owner(&target.owner)
    }
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
pub(crate) struct Unsaved {
    binding: Binding,
    witness: FileWitness,
    artifact: Artifact,
    offer_carrier: [u8; 32],
    row: outbox::Outbox,
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PreparedUse {
    Fixture,
    ManagedRead,
    ManagedWrite,
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
impl Checkpoint {
    pub(crate) fn require_reservation_owner(&self, reservation: &Reservation) -> Result<()> {
        self.check_local_owner(&reservation.owner)
    }

    #[cfg(test)]
    pub(crate) fn inspect_prepared_consent(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        wall: u64,
    ) -> Result<Candidate> {
        self.inspect_exact_prepared(source, reservation, wall, false)
    }

    fn inspect_exact_prepared(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        wall: u64,
        managed: bool,
    ) -> Result<Candidate> {
        self.require_reservation_owner(reservation)?;
        self.read_live_prepared(
            source,
            reservation.binding,
            wall,
            if managed {
                PreparedUse::ManagedRead
            } else {
                PreparedUse::Fixture
            },
            |_, candidate, _, _| Ok(candidate),
            |_| Ok(()),
        )
    }

    fn read_live_prepared<T>(
        &mut self,
        source: &Source,
        binding: Binding,
        wall: u64,
        purpose: PreparedUse,
        work: impl FnOnce(&Transaction<'_>, Candidate, &[Origin], &Epoch) -> Result<T>,
        before_commit: impl FnOnce(&Transaction<'_>) -> Result<()>,
    ) -> Result<T> {
        let managed = purpose != PreparedUse::Fixture;
        if managed
            && (binding.generation.known
                || !self.is_installed_custody()
                || !matches!(
                    self.installed_receive_policy(),
                    Some(custody::ReceivePolicy::Manual)
                ))
        {
            return Err(Error::Policy);
        }
        let epoch = self.export_epoch.ok_or(CursorError::Binding)?;
        let account = self.signer.public_key().to_bytes();
        let signer = self.signer.clone();
        let codec = self.codec;
        let route = self.route(binding.origin)?;
        let signed_route = super::super::Route::configured(&route.urls[0])?;
        let origins = self.cursor.origins.clone();
        // Stage access and schema checking run through the same exact owner turn.
        let (tx, _) = self.stage_transaction_with_expiry(source, binding, wall, !managed)?;
        schema::verify_for_epoch(&tx, account, &origins, &epoch)?;
        let current = Invitation::load(&tx, binding.key, binding.origin)?;
        if current.decision_at.is_some() {
            return Err(CursorError::Binding.into());
        }
        let offer = Self::check_signed_offer(&codec, &signer, &signed_route, &current, wall)?;
        if managed
            && (offer.version() != crate::peer_control_version::ControlVersion::V2
                || custody::owns(&tx, binding.key, binding.origin)?
                || outbox::exists(&tx, binding.key, binding.origin)?)
        {
            return Err(CursorError::Stale.into());
        }
        let candidate = Candidate::read(&tx, binding, account, offer)?;
        if managed && wall >= candidate.artifact.expires {
            return Err(Error::Expired);
        }
        let result = work(&tx, candidate, &origins, &epoch)?;
        if purpose == PreparedUse::ManagedRead {
            return Ok(result);
        }
        if managed {
            rows::advance_wall(&tx, wall)?;
            before_commit(&tx)?;
        } else {
            before_commit(&tx)?;
            rows::advance_wall(&tx, wall)?;
        }
        tx.commit()?;
        Ok(result)
    }

    #[cfg(test)]
    pub(crate) fn sign_prepared_consent(
        &self,
        candidate: &Candidate,
        package: &crate::peer_invitation::prepared_consent::NativeAttestation,
        wall: u64,
    ) -> Result<Unsaved> {
        let exact = package.require(candidate.stage(), candidate.artifact, wall)?;
        self.sign_exact_prepared(candidate, exact, wall)
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(crate) fn sign_managed_prepared_fixture(
        &self,
        candidate: &Candidate,
        target: &PackageTarget,
        exact: &[u8],
        wall: u64,
    ) -> Result<Unsaved> {
        self.require_managed_package_target(target)?;
        if candidate.stage() != target.stage()
            || candidate.artifact != target.artifact
            || wall >= target.deadline
            || Artifact::new(target.artifact.id, exact, target.artifact.expires) != target.artifact
        {
            return Err(Error::Package);
        }
        self.sign_exact_prepared(candidate, exact, wall)
    }

    fn sign_exact_prepared(
        &self,
        candidate: &Candidate,
        exact: &[u8],
        wall: u64,
    ) -> Result<Unsaved> {
        let prepared = self
            .codec
            .prepare_capped_fixture_consent(
                &self.signer,
                &candidate.offer,
                exact,
                candidate.artifact.expires,
                wall,
            )
            .map_err(|_| Error::Package)?;
        let row = outbox::Outbox::new(
            candidate,
            Transcript::from_consent(&prepared.flight),
            prepared.frame(),
        );
        Ok(Unsaved {
            binding: candidate.binding,
            witness: candidate.witness,
            artifact: candidate.artifact,
            offer_carrier: candidate.offer.flight().carrier_id.to_bytes(),
            row,
        })
    }

    #[cfg(test)]
    pub(crate) fn queue_prepared_consent(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        unsaved: Unsaved,
        wall: u64,
        before_commit: impl FnOnce(&Transaction<'_>) -> Result<()>,
    ) -> Result<Queued> {
        self.queue_exact_prepared(source, reservation, unsaved, wall, false, before_commit)
    }

    fn queue_exact_prepared(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        unsaved: Unsaved,
        wall: u64,
        managed: bool,
        before_commit: impl FnOnce(&Transaction<'_>) -> Result<()>,
    ) -> Result<Queued> {
        self.check_local_owner(&reservation.owner)?;
        if reservation.binding != unsaved.binding {
            return Err(CursorError::Stale.into());
        }
        self.read_live_prepared(
            source,
            reservation.binding,
            wall,
            if managed {
                PreparedUse::ManagedWrite
            } else {
                PreparedUse::Fixture
            },
            |tx, prepared, origins, epoch| {
                if (prepared.witness, prepared.artifact) != (unsaved.witness, unsaved.artifact)
                    || wall >= unsaved.row.transcript.expires
                    || (managed
                        && prepared.offer.flight().carrier_id.to_bytes() != unsaved.offer_carrier)
                {
                    return Err(Error::Package);
                }
                unsaved.row.insert(tx, &prepared)?;
                schema::verify_for_epoch(tx, prepared.account, origins, epoch)?;
                Ok(Queued {
                    prepared,
                    row: unsaved.row,
                })
            },
            before_commit,
        )
    }

    #[cfg(test)]
    pub(crate) fn fixture_read_offer(
        &mut self,
        source: &Source,
        key: Selector,
        creation_wall: u64,
    ) -> Result<Offer> {
        let origin = self.origin(source)?;
        let route = super::super::Route::configured(&self.route(origin)?.urls[0])?;
        let tx = self.cursor.begin(origin)?;
        let row = Invitation::load(&tx, key, origin)?;
        Self::check_signed_offer(&self.codec, &self.signer, &route, &row, creation_wall)
    }

    #[cfg(test)]
    pub(crate) fn recover_queued_consent(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
    ) -> Result<Option<Queued>> {
        let epoch = self.export_epoch.ok_or(CursorError::Binding)?;
        let origin = self.origin(source)?;
        if !self.policy.permits(key.server) {
            return Err(Error::Policy);
        }
        let route = self.route(origin)?;
        let signed_route = super::super::Route::configured(&route.urls[0])?;
        let origins = self.cursor.origins.clone();
        let account = self.signer.public_key().to_bytes();
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::check_wall(&tx, wall)?;
        schema::verify_for_epoch(&tx, account, &origins, &epoch)?;
        if matches!(epoch, Epoch::Custody(_)) && custody::owns(&tx, key, origin)? {
            return Ok(None);
        }
        let quarantined: bool = tx.query_row(
            "SELECT quarantine_until IS NOT NULL FROM peer_cursor_origin WHERE origin=?1",
            [origin],
            |r| r.get(0),
        )?;
        if quarantined {
            return Err(CursorError::RelayContract.into());
        }
        if !outbox::exists(&tx, key, origin)? {
            return Ok(None);
        }
        let generation = generation(&tx, key, origin)?.ok_or(CursorError::Binding)?;
        let binding = Binding {
            key,
            origin,
            generation,
        };
        let invite = Invitation::load(&tx, key, origin)?;
        let signed_wall = invite.expires.checked_sub(1).ok_or(CursorError::Range)?;
        let offer = Self::check_signed_offer(
            &self.codec,
            &self.signer,
            &signed_route,
            &invite,
            signed_wall,
        )?;
        let prepared = Candidate::read(&tx, binding, account, offer)?;
        let row = outbox::load(&tx, &prepared)?.ok_or(CursorError::Binding)?;
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        if wall >= row.transcript.expires {
            return Err(Error::Expired);
        }
        Ok(Some(Queued { prepared, row }))
    }

    #[cfg(test)]
    pub(crate) fn verify_queued_delivery(
        &self,
        recovered: &Queued,
        package: &crate::peer_invitation::prepared_consent::NativeAttestation,
        incoming: wire::IncomingDelivery,
        wall: u64,
    ) -> Result<wire::Delivery> {
        let prepared = &recovered.prepared;
        if super::super::Selector::from_delivery(&incoming) != prepared.binding.key
            || recovered.row.ack_at.is_none()
        {
            return Err(CursorError::Binding.into());
        }
        let exact = package.require(prepared.stage(), prepared.artifact, wall)?;
        recovered
            .row
            .transcript
            .verify_delivery(
                &self.signer,
                &prepared.offer,
                &recovered.row.frame,
                exact,
                wall,
                incoming,
            )
            .map_err(|_| Error::Package)
    }

    #[cfg(test)]
    pub(crate) fn verify_queued_consent(
        &self,
        recovered: &Queued,
        package: &crate::peer_invitation::prepared_consent::NativeAttestation,
        wall: u64,
    ) -> Result<Transcript> {
        let prepared = &recovered.prepared;
        let exact = package.require(prepared.stage(), prepared.artifact, wall)?;
        recovered
            .row
            .transcript
            .verify(
                &self.signer,
                &prepared.offer,
                &recovered.row.frame,
                exact,
                wall,
            )
            .map_err(|_| Error::Package)?;
        Ok(recovered.row.transcript)
    }
}

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
#[path = "peer_prepared_consent_attempt.rs"]
pub(crate) mod publication;
#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
pub(crate) use publication::Attempt;
