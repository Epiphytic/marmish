//! Unix same-user launch and directory-relative private storage primitives.
use crate::cipher::{StorageKeys, KEY_MAGIC};
use crate::model::*;
use crate::platform;
use crate::store::{AccountSpec, OrphanPage, ProcessIdentity, WorkerCredential};
use std::collections::BTreeSet;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{File, Metadata};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use zeroize::Zeroizing;

#[derive(Debug)]
pub(crate) struct StateRoot {
    directory: File,
    jobs: File,
    path: PathBuf,
    uid: u32,
}
#[derive(Debug)]
pub(crate) struct FrontendGuard {
    _file: File,
    root: (u64, u64),
}
#[derive(Debug)]
pub(crate) struct JobGuard {
    _file: File,
}
#[derive(Debug)]
pub(crate) struct IoGuard {
    _file: File,
    root: (u64, u64),
    job: JobId,
}
#[derive(Debug)]
pub(crate) struct LaunchSpec {
    pub root: PathBuf,
    pub job: JobId,
}
#[derive(Debug)]
pub(crate) enum WorkerPresence {
    Initializing,
    Live(ProcessIdentity),
    Absent,
}
#[derive(Debug)]
pub(crate) struct ChildSpec {
    pub account: AccountSpec,
    pub request: CommandRequest,
    pub environment: Vec<(OsString, OsString)>,
    pub cwd: PathBuf,
    pub umask: u32,
    pub canonical_exec_required: bool,
}
#[derive(Debug)]
pub(crate) struct SpawnedChild {
    pub child: Child,
    pub identity: ProcessIdentity,
    pub pgid: i32,
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
    pub stderr: ChildStderr,
}
#[derive(Debug)]
pub(crate) enum ChildSpawnError {
    NotStarted,
    Uncertain,
}

pub(super) fn io_error(error: std::io::Error) -> EngineError {
    match error.raw_os_error() {
        Some(libc::EAGAIN) | Some(libc::EACCES) => EngineError::Busy,
        Some(libc::ELOOP) | Some(libc::ENOTDIR) => EngineError::UnsafePath,
        _ => EngineError::StorageFailure,
    }
}

fn uid() -> Result<u32> {
    let real = unsafe { libc::getuid() };
    let effective = unsafe { libc::geteuid() };
    if effective == 0 || real == 0 {
        return Err(EngineError::RootForbidden);
    }
    if real != effective {
        return Err(EngineError::Unauthorized);
    }
    Ok(effective)
}

pub(crate) fn ensure_uid(expected: u32) -> Result<()> {
    if uid()? != expected {
        return Err(EngineError::Unauthorized);
    }
    Ok(())
}
fn exists_at(parent: &File, name: &str) -> Result<bool> {
    let name = CString::new(name).map_err(|_| EngineError::UnsafePath)?;
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(true);
    }
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
        Ok(false)
    } else {
        Err(EngineError::StorageFailure)
    }
}

fn openat(parent: &File, name: &OsStr, flags: i32, mode: u32) -> Result<File> {
    let name = CString::new(name.as_bytes()).map_err(|_| EngineError::UnsafePath)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn mkdirat(parent: &File, name: &OsStr) -> Result<()> {
    let name = CString::new(name.as_bytes()).map_err(|_| EngineError::UnsafePath)?;
    let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
    if result == -1 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
        return Err(EngineError::StorageFailure);
    }
    Ok(())
}
fn private(meta: &Metadata, owner: u32, directory: bool) -> Result<()> {
    if meta.uid() != owner
        || meta.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
        || if directory {
            !meta.is_dir()
        } else {
            !meta.is_file()
        }
    {
        return Err(EngineError::UnsafePath);
    }
    Ok(())
}
fn ancestor(meta: &Metadata, owner: u32) -> Result<()> {
    #[allow(clippy::useless_conversion)] // Darwin mode_t is u16, Linux u32.
    let sticky_system = meta.uid() == 0 && meta.mode() & u32::from(libc::S_ISVTX) != 0;
    if !meta.is_dir()
        || ![0, owner].contains(&meta.uid())
        || (meta.mode() & 0o022 != 0 && !sticky_system)
    {
        return Err(EngineError::UnsafePath);
    }
    Ok(())
}
fn directory_path(path: &Path, owner: u32, create: bool) -> Result<File> {
    if !path.is_absolute() {
        return Err(EngineError::UnsafePath);
    }
    let components: Vec<_> = path.components().collect();
    if components.len() < 2 {
        return Err(EngineError::UnsafePath);
    }
    let mut directory = File::open("/").map_err(io_error)?;
    for (index, component) in components.iter().enumerate().skip(1) {
        let Component::Normal(name) = component else {
            return Err(EngineError::UnsafePath);
        };
        let final_component = index + 1 == components.len();
        if final_component && create {
            mkdirat(&directory, name)?;
        }
        directory = openat(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
        let meta = directory.metadata().map_err(io_error)?;
        if final_component {
            private(&meta, owner, true)?;
        } else {
            ancestor(&meta, owner)?;
        }
    }
    Ok(directory)
}

pub(crate) fn job_hex(job: JobId) -> String {
    job.0.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(crate) fn parse_job(value: &OsStr) -> Result<JobId> {
    let bytes = value.as_bytes();
    if bytes.len() != 32
        || !bytes
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err(EngineError::InvalidRequest);
    }
    let mut id = [0u8; 16];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        let digit = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
        id[index] = digit(pair[0]) * 16 + digit(pair[1]);
    }
    Ok(JobId(id))
}

impl StateRoot {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_inner(path, true)
    }
    pub fn open_existing(path: &Path) -> Result<Self> {
        Self::open_inner(path, false)
    }
    fn open_inner(path: &Path, create: bool) -> Result<Self> {
        let uid = uid()?;
        let path = platform::state_path(path)?;
        let directory = directory_path(&path, uid, create)?;
        platform::validate_storage(&directory)?;
        if create {
            mkdirat(&directory, OsStr::new("jobs"))?;
        }
        let jobs = openat(
            &directory,
            OsStr::new("jobs"),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        private(&jobs.metadata().map_err(io_error)?, uid, true)?;
        Ok(Self {
            directory,
            jobs,
            path,
            uid,
        })
    }
    pub fn try_clone(&self) -> Result<Self> {
        Ok(Self {
            directory: self.directory.try_clone().map_err(io_error)?,
            jobs: self.jobs.try_clone().map_err(io_error)?,
            path: self.path.clone(),
            uid: self.uid,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn protected_key_file(&self, name: &str) -> Result<File> {
        let file = openat(
            &self.directory,
            OsStr::new(name),
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )?;
        let meta = file.metadata().map_err(io_error)?;
        private(&meta, self.uid, false)?;
        if meta.nlink() != 1 {
            return Err(EngineError::UnsafePath);
        }
        Ok(file)
    }
    pub fn load_storage_keys(&self) -> Result<StorageKeys> {
        let mut file = self.protected_key_file("engine.key")?;
        if file.metadata().map_err(io_error)?.len() != 40 {
            return Err(EngineError::StorageFailure);
        }
        let mut bytes = Zeroizing::new([0u8; 40]);
        file.read_exact(bytes.as_mut()).map_err(io_error)?;
        if &bytes[..8] != KEY_MAGIC {
            return Err(EngineError::StorageFailure);
        }
        let mut master = Zeroizing::new([0u8; 32]);
        master.copy_from_slice(&bytes[8..]);
        Ok(StorageKeys(master))
    }
    pub fn storage_is_pristine(&self) -> Result<bool> {
        Ok(platform::DirectoryEntries::open(&self.jobs)?
            .next()
            .transpose()?
            .is_none())
    }
    fn storage_candidate(&self) -> Result<bool> {
        // Bootstrap only, before this process opens any SQLite connection.
        self.verify_sqlite_files(&self.directory, "engine.sqlite3", false, true)?;
        let jobs_empty = self.storage_is_pristine()?;
        for entry in platform::DirectoryEntries::open(&self.jobs)? {
            let entry = entry?;
            let job = parse_job(&entry).map_err(|_| EngineError::UnsafePath)?;
            let dir = self.job_dir(job)?;
            if exists_at(&dir, "stdout")? || exists_at(&dir, "stderr")? {
                return Err(EngineError::StorageFailure);
            }
        }
        let nonempty = if exists_at(&self.directory, "engine.sqlite3")? {
            let mut file = openat(
                &self.directory,
                OsStr::new("engine.sqlite3"),
                libc::O_RDONLY | libc::O_NONBLOCK,
                0,
            )?;
            let len = file.metadata().map_err(io_error)?.len();
            let mut header = [0u8; 16];
            if len >= 16 {
                file.read_exact(&mut header).map_err(io_error)?;
                if &header == b"SQLite format 3\0" {
                    return Err(EngineError::StorageFailure);
                }
            }
            len > 0
        } else {
            false
        };
        if !nonempty
            && (!jobs_empty
                || [
                    "engine.sqlite3-wal",
                    "engine.sqlite3-shm",
                    "engine.sqlite3-journal",
                ]
                .iter()
                .any(|name| exists_at(&self.directory, name).unwrap_or(true)))
        {
            return Err(EngineError::StorageFailure);
        }
        Ok(nonempty)
    }
    pub fn initialize_storage(&self, guard: &FrontendGuard) -> Result<StorageKeys> {
        let meta = self.directory.metadata().map_err(io_error)?;
        if guard.root != (meta.dev(), meta.ino()) {
            return Err(EngineError::UnsafePath);
        }
        if exists_at(&self.directory, "engine.native.json")?
            || exists_at(&self.directory, ".engine.native.pending")?
        {
            return Err(EngineError::StorageFailure);
        }
        let nonempty = self.storage_candidate()?;
        if exists_at(&self.directory, "engine.key")? {
            if exists_at(&self.directory, ".engine.key.pending")? {
                return Err(EngineError::StorageFailure);
            }
            return self.load_storage_keys();
        }
        if nonempty {
            return Err(EngineError::StorageFailure);
        }
        if exists_at(&self.directory, ".engine.key.pending")? {
            // Only a pristine interrupted initialization can discard staging.
            let pending = self.protected_key_file(".engine.key.pending")?;
            drop(pending);
            unlink_private(&self.directory, ".engine.key.pending", self.uid, false)?;
            platform::sync(&self.directory)?;
        }
        let master = Zeroizing::new(crate::admission::random_bytes::<32>()?);
        let mut file = openat(
            &self.directory,
            OsStr::new(".engine.key.pending"),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        // Creation modes are filtered by the caller's umask. Set the exact
        // required mode on this exclusively created descriptor before checking.
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        private(&file.metadata().map_err(io_error)?, self.uid, false)?;
        file.write_all(KEY_MAGIC)
            .and_then(|()| file.write_all(master.as_ref()))
            .map_err(io_error)?;
        platform::sync(&file)?;
        crate::fault::fault_point(
            self,
            JobId([0; 16]),
            crate::fault::FaultPoint::AfterKeyStage,
        );
        platform::publish_key(&self.directory)?;
        crate::fault::fault_point(
            self,
            JobId([0; 16]),
            crate::fault::FaultPoint::AfterKeyPublish,
        );
        platform::sync(&self.directory)?;
        crate::fault::fault_point(
            self,
            JobId([0; 16]),
            crate::fault::FaultPoint::AfterKeyDirectorySync,
        );
        Ok(StorageKeys(master))
    }
    pub fn prepare_db(&self, create: bool) -> Result<PathBuf> {
        if create && !exists_at(&self.directory, "engine.sqlite3")? {
            let file = openat(
                &self.directory,
                OsStr::new("engine.sqlite3"),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            private(&file.metadata().map_err(io_error)?, self.uid, false)?;
            platform::sync(&file)?;
            platform::sync(&self.directory)?;
            crate::fault::fault_point(
                self,
                JobId([0; 16]),
                crate::fault::FaultPoint::AfterDatabaseCreate,
            );
        }
        self.verify_db_files()?;
        Ok(self.path.join("engine.sqlite3"))
    }
    pub fn verify_db_files(&self) -> Result<()> {
        self.verify_sqlite_files(&self.directory, "engine.sqlite3", true, true)?;
        self.verify_database_descriptors()
    }
    fn verify_sqlite_files(
        &self,
        parent: &File,
        name: &str,
        required: bool,
        wal: bool,
    ) -> Result<()> {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let filename = format!("{name}{suffix}");
            let name_c = CString::new(filename).map_err(|_| EngineError::UnsafePath)?;
            let mut stat: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe {
                libc::fstatat(
                    parent.as_raw_fd(),
                    name_c.as_ptr(),
                    &mut stat,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } < 0
            {
                if (!suffix.is_empty() || !required)
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
                {
                    continue;
                }
                return Err(EngineError::StorageFailure);
            }
            if stat.st_uid != self.uid
                || stat.st_mode & libc::S_IFMT != libc::S_IFREG
                || stat.st_mode & 0o7777 != 0o600
                || stat.st_nlink != 1
            {
                return Err(EngineError::UnsafePath);
            }
            if !wal && ["-wal", "-shm"].contains(&suffix) {
                return Err(EngineError::UnsafePath);
            }
        }
        Ok(())
    }
    pub fn verify_database_descriptors(&self) -> Result<()> {
        platform::verify_descriptors(&self.path, &self.directory, &self.jobs)
    }

    fn file(&self, parent: &File, name: &str, create: bool) -> Result<File> {
        let flags = libc::O_RDWR
            | if create && !exists_at(parent, name)? {
                libc::O_CREAT | libc::O_EXCL
            } else {
                0
            };
        let file = openat(parent, OsStr::new(name), flags, 0o600)?;
        private(&file.metadata().map_err(io_error)?, self.uid, false)?;
        Ok(file)
    }
    fn job_dir(&self, job: JobId) -> Result<File> {
        let dir = openat(
            &self.jobs,
            OsStr::new(&job_hex(job)),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        private(&dir.metadata().map_err(io_error)?, self.uid, true)?;
        Ok(dir)
    }
    pub fn lock_frontend(&self) -> Result<FrontendGuard> {
        let file = self.file(&self.directory, "frontend.lock", true)?;
        platform::lock_exclusive(&file).map_err(|error| {
            if matches!(error, EngineError::Busy) {
                EngineError::AlreadyOpen
            } else {
                error
            }
        })?;
        let meta = self.directory.metadata().map_err(io_error)?;
        Ok(FrontendGuard {
            _file: file,
            root: (meta.dev(), meta.ino()),
        })
    }
    pub fn prepare_job(&self, job: JobId, credential: &WorkerCredential) -> Result<()> {
        let name = CString::new(job_hex(job)).map_err(|_| EngineError::UnsafePath)?;
        if unsafe { libc::mkdirat(self.jobs.as_raw_fd(), name.as_ptr(), 0o700) } == -1 {
            return Err(EngineError::StorageFailure);
        }
        let dir = self.job_dir(job)?;
        let mut file = openat(
            &dir,
            OsStr::new("credential"),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        file.write_all(&credential.0).map_err(io_error)?;
        platform::sync(&file)?;
        for name in ["worker.lock", "io.lock"] {
            platform::sync(&self.file(&dir, name, true)?)?;
        }
        platform::sync(&dir)?;
        platform::sync(&self.jobs)?;
        Ok(())
    }
    pub fn job_directory_exists(&self, job: JobId) -> Result<bool> {
        let name = job_hex(job);
        if !exists_at(&self.jobs, &name)? {
            return Ok(false);
        }
        let file = openat(
            &self.jobs,
            OsStr::new(&name),
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        private(&file.metadata().map_err(io_error)?, self.uid, true)?;
        Ok(true)
    }

    pub fn read_credential(&self, job: JobId) -> Result<WorkerCredential> {
        let dir = self.job_dir(job)?;
        let file = self.file(&dir, "credential", false)?;
        let mut bytes = Vec::new();
        file.take(33).read_to_end(&mut bytes).map_err(io_error)?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| EngineError::UnsafePath)?;
        Ok(WorkerCredential(bytes))
    }
    pub fn lock_job(&self, job: JobId) -> Result<JobGuard> {
        let file = self.file(&self.job_dir(job)?, "worker.lock", false)?;
        platform::lock_exclusive(&file)?;
        Ok(JobGuard { _file: file })
    }
    /// Recover only an unadmitted directory, after the frontend holding this
    /// root's lock proved Store::job_exists(job) is false. Admission creates the
    /// directory before either lock file, so interrupted preparation may omit both.
    pub fn lock_orphan_job(
        &self,
        job: JobId,
        frontend: &FrontendGuard,
    ) -> Result<(JobGuard, IoGuard)> {
        ensure_uid(self.uid)?;
        let meta = self.directory.metadata().map_err(io_error)?;
        let root = (meta.dev(), meta.ino());
        if frontend.root != root {
            return Err(EngineError::UnsafePath);
        }
        let directory = self.job_dir(job)?;
        let worker = self.file(&directory, "worker.lock", true)?;
        platform::lock_exclusive(&worker)?;
        let job_guard = JobGuard { _file: worker };
        let io = self.file(&directory, "io.lock", true)?;
        platform::lock_exclusive(&io)?;
        Ok((
            job_guard,
            IoGuard {
                _file: io,
                root,
                job,
            },
        ))
    }
    pub fn lock_io(&self, job: JobId) -> Result<IoGuard> {
        let file = self.file(&self.job_dir(job)?, "io.lock", false)?;
        platform::lock_exclusive(&file)?;
        let meta = self.directory.metadata().map_err(io_error)?;
        Ok(IoGuard {
            _file: file,
            root: (meta.dev(), meta.ino()),
            job,
        })
    }
    pub fn check_io(&self, job: JobId, guard: &IoGuard) -> Result<()> {
        let meta = self.directory.metadata().map_err(io_error)?;
        if guard.job != job || guard.root != (meta.dev(), meta.ino()) {
            return Err(EngineError::UnsafePath);
        }
        Ok(())
    }
    pub fn presence(
        &self,
        job: JobId,
        recorded: Option<&ProcessIdentity>,
    ) -> Result<WorkerPresence> {
        let dir = match self.job_dir(job) {
            Ok(dir) => dir,
            Err(_) if !exists_at(&self.jobs, &job_hex(job))? => return Ok(WorkerPresence::Absent),
            Err(error) => return Err(error),
        };
        if !exists_at(&dir, "worker.lock")? {
            return Err(EngineError::UnsafePath);
        }
        let file = self.file(&dir, "worker.lock", false)?;
        if !platform::lock_is_busy(&file)? {
            return Ok(WorkerPresence::Absent);
        }
        if let Some(identity) = recorded {
            if let Ok(actual) = process_identity(identity.pid) {
                if same_identity(identity, &actual) {
                    return Ok(WorkerPresence::Live(actual));
                }
            }
        }
        Ok(WorkerPresence::Initializing)
    }
    pub fn list_job_dirs(&self, limit: u32, cursor: Option<JobId>) -> Result<OrphanPage> {
        if !(1..=128).contains(&limit) {
            return Err(EngineError::InvalidRequest);
        }
        let mut found = BTreeSet::new();
        let cursor = cursor.map(job_hex);
        let mut visits = 0;
        for entry in platform::DirectoryEntries::open(&self.jobs)? {
            visits += 1;
            if visits > 100_256 {
                return Err(EngineError::QuotaExceeded);
            }
            let entry = entry?;
            let Ok(job) = parse_job(&entry) else {
                continue;
            };
            self.job_dir(job)?;
            let name = job_hex(job);
            if cursor.as_ref().is_some_and(|cursor| name <= *cursor) {
                continue;
            }
            found.insert(name);
            if found.len() > limit as usize + 1 {
                found.pop_last();
            }
        }
        let more = found.len() > limit as usize;
        if more {
            found.pop_last();
        }
        let jobs: Vec<_> = found
            .into_iter()
            .map(|name| parse_job(OsStr::new(&name)))
            .collect::<Result<_>>()?;
        let next = if more { jobs.last().copied() } else { None };
        Ok(OrphanPage { jobs, next })
    }
    pub(crate) fn cleanup_directories(&self) -> Result<BTreeSet<[u8; 16]>> {
        let mut jobs = BTreeSet::new();
        for name in platform::DirectoryEntries::open(&self.jobs)? {
            let job = parse_job(&name?)?;
            self.job_dir(job)?;
            jobs.insert(job.0);
            if jobs.len() > 256 {
                return Err(EngineError::QuotaExceeded);
            }
        }
        Ok(jobs)
    }
    pub fn prepare_spool_db(
        &self,
        job: JobId,
        stream: Stream,
        create: bool,
        guard: &IoGuard,
    ) -> Result<PathBuf> {
        self.check_io(job, guard)?;
        let dir = self.job_dir(job)?;
        let name = format!("{}.db", stream_name(stream));
        if exists_at(&dir, stream_name(stream))? {
            return Err(EngineError::StorageFailure);
        }
        self.verify_sqlite_files(&dir, &name, false, false)?;
        if !exists_at(&dir, &name)? {
            if !create || exists_at(&dir, &format!("{name}-journal"))? {
                return Err(EngineError::StorageFailure);
            }
            let file = openat(
                &dir,
                OsStr::new(&name),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            private(&file.metadata().map_err(io_error)?, self.uid, false)?;
            platform::sync(&file)?;
            platform::sync(&dir)?;
        }
        self.verify_spool_files(job, stream, guard)?;
        Ok(self.path.join("jobs").join(job_hex(job)).join(name))
    }
    pub fn verify_spool_files(&self, job: JobId, stream: Stream, guard: &IoGuard) -> Result<()> {
        self.check_io(job, guard)?;
        self.verify_sqlite_files(
            &self.job_dir(job)?,
            &format!("{}.db", stream_name(stream)),
            true,
            false,
        )?;
        self.verify_database_descriptors()
    }
    pub fn expire_spool(&self, job: JobId, guard: &IoGuard) -> Result<()> {
        self.check_io(job, guard)?;
        let dir = self.job_dir(job)?;
        for name in ["stdout.db", "stderr.db"] {
            self.verify_sqlite_files(&dir, name, false, false)?;
        }
        for name in [
            "stdout.db",
            "stdout.db-journal",
            "stderr.db",
            "stderr.db-journal",
        ] {
            unlink_private(&dir, name, self.uid, false)?;
            if name.ends_with(".db") {
                crate::fault::fault_point(
                    self,
                    job,
                    crate::fault::FaultPoint::AfterSpoolDatabaseRemove,
                );
            }
        }
        platform::sync(&dir)
    }
    pub fn remove_job(&self, job: JobId, guard: &IoGuard) -> Result<()> {
        self.check_io(job, guard)?;
        let dir = match self.job_dir(job) {
            Ok(dir) => dir,
            Err(_) if !exists_at(&self.jobs, &job_hex(job))? => return Ok(()),
            Err(error) => return Err(error),
        };
        for name in [
            "stdout.db",
            "stdout.db-journal",
            "stderr.db",
            "stderr.db-journal",
            "credential",
            "fault",
            "worker.lock",
            "io.lock",
        ] {
            unlink_private(&dir, name, self.uid, false)?;
        }
        unlink_private(&self.jobs, &job_hex(job), self.uid, true)?;
        platform::sync(&self.jobs)
    }
    #[cfg(feature = "fault-injection")]
    pub fn read_fault(&self, job: JobId) -> Result<Option<Vec<u8>>> {
        self.read_sentinel(&self.job_dir(job)?, "fault")
    }
    #[cfg(feature = "fault-injection")]
    pub fn read_frontend_fault(&self) -> Result<Option<Vec<u8>>> {
        self.read_sentinel(&self.directory, "fault")
    }
    #[cfg(feature = "fault-injection")]
    fn read_sentinel(&self, dir: &File, name: &str) -> Result<Option<Vec<u8>>> {
        let file = match self.file(dir, name, false) {
            Ok(file) => file,
            Err(_) => {
                let mut stat: libc::stat = unsafe { std::mem::zeroed() };
                let name = CString::new(name).map_err(|_| EngineError::UnsafePath)?;
                if unsafe {
                    libc::fstatat(
                        dir.as_raw_fd(),
                        name.as_ptr(),
                        &mut stat,
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } == -1
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
                {
                    return Ok(None);
                }
                return Err(EngineError::UnsafePath);
            }
        };
        let mut bytes = Vec::new();
        file.take(257).read_to_end(&mut bytes).map_err(io_error)?;
        if bytes.len() > 256 {
            return Err(EngineError::InvalidRequest);
        }
        Ok(Some(bytes))
    }
}

pub(crate) fn stream_name(stream: Stream) -> &'static str {
    match stream {
        Stream::Stdout => "stdout",
        Stream::Stderr => "stderr",
    }
}
fn unlink_private(parent: &File, name: &str, owner: u32, directory: bool) -> Result<()> {
    let c_name = CString::new(name).map_err(|_| EngineError::UnsafePath)?;
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            c_name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == -1
    {
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
            return Ok(());
        }
        return Err(EngineError::StorageFailure);
    }
    let expected = if directory {
        libc::S_IFDIR
    } else {
        libc::S_IFREG
    };
    if stat.st_uid != owner
        || stat.st_mode & libc::S_IFMT != expected
        || stat.st_mode & 0o7777 != if directory { 0o700 } else { 0o600 }
    {
        return Err(EngineError::UnsafePath);
    }
    if unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            c_name.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    } == -1
    {
        return Err(EngineError::StorageFailure);
    }
    Ok(())
}
pub(crate) fn local_account(cwd: Option<&Path>) -> Result<AccountSpec> {
    let uid = uid()?;
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0u8; 65536];
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            &mut passwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if code != 0 || result.is_null() {
        return Err(EngineError::InvalidPolicy);
    }
    let value = |pointer: *const libc::c_char| -> Result<Vec<u8>> {
        if pointer.is_null() {
            return Err(EngineError::InvalidPolicy);
        }
        Ok(unsafe { CStr::from_ptr(pointer) }.to_bytes().to_vec())
    };
    let home = PathBuf::from(OsString::from_vec(value(passwd.pw_dir)?));
    let shell = PathBuf::from(OsString::from_vec(value(passwd.pw_shell)?));
    let user = value(passwd.pw_name)?;
    let cwd = cwd.unwrap_or(&home).to_path_buf();
    if !home.is_absolute()
        || !shell.is_absolute()
        || !cwd.is_absolute()
        || !cwd.is_dir()
        || user.is_empty()
    {
        return Err(EngineError::InvalidPolicy);
    }
    Ok(AccountSpec {
        uid,
        gid: passwd.pw_gid,
        home,
        shell,
        cwd,
        user,
    })
}
pub(crate) fn same_identity(a: &ProcessIdentity, b: &ProcessIdentity) -> bool {
    a.boot_id == b.boot_id && a.pid == b.pid && a.start_ticks == b.start_ticks
}

pub(crate) fn trusted_worker_executable(path: &Path) -> Result<PathBuf> {
    let uid = uid()?;
    if !path.is_absolute() {
        return Err(EngineError::InvalidPolicy);
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| EngineError::InvalidPolicy)?;
    let meta = std::fs::metadata(&canonical).map_err(|_| EngineError::InvalidPolicy)?;
    if !meta.is_file()
        || ![0, uid].contains(&meta.uid())
        || meta.mode() & 0o022 != 0
        || meta.mode() & 0o111 == 0
    {
        return Err(EngineError::InvalidPolicy);
    }
    let mut parent = canonical.parent();
    while let Some(path) = parent {
        ancestor(
            &std::fs::metadata(path).map_err(|_| EngineError::UnsafePath)?,
            uid,
        )?;
        parent = path.parent();
    }
    Ok(canonical)
}
pub(crate) fn child_environment(account: &AccountSpec) -> Vec<(OsString, OsString)> {
    vec![
        ("HOME".into(), account.home.as_os_str().to_owned()),
        ("USER".into(), OsString::from_vec(account.user.clone())),
        ("LOGNAME".into(), OsString::from_vec(account.user.clone())),
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("LANG".into(), "C".into()),
    ]
}
/// Global prerequisites are checked before any durable attempt reservation.
pub(crate) fn worker_preconditions(executable: &Path) -> Result<PathBuf> {
    let executable = trusted_worker_executable(executable)?;
    platform::prepare_spawn()?;
    platform::execution_marker()?;
    #[cfg(test)]
    test_spawn::precondition()?;
    Ok(executable)
}

pub(crate) struct PreparedWorker(Command, StateRoot, JobId);
pub(crate) enum WorkerSpawnError {
    Transient,
    Global(EngineError),
}
pub(crate) fn prepare_worker(executable: &Path, spec: &LaunchSpec) -> Result<PreparedWorker> {
    let executable = worker_preconditions(executable)?;
    let root = StateRoot::open_existing(&spec.root)?;
    root.prepare_db(false)?;
    let mut command = Command::new(executable);
    command
        .arg(&spec.root)
        .arg(job_hex(spec.job))
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(marker) = platform::execution_marker()? {
        command.env("EPIPHYTIC_EXECUTION_ID", marker);
    }
    unsafe {
        command.pre_exec(|| platform::pre_exec());
    }
    Ok(PreparedWorker(command, root, spec.job))
}
impl PreparedWorker {
    pub(crate) fn verify_root(&self, root: &StateRoot) -> Result<()> {
        if self.1.identity()? != root.identity()? {
            return Err(EngineError::UnsafePath);
        }
        Ok(())
    }
    pub(crate) fn spawn(
        mut self,
        keys: &StorageKeys,
        mode: &crate::storage::StorageMode,
    ) -> std::result::Result<(), WorkerSpawnError> {
        let frame =
            crate::storage::frame(&self.1, self.2, mode, keys).map_err(WorkerSpawnError::Global)?;
        let (reader, mut writer) = std::io::pipe().map_err(|_| WorkerSpawnError::Transient)?;
        writer
            .write_all(frame.as_ref())
            .map_err(|_| WorkerSpawnError::Transient)?;
        drop(writer);
        self.0.stdin(Stdio::from(reader));
        #[cfg(test)]
        test_spawn::spawn()?;
        // Reserve the reaper before spawning; resource exhaustion is a counted
        // transient attempt, while no already-spawned child loses its owner.
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Child>(1);
        std::thread::Builder::new()
            .name("marmish-worker-reaper".into())
            .spawn(move || {
                if let Ok(mut child) = receiver.recv() {
                    let _ = child.wait();
                }
            })
            .map_err(|_| WorkerSpawnError::Transient)?;
        let child = self.0.spawn().map_err(|error| match error.raw_os_error() {
            Some(libc::EAGAIN | libc::ENOMEM) => WorkerSpawnError::Transient,
            _ => WorkerSpawnError::Global(EngineError::WorkerUnavailable),
        })?;
        sender
            .send(child)
            .map_err(|_| WorkerSpawnError::Global(EngineError::RecoveryUncertain))?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_spawn {
    use super::*;
    #[derive(Clone, Copy)]
    pub(crate) enum Failure {
        Global,
        Transient,
        LateGlobal,
    }
    thread_local! { static FAILURE: std::cell::Cell<Option<Failure>> = const { std::cell::Cell::new(None) }; }
    pub(crate) struct Guard(Option<Failure>);
    impl Drop for Guard {
        fn drop(&mut self) {
            FAILURE.set(self.0);
        }
    }
    pub(crate) fn inject(failure: Failure) -> Guard {
        Guard(FAILURE.replace(Some(failure)))
    }
    pub(super) fn precondition() -> Result<()> {
        if matches!(FAILURE.get(), Some(Failure::Global)) {
            Err(EngineError::WorkerUnavailable)
        } else {
            Ok(())
        }
    }
    pub(super) fn spawn() -> std::result::Result<(), WorkerSpawnError> {
        match FAILURE.get() {
            Some(Failure::Transient) => Err(WorkerSpawnError::Transient),
            Some(Failure::LateGlobal) => {
                Err(WorkerSpawnError::Global(EngineError::WorkerUnavailable))
            }
            _ => Ok(()),
        }
    }
}
/// The returned Child still reserves its PID even when /proc inspection failed.
fn discard_spawned_child(child: &mut Child, pid: i32) {
    // Called only after Command::spawn's successful exec handshake: pre_exec
    // already established this process group. Keep the leader unreaped while
    // signalling so its PID/PGID cannot be reused during uncertain cleanup.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid as u32,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } == 0
            && unsafe { info.si_pid() } != 0
        {
            let _ = child.wait();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
pub(crate) fn spawn_child(spec: &ChildSpec) -> std::result::Result<SpawnedChild, ChildSpawnError> {
    if spec.canonical_exec_required {
        crate::policy::require_canonical_exec(&spec.request)
            .map_err(|_| ChildSpawnError::NotStarted)?;
    }
    let account = local_account(Some(&spec.cwd)).map_err(|_| ChildSpawnError::NotStarted)?;
    if account.uid != spec.account.uid
        || account.gid != spec.account.gid
        || account.user != spec.account.user
        || account.home != spec.account.home
        || account.shell != spec.account.shell
        || spec.cwd != spec.account.cwd
        || spec.umask != 0o077
        || spec.environment != child_environment(&account)
    {
        return Err(ChildSpawnError::NotStarted);
    }
    platform::prepare_spawn().map_err(|_| ChildSpawnError::NotStarted)?;
    let mut command = match &spec.request {
        CommandRequest::Exec { argv } => {
            if argv.is_empty()
                || argv.len() > 256
                || argv[0].is_empty()
                || argv[0].len() > 4096
                || argv.iter().any(|a| a.contains(&0))
                || argv.iter().map(|a| a.len() + 1).sum::<usize>() > 65536
            {
                return Err(ChildSpawnError::NotStarted);
            }
            let mut command = Command::new(OsStr::from_bytes(&argv[0]));
            command.args(argv[1..].iter().map(|a| OsStr::from_bytes(a)));
            command
        }
        CommandRequest::Shell { command } => {
            if command.len() > 65536 || command.contains(&0) {
                return Err(ChildSpawnError::NotStarted);
            }
            let mut shell = Command::new(&account.shell);
            shell.arg("-c").arg(OsStr::from_bytes(command));
            shell
        }
    };
    command
        .env_clear()
        .envs(spec.environment.iter().cloned())
        .current_dir(&spec.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        command.pre_exec(|| platform::pre_exec());
    }
    let mut child = command.spawn().map_err(|_| ChildSpawnError::NotStarted)?;
    let pid = child.id() as i32;
    let identity = match process_identity(pid) {
        Ok(identity) => identity,
        Err(_) => {
            discard_spawned_child(&mut child, pid);
            return Err(ChildSpawnError::Uncertain);
        }
    };
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        discard_spawned_child(&mut child, pid);
        return Err(ChildSpawnError::Uncertain);
    };
    Ok(SpawnedChild {
        child,
        identity,
        pgid: pid,
        stdin,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests;

pub(crate) use crate::platform::{boot_time, process_identity};

impl StateRoot {
    pub(crate) fn identity(&self) -> Result<(u64, u64)> {
        let meta = self.directory.metadata().map_err(io_error)?;
        Ok((meta.dev(), meta.ino()))
    }
    pub(crate) fn validate_files(&self) -> Result<()> {
        if exists_at(&self.directory, "engine.native.json")?
            || exists_at(&self.directory, ".engine.native.pending")?
            || exists_at(&self.directory, ".engine.key.pending")?
        {
            return Err(EngineError::StorageFailure);
        }
        drop(self.protected_key_file("engine.key")?);
        Ok(())
    }
    fn check_frontend(&self, guard: &FrontendGuard) -> Result<()> {
        if guard.root != self.identity()? {
            return Err(EngineError::UnsafePath);
        }
        Ok(())
    }
    // A protected but torn staging file can be discarded only by callers that
    // have proved pristine initialization or authenticated legacy migration.
    // A parsed marker, including a foreign binding, is never discarded.
    fn pending_matches(&self, binding: &crate::NativeBinding, allow_torn: bool) -> Result<bool> {
        let file = self.protected_key_file(".engine.native.pending")?;
        if file.metadata().map_err(io_error)?.len() > 2048 {
            return Err(EngineError::StorageFailure);
        }
        match serde_json::from_reader::<_, crate::storage::Marker>(file) {
            Ok(marker) if marker == binding.marker() => Ok(true),
            Ok(_) => Err(EngineError::StorageFailure),
            Err(_) if allow_torn => Ok(false),
            Err(_) => Err(EngineError::StorageFailure),
        }
    }
    fn marker_matches(&self, name: &str, binding: &crate::NativeBinding) -> Result<()> {
        let file = self.protected_key_file(name)?;
        if file.metadata().map_err(io_error)?.len() > 2048 {
            return Err(EngineError::StorageFailure);
        }
        let marker: crate::storage::Marker =
            serde_json::from_reader(file).map_err(|_| EngineError::StorageFailure)?;
        if marker != binding.marker() {
            return Err(EngineError::StorageFailure);
        }
        Ok(())
    }
    pub(crate) fn validate_native(
        &self,
        binding: &crate::NativeBinding,
        mixed: bool,
    ) -> Result<()> {
        if exists_at(&self.directory, ".engine.key.pending")?
            || (!mixed && exists_at(&self.directory, "engine.key")?)
            || exists_at(&self.directory, ".engine.native.pending")?
        {
            return Err(EngineError::StorageFailure);
        }
        self.marker_matches("engine.native.json", binding)
    }
    fn publish_native(&self, binding: &crate::NativeBinding, allow_torn: bool) -> Result<()> {
        use crate::fault::{fault_point, FaultPoint};
        if exists_at(&self.directory, "engine.native.json")? {
            self.marker_matches("engine.native.json", binding)?;
            if exists_at(&self.directory, ".engine.native.pending")? {
                self.pending_matches(binding, allow_torn)?;
                unlink_private(&self.directory, ".engine.native.pending", self.uid, false)?;
                platform::sync(&self.directory)?;
            }
            return Ok(());
        }
        if exists_at(&self.directory, ".engine.native.pending")?
            && !self.pending_matches(binding, allow_torn)?
        {
            unlink_private(&self.directory, ".engine.native.pending", self.uid, false)?;
            platform::sync(&self.directory)?;
        }
        if !exists_at(&self.directory, ".engine.native.pending")? {
            let mut file = openat(
                &self.directory,
                OsStr::new(".engine.native.pending"),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )?;
            if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
                return Err(EngineError::UnsafePath);
            }
            fault_point(self, JobId([0; 16]), FaultPoint::AfterNativePendingCreate);
            let bytes =
                serde_json::to_vec(&binding.marker()).map_err(|_| EngineError::StorageFailure)?;
            file.write_all(&bytes).map_err(io_error)?;
            fault_point(self, JobId([0; 16]), FaultPoint::AfterNativePendingWrite);
            platform::sync(&file)?;
            fault_point(self, JobId([0; 16]), FaultPoint::AfterNativePendingSync);
        }
        platform::publish_native(&self.directory)?;
        fault_point(self, JobId([0; 16]), FaultPoint::AfterNativePublish);
        platform::sync(&self.directory)?;
        fault_point(self, JobId([0; 16]), FaultPoint::AfterNativeDirectorySync);
        Ok(())
    }
    pub(crate) fn initialize_native(
        &self,
        guard: &FrontendGuard,
        binding: &crate::NativeBinding,
    ) -> Result<()> {
        self.check_frontend(guard)?;
        if exists_at(&self.directory, "engine.key")?
            || exists_at(&self.directory, ".engine.key.pending")?
        {
            return Err(EngineError::StorageFailure);
        }
        let nonempty = self.storage_candidate()?;
        if exists_at(&self.directory, "engine.native.json")? {
            return self.validate_native(binding, false);
        }
        if nonempty || !self.storage_is_pristine()? {
            return Err(EngineError::StorageFailure);
        }
        self.publish_native(binding, true)
    }
    pub(crate) fn validate_migration(
        &self,
        binding: &crate::NativeBinding,
        master: &[u8; 32],
    ) -> Result<()> {
        if exists_at(&self.directory, ".engine.key.pending")? || !self.storage_candidate()? {
            return Err(EngineError::StorageFailure);
        }
        let legacy = exists_at(&self.directory, "engine.key")?;
        let marker = exists_at(&self.directory, "engine.native.json")?;
        if !legacy && !marker {
            return Err(EngineError::StorageFailure);
        }
        if legacy && self.load_storage_keys()?.0.as_ref() != master {
            return Err(EngineError::StorageFailure);
        }
        if marker {
            self.marker_matches("engine.native.json", binding)?;
        }
        if exists_at(&self.directory, ".engine.native.pending")? {
            self.pending_matches(binding, legacy)?;
        }
        Ok(())
    }
    pub(crate) fn commit_native(
        &self,
        guard: &FrontendGuard,
        binding: &crate::NativeBinding,
        fingerprint: &[u8; 32],
    ) -> Result<()> {
        use crate::fault::{fault_point, FaultPoint};
        self.check_frontend(guard)?;
        if exists_at(&self.directory, ".engine.key.pending")? {
            return Err(EngineError::StorageFailure);
        }
        if exists_at(&self.directory, "engine.key")? {
            if crate::admission::sha256(self.load_storage_keys()?.0.as_ref()) != *fingerprint {
                return Err(EngineError::StorageFailure);
            }
        } else {
            self.marker_matches("engine.native.json", binding)?;
        }
        self.publish_native(binding, exists_at(&self.directory, "engine.key")?)?;
        if exists_at(&self.directory, "engine.key")? {
            unlink_private(&self.directory, "engine.key", self.uid, false)?;
            fault_point(self, JobId([0; 16]), FaultPoint::AfterNativeLegacyUnlink);
        }
        platform::sync(&self.directory)?;
        fault_point(
            self,
            JobId([0; 16]),
            FaultPoint::AfterNativeLegacyDirectorySync,
        );
        Ok(())
    }
}
