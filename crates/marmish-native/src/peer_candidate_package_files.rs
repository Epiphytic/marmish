//! Descriptor-relative files for a single registered receiver package turn.

use std::io::{Read, Write};

use sha2::Digest as _;

use super::recovery::{FileExtent, SNAPSHOT};
use super::*;
use crate::peer_invitation::{NativeFence, NativeWindow};

pub(in super::super::super) struct PackageDirectory<'p, 'l> {
    paths: &'p Paths<'l>,
    descriptor: File,
    profile: Identity,
    peer: Identity,
}

pub(in super::super::super) struct FreshCopy {
    identity: Identity,
}

impl FreshCopy {
    pub(in super::super::super) fn identity(&self) -> Identity {
        self.identity
    }
}

impl<'p, 'l> PackageDirectory<'p, 'l> {
    pub(in super::super::super) fn open(
        paths: &'p Paths<'l>,
        files: &crate::peer_quota_vfs::dispatch::ReceiverFiles<'_>,
    ) -> Result<Self> {
        let (descriptor, profile, peer) = open_dirs(paths)?;
        let result = Self {
            paths,
            descriptor,
            profile,
            peer,
        };
        result.recheck()?;
        files
            .check_package_paths(
                [profile, peer],
                &paths.directory().join("provisional-unknown.db"),
                &paths.directory().join("receiver-join-snapshot.db"),
            )
            .map_err(|_| Error::StageIdentityMismatch)?;
        Ok(result)
    }

    fn recheck(&self) -> Result<()> {
        let (_, profile, peer) = open_dirs(self.paths)?;
        if (profile, peer) != (self.profile, self.peer) {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }

    fn clean_family(&self, family: [&CStr; 4]) -> Result<()> {
        for sidecar in &family[1..] {
            if stat(&self.descriptor, sidecar)?.is_some() {
                return Err(Error::StageIdentityMismatch);
            }
        }
        Ok(())
    }

    fn create(&self, family: [&CStr; 4], fence: &NativeWindow) -> Result<(File, Identity)> {
        self.recheck()?;
        self.clean_family(family)?;
        fence.sample()?;
        let flags =
            libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_CREAT | libc::O_EXCL | libc::O_RDWR;
        // SAFETY: the checked descriptor and terminated fixed member name are live.
        let fd = unsafe {
            libc::openat(
                self.descriptor.as_raw_fd(),
                family[0].as_ptr(),
                flags,
                0o600,
            )
        };
        if fd < 0 {
            return Err(Error::StageIdentityMismatch);
        }
        // SAFETY: successful openat transferred ownership of a new descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        // SAFETY: this live descriptor names only the newly created private file.
        // Ensure the owner can reopen it even when the process umask masks owner bits.
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
            return Err(Error::Path);
        }
        let identity = identity(&file)?;
        self.exact(family, identity, Some(0))?;
        Ok((file, identity))
    }

    fn exact(&self, family: [&CStr; 4], id: Identity, size: Option<u64>) -> Result<()> {
        self.recheck()?;
        self.clean_family(family)?;
        let Some((seen, entry)) = private_stat(&self.descriptor, self.peer, family[0])? else {
            return Err(Error::StageIdentityMismatch);
        };
        if seen != id || size.is_some_and(|size| u64::try_from(entry.st_size) != Ok(size)) {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }

    pub(in super::super::super) fn allocate(&self, fence: &NativeWindow) -> Result<Identity> {
        let family = stage_names(false);
        let (file, identity) = self.create(family, fence)?;
        self.sync(&file, fence)?;
        self.exact(family, identity, Some(0))?;
        fence.sample()?;
        Ok(identity)
    }

    fn sync(&self, file: &File, fence: &NativeWindow) -> Result<()> {
        fence.sample()?;
        file.sync_all().map_err(|_| Error::Path)?;
        fence.sample()?;
        self.descriptor.sync_all().map_err(|_| Error::Path)?;
        fence.sample()?;
        Ok(())
    }

    fn open_existing(&self, family: [&CStr; 4], id: Identity, size: u64) -> Result<File> {
        self.exact(family, id, Some(size))?;
        let flags = libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_RDONLY | libc::O_NONBLOCK;
        // SAFETY: the descriptor remains owned and the fixed path is terminated.
        let fd = unsafe { libc::openat(self.descriptor.as_raw_fd(), family[0].as_ptr(), flags) };
        if fd < 0 {
            return Err(Error::Path);
        }
        // SAFETY: successful openat transferred ownership of a new descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        if identity(&file)? != id || file.metadata().map_err(|_| Error::Path)?.len() != size {
            return Err(Error::StageIdentityMismatch);
        }
        self.exact(family, id, Some(size))?;
        Ok(file)
    }

    pub(in super::super::super) fn original_digest(
        &self,
        main: FileExtent,
        fence: &NativeFence,
    ) -> Result<[u8; 32]> {
        self.digest(stage_names(false), main.identity, main.logical, fence)
    }

    fn digest(
        &self,
        family: [&CStr; 4],
        id: Identity,
        size: u64,
        fence: &NativeFence,
    ) -> Result<[u8; 32]> {
        fence.wall()?;
        let file = self.open_existing(family, id, size)?;
        let digest = copy_bounded(
            file,
            size,
            &mut std::io::sink(),
            fence,
            #[cfg(test)]
            false,
        )?;
        self.exact(family, id, Some(size))?;
        fence.wall()?;
        Ok(digest)
    }

    pub(in super::super::super) fn copy(
        &self,
        main: FileExtent,
        fence: &NativeWindow,
    ) -> Result<(FreshCopy, [u8; 32])> {
        let initial = self.original_digest(main, &NativeFence::Candidate(fence))?;
        let (mut file, identity) = self.create(SNAPSHOT, fence)?;
        let created = FreshCopy { identity };
        let result = (|| {
            let original = self.open_existing(stage_names(false), main.identity, main.logical)?;
            let digest = copy_bounded(
                original,
                main.logical,
                &mut file,
                &NativeFence::Candidate(fence),
                #[cfg(test)]
                true,
            )?;
            self.sync(&file, fence)?;
            if digest != initial
                || self.original_digest(main, &NativeFence::Candidate(fence))? != initial
                || self.digest(
                    SNAPSHOT,
                    identity,
                    main.logical,
                    &NativeFence::Candidate(fence),
                )? != initial
            {
                return Err(Error::StageIdentityMismatch);
            }
            Ok(())
        })();
        drop(file);
        if let Err(error) = result {
            self.remove_copy(created)?;
            return Err(error);
        }
        Ok((created, initial))
    }

    pub(in super::super::super) fn verify_copy(
        &self,
        copy: &FreshCopy,
        logical: u64,
        expected: [u8; 32],
        fence: &NativeWindow,
    ) -> Result<()> {
        if self.digest(
            SNAPSHOT,
            copy.identity,
            logical,
            &NativeFence::Candidate(fence),
        )? != expected
        {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }

    pub(in super::super::super) fn remove_copy(&self, copy: FreshCopy) -> Result<()> {
        self.exact(SNAPSHOT, copy.identity, None)?;
        // SAFETY: same checked directory and freshly created disposable inode,
        // with the leased owner holding single receiver file exclusion.
        let removed =
            unsafe { libc::unlinkat(self.descriptor.as_raw_fd(), SNAPSHOT[0].as_ptr(), 0) };
        if removed != 0 || stat(&self.descriptor, SNAPSHOT[0])?.is_some() {
            return Err(Error::StageIdentityMismatch);
        }
        self.clean_family(SNAPSHOT)?;
        self.descriptor.sync_all().map_err(|_| Error::Path)?;
        self.recheck()
    }
}

fn identity(file: &File) -> Result<Identity> {
    let row = file.metadata().map_err(|_| Error::Path)?;
    // SAFETY: reads the effective UID of this process.
    let uid = unsafe { libc::geteuid() };
    if !row.is_file() || row.mode() & 0o7777 != 0o600 || row.nlink() != 1 || row.uid() != uid {
        return Err(Error::StageIdentityMismatch);
    }
    Ok((row.dev(), row.ino()))
}

fn copy_bounded(
    mut file: File,
    size: u64,
    sink: &mut impl Write,
    fence: &NativeFence,
    #[cfg(test)] copying_snapshot: bool,
) -> Result<[u8; 32]> {
    if !(1..=crate::peer_quota_vfs::dispatch::CLOSED_STAGE_LIMIT).contains(&size) {
        return Err(Error::StageIdentityMismatch);
    }
    let mut total = 0;
    let mut buffer = [0; 64 * 1024];
    let mut digest = Sha256::new();
    loop {
        fence.wall()?;
        let count = file.read(&mut buffer).map_err(|_| Error::Path)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > size {
            return Err(Error::StageIdentityMismatch);
        }
        sink.write_all(&buffer[..count]).map_err(|_| Error::Path)?;
        #[cfg(test)]
        if copying_snapshot && let NativeFence::Candidate(window) = fence {
            window.copy_cut();
        }
        digest.update(&buffer[..count]);
    }
    if total != size {
        return Err(Error::StageIdentityMismatch);
    }
    fence.wall()?;
    Ok(digest.finalize().into())
}
