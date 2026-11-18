//! Prepared native custody without any physical publication.

use std::{num::NonZeroU64, path::Path};
use storage_sqlite::SqlCipherKey;

use crate::peer_quota_vfs::dispatch::{NativeOperation, NativeTurn, ProfileSlots, RawBinding};

use super::store::consent_checkpoint::provisioning::Reservation;
use super::store::consent_checkpoint::provisioning::stage_journal::{Artifact, State};
use super::*;

#[path = "peer_stage_binding_tests.rs"]
pub(super) mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cut {
    PriorToBind,
    Bound,
    Allocated,
    NativeStarted,
    NativeSigning,
    NativePrepared,
    Prepared,
}

impl InvitationState<'_> {
    pub(super) async fn prepare_provisional(
        &mut self,
        reservation: &Reservation,
        after: impl FnMut(Cut) -> Result<()>,
    ) -> Result<()> {
        self.prepare_provisional_with_quota(reservation, after, None)
            .await
    }

    pub(super) async fn prepare_provisional_with_quota(
        &mut self,
        reservation: &Reservation,
        mut after: impl FnMut(Cut) -> Result<()>,
        quota: Option<&ProfileSlots<'_>>,
    ) -> Result<()> {
        let sample = self.provisional_sample()?;
        let wall = sample.as_secs();
        let binding = reservation.stage_binding();
        let exists = self.checkpoint.has_stage(&self.source, reservation, wall)?;
        if !exists {
            self.paths.require_unallocated(binding)?;
        }
        after(Cut::PriorToBind)?;
        let stage = self
            .checkpoint
            .bind_stage(&self.source, reservation, wall)?;
        after(Cut::Bound)?;

        let key = SqlCipherKey::raw(*self.paths.provisional_key(stage.binding()));
        let label = self.paths.provisional_label(stage.binding());
        let origin = &self.origin_url;
        let (file, package) = match stage.state() {
            State::Bound => {
                let file = self.paths.resume_allocation(stage.binding())?;
                after(Cut::Allocated)?;
                let sample = self.provisional_sample()?;
                let token = self.checkpoint.start_stage(
                    &self.source,
                    &stage,
                    file.witness(),
                    sample.as_secs(),
                )?;
                after(Cut::NativeStarted)?;
                file.ensure_unchanged()?;
                if !token.matches(&stage, file.witness()) {
                    return Err(Error::StageIdentityMismatch);
                }
                let package = if let Some(quota) = quota {
                    let exact = scoped_binding(file.path(), binding.coordinates(), file.witness())?;
                    let native = NativeTurn {
                        key: &key,
                        signer: self.signer.clone(),
                        origin,
                        label,
                        sample,
                        operation: NativeOperation::Generate {
                            withhold_signing: after(Cut::NativeSigning).is_err(),
                        },
                    };
                    quota
                        .native_turn(exact, native)
                        .map_err(|_| Error::Native(StageError::Storage))??
                        .0
                } else {
                    let native = OpenStage::open_provisional(
                        file.path(),
                        &key,
                        self.signer.clone(),
                        origin,
                        label,
                        true,
                    )?;
                    // Simulate losing the native signer after the atomic native pending commit.
                    let withhold_signing = after(Cut::NativeSigning).is_err();
                    native
                        .generate_provisional(sample, withhold_signing)
                        .await?
                };
                (file, package)
            }
            State::NativeStarted(witness) | State::Prepared(witness, _) => {
                let file = self.paths.reopen_provisional(stage.binding(), witness)?;
                let before = file.digest()?;
                let native = NativeTurn {
                    key: &key,
                    signer: self.signer.clone(),
                    origin,
                    label,
                    sample,
                    operation: NativeOperation::Reopen {
                        prepared: matches!(stage.state(), State::Prepared(..)),
                    },
                };
                let (package, read_only) = if let Some(quota) = quota {
                    let exact = scoped_binding(file.path(), binding.coordinates(), file.witness())?;
                    quota
                        .native_turn(exact, native)
                        .map_err(|_| Error::Native(StageError::Storage))??
                } else {
                    native.run_unscoped(file.path()).await?
                };
                if read_only && before != file.digest()? {
                    return Err(Error::StageIdentityMismatch);
                }
                (file, package)
            }
        };
        after(Cut::NativePrepared)?;
        file.ensure_unchanged()?;
        let wall = self.provisional_sample()?.as_secs();
        package.check_lifetime(wall)?;
        if package.event.expires < stage.offer_expires().checked_add(600).ok_or(Error::Clock)? {
            return Err(Error::Native(StageError::Invalid));
        }
        let artifact = Artifact::new(
            package.event.id,
            &package.event.bytes,
            package.event.expires,
        );
        self.checkpoint
            .record_prepared(&self.source, &stage, file.witness(), artifact, wall)?;
        after(Cut::Prepared)
    }

    pub(super) fn provisional_sample(&self) -> Result<Duration> {
        let checked = self.wall.sample()?;
        let system = Wall::System.sample()?;
        if checked.as_secs().abs_diff(system.as_secs()) > 120 {
            return Err(Error::Clock);
        }
        Ok(checked)
    }
}

fn scoped_binding(
    path: &Path,
    (known, generation, _, _): (bool, u64, [u8; 32], crate::peer_invitation::Selector),
    witness: super::store::consent_checkpoint::provisioning::stage_journal::FileWitness,
) -> Result<RawBinding<'_>> {
    Ok(RawBinding {
        path,
        generation: NonZeroU64::new(generation).ok_or(Error::StageIdentityMismatch)?,
        known,
        witness: (
            u64::from_be_bytes(witness.device),
            u64::from_be_bytes(witness.inode),
        ),
    })
}
