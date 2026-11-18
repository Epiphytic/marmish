//! Measured native Welcome turns for the private installed-v2 recipient proof.

use std::{
    fs::{self, OpenOptions},
    io::{self, Read as _},
    os::unix::fs::OpenOptionsExt as _,
    ptr,
    sync::atomic::AtomicBool,
};

use crate::{
    mdk_adapter::peer_native_signer::{JoinWitness, JoinedAuthority},
    peer_quota_vfs::dispatch::{JoinMetrics, JoinTurn},
};

use super::*;
use clean::CleanFiles;
use consent_checkpoint::{
    provisioning::{
        prepared_consent::custody::{CustodyState, JoinStarted},
        stage_journal::FileWitness,
    },
    wire::{Delivery, StatusNegotiation},
};
use prepared_consent::{read_image, snapshot_inode, snapshot_sidecars};
use socket::owner::recipient::PeerRecipientPort;

pub(in crate::peer_invitation) struct PendingReceiver<'lease> {
    owner: RecoveredOwner<'lease>,
    native: NativePending<'lease>,
}

pub(in crate::peer_invitation) struct ArmedWelcome<'lease> {
    prepared: PreparedWelcome<'lease>,
    started: JoinStarted,
}

impl<'lease> PendingReceiver<'lease> {
    pub(in crate::peer_invitation) fn open(
        lease: &'lease ProfileLease,
        quota: &'lease ReceiverQuota<'lease>,
        material: &IdentityMaterial,
        origin: &'lease str,
        port: &mut PeerRecipientPort,
    ) -> Result<Self> {
        // The only stock cursor opens after checking the managed quota binding.
        quota.receiver_files(lease).map_err(|_| Error::Path)?;
        let owner = port.use_current_recipient(|account, observed_origin, _| {
            if account != material.public_key() || observed_origin != origin {
                return Err(Error::Binding);
            }
            RecoveredOwner::open(lease, material, origin)
        })??;
        Ok(Self {
            owner,
            native: NativePending {
                lease,
                quota,
                signer: material.signer(),
                origin,
            },
        })
    }

    pub(in crate::peer_invitation) fn custody_state(
        &mut self,
        port: &mut PeerRecipientPort,
        selector: Selector,
        generation: u64,
    ) -> Result<(CustodyState, [u8; 32])> {
        self.owner.cursor.unchanged()?;
        let (held, _, _) = self.owner.checkpoint.read_installed_join_custody(
            &self.owner.source,
            port,
            selector,
            generation,
        )?;
        Ok((held.state(), held.delivery_carrier()))
    }

    pub(in crate::peer_invitation) fn arm(
        &mut self,
        port: &mut PeerRecipientPort,
        selector: Selector,
        generation: u64,
        stop: &AtomicBool,
    ) -> Result<ArmedWelcome<'lease>> {
        self.owner.cursor.unchanged()?;
        let (held, delivery, wall) = self.owner.checkpoint.read_installed_join_custody(
            &self.owner.source,
            port,
            selector,
            generation,
        )?;
        let clock = Wall::System;
        let fence = TurnFence::new(&clock, stop, None, wall, held.held_deadline())?;
        let prepared = self
            .native
            .prepare(&mut self.owner, &held, &delivery, &fence)?;
        fence.wall()?;
        self.owner.cursor.unchanged()?;
        let started = self.owner.checkpoint.start_installed_join(
            &self.owner.source,
            port,
            &held,
            &prepared,
            self.native.quota,
            &fence,
        )?;
        Ok(ArmedWelcome { prepared, started })
    }

    pub(in crate::peer_invitation) fn ingest(
        &mut self,
        port: &mut PeerRecipientPort,
        armed: ArmedWelcome<'lease>,
        stop: &AtomicBool,
    ) -> Result<(Result<()>, JoinMetrics)> {
        self.owner.cursor.unchanged()?;
        let (_, generation, _, selector) = armed.prepared.enrollment.stage.0.coordinates();
        let (current, delivery, wall) = self.owner.checkpoint.read_installed_join_custody(
            &self.owner.source,
            port,
            selector,
            generation,
        )?;
        if current.state() != CustodyState::JoinStarted {
            return Err(Error::Binding);
        }
        let clock = Wall::System;
        let fence = TurnFence::new(&clock, stop, None, wall, current.held_deadline())?;
        self.native
            .join(&mut self.owner, port, &fence, armed, &current, &delivery)
    }

    pub(in crate::peer_invitation) fn qualify(
        &mut self,
        port: &mut PeerRecipientPort,
        selector: Selector,
        generation: u64,
        stop: &AtomicBool,
    ) -> Result<InstalledJoinedReadback<'lease>> {
        self.owner.cursor.unchanged()?;
        let (held, delivery, wall) = self.owner.checkpoint.read_installed_join_custody(
            &self.owner.source,
            port,
            selector,
            generation,
        )?;
        let deadline = held
            .pending_deadline()
            .unwrap_or_else(|| held.held_deadline());
        let clock = Wall::System;
        let fence = TurnFence::new(&clock, stop, None, wall, deadline)?;
        let joined = self
            .native
            .read_joined(&mut self.owner, &held, &delivery, &fence)?;
        fence.wall()?;
        self.owner.cursor.unchanged()?;
        self.owner.checkpoint.commit_installed_pending(
            &self.owner.source,
            port,
            &held,
            &joined,
            self.native.quota,
            &fence,
        )?;
        Ok(joined)
    }
}

pub(in crate::peer_invitation) struct PreparedWelcome<'lease> {
    quota: &'lease ReceiverQuota<'lease>,
    enrollment: Enrollment,
    image: [u8; 32],
}

pub(in crate::peer_invitation) struct InstalledJoinedReadback<'lease> {
    quota: &'lease ReceiverQuota<'lease>,
    enrollment: Enrollment,
    authority: JoinedAuthority,
}

struct Enrollment {
    stage: (Binding, FileWitness),
    carrier: [u8; 32],
    package_digest: [u8; 32],
    deadline: u64,
    negotiation: StatusNegotiation,
}

impl Enrollment {
    fn from_delivery(held: &HeldEvidence, delivery: &Delivery) -> Result<Self> {
        let flight = delivery.flight();
        let status = delivery.status_negotiation().ok_or(Error::Binding)?;
        if held.is_active()
            || held.stage().0.coordinates().0
            || Selector::from(held.offer()) != Selector::from(delivery.offer())
            || held.offer().routing() != delivery.offer().routing()
            || held.offer().flight().recipient != flight.recipient
            || held.offer().flight().author != flight.author
            || held.delivery_carrier() != flight.carrier_id.to_bytes()
            || held.flight_created_at() != flight.created_at
            || held.delivery_expires() != flight.expires_at
            || held.replay_until() != flight.replay_safe_until
            || held.package() != delivery.binding()
            || held.transcript().revision != status.consent_revision()
            || held.welcome_id() != delivery.welcome_id().to_bytes()
            || held.welcome_digest() != delivery.welcome_digest()
        {
            return Err(Error::Binding);
        }
        Ok(Self {
            stage: held.stage(),
            carrier: held.delivery_carrier(),
            package_digest: held.package_digest(),
            deadline: held.held_deadline(),
            negotiation: status,
        })
    }

    fn matches(&self, held: &HeldEvidence, delivery: &Delivery) -> bool {
        let Ok(current) = Self::from_delivery(held, delivery) else {
            return false;
        };
        let a = &self.negotiation;
        let b = &current.negotiation;
        self.stage == current.stage
            && self.carrier == current.carrier
            && self.package_digest == current.package_digest
            && self.deadline == current.deadline
            && a.receipt_coordinates() == b.receipt_coordinates()
            && a.consent_revision() == b.consent_revision()
            && a.delivery_revision() == b.delivery_revision()
            && a.routing() == b.routing()
            && a.app_room() == b.app_room()
    }
}

impl PreparedWelcome<'_> {
    pub(in crate::peer_invitation) fn matches(
        &self,
        held: &HeldEvidence,
        delivery: &Delivery,
    ) -> bool {
        self.enrollment.matches(held, delivery)
    }

    pub(in crate::peer_invitation) fn matches_quota(&self, quota: &ReceiverQuota<'_>) -> bool {
        ptr::eq(self.quota, quota)
    }
}

impl InstalledJoinedReadback<'_> {
    pub(in crate::peer_invitation) fn matches(
        &self,
        held: &HeldEvidence,
        delivery: &Delivery,
    ) -> bool {
        self.enrollment.matches(held, delivery)
    }

    pub(in crate::peer_invitation) fn matches_quota(&self, quota: &ReceiverQuota<'_>) -> bool {
        ptr::eq(self.quota, quota)
    }

    pub(in crate::peer_invitation) fn coordinates(&self) -> (&[u8], u64, [u8; 32], u32) {
        self.authority.coordinates()
    }
}

struct NativePending<'lease> {
    lease: &'lease ProfileLease,
    quota: &'lease ReceiverQuota<'lease>,
    signer: Arc<Keys>,
    origin: &'lease str,
}

impl<'lease> NativePending<'lease> {
    fn check(
        &self,
        owner: &RecoveredOwner<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
    ) -> Result<()> {
        if !owner.paths.owns_lease(self.lease)
            || owner.source.origin != Route::configured(self.origin)?.id
            || delivery.flight().recipient != self.signer.public_key()
            || delivery.offer().routing().relays != [self.origin]
            || Selector::from(held.offer()) != Selector::from(delivery.offer())
        {
            return Err(Error::Binding);
        }
        owner.cursor.unchanged()
    }

    fn prepare(
        &self,
        owner: &mut RecoveredOwner<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
        fence: &TurnFence<'_>,
    ) -> Result<PreparedWelcome<'lease>> {
        self.check(owner, held, delivery)?;
        if held.state() != CustodyState::Held {
            return Err(Error::Binding);
        }
        let enrollment = Enrollment::from_delivery(held, delivery)?;
        fence.require_deadline(enrollment.deadline)?;
        let (binding, witness) = enrollment.stage;
        let welcome = JoinWitness::new(delivery, self.origin, enrollment.package_digest)?;
        let key = SqlCipherKey::raw(*owner.paths.provisional_key(binding));
        let operation = self.operation(owner, binding, &key, &welcome, fence);
        let files = CleanFiles::begin(owner, self.quota, self.lease, binding, fence)?;
        let original = owner.paths.reopen_provisional(binding, witness)?;
        let ((), image) = inspect_copy(owner, &files, &original, fence, true, |permit| {
            Ok(files
                .files()
                .prepare_join(permit, operation)
                .map_err(|_| Error::Path)??)
        })?;
        Ok(PreparedWelcome {
            quota: self.quota,
            enrollment,
            image,
        })
    }

    fn join(
        &self,
        owner: &mut RecoveredOwner<'_>,
        port: &mut PeerRecipientPort,
        fence: &TurnFence<'_>,
        armed: ArmedWelcome<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
    ) -> Result<(Result<()>, JoinMetrics)> {
        let ArmedWelcome { prepared, started } = armed;
        self.check(owner, held, delivery)?;
        if !started.matches(held)
            || !prepared.matches_quota(self.quota)
            || !prepared.matches(held, delivery)
        {
            return Err(Error::Binding);
        }
        fence.require_deadline(prepared.enrollment.deadline)?;
        let (binding, witness) = prepared.enrollment.stage;
        let welcome = JoinWitness::new(delivery, self.origin, prepared.enrollment.package_digest)?;
        let key = SqlCipherKey::raw(*owner.paths.provisional_key(binding));
        let operation = self.operation(owner, binding, &key, &welcome, fence);
        let files = CleanFiles::begin(owner, self.quota, self.lease, binding, fence)?;
        let original = owner.paths.reopen_provisional(binding, witness)?;
        let permit = files.original_permit(owner, fence)?;
        if original.digest()? != prepared.image {
            return Err(Error::StageIdentityMismatch);
        }
        let attempt = port.use_current_recipient(|account, origin, wall| {
            if account != self.signer.public_key()
                || origin != self.origin
                || wall >= prepared.enrollment.deadline
            {
                return Err(Error::Binding);
            }
            files
                .files()
                .join_original(permit, operation)
                .map_err(|_| Error::Path)
        })?;
        files.finish(owner, fence, false)?;
        let (result, metrics) = attempt?;
        port.use_current_recipient(|account, origin, wall| {
            if account != self.signer.public_key()
                || origin != self.origin
                || wall >= prepared.enrollment.deadline
            {
                return Err(Error::Binding);
            }
            Ok(())
        })??;
        Ok((result.map_err(Into::into), metrics))
    }

    fn read_joined(
        &self,
        owner: &mut RecoveredOwner<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
        fence: &TurnFence<'_>,
    ) -> Result<InstalledJoinedReadback<'lease>> {
        self.check(owner, held, delivery)?;
        let enrollment = Enrollment::from_delivery(held, delivery)?;
        let deadline = match held.state() {
            CustodyState::JoinStarted => enrollment.deadline,
            CustodyState::Pending => held.pending_deadline().ok_or(Error::Binding)?,
            _ => return Err(Error::Binding),
        };
        fence.require_deadline(deadline)?;
        let (binding, witness) = enrollment.stage;
        if held.state() == CustodyState::JoinStarted
            && matches!(
                owner.recover(fence.wall()?)?.classes[1],
                Class::JournalCandidate { binding: recovered, .. } if recovered == binding
            )
        {
            owner.rollback_journal(self.lease, self.quota, binding, fence, false)?;
        }
        let welcome = JoinWitness::new(delivery, self.origin, enrollment.package_digest)?;
        let key = SqlCipherKey::raw(*owner.paths.provisional_key(binding));
        let operation = self.operation(owner, binding, &key, &welcome, fence);
        let files = CleanFiles::begin(owner, self.quota, self.lease, binding, fence)?;
        let original = owner.paths.reopen_provisional(binding, witness)?;
        let (authority, _) = inspect_copy(owner, &files, &original, fence, false, |permit| {
            Ok(files
                .files()
                .read_join(permit, operation)
                .map_err(|_| Error::Path)??)
        })?;
        if authority.coordinates().0 == enrollment.negotiation.app_room() {
            return Err(Error::Binding);
        }
        Ok(InstalledJoinedReadback {
            quota: self.quota,
            enrollment,
            authority,
        })
    }

    fn operation<'a>(
        &'a self,
        owner: &RecoveredOwner<'_>,
        binding: Binding,
        key: &'a SqlCipherKey,
        join: &'a JoinWitness<'a>,
        fence: &'a TurnFence<'a>,
    ) -> JoinTurn<'a> {
        JoinTurn {
            key,
            signer: self.signer.clone(),
            origin: self.origin,
            label: owner.paths.provisional_label(binding),
            join,
            fence,
        }
    }
}

fn inspect_copy<T>(
    owner: &mut RecoveredOwner<'_>,
    files: &CleanFiles<'_>,
    original: &paths::ProvisionalFile,
    fence: &TurnFence<'_>,
    original_replica_required: bool,
    inspect: impl FnOnce(CleanPermit<'_>) -> Result<T>,
) -> Result<(T, [u8; 32])> {
    let image = original.digest()?;
    let reader = original.read_file()?;
    let length = reader.metadata().map_err(|_| Error::Path)?.len();
    let path = files.snapshot_path(length)?;
    fence.wall()?;
    let mut copy = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Error::Path)?;
    let identity = snapshot_inode(&copy.metadata().map_err(|_| Error::Path)?)?;
    let copied = io::copy(&mut reader.take(length), &mut copy).map_err(|_| Error::Path)?;
    drop(copy);
    if copied != length || read_image(path)? != (image, identity) || original.digest()? != image {
        return Err(Error::StageIdentityMismatch);
    }

    let result = inspect(files.snapshot_permit(owner, identity, fence)?);
    files.snapshot_closed(owner, identity, fence)?;
    let (digest, after) = read_image(path)?;
    if after != identity || original.digest()? != image {
        return Err(Error::StageIdentityMismatch);
    }
    fs::remove_file(path).map_err(|_| Error::Path)?;
    snapshot_sidecars(path)?;
    files.finish(owner, fence, true)?;
    if original_replica_required && result.is_ok() && digest != image {
        return Err(Error::StageIdentityMismatch);
    }
    Ok((result?, image))
}
