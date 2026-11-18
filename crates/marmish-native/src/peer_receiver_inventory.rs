//! Recovered capacity for the sole Manual recipient lease.

use crate::peer_quota_vfs::dispatch::{CLOSED_STAGE_LIMIT, TOTAL_ADMISSION, local_volume};

use super::paths::{
    CheckedFile,
    retirement::recovery::{FileExtent, inspect},
};
use super::store::{
    HardenedPeerConnection,
    consent_checkpoint::provisioning::{
        Binding,
        prepared_consent::custody::{Charge, ReceivePolicy},
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Class {
    Vacant,
    Create(Binding),
    Identified(Binding),
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

impl<'lease> RecoveredOwner<'lease> {
    fn open(lease: &'lease ProfileLease, material: &IdentityMaterial, relay: &str) -> Result<Self> {
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
        })
    }

    fn recover(&mut self, wall: u64) -> Result<Inventory> {
        // SQLCipher can inspect a hot journal when it takes a new lock.
        // Refuse unexpected cursor sidecars before giving it that chance.
        inspect(&self.paths, &self.cursor)?;
        let charges = self.checkpoint.receiver_charges(&self.source, wall)?;
        let files = inspect(&self.paths, &self.cursor)?;
        Inventory::calculate(charges, files)
    }
}

impl Inventory {
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
        let mut classes = [Class::Fenced; 2];
        for (index, (charge, family)) in charges.into_iter().zip(&files).enumerate() {
            let absent = family.iter().all(Option::is_none);
            classes[index] = match charge {
                None if absent => Class::Vacant,
                Some(Charge { binding, stage })
                    if absent && matches!(stage, None | Some(State::Bound)) =>
                {
                    Class::Create(binding)
                }
                Some(Charge {
                    binding,
                    stage: Some(State::NativeStarted(witness) | State::Prepared(witness, _)),
                }) if family[0].is_some_and(|main| {
                    main.identity
                        == (
                            u64::from_be_bytes(witness.device),
                            u64::from_be_bytes(witness.inode),
                        )
                        && main.logical <= CLOSED_STAGE_LIMIT
                }) && family[2..].iter().all(Option::is_none) =>
                {
                    Class::Identified(binding)
                }
                _ => Class::Fenced,
            };
        }
        let charged = extents
            .iter()
            .try_fold(RESERVED_BYTES, |sum, extent| sum.checked_add(*extent))
            .ok_or(Error::Path)?;
        // The installed policy has no Known founder source. Any retained Known
        // state still contributes to capacity. An interrupted copy fences both.
        classes[0] = Class::Fenced;
        if files[2].iter().any(Option::is_some) || charged >= TOTAL_ADMISSION {
            classes.fill(Class::Fenced);
        }
        Ok(Self {
            classes,
            extents,
            charged,
        })
    }
}

#[path = "peer_receiver_inventory_tests.rs"]
mod tests;
