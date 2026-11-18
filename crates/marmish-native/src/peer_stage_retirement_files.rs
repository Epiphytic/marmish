//! Fixed dirfd-relative removal for a never-exported stage.

use std::ffi::CStr;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd};

use super::super::store::consent_checkpoint::provisioning::stage_journal::State;
use super::*;

type Identity = (u64, u64);

pub(in super::super) struct RetirementDir<'p, 'a> {
    paths: &'p Paths<'a>,
    descriptor: File,
    profile: Identity,
    peer: Identity,
    names: [&'static CStr; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) struct Witness {
    profile: Identity,
    peer: Identity,
    leaf: Option<Identity>,
}

impl Witness {
    pub(in super::super) fn encode(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(48);
        for identity in [Some(self.profile), Some(self.peer), self.leaf]
            .into_iter()
            .flatten()
        {
            bytes.extend_from_slice(&identity.0.to_be_bytes());
            bytes.extend_from_slice(&identity.1.to_be_bytes());
        }
        bytes
    }

    pub(in super::super) fn decode(bytes: &[u8]) -> Result<Self> {
        fn identity(bytes: &[u8]) -> Result<Identity> {
            let [a, b] = bytes.as_chunks::<8>().0 else {
                return Err(Error::StageIdentityMismatch);
            };
            Ok((u64::from_be_bytes(*a), u64::from_be_bytes(*b)))
        }
        match bytes.len() {
            32 | 48 => Ok(Self {
                profile: identity(&bytes[..16])?,
                peer: identity(&bytes[16..32])?,
                leaf: bytes.get(32..48).map(identity).transpose()?,
            }),
            _ => Err(Error::StageIdentityMismatch),
        }
    }
}

fn checked_dir(file: &File, device: u64) -> Result<Identity> {
    let meta = file.metadata().map_err(|_| Error::Path)?;
    // SAFETY: reads the current process's effective UID.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.mode() & 0o7777 != 0o700 || meta.uid() != uid || meta.dev() != device
    {
        return Err(Error::StageIdentityMismatch);
    }
    Ok((meta.dev(), meta.ino()))
}

fn open_dirs(paths: &Paths<'_>) -> Result<(File, Identity, Identity)> {
    let device = paths.cursor()?.before.0;
    let flags = libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(paths.lease.root())
        .map_err(|_| Error::Path)?;
    let profile = checked_dir(&root, device)?;
    // SAFETY: `root` is live; the static name is terminated, and no mode is used.
    let fd = unsafe { libc::openat(root.as_raw_fd(), c"peer".as_ptr(), flags | libc::O_RDONLY) };
    if fd < 0 {
        return Err(Error::Path);
    }
    // SAFETY: successful openat returned a new descriptor.
    let peer = unsafe { File::from_raw_fd(fd) };
    let identity = checked_dir(&peer, device)?;
    Ok((peer, profile, identity))
}

impl<'p, 'a> RetirementDir<'p, 'a> {
    pub(in super::super) fn open(paths: &'p Paths<'a>, known: bool) -> Result<Self> {
        let (descriptor, profile, peer) = open_dirs(paths)?;
        let names = if known {
            [
                c"provisional-known.db",
                c"provisional-known.db-journal",
                c"provisional-known.db-wal",
                c"provisional-known.db-shm",
            ]
        } else {
            [
                c"provisional-unknown.db",
                c"provisional-unknown.db-journal",
                c"provisional-unknown.db-wal",
                c"provisional-unknown.db-shm",
            ]
        };
        Ok(Self {
            paths,
            descriptor,
            profile,
            peer,
            names,
        })
    }

    pub(in super::super) fn capture(&self, stage: State) -> Result<Witness> {
        self.require_names_private()?;
        let leaf = self.leaf()?;
        match (stage, leaf) {
            (State::Bound, None | Some((_, 0))) => {}
            (State::NativeStarted(witness) | State::Prepared(witness, _), Some((id, _)))
                if (
                    u64::from_be_bytes(witness.device),
                    u64::from_be_bytes(witness.inode),
                ) == id => {}
            _ => return Err(Error::StageIdentityMismatch),
        }
        Ok(Witness {
            profile: self.profile,
            peer: self.peer,
            leaf: leaf.map(|(id, _)| id),
        })
    }

    pub(in super::super) fn ensure_absent(&self, witness: Witness) -> Result<()> {
        self.check_witness(witness)?;
        if self.leaf()?.is_some() {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }

    pub(in super::super) fn unlink(&self, witness: Witness, stage: State) -> Result<()> {
        self.check_witness(witness)?;
        if let Some((identity, size)) = self.leaf()? {
            if Some(identity) != witness.leaf || (matches!(stage, State::Bound) && size != 0) {
                return Err(Error::StageIdentityMismatch);
            }
            // SAFETY: descriptor and fixed NUL-terminated name remain live.
            // The profile lease serializes authorized mutations of this directory.
            let removed =
                unsafe { libc::unlinkat(self.descriptor.as_raw_fd(), self.names[0].as_ptr(), 0) };
            if removed != 0 {
                return Err(Error::Path);
            }
        }
        self.ensure_absent(witness)
    }

    pub(in super::super) fn sync_removed(&self, witness: Witness) -> Result<()> {
        self.ensure_absent(witness)?;
        self.descriptor.sync_all().map_err(|_| Error::Path)
    }

    fn check_witness(&self, witness: Witness) -> Result<()> {
        if self.profile != witness.profile || self.peer != witness.peer {
            return Err(Error::StageIdentityMismatch);
        }
        self.require_names_private()
    }

    fn require_names_private(&self) -> Result<()> {
        let (_, profile, peer) = open_dirs(self.paths)?;
        if self.profile != profile
            || self.peer != peer
            || checked_dir(&self.descriptor, self.peer.0)? != self.peer
        {
            return Err(Error::StageIdentityMismatch);
        }
        for sidecar in &self.names[1..] {
            if stat(&self.descriptor, sidecar)?.is_some() {
                return Err(Error::StageIdentityMismatch);
            }
        }
        Ok(())
    }

    fn leaf(&self) -> Result<Option<(Identity, libc::off_t)>> {
        let Some(entry) = stat(&self.descriptor, self.names[0])? else {
            return Ok(None);
        };
        #[cfg(target_os = "linux")]
        let device = entry.st_dev;
        #[cfg(target_os = "macos")]
        let device = entry.st_dev as u64;
        // SAFETY: reads this Unix process's effective UID.
        let uid = unsafe { libc::geteuid() };
        if entry.st_mode & libc::S_IFMT != libc::S_IFREG
            || entry.st_mode & 0o7777 != 0o600
            || entry.st_uid != uid
            || entry.st_nlink != 1
            || device != self.peer.0
        {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(Some(((device, entry.st_ino), entry.st_size)))
    }
}

fn stat(parent: &File, name: &CStr) -> Result<Option<libc::stat>> {
    let mut stat = MaybeUninit::uninit();
    // SAFETY: descriptor, name and writable stat buffer are live.
    let ret = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if ret == 0 {
        // SAFETY: successful fstatat initialized the entire output.
        Ok(Some(unsafe { stat.assume_init() }))
    } else if io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(Error::Path)
    }
}
