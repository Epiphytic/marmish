//! Fixed dirfd-relative removal for a private provisional stage.

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
    pub(in super::super) fn require_prepared(self, stage: State) -> Result<()> {
        let State::Prepared(file, _) = stage else {
            return Err(Error::StageIdentityMismatch);
        };
        if self.leaf
            != Some((
                u64::from_be_bytes(file.device),
                u64::from_be_bytes(file.inode),
            ))
        {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }

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

fn stage_names(known: bool) -> [&'static CStr; 4] {
    if known {
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
    }
}

impl<'p, 'a> RetirementDir<'p, 'a> {
    pub(in super::super) fn open(paths: &'p Paths<'a>, known: bool) -> Result<Self> {
        let (descriptor, profile, peer) = open_dirs(paths)?;
        let names = stage_names(known);
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
        Ok(private_stat(&self.descriptor, self.peer, self.names[0])?
            .map(|(identity, entry)| (identity, entry.st_size)))
    }
}

fn private_stat(
    descriptor: &File,
    peer: Identity,
    name: &CStr,
) -> Result<Option<(Identity, libc::stat)>> {
    let Some(entry) = stat(descriptor, name)? else {
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
        || device != peer.0
    {
        return Err(Error::StageIdentityMismatch);
    }
    Ok(Some(((device, entry.st_ino), entry)))
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
pub(in super::super) mod recovery {
    use super::*;
    use std::collections::BTreeSet;
    use std::os::fd::IntoRawFd;

    const CURSOR: [&CStr; 4] = [
        c"cursor.db",
        c"cursor.db-journal",
        c"cursor.db-wal",
        c"cursor.db-shm",
    ];

    const SNAPSHOT: [&CStr; 4] = [
        c"receiver-join-snapshot.db",
        c"receiver-join-snapshot.db-journal",
        c"receiver-join-snapshot.db-wal",
        c"receiver-join-snapshot.db-shm",
    ];

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(in super::super::super) struct FileExtent {
        pub(in super::super::super) identity: Identity,
        pub(in super::super::super) logical: u64,
        pub(in super::super::super) charged: u64,
    }

    struct DirectoryEntries(*mut libc::DIR);

    impl Drop for DirectoryEntries {
        fn drop(&mut self) {
            // SAFETY: this stream exclusively owns its descriptor.
            unsafe { libc::closedir(self.0) };
        }
    }

    fn enumerate(descriptor: &File, peer: Identity, families: &[[&CStr; 4]; 3]) -> Result<()> {
        let flags = libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_RDONLY;
        // SAFETY: the original descriptor and terminated "." are live.
        let fd = unsafe { libc::openat(descriptor.as_raw_fd(), c".".as_ptr(), flags) };
        if fd < 0 {
            return Err(Error::Path);
        }
        // SAFETY: openat returned a fresh descriptor and file description.
        let independent = unsafe { File::from_raw_fd(fd) };
        if checked_dir(&independent, peer.0)? != peer {
            return Err(Error::StageIdentityMismatch);
        }
        // SAFETY: fdopendir takes ownership only on success.
        let stream = unsafe { libc::fdopendir(independent.as_raw_fd()) };
        if stream.is_null() {
            return Err(Error::Path);
        }
        let _ = independent.into_raw_fd();
        let entries = DirectoryEntries(stream);
        let allowed = CURSOR.iter().chain(families.iter().flatten());
        let maximum = allowed.clone().count() + 2;
        for count in 0..=maximum {
            // SAFETY: this thread owns the stream and its errno. A successful
            // entry remains valid until the next readdir on this stream.
            let (entry, errno) = unsafe {
                *libc::__errno_location() = 0;
                let entry = libc::readdir(entries.0);
                (entry, *libc::__errno_location())
            };
            if entry.is_null() {
                return if errno == 0 { Ok(()) } else { Err(Error::Path) };
            }
            // SAFETY: readdir supplies a terminated d_name on success.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if count == maximum
                || (name != c"." && name != c".." && !allowed.clone().any(|entry| *entry == name))
            {
                return Err(Error::StageIdentityMismatch);
            }
        }
        Err(Error::StageIdentityMismatch)
    }

    pub(in super::super::super) fn inspect(
        paths: &Paths<'_>,
        cursor: &CheckedFile,
    ) -> Result<[[Option<FileExtent>; 4]; 3]> {
        cursor.unchanged()?;
        let (descriptor, profile, peer) = open_dirs(paths)?;
        let families = [stage_names(true), stage_names(false), SNAPSHOT];
        enumerate(&descriptor, peer, &families)?;
        let Some((id, _)) = private_stat(&descriptor, peer, CURSOR[0])? else {
            return Err(Error::StageIdentityMismatch);
        };
        for sidecar in &CURSOR[1..] {
            if stat(&descriptor, sidecar)?.is_some() {
                return Err(Error::StageIdentityMismatch);
            }
        }
        if id != cursor.before {
            return Err(Error::StageIdentityMismatch);
        }

        let mut identities = BTreeSet::from([id]);
        let mut extents = [[None; 4]; 3];
        for (family, extents) in families.into_iter().zip(&mut extents) {
            for (name, extent) in family.into_iter().zip(extents) {
                if let Some((identity, entry)) = private_stat(&descriptor, peer, name)? {
                    let logical = u64::try_from(entry.st_size).map_err(|_| Error::Path)?;
                    let allocated = u64::try_from(entry.st_blocks)
                        .ok()
                        .and_then(|blocks| blocks.checked_mul(512))
                        .ok_or(Error::Path)?;
                    if !identities.insert(identity) {
                        return Err(Error::StageIdentityMismatch);
                    }
                    *extent = Some(FileExtent {
                        identity,
                        logical,
                        charged: logical.max(allocated),
                    });
                }
            }
        }
        let (_, current_profile, current_peer) = open_dirs(paths)?;
        cursor.unchanged()?;
        if (profile, peer) != (current_profile, current_peer) {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(extents)
    }

    #[test]
    fn peer_receiver_inventory_rescans_same_descriptor_after_abort() {
        use crate::peer_invitation::provisional::tests::Fixture;

        let fixture = Fixture::new();
        let paths = Paths::new(&fixture.lease, &fixture.identity).unwrap();
        let (descriptor, _, peer) = open_dirs(&paths).unwrap();
        let unrecognized = paths.directory().join("unexpected.db");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&unrecognized)
            .unwrap();
        let families = [stage_names(true), stage_names(false), SNAPSHOT];
        for _ in 0..2 {
            assert!(matches!(
                enumerate(&descriptor, peer, &families),
                Err(Error::StageIdentityMismatch)
            ));
        }
        fs::remove_file(unrecognized).unwrap();
        enumerate(&descriptor, peer, &families).unwrap();
        // SAFETY: the live descriptor belongs to the unenumerated original.
        assert_eq!(
            unsafe { libc::lseek(descriptor.as_raw_fd(), 0, libc::SEEK_CUR) },
            0
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_invitation::{
        provisional::tests::Fixture,
        store::consent_checkpoint::provisioning::stage_journal::{Artifact, FileWitness},
    };

    fn prepared(path: &Path) -> State {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.set_len(4096).unwrap();
        let meta = file.metadata().unwrap();
        State::Prepared(
            FileWitness {
                device: meta.dev().to_be_bytes(),
                inode: meta.ino().to_be_bytes(),
            },
            Artifact::new([1; 32], b"test artifact", 300),
        )
    }

    #[test]
    fn joined_retirement_distinguishes_absence_before_and_after_intent() {
        for known in [false, true] {
            let fixture = Fixture::new();
            let paths = Paths::new(&fixture.lease, &fixture.identity).unwrap();
            let path = if known {
                fixture.lease.root().join("peer/provisional-known.db")
            } else {
                fixture.stage()
            };
            let stage = prepared(&path);
            let dir = RetirementDir::open(&paths, known).unwrap();
            let witness = dir.capture(stage).unwrap();
            witness.require_prepared(stage).unwrap();
            assert_eq!(witness.encode().len(), 48);
            dir.unlink(witness, stage).unwrap();
            assert_eq!(dir.capture(stage), Err(Error::StageIdentityMismatch));
            drop(dir);

            // A committed exact Intent can resume after unlink but before sync.
            let dir = RetirementDir::open(&paths, known).unwrap();
            let witness = Witness::decode(&witness.encode()).unwrap();
            witness.require_prepared(stage).unwrap();
            dir.unlink(witness, stage).unwrap();
            dir.sync_removed(witness).unwrap();
            dir.ensure_absent(witness).unwrap();
            assert!(!path.exists());
        }
    }

    #[test]
    fn joined_retirement_preserves_replacement_leaf_and_sidecar() {
        let fixture = Fixture::new();
        let paths = Paths::new(&fixture.lease, &fixture.identity).unwrap();
        let path = fixture.stage();
        let stage = prepared(&path);
        let dir = RetirementDir::open(&paths, false).unwrap();
        let witness = dir.capture(stage).unwrap();
        let sidecar = path.with_file_name("provisional-unknown.db-wal");
        prepared(&sidecar);
        assert_eq!(dir.capture(stage), Err(Error::StageIdentityMismatch));
        assert_eq!(
            dir.unlink(witness, stage),
            Err(Error::StageIdentityMismatch)
        );
        assert!(sidecar.exists());
        assert!(path.exists());
        fs::remove_file(sidecar).unwrap();

        // Retain the original under another name to prevent inode-number reuse.
        let original = path.with_file_name("original.db");
        fs::rename(&path, &original).unwrap();
        let replacement = prepared(&path);
        assert_eq!(
            witness.require_prepared(replacement),
            Err(Error::StageIdentityMismatch)
        );
        assert_eq!(
            dir.unlink(witness, stage),
            Err(Error::StageIdentityMismatch)
        );
        assert_eq!(dir.sync_removed(witness), Err(Error::StageIdentityMismatch));
        assert_eq!(
            dir.ensure_absent(witness),
            Err(Error::StageIdentityMismatch)
        );
        assert_eq!(fs::metadata(&path).unwrap().len(), 4096);
        assert_eq!(fs::metadata(original).unwrap().len(), 4096);
    }

    #[test]
    fn joined_retirement_refuses_a_replaced_private_directory() {
        let fixture = Fixture::new();
        let paths = Paths::new(&fixture.lease, &fixture.identity).unwrap();
        let path = fixture.stage();
        let stage = prepared(&path);
        let dir = RetirementDir::open(&paths, false).unwrap();
        let witness = dir.capture(stage).unwrap();
        let peer = path.parent().unwrap();
        let original = peer.with_file_name("original-peer");
        fs::rename(peer, &original).unwrap();
        paths.initialize().unwrap();
        assert_eq!(
            dir.unlink(witness, stage),
            Err(Error::StageIdentityMismatch)
        );
        let replacement = RetirementDir::open(&paths, false).unwrap();
        assert_eq!(
            replacement.ensure_absent(witness),
            Err(Error::StageIdentityMismatch)
        );
        assert!(original.join(path.file_name().unwrap()).exists());
    }
}
