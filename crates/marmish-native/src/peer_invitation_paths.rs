#[cfg(test)]
#[path = "peer_stage_files.rs"]
mod provisional;
#[cfg(test)]
pub(super) use provisional::{MAX_STAGE_LEN, ProvisionalFile};
#[cfg(test)]
#[path = "peer_stage_retirement_files.rs"]
pub(super) mod retirement;

use super::{Error, Result};
use crate::identity::{IdentityMaterial, ProfileLease, check_dir, open_private};
use hkdf::Hkdf;
use sha2::Sha256;
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

const STAGES: u8 = 16;

#[derive(Clone, Copy)]
pub(super) struct StageSlot(u8);

impl StageSlot {
    pub(super) fn new(index: u8) -> Result<Self> {
        (index < STAGES).then_some(Self(index)).ok_or(Error::Path)
    }
}

pub(super) struct Paths<'a> {
    lease: &'a ProfileLease,
    keys: Hkdf<Sha256>,
}

pub(super) struct CheckedFile {
    path: PathBuf,
    before: (u64, u64),
}

impl Paths<'_> {
    #[cfg(test)]
    pub(super) fn owns_lease(&self, lease: &ProfileLease) -> bool {
        std::ptr::eq(self.lease, lease)
    }

    pub(super) fn new<'a>(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
    ) -> Result<Paths<'a>> {
        if lease.public_key() != Some(material.public_key()) {
            return Err(Error::Path);
        }
        let account = material.public_key().to_bytes();
        let keys = Hkdf::new(Some(&account), material.sqlcipher_root());
        Ok(Paths { lease, keys })
    }

    pub(super) fn initialize(&self) -> Result<()> {
        let dir = self.directory();
        DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|_| Error::Path)?;
        create_file(&dir.join("cursor.db"))?;
        File::open(self.lease.root())
            .and_then(|parent| parent.sync_all())
            .map_err(|_| Error::Path)
    }

    pub(super) fn allocate(&self, slot: StageSlot) -> Result<()> {
        let cursor = self.cursor()?;
        let stage = self.stage_path(slot);
        create_file(&stage)?;
        let stage = CheckedFile::read(stage, &["-journal", "-wal", "-shm"])?;
        stage.distinct(&cursor)
    }

    pub(super) fn cursor(&self) -> Result<CheckedFile> {
        let dir = self.directory();
        directory(&dir)?;
        CheckedFile::read(dir.join("cursor.db"), &["-journal", "-wal", "-shm"])
    }

    pub(super) fn stage(&self, slot: StageSlot) -> Result<CheckedFile> {
        let cursor = self.cursor()?;
        let stage = CheckedFile::read(self.stage_path(slot), &["-journal", "-wal", "-shm"])?;
        stage.distinct(&cursor)?;
        Ok(stage)
    }

    pub(super) fn cursor_key(&self) -> Zeroizing<[u8; 32]> {
        self.key(b"marmish/peer/cursor/2", &[])
    }

    pub(super) fn stage_key(&self, slot: StageSlot) -> Zeroizing<[u8; 32]> {
        self.key(b"marmish/peer/native-stage/1", &[slot.0])
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn initialize_founder_directory(&self) -> Result<()> {
        let dir = self.directory();
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => File::open(self.lease.root())
                .and_then(|parent| parent.sync_all())
                .map_err(|_| Error::Path)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Error::Path),
        }
        directory(&dir)
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn founder_ledger_key(&self) -> Zeroizing<[u8; 32]> {
        self.key(b"marmish/peer/founder-ledger/1", &[])
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn founder_image_key(
        &self,
        generation: std::num::NonZeroU64,
    ) -> Zeroizing<[u8; 32]> {
        self.key(
            b"marmish/peer/founder-native/1",
            &generation.get().to_be_bytes(),
        )
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn check_founder_ledger(path: &Path) -> Result<CheckedFile> {
        directory(path.parent().ok_or(Error::Path)?)?;
        if path.file_name().and_then(|name| name.to_str()) != Some("founder-ledger.db") {
            return Err(Error::Path);
        }
        CheckedFile::read(path.to_owned(), &["-journal", "-wal", "-shm"])
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn create_founder_file(&self, path: &Path) -> Result<(u64, u64)> {
        let dir = self.directory();
        if path.parent() != Some(dir.as_path()) {
            return Err(Error::Path);
        }
        directory(&dir)?;
        create_file(path)?;
        inode(path)
    }

    fn key(&self, domain: &[u8], slot: &[u8]) -> Zeroizing<[u8; 32]> {
        expand_peer_key(&self.keys, domain, self.lease.profile_id().as_bytes(), slot)
    }

    fn stage_path(&self, slot: StageSlot) -> PathBuf {
        self.directory().join(format!("stage-{:02}.db", slot.0))
    }

    fn directory(&self) -> PathBuf {
        self.lease.root().join("peer")
    }
}

fn expand_peer_key(
    keys: &Hkdf<Sha256>,
    domain: &[u8],
    profile_id: &[u8],
    slot: &[u8],
) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0; 32]);
    keys.expand_multi_info(&[domain, profile_id, slot], &mut key[..])
        .expect("fixed HKDF output length is valid");
    key
}

impl CheckedFile {
    fn read(path: PathBuf, suffixes: &[&str]) -> Result<Self> {
        let before = inode(&path)?;
        check_sidecars(&path, suffixes)?;
        Ok(Self { path, before })
    }

    fn distinct(&self, other: &Self) -> Result<()> {
        if self.before == other.before {
            return Err(Error::Path);
        }
        Ok(())
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn unchanged(&self) -> Result<()> {
        let dir = self.path.parent().ok_or(Error::Path)?;
        directory(dir)?;
        if inode(&self.path)? != self.before {
            return Err(Error::Path);
        }
        check_sidecars(&self.path, &["-journal", "-wal", "-shm"])
    }
}

fn directory(path: &Path) -> Result<()> {
    check_dir(path).map_err(|_| Error::Path)?;
    if fs::symlink_metadata(path).map_err(|_| Error::Path)?.mode() & 0o7777 != 0o700 {
        return Err(Error::Path);
    }
    Ok(())
}

fn inode(path: &Path) -> Result<(u64, u64)> {
    let file = open_private(path, false).map_err(|_| Error::Path)?;
    let metadata = file.metadata().map_err(|_| Error::Path)?;
    if metadata.mode() & 0o7777 != 0o600 {
        return Err(Error::Path);
    }
    Ok((metadata.dev(), metadata.ino()))
}

fn check_sidecars(path: &Path, suffixes: &[&str]) -> Result<()> {
    for suffix in suffixes {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        let sidecar = Path::new(&sidecar);
        match fs::symlink_metadata(sidecar) {
            Ok(_) => {
                inode(sidecar)?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Path),
        }
    }
    Ok(())
}

fn create_file(path: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Error::Path)?;
    file.sync_all().map_err(|_| Error::Path)?;
    File::open(path.parent().ok_or(Error::Path)?)
        .and_then(|parent| parent.sync_all())
        .map_err(|_| Error::Path)?;
    Ok(())
}
