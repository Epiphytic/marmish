//! Recovered capacity for the sole Manual recipient lease.

use std::num::NonZeroU64;
use storage_sqlite::SqlCipherKey;

use crate::peer_quota_vfs::dispatch::{
    CLOSED_STAGE_LIMIT, ReceiverFiles, ReceiverQuota, RecoveryRequest, TOTAL_ADMISSION,
    local_volume,
};

use super::paths::{
    CheckedFile,
    retirement::recovery::{FileExtent, RollbackFiles, inspect},
};
use super::store::{
    HardenedPeerConnection,
    consent_checkpoint::provisioning::{
        Binding,
        prepared_consent::custody::{ReceivePolicy, receiver_inventory_store::Charge},
        stage_journal::State,
    },
    installed::RESERVED_BYTES,
};
use super::*;

struct RecoveredOwner<'lease> {
    paths: Paths<'lease>,
    cursor: CheckedFile,
    checkpoint: Checkpoint,
    source: Source,
    signer: Arc<Keys>,
    relay: String,
    package_quota: Option<&'lease ReceiverQuota<'lease>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Class {
    Vacant,
    Create(Binding),
    Identified {
        binding: Binding,
        main: FileExtent,
    },
    JournalCandidate {
        binding: Binding,
        main: FileExtent,
        journal: FileExtent,
    },
    Fenced,
}

#[derive(Debug, Eq, PartialEq)]
// Filesystem and charge evidence only. Offer, custody and route expiry remain
// the caller's exact turn checks.
struct Inventory {
    classes: [Class; 2],
    extents: [u64; 3],
    charged: u64,
}

// Measured only from the selected durable Unknown row plus its exact clean
// main inode. Quota admission must still recheck this value at the native turn.
#[derive(Debug, Eq, PartialEq)]
struct CleanExtent {
    main: FileExtent,
    charged_other: u64,
}

#[path = "peer_receiver_clean_charge.rs"]
mod clean;
pub(crate) use clean::CleanPermit;
#[path = "peer_candidate_package_owner.rs"]
pub(super) mod package;
#[cfg(test)]
#[path = "peer_receiver_v2_pending.rs"]
pub(super) mod v2_pending;

impl<'lease> RecoveredOwner<'lease> {
    fn open_registered(
        lease: &'lease ProfileLease,
        material: &IdentityMaterial,
        relay: &str,
        registration: &'lease crate::peer_relay_trust::candidate_registration::LocalReceiverRegistration<'lease>,
    ) -> Result<Self> {
        let quota = registration.quota();
        let _files = quota.receiver_files(lease).map_err(|_| Error::Path)?;
        Self::open_core(lease, material, relay, Some(quota))
    }

    #[cfg(test)]
    fn open(lease: &'lease ProfileLease, material: &IdentityMaterial, relay: &str) -> Result<Self> {
        Self::open_core(lease, material, relay, None)
    }

    fn open_core(
        lease: &'lease ProfileLease,
        material: &IdentityMaterial,
        relay: &str,
        package_quota: Option<&'lease ReceiverQuota<'lease>>,
    ) -> Result<Self> {
        let paths = Paths::new(lease, material)?;
        let cursor = paths.cursor()?;
        if !local_volume(cursor.path()).map_err(|_| Error::Path)? {
            return Err(Error::Path);
        }
        inspect(&paths, &cursor)?;
        let signer = material.signer();
        // SAFETY: reads this process's own effective UID.
        let uid = unsafe { libc::geteuid() };
        let fresh = std::fs::symlink_metadata(cursor.path())
            .map_err(|_| Error::Path)?
            .len()
            == 0;
        let open = || {
            cursor.unchanged()?;
            let conn = HardenedPeerConnection::open_installed(cursor.path(), *paths.cursor_key())?;
            cursor.unchanged()?;
            Ok::<_, Error>(conn)
        };
        if fresh {
            Checkpoint::attach_installed_v2(
                open()?,
                (*signer).clone(),
                uid,
                relay,
                Policy::default(),
            )?;
        }
        let checkpoint = Checkpoint::attach_installed_custody(
            open()?,
            (*signer).clone(),
            uid,
            relay,
            Policy::default(),
            ReceivePolicy::Manual,
        )?;
        cursor.unchanged()?;
        let source = checkpoint.bind_source(relay)?;
        Ok(Self {
            paths,
            cursor,
            checkpoint,
            source,
            signer,
            relay: relay.to_owned(),
            package_quota,
        })
    }

    #[cfg(test)]
    fn recover_journal(
        &mut self,
        lease: &Arc<ProfileLease>,
        exact: Binding,
        fence: &TurnFence<'_>,
        interrupt_after_write: bool,
    ) -> Result<CleanExtent> {
        let quota = ReceiverQuota::bind_receiver(lease).map_err(|_| Error::Path)?;
        self.rollback_journal(lease, &quota, exact, &fence.into(), interrupt_after_write)
    }

    fn rollback_journal(
        &mut self,
        lease: &ProfileLease,
        quota: &ReceiverQuota<'_>,
        exact: Binding,
        fence: &NativeFence<'_>,
        #[cfg(test)] interrupt_after_write: bool,
    ) -> Result<CleanExtent> {
        if !self.paths.owns_lease(lease) {
            return Err(Error::StageIdentityMismatch);
        }
        let _files = quota.receiver_files(lease).map_err(|_| Error::Path)?;
        let before = self.recover(fence.wall()?)?;
        let Class::JournalCandidate {
            binding,
            main,
            journal,
        } = before.classes[1]
        else {
            return Err(Error::StageIdentityMismatch);
        };
        if binding != exact {
            return Err(Error::StageIdentityMismatch);
        }
        let (known, generation, _, _) = binding.coordinates();
        if known {
            return Err(Error::StageIdentityMismatch);
        }
        let generation = NonZeroU64::new(generation).ok_or(Error::StageIdentityMismatch)?;
        let active_logical = main
            .logical
            .checked_add(journal.logical)
            .ok_or(Error::Path)?;
        let charged_other = clean::other_charge(before.charged, active_logical)?;
        let files = RollbackFiles::open(&self.paths, main, journal)?;
        let key = SqlCipherKey::raw(*self.paths.provisional_key(exact));
        quota
            .recover_unknown(
                RecoveryRequest {
                    generation,
                    files,
                    charged_other,
                    key: &key,
                    #[cfg(test)]
                    interrupt_after_write,
                },
                fence,
            )
            .map_err(|_| Error::Path)?;
        let clean = self.recover(fence.wall()?)?.clean_extent(binding)?;
        if clean.main.identity != main.identity {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(clean)
    }

    fn recover(&mut self, wall: u64) -> Result<Inventory> {
        let (charges, files) = self.observe(wall)?;
        Inventory::calculate(charges, files)
    }

    fn observe(&mut self, wall: u64) -> Result<([Option<Charge>; 2], clean::ObservedFiles)> {
        // SQLCipher can inspect a hot journal when it takes a new lock.
        // Refuse unexpected cursor sidecars before giving it that chance.
        inspect(&self.paths, &self.cursor)?;
        let charges = self.checkpoint.receiver_charges(&self.source, wall)?;
        let files = inspect(&self.paths, &self.cursor)?;
        Ok((charges, files))
    }
}

impl Inventory {
    fn clean_extent(&self, exact: Binding) -> Result<CleanExtent> {
        let Class::Identified { binding, main } = self.classes[1] else {
            return Err(Error::StageIdentityMismatch);
        };
        if binding != exact {
            return Err(Error::StageIdentityMismatch);
        }
        // Keep other families, the fixed cursor reservation, and physically
        // allocated excess for this active main. The VFS counts its logical
        // bytes itself. Reserve one byte to preserve the strict aggregate cap.
        let charged_other = clean::other_charge(self.charged, main.logical)?;
        Ok(CleanExtent {
            main,
            charged_other,
        })
    }

    fn calculate(
        charges: [Option<Charge>; 2],
        files: [[Option<FileExtent>; 4]; 3],
    ) -> Result<Self> {
        let mut extents = [0; 3];
        for (family, total) in files.iter().zip(&mut extents) {
            *total = family
                .iter()
                .flatten()
                .try_fold(0u64, |sum, file| sum.checked_add(file.charged))
                .ok_or(Error::Path)?;
        }
        let charged = extents
            .iter()
            .try_fold(RESERVED_BYTES, |sum, extent| sum.checked_add(*extent))
            .ok_or(Error::Path)?;
        // Known contributes to capacity but cannot be selected. A shared copy
        // or lack of remaining capacity also fences the Unknown family.
        let unknown = if files[2].iter().any(Option::is_some) || charged >= TOTAL_ADMISSION {
            Class::Fenced
        } else {
            classify_unknown(charges[1], files[1])
        };
        Ok(Self {
            classes: [Class::Fenced, unknown],
            extents,
            charged,
        })
    }
}

fn classify_unknown(charge: Option<Charge>, family: [Option<FileExtent>; 4]) -> Class {
    match (charge, family) {
        (None, [None, None, None, None]) => Class::Vacant,
        (
            Some(Charge {
                binding,
                stage: None | Some(State::Bound),
            }),
            [None, None, None, None],
        ) => Class::Create(binding),
        (
            Some(Charge {
                binding,
                stage: Some(State::NativeStarted(witness) | State::Prepared(witness, _)),
            }),
            [Some(main), journal, None, None],
        ) if main.identity
            == (
                u64::from_be_bytes(witness.device),
                u64::from_be_bytes(witness.inode),
            )
            && main.logical <= CLOSED_STAGE_LIMIT =>
        {
            // The caller has charged both observed extents against the exact
            // allowance left by the cursor and other families. A candidate
            // still needs a scoped pager check before native access.
            match journal {
                Some(journal) => Class::JournalCandidate {
                    binding,
                    main,
                    journal,
                },
                None => Class::Identified { binding, main },
            }
        }
        _ => Class::Fenced,
    }
}

#[cfg(test)]
#[path = "peer_receiver_inventory_tests.rs"]
mod tests;
