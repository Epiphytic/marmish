//! Single-use Welcome ingress and readback under the private fixture quota.

use super::*;
use crate::peer_quota_vfs::dispatch::{
    FixtureFileTurn, JoinMetrics, JoinTurn, ProfileSlots, RawBinding,
};
use consent_checkpoint::provisioning::prepared_consent::custody::{CustodyState, JoinStarted};
use std::num::NonZeroU64;

pub(in crate::peer_invitation) struct CappedJoin<'a> {
    lease: &'a ProfileLease,
    slots: ProfileSlots<'a>,
}

pub(in crate::peer_invitation) struct PreparedJoin<'a> {
    owner: &'a ProfileSlots<'a>,
    stage: (Binding, FileWitness),
    carrier: [u8; 32],
    digest: [u8; 32],
}

pub(in crate::peer_invitation) struct CappedJoinedReadback(JoinedReadback);

impl CappedJoinedReadback {
    pub(in crate::peer_invitation) fn proof(&self) -> &JoinedReadback {
        &self.0
    }
}

impl<'a> CappedJoin<'a> {
    pub(in crate::peer_invitation) fn new(
        lease: &'a ProfileLease,
        external_fixture_charge: u64,
    ) -> Result<Self> {
        Ok(Self {
            lease,
            slots: ProfileSlots::reserve(lease, external_fixture_charge)
                .map_err(|_| Error::Path)?,
        })
    }

    fn bind(
        &self,
        state: &InvitationState<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
        fence: &TurnFence<'_>,
        deadline: u64,
    ) -> Result<()> {
        let flight = delivery.flight();
        if !state.paths.owns_lease(self.lease)
            || held.is_active()
            || delivery.offer().version() != held.offer().version()
            || delivery.offer().flight().revision != held.offer().flight().revision
            || delivery.offer().routing() != held.offer().routing()
            || flight.author != held.offer().flight().author
            || flight.recipient != state.signer.public_key()
            || flight.carrier_id.to_bytes() != held.delivery_carrier()
            || flight.created_at != held.flight_created_at()
            || flight.expires_at != held.delivery_expires()
            || flight.replay_safe_until != held.replay_until()
            || delivery.consent_revision().to_bytes() != held.transcript().revision
            || delivery.binding() != held.package()
            || delivery.welcome_id().to_bytes() != held.welcome_id()
            || delivery.welcome_digest() != held.welcome_digest()
        {
            return Err(Error::Binding);
        }
        fence.require_deadline(deadline)
    }

    pub(in crate::peer_invitation) fn prepare(
        &self,
        state: &InvitationState<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
        fence: &TurnFence<'_>,
    ) -> Result<PreparedJoin<'_>> {
        if held.state() != CustodyState::Held {
            return Err(Error::Binding);
        }
        self.bind(state, held, delivery, fence, held.held_deadline())?;
        let stage = held.stage();
        let files = self.slots.file_turn().map_err(|_| Error::Path)?;
        let original = state.paths.reopen_provisional(stage.0, stage.1)?;
        let welcome = JoinWitness::new(delivery, &state.origin_url, held.package_digest())?;
        let key = SqlCipherKey::raw(*state.paths.provisional_key(stage.0));
        let operation = join_turn(state, stage.0, &welcome, &key, fence);
        let ((), digest) = checked_snapshot(
            &files,
            &original,
            fence,
            SnapshotCopy::Unjoined,
            |path, inode| {
                Ok(files
                    .prepare_join_snapshot(path, inode, operation)
                    .map_err(|_| Error::Path)??)
            },
        )?;
        Ok(PreparedJoin {
            owner: &self.slots,
            stage,
            carrier: held.delivery_carrier(),
            digest,
        })
    }

    pub(in crate::peer_invitation) fn join(
        &self,
        state: &InvitationState<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
        fence: &TurnFence<'_>,
        prepared: PreparedJoin<'_>,
        started: JoinStarted,
    ) -> Result<()> {
        let (outcome, _) = self.join_once(state, held, delivery, fence, prepared, started)?;
        Ok(outcome?)
    }

    fn join_once(
        &self,
        state: &InvitationState<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
        fence: &TurnFence<'_>,
        prepared: PreparedJoin<'_>,
        started: JoinStarted,
    ) -> Result<(std::result::Result<(), StageError>, JoinMetrics)> {
        self.bind(state, held, delivery, fence, held.held_deadline())?;
        if !started.matches(held)
            || !std::ptr::eq(prepared.owner, &self.slots)
            || prepared.stage != held.stage()
            || prepared.carrier != held.delivery_carrier()
        {
            return Err(Error::Binding);
        }
        let (binding, file) = held.stage();
        let files = self.slots.file_turn().map_err(|_| Error::Path)?;
        let original = state.paths.reopen_provisional(binding, file)?;
        let (known, generation, _, _) = binding.coordinates();
        let welcome = JoinWitness::new(delivery, &state.origin_url, held.package_digest())?;
        let key = SqlCipherKey::raw(*state.paths.provisional_key(binding));
        let operation = join_turn(state, binding, &welcome, &key, fence);
        let raw = RawBinding {
            path: original.path(),
            generation: NonZeroU64::new(generation).ok_or(Error::Binding)?,
            known,
            witness: (
                u64::from_be_bytes(file.device),
                u64::from_be_bytes(file.inode),
            ),
        };
        if original.digest()? != prepared.digest {
            return Err(Error::StageIdentityMismatch);
        }
        let result = files.join_original(raw, operation).map_err(|_| Error::Path);
        original.ensure_unchanged()?;
        fence.wall()?;
        result
    }

    pub(in crate::peer_invitation) fn read_joined(
        &self,
        state: &InvitationState<'_>,
        held: &HeldEvidence,
        delivery: &Delivery,
        fence: &TurnFence<'_>,
    ) -> Result<CappedJoinedReadback> {
        let deadline = match held.state() {
            CustodyState::JoinStarted => held.held_deadline(),
            CustodyState::Pending => held.pending_deadline().ok_or(Error::Binding)?,
            _ => return Err(Error::Binding),
        };
        self.bind(state, held, delivery, fence, deadline)?;
        let stage = held.stage();
        let files = self.slots.file_turn().map_err(|_| Error::Path)?;
        let original = state.paths.reopen_provisional(stage.0, stage.1)?;
        let welcome = JoinWitness::new(delivery, &state.origin_url, held.package_digest())?;
        let key = SqlCipherKey::raw(*state.paths.provisional_key(stage.0));
        let operation = join_turn(state, stage.0, &welcome, &key, fence);
        let (authority, _) = checked_snapshot(
            &files,
            &original,
            fence,
            SnapshotCopy::Joined,
            |path, inode| {
                Ok(files
                    .read_join_snapshot(path, inode, operation)
                    .map_err(|_| Error::Path)??)
            },
        )?;
        Ok(CappedJoinedReadback(JoinedReadback {
            stage,
            carrier: held.delivery_carrier(),
            authority,
        }))
    }
}

fn join_turn<'a>(
    state: &'a InvitationState<'_>,
    binding: Binding,
    welcome: &'a JoinWitness<'a>,
    key: &'a SqlCipherKey,
    fence: &'a TurnFence<'a>,
) -> JoinTurn<'a> {
    JoinTurn {
        key,
        signer: state.signer.clone(),
        origin: &state.origin_url,
        label: state.paths.provisional_label(binding),
        join: welcome,
        fence: crate::mdk_adapter::peer_native_signer::WelcomeFence::Fixture(fence),
    }
}

fn checked_snapshot<T>(
    files: &FixtureFileTurn<'_>,
    original: &paths::ProvisionalFile,
    fence: &TurnFence<'_>,
    mode: SnapshotCopy,
    inspect: impl FnOnce(&Path, (u64, u64)) -> Result<T>,
) -> Result<(T, [u8; 32])> {
    let image = original.digest()?;
    let reader = original.read_file()?;
    let length = reader.metadata().map_err(|_| Error::Path)?.len();
    let path = files.snapshot_path(length).map_err(|_| Error::Path)?;
    fence.wall()?;
    let mut copy = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Error::Path)?;
    let result = (|| {
        let inode = snapshot_inode(&copy.metadata().map_err(|_| Error::Path)?)?;
        let old = original.witness();
        if inode.0 != u64::from_be_bytes(old.device) || inode.1 == u64::from_be_bytes(old.inode) {
            return Err(Error::StageIdentityMismatch);
        }
        let copied = io::copy(&mut reader.take(length), &mut copy).map_err(|_| Error::Path)?;
        drop(copy);
        if copied != length || read_image(path)? != (image, inode) || original.digest()? != image {
            return Err(Error::StageIdentityMismatch);
        }
        let inspected = inspect(path, inode);
        let (digest, after) = read_image(path)?;
        if after != inode || original.digest()? != image {
            return Err(Error::StageIdentityMismatch);
        }
        fs::remove_file(path).map_err(|_| Error::Path)?;
        snapshot_sidecars(path)?;
        if matches!(mode, SnapshotCopy::Unjoined) && inspected.is_ok() && digest != image {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(inspected)
    })()
    .inspect_err(|_| files.quarantine_snapshot())?;
    let value = result?;
    fence.wall()?;
    Ok((value, image))
}

#[path = "peer_capped_join_tests.rs"]
mod tests;
