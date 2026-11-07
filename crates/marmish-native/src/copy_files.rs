//! Blocking regular-file copy primitives. Call only on an owned filesystem
//! worker, never the transport actor. Principal claims must already be checked
//! against the actual authenticated binding before constructing a Manifest.
use fs2::FileExt as LockExt;
use marmish_stream::copy_protocol::{
    self as protocol, AttemptMode, Checkpoint, CopyError, FileIdentity, Manifest, Phase,
};
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, CString},
    fs::{File, Metadata, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{FileExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

pub type Result<T> = std::result::Result<T, Failure>;
#[derive(Debug)]
pub struct Failure {
    pub code: CopyError,
    pub source: Option<io::Error>,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "copy {:?}", self.code)
    }
}
impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|error| error as &dyn std::error::Error)
    }
}
impl From<io::Error> for Failure {
    fn from(source: io::Error) -> Self {
        let code = match source.raw_os_error() {
            Some(libc::ENOENT) => CopyError::NotFound,
            Some(libc::EACCES | libc::EPERM) => CopyError::PermissionDenied,
            Some(libc::ELOOP | libc::ENOTDIR) => CopyError::UnsafePath,
            Some(libc::EEXIST) => CopyError::Conflict,
            Some(libc::ENOSPC | libc::EDQUOT) => CopyError::DiskFull,
            Some(libc::ENOSYS | libc::ENOTSUP) => CopyError::Unsupported,
            _ => CopyError::LocalIo,
        };
        Self {
            code,
            source: Some(source),
        }
    }
}
impl From<marmish_stream::Error> for Failure {
    fn from(_: marmish_stream::Error) -> Self {
        failure(CopyError::Protocol)
    }
}
fn failure(code: CopyError) -> Failure {
    Failure { code, source: None }
}
fn syscall(value: libc::c_int) -> Result<()> {
    if value < 0 {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}
fn identity(meta: &Metadata) -> FileIdentity {
    FileIdentity {
        device: meta.dev(),
        inode: meta.ino(),
    }
}
fn stat_identity(stat: &libc::stat) -> FileIdentity {
    #[cfg(target_os = "linux")]
    let device = stat.st_dev;
    #[cfg(target_os = "macos")]
    let device = stat.st_dev as u64;
    FileIdentity {
        device,
        inode: stat.st_ino,
    }
}
fn name(bytes: &[u8]) -> Result<CString> {
    CString::new(bytes).map_err(|_| failure(CopyError::UnsafePath))
}
fn open_at(parent: &File, name: &CStr, flags: i32, mode: libc::mode_t) -> Result<File> {
    #[cfg(target_os = "macos")]
    let mode = libc::c_uint::from(mode);
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn entry_stat(parent: &File, name: &CStr) -> Result<libc::stat> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    syscall(unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    Ok(unsafe { stat.assume_init() })
}
fn open_existing_private(
    parent: &File,
    name: &CStr,
    flags: i32,
    exact_mode: Option<u32>,
) -> Result<File> {
    let before = entry_stat(parent, name)?;
    let mode = before.st_mode as u32 & 0o7777;
    if before.st_mode & libc::S_IFMT != libc::S_IFREG
        || before.st_uid != unsafe { libc::geteuid() }
        || before.st_nlink != 1
        || mode & 0o600 != 0o600
        || exact_mode.is_some_and(|expected| mode != expected)
    {
        return Err(failure(CopyError::UnsafePath));
    }
    let file = open_at(parent, name, flags, 0)?;
    let after = file.metadata()?;
    if !after.is_file() || identity(&after) != stat_identity(&before) {
        return Err(failure(CopyError::IdentityChanged));
    }
    Ok(file)
}
fn optional_stat(parent: &File, name: &CStr) -> Result<Option<libc::stat>> {
    match entry_stat(parent, name) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.code == CopyError::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn private_meta(file: &File, directory: bool) -> Result<Metadata> {
    let m = file.metadata()?;
    let expected = if directory { 0o700 } else { 0o600 };
    if (if directory { !m.is_dir() } else { !m.is_file() })
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o7777 != expected
        || (!directory && m.nlink() != 1)
    {
        return Err(failure(CopyError::UnsafePath));
    }
    Ok(m)
}
fn sync_file(file: &File) -> Result<()> {
    file.sync_all()?;
    #[cfg(target_os = "macos")]
    syscall(unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) })?;
    Ok(())
}
fn sync_directory(file: &File) -> Result<()> {
    file.sync_all().map_err(Into::into)
}

/// Only trusted bases may follow symlinks. Untrusted components below this FD
/// always use descriptor-relative O_NOFOLLOW opens.
pub fn trusted_base(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(path)
        .map_err(Into::into)
}

pub struct PinnedPath {
    parent: File,
    basename: CString,
    normalized: Vec<u8>,
}
impl PinnedPath {
    pub fn open(base: &File, raw: &[u8]) -> Result<Self> {
        if raw.is_empty()
            || raw.len() > protocol::MAX_PATH
            || raw.contains(&0)
            || raw.ends_with(b"/")
        {
            return Err(failure(CopyError::UnsafePath));
        }
        let normalized = normalize(raw);
        let parts: Vec<&[u8]> = normalized
            .split(|b| *b == b'/')
            .filter(|part| !part.is_empty())
            .collect();
        if parts.is_empty() || parts.iter().any(|part| *part == b"." || *part == b"..") {
            return Err(failure(CopyError::UnsafePath));
        }
        let mut parent = if normalized.starts_with(b"/") {
            trusted_base(Path::new("/"))?
        } else {
            base.try_clone()?
        };
        if !parent.metadata()?.is_dir() {
            return Err(failure(CopyError::UnsafePath));
        }
        for component in &parts[..parts.len() - 1] {
            parent = open_at(
                &parent,
                &name(component)?,
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            )?;
        }
        let basename = name(parts.last().unwrap())?;
        Ok(Self {
            parent,
            basename,
            normalized,
        })
    }
    pub fn parent_identity(&self) -> Result<FileIdentity> {
        Ok(identity(&self.parent.metadata()?))
    }
    fn regular(&self) -> Result<File> {
        let observed = entry_stat(&self.parent, &self.basename)?;
        if observed.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(failure(CopyError::UnsafePath));
        }
        let file = open_at(
            &self.parent,
            &self.basename,
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOCTTY,
            0,
        )?;
        let m = file.metadata()?;
        if !m.is_file() || identity(&m) != stat_identity(&observed) {
            return Err(failure(CopyError::IdentityChanged));
        }
        Ok(file)
    }
}
fn normalize(raw: &[u8]) -> Vec<u8> {
    #[cfg(target_os = "macos")]
    for alias in [b"/tmp".as_slice(), b"/var", b"/etc"] {
        if raw == alias || (raw.starts_with(alias) && raw.get(alias.len()) == Some(&b'/')) {
            let mut path = b"/private".to_vec();
            path.extend(raw);
            return path;
        }
    }
    raw.to_vec()
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SourceStamp {
    id: FileIdentity,
    size: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}
impl SourceStamp {
    fn read(file: &File) -> Result<Self> {
        let m = file.metadata()?;
        if !m.is_file() || m.len() > marmish_stream::MAX_OFFSET {
            return Err(failure(CopyError::UnsafePath));
        }
        Ok(Self {
            id: identity(&m),
            size: m.len(),
            mtime: m.mtime(),
            mtime_ns: m.mtime_nsec(),
            ctime: m.ctime(),
            ctime_ns: m.ctime_nsec(),
        })
    }
}
pub struct CopySource {
    file: File,
    stamp: SourceStamp,
}
impl CopySource {
    pub fn open(path: PinnedPath) -> Result<Self> {
        let file = path.regular()?;
        let stamp = SourceStamp::read(&file)?;
        Ok(Self { file, stamp })
    }
    pub fn size(&self) -> u64 {
        self.stamp.size
    }
    pub fn metadata(&self) -> Result<Metadata> {
        self.unchanged()?;
        Ok(self.file.metadata()?)
    }
    pub fn unchanged(&self) -> Result<()> {
        if SourceStamp::read(&self.file)? != self.stamp {
            Err(failure(CopyError::SourceChanged))
        } else {
            Ok(())
        }
    }
    #[cfg(test)]
    pub fn prefix_hash(&self, length: u64) -> Result<[u8; 32]> {
        self.unchanged()?;
        if length > self.stamp.size {
            return Err(failure(CopyError::SourceChanged));
        }
        let hash = hash_prefix(&self.file, length)?;
        self.unchanged()?;
        Ok(hash.finalize().into())
    }
    pub fn read_chunk(&self, offset: u64) -> Result<Vec<u8>> {
        self.unchanged()?;
        if offset > self.stamp.size {
            return Err(failure(CopyError::Protocol));
        }
        let mut bytes =
            vec![0; ((self.stamp.size - offset).min(marmish_stream::MAX_BLOCK as u64)) as usize];
        self.file.read_exact_at(&mut bytes, offset)?;
        self.unchanged()?;
        Ok(bytes)
    }
}
fn hash_prefix(file: &File, length: u64) -> Result<Sha256> {
    if length > marmish_stream::MAX_OFFSET {
        return Err(failure(CopyError::Protocol));
    }
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    let mut offset = 0;
    while offset < length {
        let n = (length - offset).min(bytes.len() as u64) as usize;
        file.read_exact_at(&mut bytes[..n], offset)?;
        hash.update(&bytes[..n]);
        offset += n as u64;
    }
    Ok(hash)
}

struct HeldLock(File);
impl HeldLock {
    fn acquire(file: File) -> Result<Self> {
        private_meta(&file, false)?;
        LockExt::try_lock_exclusive(&file).map_err(|e| {
            if e.kind() == io::ErrorKind::WouldBlock {
                failure(CopyError::Busy)
            } else {
                e.into()
            }
        })?;
        Ok(Self(file))
    }
}
impl Drop for HeldLock {
    fn drop(&mut self) {
        let _ = LockExt::unlock(&self.0);
    }
}
fn admission_lock(parent: &File) -> Result<HeldLock> {
    let file = match open_at(
        parent,
        c".marmish-copy-admission.lock",
        libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
        0o600,
    ) {
        Ok(f) => f,
        Err(e) if e.code == CopyError::Conflict => open_existing_private(
            parent,
            c".marmish-copy-admission.lock",
            libc::O_RDWR | libc::O_NONBLOCK | libc::O_NOCTTY,
            Some(0o600),
        )?,
        Err(e) => return Err(e),
    };
    HeldLock::acquire(file)
}
fn enforce_count(parent: &File) -> Result<()> {
    // fdopendir owns an independent descriptor. No other operation traverses it.
    let duplicate = unsafe { libc::fcntl(parent.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let dir = unsafe { libc::fdopendir(duplicate) };
    if dir.is_null() {
        unsafe { libc::close(duplicate) };
        return Err(io::Error::last_os_error().into());
    }
    struct CloseDir(*mut libc::DIR);
    impl Drop for CloseDir {
        fn drop(&mut self) {
            unsafe { libc::closedir(self.0) };
        }
    }
    let owned = CloseDir(dir);
    let mut entries = 0usize;
    let mut transfers = 0usize;
    // A cloned directory descriptor shares its seek cursor: rewind before scan.
    unsafe { libc::rewinddir(owned.0) };
    loop {
        // Clear errno to distinguish normal EOF from readdir failure.
        set_errno_zero();
        let entry = unsafe { libc::readdir(owned.0) };
        if entry.is_null() {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(0) {
                return Err(e.into());
            }
            break;
        }
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        entries += 1;
        if entries > 100_000 {
            return Err(failure(CopyError::Quota));
        }
        if bytes.starts_with(b".marmish-copy-v1-") {
            transfers += 1;
        }
    }
    if transfers >= 1024 {
        return Err(failure(CopyError::Quota));
    }
    Ok(())
}
fn set_errno_zero() {
    unsafe {
        #[cfg(target_os = "linux")]
        {
            *libc::__errno_location() = 0;
        }
        #[cfg(target_os = "macos")]
        {
            *libc::__error() = 0;
        }
    }
}

pub struct DurablePrefix {
    pub offset: u64,
    pub sha256: [u8; 32],
}
pub struct Receipt {
    pub transfer: [u8; 32],
    pub size: u64,
    pub sha256: [u8; 32],
}

/// Holds the private transfer flock until dropped, including disconnect waits.
/// Drop never commits, removes partials or signals another attempt.
pub struct CopyReceiver {
    destination: PinnedPath,
    private: File,
    _lock: HeldLock,
    data: Option<File>,
    checkpoint: Checkpoint,
    accepted: u64,
    hash: Sha256,
}
impl CopyReceiver {
    pub(crate) fn parent_identity(&self) -> Result<FileIdentity> {
        self.destination.parent_identity()
    }

    pub fn open_or_recover(
        base: &File,
        raw_path: &[u8],
        manifest: Manifest,
        mode: AttemptMode,
        stream: [u8; 16],
        now: i64,
        retention: i64,
    ) -> Result<Self> {
        manifest.validate()?;
        if raw_path != manifest.destination {
            return Err(failure(CopyError::Conflict));
        }
        if !(protocol::MIN_RETENTION_SECS..=protocol::MAX_RETENTION_SECS as i64)
            .contains(&retention)
        {
            return Err(failure(CopyError::Protocol));
        }
        let destination = PinnedPath::open(base, raw_path)?;
        let _admission = admission_lock(&destination.parent)?;
        let namespace = name(manifest.namespace().as_bytes())?;
        match mode {
            AttemptMode::Create => {
                if optional_stat(&destination.parent, &namespace)?.is_some() {
                    return Err(failure(CopyError::Conflict));
                }
                enforce_count(&destination.parent)?;
                syscall(unsafe {
                    libc::mkdirat(destination.parent.as_raw_fd(), namespace.as_ptr(), 0o700)
                })?;
                sync_directory(&destination.parent)?;
                let private = open_at(
                    &destination.parent,
                    &namespace,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?;
                let private_id = identity(&private_meta(&private, true)?);
                let lock = HeldLock::acquire(open_at(
                    &private,
                    c"lock",
                    libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                    0o600,
                )?)?;
                let data = open_at(
                    &private,
                    c"data",
                    libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                    0o600,
                )?;
                let data_id = identity(&private_meta(&data, false)?);
                sync_file(&data)?;
                let checkpoint = Checkpoint {
                    manifest,
                    phase: Phase::Created,
                    normalized_path: destination.normalized.clone(),
                    parent: destination.parent_identity()?,
                    private: private_id,
                    data: data_id,
                    durable_offset: 0,
                    prefix_sha256: Sha256::digest([]).into(),
                    last_stream: stream,
                    created: now,
                    updated: now,
                    expires: now
                        .checked_add(retention)
                        .ok_or_else(|| failure(CopyError::Protocol))?,
                    committed_final: None,
                };
                let mut receiver = Self {
                    destination,
                    private,
                    _lock: lock,
                    data: Some(data),
                    checkpoint,
                    accepted: 0,
                    hash: Sha256::new(),
                };
                receiver.persist()?;
                Ok(receiver)
            }
            AttemptMode::Resume => {
                let private = open_at(
                    &destination.parent,
                    &namespace,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                    0,
                )?;
                private_meta(&private, true)?;
                let lock_file = open_existing_private(
                    &private,
                    c"lock",
                    libc::O_RDWR | libc::O_NONBLOCK | libc::O_NOCTTY,
                    Some(0o600),
                )
                .map_err(|e| {
                    if e.code == CopyError::NotFound {
                        failure(CopyError::Busy)
                    } else {
                        e
                    }
                })?;
                let lock = HeldLock::acquire(lock_file)?;
                let file = open_existing_private(
                    &private,
                    c"checkpoint",
                    libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOCTTY,
                    Some(0o600),
                )
                .map_err(|e| {
                    if e.code == CopyError::NotFound {
                        failure(CopyError::Busy)
                    } else {
                        e
                    }
                })?;
                private_meta(&file, false)?;
                let mut bytes = Vec::new();
                file.take((protocol::MAX_CONTROL + 1) as u64)
                    .read_to_end(&mut bytes)?;
                let mut checkpoint = Checkpoint::decode(&bytes)?;
                if checkpoint.manifest != manifest
                    || checkpoint.normalized_path != destination.normalized
                {
                    return Err(failure(CopyError::Conflict));
                }
                if checkpoint.parent != destination.parent_identity()?
                    || checkpoint.private != identity(&private.metadata()?)
                {
                    return Err(failure(CopyError::IdentityChanged));
                }
                let data = match open_existing_private(
                    &private,
                    c"data",
                    libc::O_RDWR | libc::O_NONBLOCK | libc::O_NOCTTY,
                    None,
                ) {
                    Ok(file) => Some(file),
                    Err(e)
                        if e.code == CopyError::NotFound
                            && matches!(
                                checkpoint.phase,
                                Phase::CommitPrepared | Phase::Committed
                            ) =>
                    {
                        None
                    }
                    Err(e) => return Err(e),
                };
                if let Some(data) = &data {
                    let meta = data.metadata()?;
                    if !meta.is_file()
                        || meta.uid() != unsafe { libc::geteuid() }
                        || meta.nlink() != 1
                        || identity(&meta) != checkpoint.data
                    {
                        return Err(failure(CopyError::IdentityChanged));
                    }
                    let expected_mode =
                        if matches!(checkpoint.phase, Phase::Verifying | Phase::CommitPrepared) {
                            manifest.mode.unwrap_or(0o600)
                        } else {
                            0o600
                        };
                    if meta.mode() & 0o7777 != expected_mode
                        && !(checkpoint.phase == Phase::Verifying && meta.mode() & 0o7777 == 0o600)
                    {
                        return Err(failure(CopyError::UnsafePath));
                    }
                }
                let mut receiver = Self {
                    destination,
                    private,
                    _lock: lock,
                    data,
                    accepted: checkpoint.durable_offset,
                    hash: Sha256::new(),
                    checkpoint: checkpoint.clone(),
                };
                if matches!(
                    checkpoint.phase,
                    Phase::Created | Phase::Receiving | Phase::Verifying
                ) {
                    let data = receiver
                        .data
                        .as_ref()
                        .ok_or_else(|| failure(CopyError::IdentityChanged))?;
                    if data.metadata()?.len() < checkpoint.durable_offset {
                        return Err(failure(CopyError::HashMismatch));
                    }
                    let before = SourceStamp::read(data)?;
                    receiver.hash = hash_prefix(data, checkpoint.durable_offset)?;
                    if SourceStamp::read(data)? != before {
                        return Err(failure(CopyError::SourceChanged));
                    }
                    if receiver.hash.clone().finalize().as_slice() != checkpoint.prefix_sha256 {
                        return Err(failure(CopyError::HashMismatch));
                    }
                    if data.metadata()?.len() != checkpoint.durable_offset {
                        data.set_len(checkpoint.durable_offset)?;
                        sync_file(data)?;
                    }
                } else {
                    receiver.recover_commit()?;
                }
                checkpoint = receiver.checkpoint.clone();
                checkpoint.last_stream = stream;
                checkpoint.updated = now.max(checkpoint.updated);
                checkpoint.expires = checkpoint.expires.max(
                    checkpoint
                        .updated
                        .checked_add(retention)
                        .ok_or_else(|| failure(CopyError::Protocol))?,
                );
                receiver.checkpoint = checkpoint;
                receiver.persist()?;
                Ok(receiver)
            }
        }
    }
    pub fn phase(&self) -> Phase {
        self.checkpoint.phase
    }
    pub fn durable_prefix(&self) -> DurablePrefix {
        DurablePrefix {
            offset: self.checkpoint.durable_offset,
            sha256: self.checkpoint.prefix_sha256,
        }
    }
    #[cfg(test)]
    pub fn accepted(&self) -> u64 {
        self.accepted
    }
    pub fn receive_data(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        if !matches!(self.checkpoint.phase, Phase::Created | Phase::Receiving)
            || bytes.is_empty()
            || bytes.len() > marmish_stream::MAX_BLOCK
            || offset != self.accepted
        {
            return Err(failure(CopyError::Protocol));
        }
        let end = offset
            .checked_add(bytes.len() as u64)
            .filter(|n| *n <= self.checkpoint.manifest.size)
            .ok_or_else(|| failure(CopyError::Protocol))?;
        if end - self.checkpoint.durable_offset > protocol::DURABLE_WINDOW {
            return Err(failure(CopyError::Quota));
        }
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| failure(CopyError::IdentityChanged))?;
        data.write_all_at(bytes, offset)?;
        self.hash.update(bytes);
        self.accepted = end;
        self.checkpoint.phase = Phase::Receiving;
        Ok(())
    }
    pub fn checkpoint(&mut self, now: i64) -> Result<DurablePrefix> {
        if !matches!(self.checkpoint.phase, Phase::Created | Phase::Receiving) {
            return Err(failure(CopyError::Protocol));
        }
        sync_file(
            self.data
                .as_ref()
                .ok_or_else(|| failure(CopyError::IdentityChanged))?,
        )?;
        let mut next = self.checkpoint.clone();
        next.durable_offset = self.accepted;
        next.prefix_sha256 = self.hash.clone().finalize().into();
        if next.durable_offset == next.manifest.size && next.prefix_sha256 != next.manifest.sha256 {
            return Err(failure(CopyError::HashMismatch));
        }
        advance_time(&mut next, now)?;
        // Update memory only after durable publication succeeds.
        self.persist_value(&next)?;
        self.checkpoint = next;
        Ok(self.durable_prefix())
    }
    /// Caller first publishes a full-length checkpoint to its peer, then calls
    /// this method. Final verification and metadata run only on the worker.
    pub fn prepare_commit(&mut self, now: i64) -> Result<()> {
        if self.checkpoint.phase == Phase::Committed {
            return self.receipt().map(|_| ());
        }
        if self.checkpoint.phase == Phase::CommitPrepared {
            return Ok(());
        }
        if self.accepted != self.checkpoint.manifest.size
            || self.checkpoint.durable_offset != self.accepted
        {
            return Err(failure(CopyError::Protocol));
        }
        self.checkpoint.phase = Phase::Verifying;
        advance_time(&mut self.checkpoint, now)?;
        self.persist()?;
        let data = self
            .data
            .as_ref()
            .ok_or_else(|| failure(CopyError::IdentityChanged))?;
        verify_content(data, self.checkpoint.data, &self.checkpoint.manifest)?;
        let mode = self.checkpoint.manifest.mode.unwrap_or(0o600);
        syscall(unsafe { libc::fchmod(data.as_raw_fd(), mode as libc::mode_t) })?;
        if let Some(mtime) = self.checkpoint.manifest.mtime {
            // All supported Linux/macOS distribution targets have 64-bit time_t.
            let seconds: libc::time_t = mtime.seconds;
            let times = [
                libc::timespec {
                    tv_sec: 0,
                    tv_nsec: libc::UTIME_OMIT,
                },
                libc::timespec {
                    tv_sec: seconds,
                    tv_nsec: mtime.nanos as _,
                },
            ];
            syscall(unsafe { libc::futimens(data.as_raw_fd(), times.as_ptr()) })?;
        }
        sync_file(data)?;
        self.checkpoint.phase = Phase::CommitPrepared;
        self.persist()
    }
    #[cfg(test)]
    pub fn commit(&mut self) -> Result<Receipt> {
        self.commit_cancellable(&std::sync::atomic::AtomicBool::new(false))?
            .ok_or_else(|| failure(CopyError::Protocol))
    }
    /// None means cancellation observed before a new rename. Once rename has
    /// succeeded, finish durability/receipt even if cancellation becomes set.
    pub fn commit_cancellable(
        &mut self,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Option<Receipt>> {
        if self.checkpoint.phase == Phase::Committed {
            return self.receipt().map(Some);
        }
        if self.checkpoint.phase != Phase::CommitPrepared {
            return Err(failure(CopyError::Protocol));
        }
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(None);
        }
        self.recover_commit()?;
        if self.checkpoint.phase == Phase::Committed {
            return self.receipt().map(Some);
        }
        let observed = optional_stat(&self.destination.parent, &self.destination.basename)?;
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(None);
        }
        #[cfg(test)]
        crate::copy_commit_hook::at(crate::copy_commit_hook::Point::BeforeRename)?;
        if self.checkpoint.manifest.overwrite {
            if observed.is_some_and(|s| s.st_mode & libc::S_IFMT != libc::S_IFREG) {
                return Err(failure(CopyError::UnsafePath));
            }
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Ok(None);
            }
            syscall(unsafe {
                libc::renameat(
                    self.private.as_raw_fd(),
                    c"data".as_ptr(),
                    self.destination.parent.as_raw_fd(),
                    self.destination.basename.as_ptr(),
                )
            })?;
        } else {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Ok(None);
            }
            rename_noreplace(
                &self.private,
                c"data",
                &self.destination.parent,
                &self.destination.basename,
            )?;
        }
        #[cfg(test)]
        crate::copy_commit_hook::at(crate::copy_commit_hook::Point::AfterRename)
            .map_err(|_| failure(CopyError::CommitUncertain))?;
        sync_directory(&self.private)?;
        sync_directory(&self.destination.parent)?;
        self.checkpoint.phase = Phase::Committed;
        self.checkpoint.committed_final = Some(self.checkpoint.data);
        self.persist()?;
        self.receipt().map(Some)
    }
    pub fn receipt(&self) -> Result<Receipt> {
        if self.checkpoint.phase != Phase::Committed {
            return Err(failure(CopyError::Protocol));
        }
        let file = self
            .destination
            .regular()
            .map_err(|_| failure(CopyError::CommitUncertain))?;
        verify_content(&file, self.checkpoint.data, &self.checkpoint.manifest)
            .map_err(|_| failure(CopyError::CommitUncertain))?;
        Ok(Receipt {
            transfer: self.checkpoint.manifest.transfer,
            size: self.checkpoint.manifest.size,
            sha256: self.checkpoint.manifest.sha256,
        })
    }
    fn recover_commit(&mut self) -> Result<()> {
        if self.checkpoint.phase == Phase::Committed {
            return self.receipt().map(|_| ());
        }
        if self.checkpoint.phase != Phase::CommitPrepared {
            return Err(failure(CopyError::Protocol));
        }
        match optional_stat(&self.private, c"data")? {
            Some(stat) => {
                if stat.st_mode & libc::S_IFMT != libc::S_IFREG
                    || stat.st_nlink != 1
                    || stat_identity(&stat) != self.checkpoint.data
                {
                    return Err(failure(CopyError::IdentityChanged));
                }
                verify_content(
                    self.data
                        .as_ref()
                        .ok_or_else(|| failure(CopyError::IdentityChanged))?,
                    self.checkpoint.data,
                    &self.checkpoint.manifest,
                )?;
                Ok(())
            }
            None => {
                let file = self
                    .destination
                    .regular()
                    .map_err(|_| failure(CopyError::CommitUncertain))?;
                verify_content(&file, self.checkpoint.data, &self.checkpoint.manifest)
                    .map_err(|_| failure(CopyError::CommitUncertain))?;
                sync_directory(&self.private)?;
                sync_directory(&self.destination.parent)?;
                self.checkpoint.phase = Phase::Committed;
                self.checkpoint.committed_final = Some(self.checkpoint.data);
                self.persist()
            }
        }
    }
    fn persist(&mut self) -> Result<()> {
        self.persist_value(&self.checkpoint)
    }
    fn persist_value(&self, value: &Checkpoint) -> Result<()> {
        let bytes = value.encode()?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| failure(CopyError::LocalIo))?;
        let temporary = name(format!("checkpoint-{}.tmp", hex::encode(random)).as_bytes())?;
        let result = (|| {
            let mut output = open_at(
                &self.private,
                &temporary,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            output.write_all(&bytes)?;
            sync_file(&output)?;
            syscall(unsafe {
                libc::renameat(
                    self.private.as_raw_fd(),
                    temporary.as_ptr(),
                    self.private.as_raw_fd(),
                    c"checkpoint".as_ptr(),
                )
            })?;
            sync_directory(&self.private)
        })();
        if result.is_err() {
            unsafe { libc::unlinkat(self.private.as_raw_fd(), temporary.as_ptr(), 0) };
        }
        result
    }
}
fn advance_time(checkpoint: &mut Checkpoint, now: i64) -> Result<()> {
    let retention = checkpoint
        .expires
        .checked_sub(checkpoint.updated)
        .ok_or_else(|| failure(CopyError::Protocol))?;
    checkpoint.updated = checkpoint.updated.max(now);
    checkpoint.expires = checkpoint
        .updated
        .checked_add(retention)
        .ok_or_else(|| failure(CopyError::Protocol))?;
    Ok(())
}
fn verify_content(file: &File, id: FileIdentity, manifest: &Manifest) -> Result<()> {
    let before = SourceStamp::read(file)?;
    if before.id != id {
        return Err(failure(CopyError::IdentityChanged));
    }
    if before.size != manifest.size || file.metadata()?.nlink() != 1 {
        return Err(failure(CopyError::HashMismatch));
    }
    if hash_prefix(file, manifest.size)?.finalize().as_slice() != manifest.sha256 {
        return Err(failure(CopyError::HashMismatch));
    }
    if SourceStamp::read(file)? != before {
        return Err(failure(CopyError::SourceChanged));
    }
    Ok(())
}
fn rename_noreplace(from: &File, source: &CStr, to: &File, destination: &CStr) -> Result<()> {
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            from.as_raw_fd(),
            source.as_ptr(),
            to.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        ) as libc::c_int
    };
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            from.as_raw_fd(),
            source.as_ptr(),
            to.as_raw_fd(),
            destination.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP)
        ) {
            return Err(failure(CopyError::Unsupported));
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Direction, Mtime, VERSION};
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    };
    fn manifest(bytes: &[u8]) -> Manifest {
        Manifest {
            version: VERSION,
            transfer: [1; 32],
            client: [2; 32],
            server: [3; 32],
            direction: Direction::Put,
            source: b"source".to_vec(),
            destination: b"destination".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
            size: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
            mode: None,
            mtime: None,
        }
    }
    fn base() -> (tempfile::TempDir, File) {
        let dir = tempfile::tempdir().unwrap();
        let base = trusted_base(dir.path()).unwrap();
        (dir, base)
    }
    fn open(base: &File, m: Manifest, mode: AttemptMode) -> Result<CopyReceiver> {
        let path = m.destination.clone();
        CopyReceiver::open_or_recover(
            base,
            &path,
            m,
            mode,
            [7; 16],
            100,
            protocol::MIN_RETENTION_SECS,
        )
    }
    fn code<T>(result: Result<T>) -> CopyError {
        match result {
            Ok(_) => panic!("unexpected copy success"),
            Err(e) => e.code,
        }
    }
    fn receive(receiver: &mut CopyReceiver, bytes: &[u8]) {
        for chunk in bytes.chunks(marmish_stream::MAX_BLOCK) {
            receiver.receive_data(receiver.accepted(), chunk).unwrap();
        }
    }
    fn finish(receiver: &mut CopyReceiver) {
        receiver.checkpoint(101).unwrap();
        receiver.prepare_commit(102).unwrap();
        receiver.commit().unwrap();
    }

    #[test]
    fn byte_preserving_commit_receipt_and_resume_never_overwrites_changed_final() {
        let (dir, base) = base();
        let bytes = b"\0\xff\n\r'$* ";
        let m = manifest(bytes);
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, bytes);
        finish(&mut receiver);
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            bytes
        );
        assert_eq!(
            std::fs::metadata(dir.path().join("destination"))
                .unwrap()
                .mode()
                & 0o7777,
            0o600
        );
        receiver.commit().unwrap();
        drop(receiver);
        let recovered = open(&base, m.clone(), AttemptMode::Resume).unwrap();
        assert_eq!(recovered.phase(), Phase::Committed);
        recovered.receipt().unwrap();
        drop(recovered);
        std::fs::write(dir.path().join("destination"), b"changed").unwrap();
        assert_eq!(
            code(open(&base, m, AttemptMode::Resume)),
            CopyError::CommitUncertain
        );
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"changed"
        );
    }
    #[test]
    fn large_files_checkpoint_resume_and_preserve_literal_paths() {
        let size = 66 * 1024 * 1024 + 17;
        for random in [false, true] {
            let (dir, base) = base();
            let mut m = manifest(b"");
            m.source = b"-source-\n'$()".to_vec();
            m.destination = b"-destination-\t*".to_vec();
            // Invalid UTF-8 names are a Linux filesystem check, not a macOS promise.
            #[cfg(target_os = "linux")]
            {
                m.source.push(0xff);
                m.destination.push(0xfe);
            }
            m.size = size;
            m.preserve_mode = true;
            m.mode = Some(0o640);
            let source_path = dir.path().join(std::ffi::OsStr::from_bytes(&m.source));
            let mut file = File::create(&source_path).unwrap();
            let mut hash = Sha256::new();
            let mut remaining = size;
            let mut state = 0x91a7_3c25_dde4_58b1_u64;
            let mut block = [0u8; marmish_stream::MAX_BLOCK];
            while remaining > 0 {
                if random {
                    for byte in &mut block {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        *byte = (state >> 32) as u8;
                    }
                }
                let length = remaining.min(block.len() as u64) as usize;
                file.write_all(&block[..length]).unwrap();
                hash.update(&block[..length]);
                remaining -= length as u64;
            }
            file.sync_all().unwrap();
            drop(file);
            m.sha256 = hash.finalize().into();
            let source = CopySource::open(PinnedPath::open(&base, &m.source).unwrap()).unwrap();
            assert_eq!(source.size(), size);
            assert_eq!(source.prefix_hash(size).unwrap(), m.sha256);
            let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
            let mut resumed = false;
            let mut checkpoints = 0;
            while receiver.accepted() < size {
                let bytes = source.read_chunk(receiver.accepted()).unwrap();
                receiver.receive_data(receiver.accepted(), &bytes).unwrap();
                if receiver.accepted() - receiver.durable_prefix().offset >= 1024 * 1024 {
                    receiver.checkpoint(101 + checkpoints).unwrap();
                    checkpoints += 1;
                    if !resumed && receiver.accepted() >= size / 2 {
                        let durable = receiver.durable_prefix();
                        let suffix = source.read_chunk(receiver.accepted()).unwrap();
                        receiver.receive_data(receiver.accepted(), &suffix).unwrap();
                        assert!(receiver.accepted() > durable.offset);
                        assert!(
                            receiver.data.as_ref().unwrap().metadata().unwrap().len()
                                > durable.offset
                        );
                        drop(receiver);
                        receiver = open(&base, m.clone(), AttemptMode::Resume).unwrap();
                        assert_eq!(receiver.durable_prefix().offset, durable.offset);
                        assert_eq!(receiver.durable_prefix().sha256, durable.sha256);
                        assert_eq!(receiver.accepted(), durable.offset);
                        assert_eq!(
                            receiver.data.as_ref().unwrap().metadata().unwrap().len(),
                            durable.offset
                        );
                        resumed = true;
                    }
                }
            }
            assert!(resumed);
            assert_eq!(checkpoints, 66);
            finish(&mut receiver);
            drop(receiver);
            let recovered = open(&base, m.clone(), AttemptMode::Resume).unwrap();
            assert_eq!(recovered.phase(), Phase::Committed);
            recovered.receipt().unwrap();
            let destination = dir.path().join(std::ffi::OsStr::from_bytes(&m.destination));
            assert_eq!(
                std::fs::metadata(&destination).unwrap().mode() & 0o7777,
                0o640
            );
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 4);
            let copied = File::open(destination).unwrap();
            assert_eq!(copied.metadata().unwrap().len(), size);
            assert_eq!(
                <[u8; 32]>::from(hash_prefix(&copied, size).unwrap().finalize()),
                m.sha256
            );
        }
    }

    #[test]
    fn preserved_modes_without_owner_read_write_refuse_before_staging() {
        let (dir, base) = base();
        for mode in [0, 0o400, 0o200, 0o7777] {
            let mut m = manifest(b"payload");
            m.preserve_mode = true;
            m.mode = Some(mode);
            assert_eq!(
                code(open(&base, m, AttemptMode::Create)),
                CopyError::Protocol
            );
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn resume_rehashes_prefix_and_truncates_uncheckpointed_suffix() {
        let (dir, base) = base();
        let m = manifest(b"abcdef");
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receiver.receive_data(0, b"abc").unwrap();
        let prefix = receiver.checkpoint(101).unwrap();
        assert_eq!(prefix.offset, 3);
        receiver.receive_data(3, b"ZZ").unwrap();
        drop(receiver);
        let mut receiver = open(&base, m, AttemptMode::Resume).unwrap();
        assert_eq!(receiver.accepted(), 3);
        assert_eq!(receiver.data.as_ref().unwrap().metadata().unwrap().len(), 3);
        assert_eq!(
            receiver.durable_prefix().sha256,
            Sha256::digest(b"abc").as_slice()
        );
        receiver.receive_data(3, b"def").unwrap();
        finish(&mut receiver);
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"abcdef"
        );
    }
    #[test]
    fn active_attempt_lock_is_busy_and_drop_preserves_partial() {
        let (dir, base) = base();
        let m = manifest(b"abc");
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        assert_eq!(
            code(open(&base, m.clone(), AttemptMode::Resume)),
            CopyError::Busy
        );
        assert_eq!(
            code(open(&base, m.clone(), AttemptMode::Create)),
            CopyError::Conflict
        );
        receiver.receive_data(0, b"a").unwrap();
        receiver.checkpoint(101).unwrap();
        drop(receiver);
        assert!(!dir.path().join("destination").exists());
        let recovered = open(&base, m, AttemptMode::Resume).unwrap();
        assert_eq!(recovered.accepted(), 1);
    }
    #[test]
    fn source_rejects_symlink_fifo_and_mutation_without_blocking() {
        let (dir, base) = base();
        std::fs::write(dir.path().join("source"), b"abc").unwrap();
        symlink("source", dir.path().join("link")).unwrap();
        assert_eq!(
            code(CopySource::open(PinnedPath::open(&base, b"link").unwrap())),
            CopyError::UnsafePath
        );
        let fifo = name(dir.path().join("fifo").as_os_str().as_bytes()).unwrap();
        syscall(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }).unwrap();
        assert_eq!(
            code(CopySource::open(PinnedPath::open(&base, b"fifo").unwrap())),
            CopyError::UnsafePath
        );
        let source = CopySource::open(PinnedPath::open(&base, b"source").unwrap()).unwrap();
        assert_eq!(
            source.prefix_hash(3).unwrap(),
            Sha256::digest(b"abc").as_slice()
        );
        std::fs::write(dir.path().join("source"), b"changed length").unwrap();
        assert_eq!(code(source.read_chunk(0)), CopyError::SourceChanged);
    }
    #[test]
    fn untrusted_components_and_empty_final_names_are_rejected() {
        let (dir, base) = base();
        std::fs::create_dir(dir.path().join("real")).unwrap();
        symlink("real", dir.path().join("alias")).unwrap();
        for operand in [
            b"../outside".as_slice(),
            b"real/../outside",
            b"./source",
            b"real/",
            b"real/.",
            b"",
            b"bad\0name",
            b"alias/file",
        ] {
            assert_eq!(
                code(PinnedPath::open(&base, operand)),
                CopyError::UnsafePath
            );
        }
        let mut m = manifest(b"abc");
        m.destination = b"-strange\xff\n' *".to_vec();
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, b"abc");
        finish(&mut receiver);
        assert_eq!(
            std::fs::read(dir.path().join(std::ffi::OsStr::from_bytes(&m.destination))).unwrap(),
            b"abc"
        );
    }
    #[test]
    fn no_clobber_conflict_retains_commit_prepared_for_explicit_retry() {
        let (dir, base) = base();
        let m = manifest(b"new");
        std::fs::write(dir.path().join("destination"), b"old").unwrap();
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, b"new");
        receiver.checkpoint(101).unwrap();
        receiver.prepare_commit(102).unwrap();
        assert_eq!(code(receiver.commit()), CopyError::Conflict);
        assert_eq!(receiver.phase(), Phase::CommitPrepared);
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"old"
        );
        drop(receiver);
        std::fs::remove_file(dir.path().join("destination")).unwrap();
        let mut receiver = open(&base, m, AttemptMode::Resume).unwrap();
        receiver.commit().unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"new"
        );
    }
    #[test]
    fn each_parent_symlink_refuses_before_creating_any_copy_state() {
        for component in 0..4 {
            let (dir, base) = base();
            let real = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(real.path().join("a/b/c")).unwrap();
            std::fs::write(real.path().join("a/b/c/file"), b"sentinel").unwrap();
            let parts = ["a", "b", "c"];
            let index = component.min(2);
            let mut at = dir.path().to_path_buf();
            for part in &parts[..index] {
                at.push(part);
                std::fs::create_dir(&at).unwrap();
            }
            let target = if component == 3 {
                real.path().join("missing")
            } else {
                real.path().join(parts[..=index].join("/"))
            };
            symlink(target, at.join(parts[index])).unwrap();
            let operand = b"a/b/c/file";
            assert_eq!(
                code(PinnedPath::open(&base, operand)),
                CopyError::UnsafePath
            );
            let mut m = manifest(b"new");
            m.destination = operand.to_vec();
            for mode in [AttemptMode::Create, AttemptMode::Resume] {
                assert_eq!(code(open(&base, m.clone(), mode)), CopyError::UnsafePath);
            }
            assert_eq!(
                std::fs::read(real.path().join("a/b/c/file")).unwrap(),
                b"sentinel"
            );
            for root in [dir.path(), real.path()] {
                for relative in ["", "a", "a/b", "a/b/c"] {
                    let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
                        continue;
                    };
                    for entry in entries {
                        assert!(
                            !entry
                                .unwrap()
                                .file_name()
                                .as_bytes()
                                .starts_with(b".marmish-copy")
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn missing_parent_and_missing_resume_create_no_state() {
        let (dir, base) = base();
        for parent_missing in [true, false] {
            let mut m = manifest(b"abc");
            if parent_missing {
                m.destination = b"missing/destination".to_vec();
            }
            let modes: &[AttemptMode] = if parent_missing {
                &[AttemptMode::Create, AttemptMode::Resume]
            } else {
                &[AttemptMode::Resume]
            };
            for mode in modes {
                assert_eq!(code(open(&base, m.clone(), *mode)), CopyError::NotFound);
                // Resume takes the shared admission lock before discovering absence.
                for entry in std::fs::read_dir(dir.path()).unwrap() {
                    assert_eq!(entry.unwrap().file_name(), ".marmish-copy-admission.lock");
                }
            }
        }
    }

    #[test]
    fn read_only_parent_refuses_admission_and_namespace_creation() {
        struct Restore(std::path::PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
            }
        }
        for existing_lock in [false, true] {
            let (dir, base) = base();
            if existing_lock {
                let mut old = manifest(b"old");
                old.transfer = [9; 32];
                drop(open(&base, old, AttemptMode::Create).unwrap());
            }
            let _restore = Restore(dir.path().to_path_buf());
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
            let probe = dir.path().join("permission-probe");
            match File::create(&probe) {
                Ok(file) => {
                    drop(file);
                    std::fs::remove_file(&probe).unwrap();
                    assert_ne!(
                        std::env::var("MARMISH_REQUIRE_NONROOT_PERMISSION_TESTS").as_deref(),
                        Ok("1"),
                        "qualification requires real directory permission enforcement"
                    );
                    eprintln!("read-only parent check skipped: directory permissions bypassed");
                    continue;
                }
                Err(error) => assert_eq!(error.kind(), io::ErrorKind::PermissionDenied),
            }
            let m = manifest(b"new");
            assert_eq!(
                code(open(&base, m.clone(), AttemptMode::Create)),
                CopyError::PermissionDenied
            );
            assert!(!dir.path().join(m.namespace()).exists());
            if !existing_lock {
                assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
            }
        }
    }

    #[test]
    fn destination_appearing_after_prepare_is_not_adopted_by_content() {
        let (dir, base) = base();
        let m = manifest(b"new");
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, b"new");
        receiver.checkpoint(101).unwrap();
        receiver.prepare_commit(102).unwrap();
        std::fs::write(dir.path().join("destination"), b"new").unwrap();
        for _ in 0..2 {
            assert_eq!(code(receiver.commit()), CopyError::Conflict);
            assert_eq!(receiver.phase(), Phase::CommitPrepared);
            assert_eq!(
                std::fs::read(dir.path().join("destination")).unwrap(),
                b"new"
            );
            assert_eq!(
                std::fs::metadata(dir.path().join(m.namespace()).join("data"))
                    .unwrap()
                    .nlink(),
                1
            );
        }
        drop(receiver);
        std::fs::remove_file(dir.path().join("destination")).unwrap();
        let mut resumed = open(&base, m.clone(), AttemptMode::Resume).unwrap();
        resumed.commit().unwrap();
        drop(resumed);
        let committed = open(&base, m, AttemptMode::Resume).unwrap();
        assert_eq!(committed.phase(), Phase::Committed);
        committed.receipt().unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn duplicate_create_and_changed_resume_bindings_preserve_original_prefix() {
        let (dir, base) = base();
        let m = manifest(b"abcdef");
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, b"abc");
        receiver.checkpoint(101).unwrap();
        assert_eq!(
            code(open(&base, m.clone(), AttemptMode::Create)),
            CopyError::Conflict
        );
        drop(receiver);
        assert_eq!(
            code(open(&base, m.clone(), AttemptMode::Create)),
            CopyError::Conflict
        );
        for variant in 0..9 {
            let mut changed = m.clone();
            match variant {
                0 => changed.client[0] ^= 1,
                1 => changed.server[0] ^= 1,
                2 => changed.transfer[0] ^= 1,
                3 => changed.direction = Direction::Get,
                4 => changed.sha256[0] ^= 1,
                5 => changed.size += 1,
                6 => {
                    changed.preserve_mode = true;
                    changed.mode = Some(0o640);
                }
                7 => changed.overwrite = true,
                _ => changed.destination = b"other".to_vec(),
            }
            changed.validate().unwrap();
            let expected = if variant < 4 {
                CopyError::NotFound
            } else {
                CopyError::Conflict
            };
            assert_eq!(code(open(&base, changed, AttemptMode::Resume)), expected);
            assert_eq!(
                std::fs::read(dir.path().join(m.namespace()).join("data")).unwrap(),
                b"abc"
            );
        }
        let mut original = open(&base, m, AttemptMode::Resume).unwrap();
        assert_eq!(original.accepted(), 3);
        assert_eq!(
            original.checkpoint.prefix_sha256,
            Sha256::digest(b"abc").as_slice()
        );
        receive(&mut original, b"def");
        finish(&mut original);
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"abcdef"
        );
    }

    #[test]
    fn explicit_overwrite_replaces_entry_and_preserves_other_hardlink() {
        let (dir, base) = base();
        std::fs::write(dir.path().join("destination"), b"old").unwrap();
        std::fs::hard_link(dir.path().join("destination"), dir.path().join("other")).unwrap();
        let mut m = manifest(b"new");
        m.overwrite = true;
        let mut receiver = open(&base, m, AttemptMode::Create).unwrap();
        receive(&mut receiver, b"new");
        finish(&mut receiver);
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"new"
        );
        assert_eq!(std::fs::read(dir.path().join("other")).unwrap(), b"old");
    }
    #[test]
    fn overwrite_does_not_follow_observed_symlink() {
        let (dir, base) = base();
        std::fs::write(dir.path().join("referent"), b"old").unwrap();
        symlink("referent", dir.path().join("destination")).unwrap();
        let mut m = manifest(b"new");
        m.overwrite = true;
        let mut receiver = open(&base, m, AttemptMode::Create).unwrap();
        receive(&mut receiver, b"new");
        receiver.checkpoint(101).unwrap();
        receiver.prepare_commit(102).unwrap();
        assert_eq!(code(receiver.commit()), CopyError::UnsafePath);
        assert_eq!(std::fs::read(dir.path().join("referent")).unwrap(), b"old");
    }
    #[test]
    fn rename_completed_before_receipt_is_recognized_by_identity_and_hash() {
        let (dir, base) = base();
        let m = manifest(b"abc");
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, b"abc");
        receiver.checkpoint(101).unwrap();
        receiver.prepare_commit(102).unwrap();
        rename_noreplace(
            &receiver.private,
            c"data",
            &receiver.destination.parent,
            &receiver.destination.basename,
        )
        .unwrap();
        drop(receiver);
        let receiver = open(&base, m, AttemptMode::Resume).unwrap();
        assert_eq!(receiver.phase(), Phase::Committed);
        receiver.receipt().unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"abc"
        );
    }
    #[test]
    fn missing_stage_after_commit_prepare_cannot_adopt_same_content_other_inode() {
        let (dir, base) = base();
        let m = manifest(b"abc");
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, b"abc");
        receiver.checkpoint(101).unwrap();
        receiver.prepare_commit(102).unwrap();
        syscall(unsafe { libc::unlinkat(receiver.private.as_raw_fd(), c"data".as_ptr(), 0) })
            .unwrap();
        std::fs::write(dir.path().join("destination"), b"abc").unwrap();
        drop(receiver);
        assert_eq!(
            code(open(&base, m, AttemptMode::Resume)),
            CopyError::CommitUncertain
        );
    }
    #[test]
    fn corrupt_prefix_short_data_hardlinks_and_parent_identity_fail_closed() {
        for scenario in 0..4 {
            let (dir, base) = base();
            let m = manifest(b"abcdef");
            let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
            receive(&mut receiver, b"abc");
            receiver.checkpoint(101).unwrap();
            match scenario {
                0 => receiver
                    .data
                    .as_ref()
                    .unwrap()
                    .write_all_at(b"X", 0)
                    .unwrap(),
                1 => receiver.data.as_ref().unwrap().set_len(1).unwrap(),
                2 => {
                    let private = dir.path().join(m.namespace());
                    std::fs::hard_link(private.join("data"), private.join("extra")).unwrap();
                }
                _ => {
                    receiver.checkpoint.parent.inode ^= 1;
                    receiver.persist().unwrap();
                }
            }
            drop(receiver);
            assert_eq!(
                code(open(&base, m, AttemptMode::Resume)),
                match scenario {
                    2 => CopyError::UnsafePath,
                    3 => CopyError::IdentityChanged,
                    _ => CopyError::HashMismatch,
                }
            );
        }
    }
    #[test]
    fn principal_path_and_manifest_mismatches_never_adopt_existing_state() {
        let (_dir, base) = base();
        let m = manifest(b"abc");
        let receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        drop(receiver);
        let mut changed = m.clone();
        changed.client[0] ^= 1;
        assert_eq!(
            code(open(&base, changed, AttemptMode::Resume)),
            CopyError::NotFound
        );
        let mut changed = m.clone();
        changed.sha256 = [0; 32];
        assert_eq!(
            code(open(&base, changed, AttemptMode::Resume)),
            CopyError::Conflict
        );
        assert_eq!(
            code(CopyReceiver::open_or_recover(
                &base,
                b"other",
                m,
                AttemptMode::Resume,
                [0; 16],
                100,
                protocol::MIN_RETENTION_SECS
            )),
            CopyError::Conflict
        );
    }
    #[test]
    fn incomplete_tree_and_unsafe_admission_collision_are_not_adopted() {
        let (dir, base) = base();
        let m = manifest(b"abc");
        std::fs::create_dir(dir.path().join(m.namespace())).unwrap();
        std::fs::set_permissions(
            dir.path().join(m.namespace()),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert_eq!(
            code(open(&base, m.clone(), AttemptMode::Resume)),
            CopyError::Busy
        );
        std::fs::remove_file(dir.path().join(".marmish-copy-admission.lock")).unwrap();
        symlink(
            "destination",
            dir.path().join(".marmish-copy-admission.lock"),
        )
        .unwrap();
        assert_eq!(
            code(open(&base, m, AttemptMode::Create)),
            CopyError::UnsafePath
        );
    }
    #[test]
    fn metadata_and_crash_during_verifying_recover_through_owned_fd() {
        let (dir, base) = base();
        let mut m = manifest(b"abc");
        m.preserve_mode = true;
        m.mode = Some(0o640);
        m.preserve_mtime = true;
        m.mtime = Some(Mtime {
            seconds: 1_000_000,
            nanos: 123,
        });
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, b"abc");
        receiver.checkpoint(101).unwrap();
        receiver.checkpoint.phase = Phase::Verifying;
        receiver.persist().unwrap();
        syscall(unsafe { libc::fchmod(receiver.data.as_ref().unwrap().as_raw_fd(), 0o640) })
            .unwrap();
        drop(receiver);
        let mut receiver = open(&base, m, AttemptMode::Resume).unwrap();
        receiver.prepare_commit(102).unwrap();
        receiver.commit().unwrap();
        let meta = std::fs::metadata(dir.path().join("destination")).unwrap();
        assert_eq!(meta.mode() & 0o7777, 0o640);
        assert_eq!(meta.mtime(), 1_000_000);
        assert_eq!(meta.mtime_nsec(), 123);
    }
    #[test]
    fn durable_credit_is_distinct_from_transport_block_capacity() {
        let (_dir, base) = base();
        let data = vec![0x57; protocol::DURABLE_WINDOW as usize + 1];
        let m = manifest(&data);
        let mut receiver = open(&base, m, AttemptMode::Create).unwrap();
        receive(&mut receiver, &data[..protocol::DURABLE_WINDOW as usize]);
        assert_eq!(
            code(receiver.receive_data(receiver.accepted(), &[0x57])),
            CopyError::Quota
        );
        receiver.checkpoint(101).unwrap();
        receiver.receive_data(receiver.accepted(), &[0x57]).unwrap();
        finish(&mut receiver);
    }
    #[test]
    fn empty_file_commits_and_retention_has_an_inclusive_lower_bound() {
        let (_dir, base) = base();
        let m = manifest(b"");
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        finish(&mut receiver);
        assert_eq!(receiver.receipt().unwrap().size, 0);
        drop(receiver);
        let mut changed = m;
        changed.transfer[0] ^= 1;
        assert_eq!(
            code(CopyReceiver::open_or_recover(
                &base,
                b"destination",
                changed,
                AttemptMode::Create,
                [0; 16],
                100,
                protocol::MIN_RETENTION_SECS - 1
            )),
            CopyError::Protocol
        );
    }
    #[test]
    fn resume_with_lower_policy_never_shortens_existing_receipt() {
        let (_dir, base) = base();
        let m = manifest(b"");
        let longer = protocol::MIN_RETENTION_SECS + 500;
        let first = CopyReceiver::open_or_recover(
            &base,
            b"destination",
            m.clone(),
            AttemptMode::Create,
            [0; 16],
            100,
            longer,
        )
        .unwrap();
        let expiry = first.checkpoint.expires;
        drop(first);
        let resumed = CopyReceiver::open_or_recover(
            &base,
            b"destination",
            m,
            AttemptMode::Resume,
            [1; 16],
            101,
            protocol::MIN_RETENTION_SECS,
        )
        .unwrap();
        assert_eq!(resumed.checkpoint.expires, expiry);
    }
    #[test]
    fn cancellation_before_commit_keeps_stage_and_never_replaces_destination() {
        let (dir, base) = base();
        let mut m = manifest(b"new");
        m.overwrite = true;
        std::fs::write(dir.path().join("destination"), b"old").unwrap();
        let mut receiver = open(&base, m.clone(), AttemptMode::Create).unwrap();
        receive(&mut receiver, b"new");
        receiver.checkpoint(101).unwrap();
        receiver.prepare_commit(102).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(true);
        assert!(receiver.commit_cancellable(&cancel).unwrap().is_none());
        assert_eq!(receiver.phase(), Phase::CommitPrepared);
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"old"
        );
        drop(receiver);
        let mut receiver = open(&base, m, AttemptMode::Resume).unwrap();
        receiver.commit().unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"new"
        );
        assert!(receiver.commit_cancellable(&cancel).unwrap().is_some());
    }
    #[test]
    fn genuine_admission_contention_and_explicit_unlock_with_cloned_descriptor() {
        let (_dir, base) = base();
        let held = admission_lock(&base).unwrap();
        assert_eq!(code(admission_lock(&base)), CopyError::Busy);
        let inherited = held.0.try_clone().unwrap();
        drop(held);
        let acquired = admission_lock(&base).unwrap();
        drop(acquired);
        drop(inherited);
    }
    #[test]
    fn receiver_stays_on_pinned_directory_when_parent_path_is_replaced() {
        let (dir, base) = base();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let mut m = manifest(b"abc");
        m.destination = b"sub/destination".to_vec();
        let mut receiver = open(&base, m, AttemptMode::Create).unwrap();
        std::fs::rename(dir.path().join("sub"), dir.path().join("moved")).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        receive(&mut receiver, b"abc");
        finish(&mut receiver);
        assert_eq!(
            std::fs::read(dir.path().join("moved/destination")).unwrap(),
            b"abc"
        );
        assert!(!dir.path().join("sub/destination").exists());
    }
    #[test]
    fn quota_rejects_new_state_without_unbounded_or_implicit_cleanup() {
        let (dir, base) = base();
        for n in 0..1024 {
            std::fs::create_dir(dir.path().join(format!(".marmish-copy-v1-{n}"))).unwrap();
        }
        assert_eq!(
            code(open(&base, manifest(b"abc"), AttemptMode::Create)),
            CopyError::Quota
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1025); // quota trees plus persistent admission lock
    }
    #[test]
    fn only_macos_exact_public_alias_prefixes_are_normalized() {
        assert_eq!(normalize(b"relative/file"), b"relative/file");
        assert_eq!(normalize(b"/tmp-other/file"), b"/tmp-other/file");
        #[cfg(target_os = "macos")]
        for alias in [b"/tmp/file".as_slice(), b"/var/file", b"/etc/file"] {
            let mut expected = b"/private".to_vec();
            expected.extend(alias);
            assert_eq!(normalize(alias), expected);
        }
        #[cfg(target_os = "linux")]
        assert_eq!(normalize(b"/tmp/file"), b"/tmp/file");
    }
    fn launch_for(manifest: &Manifest) -> protocol::Launch {
        protocol::Launch {
            version: 1,
            mode: AttemptMode::Create,
            transfer: manifest.transfer,
            client: manifest.client,
            server: manifest.server,
            direction: manifest.direction,
            source: manifest.source.clone(),
            destination: manifest.destination.clone(),
            overwrite: manifest.overwrite,
            preserve_mode: manifest.preserve_mode,
            preserve_mtime: manifest.preserve_mtime,
        }
    }
    #[test]
    fn authorized_gc_keeps_live_unexpired_and_prepared_receipts() {
        let (_dir, base) = base();
        let m = manifest(b"hello");
        let launch = launch_for(&m);
        let mut receiver = open(&base, m, AttemptMode::Create).unwrap();
        assert_eq!(
            code(reclaim_authorized(&base, &launch, [7; 16], 1_000_000)),
            CopyError::Busy
        );
        receive(&mut receiver, b"hello");
        receiver.checkpoint(101).unwrap();
        drop(receiver);
        assert!(!reclaim_authorized(&base, &launch, [7; 16], 102).unwrap());
        let mut forged = launch.clone();
        forged.source = b"another-source".to_vec();
        assert_eq!(
            code(reclaim_authorized(&base, &forged, [8; 16], 1_000_000)),
            CopyError::IdentityChanged
        );
        let mut receiver = open(&base, manifest(b"hello"), AttemptMode::Resume).unwrap();
        receiver.prepare_commit(102).unwrap();
        drop(receiver);
        assert_eq!(
            code(reclaim_authorized(&base, &launch, [7; 16], 1_000_000)),
            CopyError::CommitUncertain
        );
    }
    #[test]
    fn authorized_gc_removes_private_receipt_without_touching_committed_file() {
        let (dir, base) = base();
        let m = manifest(b"hello");
        let launch = launch_for(&m);
        let mut receiver = open(&base, m, AttemptMode::Create).unwrap();
        receive(&mut receiver, b"hello");
        finish(&mut receiver);
        drop(receiver);
        let namespace = dir.path().join(launch.namespace());
        std::fs::write(namespace.join("unknown"), b"retain").unwrap();
        assert_eq!(
            code(reclaim_authorized(&base, &launch, [7; 16], 1_000_000)),
            CopyError::Conflict
        );
        assert!(namespace.join("checkpoint").exists());
        std::fs::remove_file(namespace.join("unknown")).unwrap();
        assert!(reclaim_authorized(&base, &launch, [7; 16], 1_000_000).unwrap());
        assert!(!namespace.exists());
        assert_eq!(
            std::fs::read(dir.path().join("destination")).unwrap(),
            b"hello"
        );
        assert!(!reclaim_authorized(&base, &launch, [7; 16], 1_000_000).unwrap());
    }
    #[test]
    fn authorized_gc_accepts_an_older_checkpoint_after_unopened_resume() {
        let (dir, base) = base();
        let m = manifest(b"hello");
        let launch = launch_for(&m);
        let mut receiver = open(&base, m, AttemptMode::Create).unwrap();
        receive(&mut receiver, b"hello");
        receiver.checkpoint(101).unwrap();
        drop(receiver);
        // Current eligible index may be Resume B while the checkpoint is still A.
        assert!(reclaim_authorized(&base, &launch, [8; 16], 1_000_000).unwrap());
        assert!(!dir.path().join(launch.namespace()).exists());
    }
    #[test]
    fn authorized_gc_cannot_prove_absence_under_replaced_parent_or_cwd() {
        let (dir, base) = base();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let mut m = manifest(b"hello");
        m.destination = b"sub/destination".to_vec();
        let launch = launch_for(&m);
        let receiver = open(&base, m, AttemptMode::Create).unwrap();
        drop(receiver);
        std::fs::rename(dir.path().join("sub"), dir.path().join("moved")).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(!reclaim_authorized(&base, &launch, [7; 16], 1_000_000).unwrap());
        let original = dir.path().join("moved").join(launch.namespace());
        assert!(original.join("checkpoint").exists());
        // A same-named moved namespace under a new parent fails inode binding.
        std::fs::rename(&original, dir.path().join("sub").join(launch.namespace())).unwrap();
        assert_eq!(
            code(reclaim_authorized(&base, &launch, [7; 16], 1_000_000)),
            CopyError::IdentityChanged
        );
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join("sub")).unwrap();
        let otherbase = trusted_base(other.path()).unwrap();
        assert!(!reclaim_authorized(&otherbase, &launch, [7; 16], 1_000_000).unwrap());
    }
}

fn validate_reclaim_entries(private: &File) -> Result<()> {
    let fd = unsafe { libc::fcntl(private.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let dir = unsafe { libc::fdopendir(fd) };
    if dir.is_null() {
        unsafe { libc::close(fd) };
        return Err(io::Error::last_os_error().into());
    }
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe { libc::closedir(self.0) };
        }
    }
    let directory = Directory(dir);
    unsafe { libc::rewinddir(directory.0) };
    let mut count = 0;
    loop {
        set_errno_zero();
        let entry = unsafe { libc::readdir(directory.0) };
        if entry.is_null() {
            let e = io::Error::last_os_error();
            return if e.raw_os_error() == Some(0) {
                Ok(())
            } else {
                Err(e.into())
            };
        }
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        count += 1;
        if count > 3 || ![b"lock".as_slice(), b"checkpoint", b"data"].contains(&bytes) {
            return Err(failure(CopyError::Conflict));
        }
    }
}

/// Cleanup authority comes from the caller's durable inactive attempt index,
/// never merely from age or acquiring a flock. Only known private entries are
/// removed after immutable account/path/transfer and descriptor identities match.
/// The caller's durable GC claim excludes new attempts; an older checkpoint's
/// last_stream is valid when a later confirmed attempt never opened the receiver.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReclaimOutcome {
    Cleaned,
    NotExpired(i64),
    UnanchoredAbsent,
}

pub(crate) fn reclaim_authorized(
    base: &File,
    launch: &protocol::Launch,
    _stream: [u8; 16],
    timestamp: i64,
) -> Result<bool> {
    if launch.direction != protocol::Direction::Put {
        return Err(failure(CopyError::Protocol));
    }
    Ok(matches!(
        reclaim_checked(base, launch, None, timestamp)?,
        ReclaimOutcome::Cleaned
    ))
}
/// Local Get collection requires the actor's confirmed physical location. In
/// particular absence is proof only after both original base and parent match.
pub(crate) fn reclaim_local_get_authorized(
    base: &File,
    expected_base: FileIdentity,
    expected_parent: FileIdentity,
    launch: &protocol::Launch,
    physical_stream: [u8; 16],
    timestamp: i64,
) -> Result<ReclaimOutcome> {
    if launch.direction != protocol::Direction::Get {
        return Err(failure(CopyError::Protocol));
    }
    reclaim_checked(
        base,
        launch,
        Some((expected_base, expected_parent, physical_stream)),
        timestamp,
    )
}
fn reclaim_checked(
    base: &File,
    launch: &protocol::Launch,
    anchor: Option<(FileIdentity, FileIdentity, [u8; 16])>,
    timestamp: i64,
) -> Result<ReclaimOutcome> {
    launch.validate()?;
    if let Some((expected_base, _, _)) = anchor
        && identity(&base.metadata()?) != expected_base
    {
        return Err(failure(CopyError::IdentityChanged));
    }
    let destination = PinnedPath::open(base, &launch.destination)?;
    if let Some((_, expected_parent, _)) = anchor
        && destination.parent_identity()? != expected_parent
    {
        return Err(failure(CopyError::IdentityChanged));
    }
    let _admission = admission_lock(&destination.parent)?;
    let namespace = name(launch.namespace().as_bytes())?;
    if optional_stat(&destination.parent, &namespace)?.is_none() {
        // A freshly resolved path may name a replaced parent or changed cwd.
        // Absence proves nothing about the originally owned receiver tree.
        return Ok(if anchor.is_some() {
            ReclaimOutcome::Cleaned
        } else {
            ReclaimOutcome::UnanchoredAbsent
        });
    }
    let private = open_at(
        &destination.parent,
        &namespace,
        libc::O_RDONLY | libc::O_DIRECTORY,
        0,
    )?;
    let private_id = identity(&private_meta(&private, true)?);
    let lock = HeldLock::acquire(open_existing_private(
        &private,
        c"lock",
        libc::O_RDWR | libc::O_NONBLOCK | libc::O_NOCTTY,
        Some(0o600),
    )?)?;
    let file = open_existing_private(
        &private,
        c"checkpoint",
        libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOCTTY,
        Some(0o600),
    )?;
    let mut bytes = Vec::new();
    file.take((protocol::MAX_CONTROL + 1) as u64)
        .read_to_end(&mut bytes)?;
    let checkpoint = Checkpoint::decode(&bytes)?;
    if !launch.matches_manifest(&checkpoint.manifest)
        || checkpoint.normalized_path != destination.normalized
        || checkpoint.parent != destination.parent_identity()?
        || checkpoint.private != private_id
    {
        return Err(failure(CopyError::IdentityChanged));
    }
    if let Some((_, _, stream)) = anchor
        && checkpoint.last_stream != stream
    {
        return Err(failure(CopyError::IdentityChanged));
    }
    if timestamp <= checkpoint.expires {
        return Ok(ReclaimOutcome::NotExpired(checkpoint.expires));
    }
    // Never guess whether a rename committed. An unresolved prepared commit is
    // retained for explicit recovery even after its nominal retention deadline.
    if checkpoint.phase == Phase::CommitPrepared {
        return Err(failure(CopyError::CommitUncertain));
    }
    if let Some(stat) = optional_stat(&private, c"data")? {
        let data = open_existing_private(
            &private,
            c"data",
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOCTTY,
            None,
        )?;
        if identity(&data.metadata()?) != checkpoint.data || stat_identity(&stat) != checkpoint.data
        {
            return Err(failure(CopyError::IdentityChanged));
        }
    } else if checkpoint.phase != Phase::Committed {
        return Err(failure(CopyError::IdentityChanged));
    }
    // A replaced directory entry is never traversed or unlinked by authority
    // granted to its earlier inode. Unknown extra entries make rmdir fail safely.
    if stat_identity(&entry_stat(&destination.parent, &namespace)?) != private_id {
        return Err(failure(CopyError::IdentityChanged));
    }
    validate_reclaim_entries(&private)?;
    for entry in [c"data", c"checkpoint", c"lock"] {
        if optional_stat(&private, entry)?.is_some() {
            syscall(unsafe { libc::unlinkat(private.as_raw_fd(), entry.as_ptr(), 0) })?;
        }
    }
    sync_directory(&private)?;
    // Keep the flock until after removal; retained open descriptors do not
    // authorize a new admission, which remains fenced in the actor throughout.
    syscall(unsafe {
        libc::unlinkat(
            destination.parent.as_raw_fd(),
            namespace.as_ptr(),
            libc::AT_REMOVEDIR,
        )
    })?;
    sync_directory(&destination.parent)?;
    drop(lock);
    Ok(ReclaimOutcome::Cleaned)
}
