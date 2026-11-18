//! Checked clean file admission for the same receiver lease and cursor.

use std::path::Path;

use super::*;

pub(super) type ObservedFiles = [[Option<FileExtent>; 4]; 3];

/// Binds a single measured original or snapshot to the held file turn.
/// Measure immediately before the native operation under the same exclusion.
pub(crate) struct CleanPermit<'a> {
    files: &'a ReceiverFiles<'a>,
    generation: NonZeroU64,
    main: FileExtent,
    charged_other: u64,
    snapshot: bool,
}

impl CleanPermit<'_> {
    pub(crate) fn verify(
        &self,
        files: &ReceiverFiles<'_>,
        snapshot: bool,
    ) -> Option<MeasuredExtent> {
        (std::ptr::eq(self.files, files) && self.snapshot == snapshot).then_some(MeasuredExtent {
            generation: self.generation,
            identity: self.main.identity,
            logical: self.main.logical,
            charged: self.main.charged,
            charged_other: self.charged_other,
        })
    }
}

pub(crate) struct MeasuredExtent {
    pub(crate) generation: NonZeroU64,
    pub(crate) identity: (u64, u64),
    pub(crate) logical: u64,
    pub(crate) charged: u64,
    pub(crate) charged_other: u64,
}

/// The physical measurement and exclusion cover each fixed native operation.
/// This value is never retained across the independent custody transaction.
pub(super) struct CleanFiles<'a> {
    files: ReceiverFiles<'a>,
    lease: &'a ProfileLease,
    binding: Binding,
    generation: NonZeroU64,
    original: FileExtent,
    charged: u64,
}

impl<'a> CleanFiles<'a> {
    pub(super) fn begin(
        owner: &mut RecoveredOwner<'_>,
        quota: &'a ReceiverQuota<'a>,
        lease: &'a ProfileLease,
        binding: Binding,
        fence: &TurnFence<'_>,
    ) -> Result<Self> {
        let (known, generation, _, _) = binding.coordinates();
        if known || !owner.paths.owns_lease(lease) {
            return Err(Error::StageIdentityMismatch);
        }
        let generation = NonZeroU64::new(generation).ok_or(Error::StageIdentityMismatch)?;
        let files = quota.receiver_files(lease).map_err(|_| Error::Path)?;
        let measured = owner.recover(fence.wall()?)?;
        let clean = measured.clean_extent(binding)?;
        Ok(Self {
            files,
            lease,
            binding,
            generation,
            original: clean.main,
            charged: measured.charged,
        })
    }

    pub(super) fn files(&self) -> &ReceiverFiles<'_> {
        &self.files
    }

    pub(super) fn snapshot_path(&self, length: u64) -> Result<&Path> {
        // Only a full copy of the measured original can reserve the overlap.
        if length == 0 || length != self.original.logical {
            return Err(Error::StageIdentityMismatch);
        }
        if self
            .charged
            .checked_add(length)
            .is_none_or(|total| total >= TOTAL_ADMISSION)
        {
            return Err(Error::Path);
        }
        Ok(self.files.snapshot())
    }

    pub(super) fn original_permit<'b>(
        &'b self,
        owner: &mut RecoveredOwner<'_>,
        fence: &TurnFence<'_>,
    ) -> Result<CleanPermit<'b>> {
        self.check_owner(owner)?;
        let clean = owner.recover(fence.wall()?)?.clean_extent(self.binding)?;
        if clean.main != self.original {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(CleanPermit {
            files: &self.files,
            generation: self.generation,
            main: clean.main,
            charged_other: clean.charged_other,
            snapshot: false,
        })
    }

    pub(super) fn snapshot_permit<'b>(
        &'b self,
        owner: &mut RecoveredOwner<'_>,
        identity: (u64, u64),
        fence: &TurnFence<'_>,
    ) -> Result<CleanPermit<'b>> {
        let (copy, charged) = self.measure_snapshot(owner, identity, fence)?;
        if copy.logical != self.original.logical {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(CleanPermit {
            files: &self.files,
            generation: self.generation,
            main: copy,
            charged_other: other_charge(charged, copy.logical)?,
            snapshot: true,
        })
    }

    pub(super) fn snapshot_closed(
        &self,
        owner: &mut RecoveredOwner<'_>,
        identity: (u64, u64),
        fence: &TurnFence<'_>,
    ) -> Result<()> {
        self.measure_snapshot(owner, identity, fence)?;
        Ok(())
    }

    fn measure_snapshot(
        &self,
        owner: &mut RecoveredOwner<'_>,
        identity: (u64, u64),
        fence: &TurnFence<'_>,
    ) -> Result<(FileExtent, u64)> {
        self.check_owner(owner)?;
        let (charges, files) = owner.observe(fence.wall()?)?;
        let [Some(copy), None, None, None] = files[2] else {
            return Err(Error::StageIdentityMismatch);
        };
        if copy.identity != identity
            || copy.identity == self.original.identity
            || !(1..=CLOSED_STAGE_LIMIT).contains(&copy.logical)
            || files[1] != [Some(self.original), None, None, None]
            || classify_unknown(charges[1], files[1])
                != (Class::Identified {
                    binding: self.binding,
                    main: self.original,
                })
        {
            return Err(Error::StageIdentityMismatch);
        }
        let measured = Inventory::calculate(charges, files)?;
        if measured.charged >= TOTAL_ADMISSION {
            return Err(Error::Path);
        }
        Ok((copy, measured.charged))
    }

    pub(super) fn finish(
        &self,
        owner: &mut RecoveredOwner<'_>,
        fence: &TurnFence<'_>,
        original_unchanged: bool,
    ) -> Result<()> {
        self.check_owner(owner)?;
        let clean = owner.recover(fence.wall()?)?.clean_extent(self.binding)?;
        if clean.main.identity != self.original.identity
            || (original_unchanged && clean.main != self.original)
        {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }

    fn check_owner(&self, owner: &RecoveredOwner<'_>) -> Result<()> {
        if !owner.paths.owns_lease(self.lease) {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }
}

pub(super) fn other_charge(total: u64, active_logical: u64) -> Result<u64> {
    if total >= TOTAL_ADMISSION {
        return Err(Error::Path);
    }
    total
        .checked_sub(active_logical)
        .and_then(|value| value.checked_add(1))
        .ok_or(Error::Path)
}
