//! Bounded encrypted-image digest and file-identity checks.

use std::{
    fs::{self, Metadata, OpenOptions},
    io::{self, Read as _},
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
    path::Path,
};

use super::{Error, Result, Sha256, paths};

pub(super) fn snapshot_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match fs::symlink_metadata(name) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(Error::StageIdentityMismatch),
        }
    }
    Ok(())
}

pub(super) fn snapshot_inode(metadata: &Metadata) -> Result<(u64, u64)> {
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o7777 != 0o600 {
        return Err(Error::StageIdentityMismatch);
    }
    Ok((metadata.dev(), metadata.ino()))
}

pub(super) fn read_image(path: &Path) -> Result<([u8; 32], (u64, u64))> {
    use sha2::Digest;
    snapshot_sidecars(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Error::Path)?;
    let metadata = file.metadata().map_err(|_| Error::Path)?;
    let inode = snapshot_inode(&metadata)?;
    let mut digest = Sha256::new();
    let sum =
        io::copy(&mut file.take(paths::MAX_STAGE_LEN + 1), &mut digest).map_err(|_| Error::Path)?;
    let after = fs::symlink_metadata(path).map_err(|_| Error::Path)?;
    snapshot_sidecars(path)?;
    if sum == 0
        || sum > paths::MAX_STAGE_LEN
        || sum != metadata.len()
        || after.len() != sum
        || snapshot_inode(&after)? != inode
    {
        return Err(Error::StageIdentityMismatch);
    }
    Ok((digest.finalize().into(), inode))
}
