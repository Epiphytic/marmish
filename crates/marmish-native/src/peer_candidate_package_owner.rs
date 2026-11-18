//! The Manual receiver's metered original generation and separately charged readback.

use crate::mdk_adapter::peer_native_signer::ProvisionalPackage;

use super::clean::CleanFiles;
use super::paths::retirement::package_files::PackageDirectory;
use super::store::consent_checkpoint::provisioning::{
    Reservation,
    stage_journal::{Artifact, BoundStage, FileWitness, NativeStart},
};
use super::*;

type NativeResult<T> = std::result::Result<T, StageError>;

const MIN_PACKAGE_MARGIN: u64 = 600 + 120;
const NATIVE_PACKAGE_LIFETIME: u64 = 30 * 86_400;

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
        let opened = self.open(path, true);
        #[cfg(test)]
        self.window.timing("native_open", point);
        let stage = opened?;
        stage.issue_managed_provisional(self.window).await
    }

    pub(crate) fn attest(self, path: &std::path::Path) -> NativeResult<ProvisionalPackage> {
        let stage = self.open(path, false)?;
        stage.require_unjoined_provisional()?;
        let package = stage.read_provisional(self.window.stage_sample()?)?;
        self.window.check()?;
        Ok(package)
    }

    fn open(&self, path: &std::path::Path, issue: bool) -> NativeResult<OpenStage> {
        OpenStage::open_managed_provisional(
            path,
            &self.key,
            self.signer.clone(),
            self.relay,
            self.label.clone(),
            issue,
            self.window,
        )
    }
}

pub(super) struct StartedPackage {
    native: NativeStart,
    file: FileWitness,
}

pub(super) struct PackageProof {
    stage: BoundStage,
    file: FileWitness,
    artifact: Artifact,
    package: ProvisionalPackage,
}

impl RecoveredOwner<'_> {
    fn package_scope(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        window: &NativeWindow,
    ) -> Result<BoundStage> {
        if !self
            .package_quota
            .is_some_and(|owned| std::ptr::eq(owned, quota))
            || reservation.stage_binding().coordinates().0
            || !self.checkpoint.is_installed_custody()
            || !matches!(
                self.checkpoint.installed_receive_policy(),
                Some(ReceivePolicy::Manual)
            )
        {
            return Err(Error::Binding);
        }
        let stage =
            self.checkpoint
                .bind_stage(&self.source, reservation, window.sample()?.as_secs())?;
        window.require_offer_limit(stage.offer_expires())?;
        Ok(stage)
    }

    pub(super) fn start_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        window: &NativeWindow,
    ) -> Result<Option<StartedPackage>> {
        let stage = self.package_scope(quota, reservation, window)?;
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
        Ok(Some(StartedPackage { native, file }))
    }

    pub(super) fn issue_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        started: StartedPackage,
        window: &NativeWindow,
    ) -> Result<Artifact> {
        let stage = self.package_scope(quota, reservation, window)?;
        if stage.state() != State::NativeStarted(started.file) {
            return Err(Error::StageIdentityMismatch);
        }
        let binding = stage.binding();
        let files = CleanFiles::begin(
            self,
            quota,
            self.paths.lease(),
            binding,
            NativeFence::Candidate(window),
        )?;
        PackageDirectory::open(&self.paths, files.files())?;
        let permit = files.original_permit(self, NativeFence::Candidate(window))?;
        let operation = self.package_operation(binding, window);
        let package = files
            .files()
            .generate_package(permit, operation, started.native, &stage, started.file)
            .map_err(|_| Error::Path)?;
        files.finish(self, NativeFence::Candidate(window), false)?;
        exact_artifact(&package?, window)
    }

    pub(super) fn attest_package(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        live: Option<Artifact>,
        window: &NativeWindow,
    ) -> Result<PackageProof> {
        let stage = self.package_scope(quota, reservation, window)?;
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
        let package = {
            let files = CleanFiles::begin(self, quota, lease, binding, fence())?;
            let original = files.original();
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
                let read = files
                    .files()
                    .attest_package(permit, operation)
                    .map_err(|_| Error::Path)?;
                files.snapshot_closed(self, copy.identity(), fence())?;
                Ok::<_, Error>(read?)
            })();
            let directory = PackageDirectory::open(&self.paths, files.files())?;
            directory.remove_copy(copy)?;
            let package = result?;
            if directory.original_digest(original, window)? != digest {
                return Err(Error::StageIdentityMismatch);
            }
            files.finish(self, fence(), true)?;
            package
        };
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
        window.sample()?;
        Ok(PackageProof {
            stage,
            file,
            artifact,
            package,
        })
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
