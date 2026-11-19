//! The Manual receiver's metered original generation and separately charged readback.

use crate::mdk_adapter::peer_native_signer::ProvisionalPackage;

use super::clean::CleanFiles;
use super::paths::retirement::package_files::PackageDirectory;
use super::store::consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::retry::retirement::{ExpiredSaved, HistoricalSample};
use super::store::consent_checkpoint::provisioning::{
    Reservation,
    prepared_consent::{ManagedQueued, PackageTarget},
    stage_journal::{Artifact, BoundStage, FileWitness, NativeStart},
};
use super::*;

type NativeResult<T> = std::result::Result<T, StageError>;

const MIN_PACKAGE_MARGIN: u64 = 600 + 120;
const NATIVE_PACKAGE_LIFETIME: u64 = 30 * 86_400;

pub(crate) struct SchemaOperation<'a> {
    key: SqlCipherKey,
    window: &'a SchemaWindow,
}

impl SchemaOperation<'_> {
    pub(crate) fn run(self, path: &std::path::Path) -> NativeResult<()> {
        OpenStage::prestage_managed_provisional(path, &self.key, self.window)
    }
}

pub(crate) struct PackageOperation<'a> {
    key: SqlCipherKey,
    signer: Arc<Keys>,
    relay: &'a str,
    label: String,
    window: &'a NativeWindow,
}

impl PackageOperation<'_> {
    pub(crate) async fn issue(self, path: &std::path::Path) -> NativeResult<ProvisionalPackage> {
        #[cfg(test)]
        let point = std::time::Instant::now();
        let opened = self.open(path);
        #[cfg(test)]
        self.window.timing("native_open", point);
        let stage = opened?;
        stage.issue_managed_provisional(self.window).await
    }

    pub(crate) fn attest(self, path: &std::path::Path) -> NativeResult<ProvisionalPackage> {
        let stage = self.open(path)?;
        stage.require_unjoined_provisional()?;
        let package = stage.read_provisional(self.window.stage_sample()?)?;
        self.window.check()?;
        Ok(package)
    }

    fn open(&self, path: &std::path::Path) -> NativeResult<OpenStage> {
        OpenStage::open_managed_provisional(
            path,
            &self.key,
            self.signer.clone(),
            self.relay,
            self.label.clone(),
            self.window,
        )
    }
}

/// Charged copy only. This operation carries no original native effect.
pub(crate) struct RetirementPackageOperation<'a> {
    copy: PackageOperation<'a>,
    sample: &'a HistoricalSample,
}

impl RetirementPackageOperation<'_> {
    pub(crate) fn read(self, path: &std::path::Path) -> NativeResult<ProvisionalPackage> {
        let stage = self.copy.open(path)?;
        stage.require_unjoined_provisional()?;
        let package = stage.read_provisional(self.sample.stage_sample())?;
        self.copy.window.check()?;
        Ok(package)
    }
}

pub(in crate::peer_invitation) struct RetirementImage {
    stage: (Binding, FileWitness),
    extent: FileExtent,
    digest: [u8; 32],
    package: ProvisionalPackage,
}

impl RetirementImage {
    pub(in crate::peer_invitation) fn require(&self, expired: &ExpiredSaved<'_>) -> Option<&[u8]> {
        let (stage, artifact) = expired.stage();
        let event = &self.package.event;
        (self.stage == stage && Artifact::new(event.id, &event.bytes, event.expires) == artifact)
            .then_some(&event.bytes)
    }
}

pub(super) struct StartedPackage {
    native: NativeStart,
    file: FileWitness,
    window: NativeWindow,
}

pub(super) struct PrestagedPackage {
    native: NativeStart,
    file: FileWitness,
    extent: FileExtent,
    digest: [u8; 32],
    window: NativeWindow,
}

pub(in crate::peer_invitation) struct PackageProof<'lease> {
    target: PackageTarget,
    extent: FileExtent,
    digest: [u8; 32],
    package: ProvisionalPackage,
    quota: &'lease ReceiverQuota<'lease>,
}

impl PackageProof<'_> {
    pub(in crate::peer_invitation) fn target(&self) -> &PackageTarget {
        &self.target
    }

    pub(in crate::peer_invitation) fn require(
        &self,
        checkpoint: &Checkpoint,
        target: &PackageTarget,
        wall: u64,
    ) -> Result<&[u8]> {
        checkpoint.require_managed_package_target(&self.target)?;
        checkpoint.require_managed_package_target(target)?;
        if self.target.stage() != target.stage()
            || self.target.artifact() != target.artifact()
            || self.target.deadline() != target.deadline()
            || wall >= target.deadline()
        {
            return Err(Error::Binding);
        }
        self.package.check_lifetime(wall)?;
        Ok(self.package.event.bytes.as_slice())
    }
}

impl<'lease> RecoveredOwner<'lease> {
    fn exact_package_quota(
        &self,
        quota: &ReceiverQuota<'_>,
    ) -> Result<&'lease ReceiverQuota<'lease>> {
        self.package_quota
            .filter(|owned| std::ptr::eq(*owned, quota))
            .ok_or(Error::Binding)
    }

    fn package_scope(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        fence: NativeFence<'_>,
    ) -> Result<BoundStage> {
        self.exact_package_quota(quota)?;
        if reservation.stage_binding().coordinates().0
            || !self.checkpoint.is_installed_custody()
            || !matches!(
                self.checkpoint.installed_receive_policy(),
                Some(ReceivePolicy::Manual)
            )
        {
            return Err(Error::Binding);
        }
        let stage = self
            .checkpoint
            .bind_stage(&self.source, reservation, fence.wall()?)?;
        fence.require_offer_limit(stage.offer_expires())?;
        Ok(stage)
    }

    pub(super) fn start_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        window: &NativeWindow,
    ) -> Result<Option<StartedPackage>> {
        let stage = self.package_scope(quota, reservation, NativeFence::Candidate(window))?;
        if stage.state() != State::Bound {
            return Ok(None);
        }
        let wall = window.sample()?.as_secs();
        if !lifetime_possible(stage.offer_expires(), wall) {
            return Err(Error::Binding);
        }
        let binding = stage.binding();
        let lease = self.paths.lease();
        let file = {
            let files = quota.receiver_files(lease).map_err(|_| Error::Path)?;
            if self.recover(window.sample()?.as_secs())?.classes[1] != Class::Create(binding) {
                return Err(Error::StageIdentityMismatch);
            }
            let directory = PackageDirectory::open(&self.paths, &files)?;
            let (device, inode) = directory.allocate(window)?;
            let (charges, observed) = self.observe(window.sample()?.as_secs())?;
            let [Some(main), None, None, None] = observed[1] else {
                return Err(Error::StageIdentityMismatch);
            };
            if main.identity != (device, inode)
                || main.logical != 0
                || Inventory::calculate(charges, observed)?.charged >= TOTAL_ADMISSION
            {
                return Err(Error::StageIdentityMismatch);
            }
            FileWitness {
                device: device.to_be_bytes(),
                inode: inode.to_be_bytes(),
            }
        };
        let native =
            self.checkpoint
                .start_stage(&self.source, &stage, file, window.sample()?.as_secs())?;
        window.sample()?;
        Ok(Some(StartedPackage {
            native,
            file,
            window: window.clone(),
        }))
    }

    pub(super) fn prestage_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        started: StartedPackage,
    ) -> Result<PrestagedPackage> {
        let schema = started.window.schema_turn()?;
        let fence = || NativeFence::Schema(&schema);
        let stage = self.package_scope(quota, reservation, fence())?;
        if !started.native.same_stage(&stage, started.file)
            || stage.state() != State::NativeStarted(started.file)
        {
            return Err(Error::StageIdentityMismatch);
        }
        let binding = stage.binding();
        let files = CleanFiles::begin(self, quota, self.paths.lease(), binding, fence())?;
        PackageDirectory::open(&self.paths, files.files())?;
        let permit = files.original_permit(self, fence())?;
        let operation = SchemaOperation {
            key: SqlCipherKey::raw(*self.paths.provisional_key(binding)),
            window: &schema,
        };
        let result = files
            .files()
            .prestage_package(permit, operation, started.file)
            .map_err(|_| Error::Path)?;
        #[cfg(test)]
        schema.wait_for_fixture_expiry()?;
        let extent = files.finish(self, fence(), false)?;
        result?;
        let digest = PackageDirectory::open(&self.paths, files.files())?
            .original_digest(extent, &fence())?;
        drop(files);
        schema.require_offer_limit(stage.offer_expires())?;
        let window = schema.into_issue()?;
        Ok(PrestagedPackage {
            native: started.native,
            file: started.file,
            extent,
            digest,
            window,
        })
    }

    pub(super) fn issue_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        prepared: PrestagedPackage,
    ) -> Result<Artifact> {
        let window = &prepared.window;
        let fence = || NativeFence::Candidate(window);
        let stage = self.package_scope(quota, reservation, fence())?;
        if stage.state() != State::NativeStarted(prepared.file) {
            return Err(Error::StageIdentityMismatch);
        }
        let binding = stage.binding();
        let files = CleanFiles::begin(self, quota, self.paths.lease(), binding, fence())?;
        if files.original() != prepared.extent
            || PackageDirectory::open(&self.paths, files.files())?
                .original_digest(prepared.extent, &fence())?
                != prepared.digest
        {
            return Err(Error::StageIdentityMismatch);
        }
        let permit = files.original_permit(self, fence())?;
        let operation = self.package_operation(binding, window);
        let package = files
            .files()
            .generate_package(permit, operation, prepared.native, &stage, prepared.file)
            .map_err(|_| Error::Path)?;
        files.finish(self, fence(), false)?;
        exact_artifact(&package?, window)
    }

    pub(super) fn attest_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        live: Option<Artifact>,
        window: &NativeWindow,
    ) -> Result<PackageProof<'lease>> {
        let stage = self.package_scope(quota, reservation, NativeFence::Candidate(window))?;
        let (file, recorded) = match stage.state() {
            State::NativeStarted(file) => (file, None),
            State::Prepared(file, artifact) => (file, Some(artifact)),
            State::Bound => return Err(Error::StageIdentityMismatch),
        };
        let binding = stage.binding();
        let fence = || NativeFence::Candidate(window);
        let lease = self.paths.lease();
        if recorded.is_none()
            && matches!(
                self.recover(window.sample()?.as_secs())?.classes[1],
                Class::JournalCandidate { .. }
            )
        {
            self.rollback_journal(
                lease,
                quota,
                binding,
                &fence(),
                #[cfg(test)]
                false,
            )?;
        }
        let (package, extent, digest) =
            self.read_charged_package(quota, (binding, file), window, None)?;
        let artifact = exact_artifact(&package, window)?;
        if live.is_some_and(|live| live != artifact)
            || recorded.is_some_and(|stored| stored != artifact)
        {
            return Err(Error::Binding);
        }
        self.checkpoint.record_prepared(
            &self.source,
            &stage,
            file,
            artifact,
            window.sample()?.as_secs(),
        )?;
        let target = self.checkpoint.issued_package_target(
            &self.source,
            self.signer.public_key(),
            &self.relay,
            &stage,
            (file, artifact),
            window.sample()?.as_secs(),
        )?;
        window.sample()?;
        Ok(PackageProof {
            target,
            extent,
            digest,
            package,
            quota: self.exact_package_quota(quota)?,
        })
    }

    pub(super) fn attest_queued_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        queued: &ManagedQueued,
        window: &NativeWindow,
    ) -> Result<PackageProof<'lease>> {
        let quota = self.exact_package_quota(quota)?;
        let target = queued.target();
        window.require_offer_limit(target.deadline())?;
        let confirm = |owner: &mut Self| -> Result<()> {
            owner.checkpoint.confirm_managed_queued(
                &owner.source,
                owner.signer.public_key(),
                &owner.relay,
                queued,
                window.sample()?.as_secs(),
            )?;
            Ok(())
        };
        confirm(self)?;
        let (binding, file) = target.stage();
        let (package, extent, digest) =
            self.read_charged_package(quota, (binding, file), window, None)?;
        if exact_artifact(&package, window)? != target.artifact() {
            return Err(Error::Binding);
        }
        confirm(self)?;
        window.sample()?;
        Ok(PackageProof {
            target: target.clone(),
            extent,
            digest,
            package,
            quota,
        })
    }

    pub(super) fn recheck_managed_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        proof: &PackageProof<'_>,
        window: &NativeWindow,
    ) -> Result<()> {
        if !std::ptr::eq(self.exact_package_quota(quota)?, proof.quota) {
            return Err(Error::Binding);
        }
        self.checkpoint
            .require_managed_package_target(&proof.target)?;
        window.require_offer_limit(proof.target.deadline())?;
        self.recheck_package_image(
            quota,
            proof.target.stage(),
            (proof.extent, proof.digest),
            window,
        )
    }

    pub(super) fn read_retirement_image(
        &mut self,
        quota: &ReceiverQuota<'_>,
        expired: &ExpiredSaved<'_>,
        window: &NativeWindow,
    ) -> Result<RetirementImage> {
        let quota = self.exact_package_quota(quota)?;
        let (stage, _) = expired.stage();
        let (package, extent, digest) =
            self.read_charged_package(quota, stage, window, Some(&expired.historical_sample()))?;
        Ok(RetirementImage {
            stage,
            extent,
            digest,
            package,
        })
    }

    pub(super) fn recheck_retirement_image(
        &mut self,
        quota: &ReceiverQuota<'_>,
        image: &RetirementImage,
        window: &NativeWindow,
    ) -> Result<()> {
        let quota = self.exact_package_quota(quota)?;
        self.recheck_package_image(quota, image.stage, (image.extent, image.digest), window)
    }

    fn recheck_package_image(
        &mut self,
        quota: &ReceiverQuota<'_>,
        (binding, file): (Binding, FileWitness),
        (extent, digest): (FileExtent, [u8; 32]),
        window: &NativeWindow,
    ) -> Result<()> {
        let fence = || NativeFence::Candidate(window);
        let files = CleanFiles::begin(self, quota, self.paths.lease(), binding, fence())?;
        let original = files.original();
        check_file_witness(original, file)?;
        if original != extent
            || PackageDirectory::open(&self.paths, files.files())?
                .original_digest(original, &fence())?
                != digest
            || files.finish(self, fence(), true)? != original
        {
            return Err(Error::StageIdentityMismatch);
        }
        window.sample()?;
        Ok(())
    }

    #[cfg(test)]
    fn fixture_attest_delivery(
        &mut self,
        quota: &ReceiverQuota<'_>,
        queued: &ManagedQueued,
        window: &NativeWindow,
        (stage, observed): ((Binding, FileWitness), u64),
    ) -> Result<super::super::prepared_consent::NativeAttestation> {
        use super::super::prepared_consent::NativeAttestation;
        let proof = self.attest_queued_package(quota, queued, window)?;
        if proof.target.stage() != stage {
            return Err(Error::Binding);
        }
        self.recheck_managed_package(quota, &proof, window)?;
        let wall = window.sample()?.as_secs();
        if wall < observed {
            return Err(Error::Clock);
        }
        proof.package.check_lifetime(wall)?;
        Ok(NativeAttestation::fixture_managed(
            stage,
            proof.package.event,
            proof.digest,
        ))
    }

    fn read_charged_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        (binding, file): (Binding, FileWitness),
        window: &NativeWindow,
        historical: Option<&HistoricalSample>,
    ) -> Result<(ProvisionalPackage, FileExtent, [u8; 32])> {
        let fence = || NativeFence::Candidate(window);
        let files = CleanFiles::begin(self, quota, self.paths.lease(), binding, fence())?;
        let original = files.original();
        check_file_witness(original, file)?;
        files.snapshot_path(original.logical)?;
        let (copy, digest) =
            PackageDirectory::open(&self.paths, files.files())?.copy(original, window)?;
        let result = (|| {
            let permit = files.snapshot_permit(self, copy.identity(), fence())?;
            PackageDirectory::open(&self.paths, files.files())?.verify_copy(
                &copy,
                original.logical,
                digest,
                window,
            )?;
            let operation = self.package_operation(binding, window);
            let read = if let Some(sample) = historical {
                files.files().attest_expired_package(
                    permit,
                    RetirementPackageOperation {
                        copy: operation,
                        sample,
                    },
                )
            } else {
                files.files().attest_package(permit, operation)
            }
            .map_err(|cause| match cause {
                "dispatch_active_turn" | "dispatch_snapshot_quarantined"
                    if historical.is_some() =>
                {
                    Error::RetirementBusy
                }
                _ => Error::Path,
            })?;
            files.snapshot_closed(self, copy.identity(), fence())?;
            Ok::<_, Error>(read?)
        })();
        let directory = PackageDirectory::open(&self.paths, files.files())?;
        directory.remove_copy(copy)?;
        let package = result?;
        if directory.original_digest(original, &fence())? != digest
            || files.finish(self, fence(), true)? != original
        {
            return Err(Error::StageIdentityMismatch);
        }
        Ok((package, original, digest))
    }

    fn package_operation<'a>(
        &'a self,
        binding: Binding,
        window: &'a NativeWindow,
    ) -> PackageOperation<'a> {
        PackageOperation {
            key: SqlCipherKey::raw(*self.paths.provisional_key(binding)),
            signer: self.signer.clone(),
            relay: &self.relay,
            label: self.paths.provisional_label(binding),
            window,
        }
    }
}

// The native file witness records device and inode. Callers separately verify
// the measured extent and, when reusing proof, the ciphertext digest.
fn check_file_witness(extent: FileExtent, file: FileWitness) -> Result<()> {
    if extent.identity
        != (
            u64::from_be_bytes(file.device),
            u64::from_be_bytes(file.inode),
        )
    {
        return Err(Error::StageIdentityMismatch);
    }
    Ok(())
}

fn lifetime_possible(offer_expires: u64, wall: u64) -> bool {
    offer_expires
        .checked_add(MIN_PACKAGE_MARGIN)
        .zip(wall.checked_add(NATIVE_PACKAGE_LIFETIME))
        .is_some_and(|(required, available)| required <= available)
}

fn exact_artifact(package: &ProvisionalPackage, window: &NativeWindow) -> Result<Artifact> {
    let wall = window.sample()?.as_secs();
    package.check_lifetime(wall)?;
    let native = &package.event;
    Ok(Artifact::new(native.id, &native.bytes, native.expires))
}

#[cfg(test)]
#[path = "peer_candidate_package_tests.rs"]
mod tests;
