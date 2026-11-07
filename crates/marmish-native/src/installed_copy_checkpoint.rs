//! Feature-only, read-only receiver proof. Never grants cleanup or lock authority.
//! Linked only after the canonical copy protocol is integrated.
use super::records;
use crate::{Error, Result};
use marmish_stream::copy_protocol::{Checkpoint, FileIdentity, Launch, MAX_CONTROL, Phase};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{File, Metadata, OpenOptions},
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::Path,
};
fn bad() -> Error {
    Error::Invalid("installed_copy_checkpoint")
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Proof {
    pub transfer_sha256: String,
    pub stream_sha256: String,
    pub manifest_sha256: String,
    pub phase: String,
    pub durable_offset: u64,
    pub prefix_sha256: String,
    pub size: u64,
    pub sha256: String,
}
pub(super) struct Expected<'a> {
    /// Owned, canonical destination parent. No artifact can select this path.
    pub parent: &'a Path,
    pub launch: &'a Launch,
    pub stream: [u8; 16],
    pub size: u64,
    pub sha256: [u8; 32],
    pub committed: bool,
}
fn identity(meta: &Metadata) -> FileIdentity {
    FileIdentity {
        device: meta.dev(),
        inode: meta.ino(),
    }
}
fn check(file: &File, directory: bool) -> Result<Metadata> {
    let m = file.metadata()?;
    if m.uid() != unsafe { libc::geteuid() }
        || (if directory {
            !m.is_dir() || m.mode() & 0o7777 != 0o700
        } else {
            !m.is_file() || m.mode() & 0o7777 != 0o600 || m.nlink() != 1
        })
    {
        return Err(bad());
    }
    Ok(m)
}
fn open(parent: &File, name: &[u8], directory: bool) -> Result<File> {
    if name.is_empty() || name.contains(&b'/') {
        return Err(bad());
    }
    let name = CString::new(name).map_err(|_| bad())?;
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    check(&file, directory)?;
    Ok(file)
}
fn checkpoint(private: &File) -> Result<Vec<u8>> {
    let mut file = open(private, b"checkpoint", false)?;
    if file.metadata()?.len() > MAX_CONTROL as u64 {
        return Err(bad());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_CONTROL as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CONTROL {
        return Err(bad());
    }
    Ok(bytes)
}
/// A changing checkpoint is retriable by the bounded caller. No flock is taken:
/// the active receiver must retain its exclusive lock throughout observation.
pub(super) fn observe(e: Expected<'_>) -> Result<Proof> {
    e.launch.validate().map_err(|_| bad())?;
    crate::identity::check_dir(e.parent)?;
    if e.parent.canonicalize()? != e.parent {
        return Err(bad());
    }
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(e.parent)?;
    let parent_id = identity(&check(&parent, true)?);
    // Qualification workspaces are canonical. Explicitly reject an absolute
    // alias (notably /var, /tmp or /etc on macOS) rather than compare raw launch
    // bytes with a differently normalized checkpoint path.
    let destination = Path::new(std::ffi::OsStr::from_bytes(&e.launch.destination));
    if destination.is_absolute() && destination.parent() != Some(e.parent) {
        return Err(bad());
    }
    let namespace = e.launch.namespace();
    let private = open(&parent, namespace.as_bytes(), true)?;
    let private_id = identity(&check(&private, true)?);
    // Validate the lock inode without acquiring or claiming ownership of its flock.
    let _lock = open(&private, b"lock", false)?;
    let before = checkpoint(&private)?;
    let c = Checkpoint::decode(&before).map_err(|_| bad())?;
    if !e.launch.matches_manifest(&c.manifest)
        || c.manifest.size != e.size
        || c.manifest.sha256 != e.sha256
        || c.last_stream != e.stream
        || c.parent != parent_id
        || c.private != private_id
        || c.normalized_path != e.launch.destination
        || (e.committed && c.phase != Phase::Committed)
        || (!e.committed
            && (c.phase != Phase::Receiving || c.durable_offset == 0 || c.durable_offset >= e.size))
    {
        return Err(bad());
    }
    // Committed data was renamed out of the private namespace. Only that exact
    // destination basename is opened; relative components cannot escape this FD.
    let data = if e.committed {
        let name = Path::new(std::ffi::OsStr::from_bytes(&e.launch.destination))
            .file_name()
            .ok_or_else(bad)?;
        open(&parent, name.as_bytes(), false)?
    } else {
        open(&private, b"data", false)?
    };
    let metadata = check(&data, false)?;
    if identity(&metadata) != c.data
        || metadata.len() < c.durable_offset
        || (e.committed && metadata.len() != e.size)
    {
        return Err(bad());
    }
    let mut hash = Sha256::new();
    let mut reader = data.take(c.durable_offset);
    let mut block = [0; 32768];
    loop {
        let n = reader.read(&mut block)?;
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
    }
    if reader.limit() != 0
        || hash.finalize().as_slice() != c.prefix_sha256
        || checkpoint(&private)? != before
    {
        return Err(bad());
    }
    Ok(Proof {
        transfer_sha256: records::digest(hex::encode(c.manifest.transfer).as_bytes()),
        stream_sha256: records::digest(hex::encode(c.last_stream).as_bytes()),
        manifest_sha256: records::digest(&c.manifest.encode().map_err(|_| bad())?),
        phase: if e.committed {
            "committed"
        } else {
            "receiving"
        }
        .into(),
        durable_offset: c.durable_offset,
        prefix_sha256: hex::encode(c.prefix_sha256),
        size: c.manifest.size,
        sha256: hex::encode(c.manifest.sha256),
    })
}

/// Exclude exactly one proven receiver namespace after validating its committed
/// checkpoint. Never skip a name pattern or treat a namespace as payload.
pub(super) fn inventory(e: Expected<'_>) -> Result<(Proof, Vec<super::transfer_fixture::Item>)> {
    if !e.committed {
        return Err(bad());
    }
    let namespace = e.launch.namespace();
    let root = e.parent;
    let proof = observe(e)?;
    // CopyReceiver also retains one fixed parent admission lock. Validate it
    // through the owned parent descriptor before excluding that exact file;
    // unrelated private names remain visible and fail strict comparisons.
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)?;
    check(&parent, true)?;
    let admission = open(&parent, b".marmish-copy-admission.lock", false)?;
    if admission.metadata()?.len() != 0 {
        return Err(bad());
    }
    let mut items = super::transfer_fixture::inventory_except(root, Some(&namespace))?;
    let lock_name = hex::encode(b".marmish-copy-admission.lock");
    let before = items.len();
    items.retain(|item| item.path_hex != lock_name);
    if items.len() + 1 != before {
        return Err(bad());
    }
    Ok((proof, items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copy_files::{CopyReceiver, trusted_base};
    use marmish_stream::copy_protocol::{
        AttemptMode, Direction, MIN_RETENTION_SECS, Manifest, VERSION,
    };

    fn launch() -> Launch {
        Launch {
            version: VERSION,
            mode: AttemptMode::Create,
            transfer: [1; 32],
            client: [2; 32],
            server: [3; 32],
            direction: Direction::Put,
            source: b"source".to_vec(),
            destination: b"target".to_vec(),
            overwrite: false,
            preserve_mode: true,
            preserve_mtime: false,
        }
    }
    fn manifest(l: &Launch, bytes: &[u8]) -> Manifest {
        Manifest {
            version: l.version,
            transfer: l.transfer,
            client: l.client,
            server: l.server,
            direction: l.direction,
            source: l.source.clone(),
            destination: l.destination.clone(),
            overwrite: l.overwrite,
            preserve_mode: l.preserve_mode,
            preserve_mtime: l.preserve_mtime,
            size: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
            mode: Some(0o600),
            mtime: None,
        }
    }
    #[test]
    fn real_receiver_proves_durable_prefix_while_lock_held_then_exact_commit() {
        let temp = crate::journal::private_test_directory();
        let parent = temp.path().canonicalize().unwrap();
        let base = trusted_base(&parent).unwrap();
        let launch = launch();
        let bytes = b"complete deterministic bytes";
        let m = manifest(&launch, bytes);
        let mut receiver = CopyReceiver::open_or_recover(
            &base,
            b"target",
            m.clone(),
            AttemptMode::Create,
            [4; 16],
            100,
            MIN_RETENTION_SECS,
        )
        .unwrap();
        receiver.receive_data(0, &bytes[..8]).unwrap();
        receiver.checkpoint(101).unwrap();
        let expected = || Expected {
            parent: &parent,
            launch: &launch,
            stream: [4; 16],
            size: m.size,
            sha256: m.sha256,
            committed: false,
        };
        let proof = observe(expected()).unwrap();
        assert_eq!(proof.durable_offset, 8);
        assert_eq!(proof.prefix_sha256, records::digest(&bytes[..8]));
        let mut wrong = expected();
        wrong.stream = [5; 16];
        assert!(observe(wrong).is_err());
        let mut wrong = expected();
        wrong.committed = true;
        assert!(observe(wrong).is_err());
        receiver.receive_data(8, &bytes[8..]).unwrap();
        receiver.checkpoint(102).unwrap();
        receiver.prepare_commit(103).unwrap();
        receiver.commit().unwrap();
        let mut final_expected = expected();
        final_expected.committed = true;
        let proof = observe(final_expected).unwrap();
        assert_eq!(proof.phase, "committed");
        assert_eq!(proof.durable_offset, bytes.len() as u64);
        assert_eq!(proof.prefix_sha256, records::digest(bytes));
        let mut inventory_expected = expected();
        inventory_expected.committed = true;
        let (_, items) = inventory(inventory_expected).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path_hex, hex::encode(b"target"));
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(parent.join(".marmish-copy-v1-unrelated"))
            .unwrap();
        let mut inventory_expected = expected();
        inventory_expected.committed = true;
        let (_, items) = inventory(inventory_expected).unwrap();
        assert_eq!(
            items.len(),
            2,
            "unknown namespaces must remain visible to strict fixture comparisons"
        );
        // A full-size checkpoint cannot qualify the intermediate interruption gate.
        assert!(observe(expected()).is_err());
        std::fs::write(parent.join(".marmish-copy-admission.lock"), b"unexpected").unwrap();
        let mut inventory_expected = expected();
        inventory_expected.committed = true;
        assert!(inventory(inventory_expected).is_err());
    }
    #[test]
    fn checksum_prefix_and_symlink_tampering_cannot_qualify_a_checkpoint() {
        let temp = crate::journal::private_test_directory();
        let parent = temp.path().canonicalize().unwrap();
        let base = trusted_base(&parent).unwrap();
        let launch = launch();
        let m = manifest(&launch, b"abcdefgh");
        let mut receiver = CopyReceiver::open_or_recover(
            &base,
            b"target",
            m.clone(),
            AttemptMode::Create,
            [4; 16],
            100,
            MIN_RETENTION_SECS,
        )
        .unwrap();
        receiver.receive_data(0, b"abcd").unwrap();
        receiver.checkpoint(101).unwrap();
        let expected = || Expected {
            parent: &parent,
            launch: &launch,
            stream: [4; 16],
            size: m.size,
            sha256: m.sha256,
            committed: false,
        };
        let namespace = parent.join(launch.namespace());
        std::fs::write(namespace.join("data"), b"xxxx").unwrap();
        assert!(observe(expected()).is_err());
        std::fs::write(namespace.join("data"), b"abcd").unwrap();
        let original = std::fs::read(namespace.join("checkpoint")).unwrap();
        let mut corrupt = original.clone();
        corrupt[0] ^= 1;
        std::fs::write(namespace.join("checkpoint"), corrupt).unwrap();
        assert!(observe(expected()).is_err());
        std::fs::write(namespace.join("checkpoint"), original).unwrap();
        std::fs::rename(namespace.join("checkpoint"), namespace.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink("elsewhere", namespace.join("checkpoint")).unwrap();
        assert!(observe(expected()).is_err());
    }
}
