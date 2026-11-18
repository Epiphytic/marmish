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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Class {
    Vacant,
    Create(Binding),
    Identified(Binding),
    JournalCandidate {
        binding: Binding,
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
                Some(journal) => Class::JournalCandidate { binding, journal },
                None => Class::Identified(binding),
            }
        }
        _ => Class::Fenced,
    }
}

#[path = "peer_receiver_inventory_tests.rs"]
mod tests;
