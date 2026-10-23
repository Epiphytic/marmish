//! Darwin APIs; native runner tests remain the authority for filesystem support.
use super::*;
use std::ffi::CString;
use std::mem::{size_of, zeroed};

const FD_CEILING: i32 = 65_536;
const MAX_FDS: usize = 262_144;
unsafe extern "C" {
    fn mach_continuous_time() -> u64;
}

fn unsupported(error: std::io::Error) -> EngineError {
    match error.raw_os_error() {
        Some(libc::ENOTSUP | libc::EINVAL | libc::ENOSYS) => EngineError::UnsupportedStorage,
        _ => io_error(error),
    }
}
pub(super) fn validate_storage(root: &File) -> Result<()> {
    let mut stat: libc::statfs = unsafe { zeroed() };
    if unsafe { libc::fstatfs(root.as_raw_fd(), &mut stat) } != 0 {
        return Err(EngineError::UnsupportedStorage);
    }
    let kind = unsafe { CStr::from_ptr(stat.f_fstypename.as_ptr()) };
    if kind.to_bytes() != b"apfs" || stat.f_flags & libc::MNT_LOCAL as u32 == 0 {
        return Err(EngineError::UnsupportedStorage);
    }
    Ok(())
}
pub(super) fn sync(file: &File) -> Result<()> {
    file.sync_all().map_err(unsupported)?;
    if file.metadata().map_err(io_error)?.is_file()
        && unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != 0
    {
        return Err(unsupported(std::io::Error::last_os_error()));
    }
    Ok(())
}
pub(super) fn publish_key(directory: &File) -> Result<()> {
    if unsafe {
        libc::renameatx_np(
            directory.as_raw_fd(),
            c".engine.key.pending".as_ptr(),
            directory.as_raw_fd(),
            c"engine.key".as_ptr(),
            libc::RENAME_EXCL,
        )
    } != 0
    {
        return Err(unsupported(std::io::Error::last_os_error()));
    }
    Ok(())
}
pub(super) fn lock_exclusive(file: &File) -> Result<()> {
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}
pub(super) fn lock_is_busy(file: &File) -> Result<bool> {
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EWOULDBLOCK) => Ok(true),
            _ => Err(EngineError::StorageFailure),
        };
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } != 0 {
        return Err(EngineError::StorageFailure);
    }
    Ok(false)
}
fn boot_id() -> Result<[u8; 16]> {
    let mut bytes = [0u8; 64];
    let mut length = bytes.len();
    if unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(EngineError::StorageFailure);
    }
    parse_boot_uuid(bytes.get(..length).ok_or(EngineError::StorageFailure)?)
}
fn parse_boot_uuid(raw: &[u8]) -> Result<[u8; 16]> {
    let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
    if raw.len() != 36 {
        return Err(EngineError::StorageFailure);
    }
    let mut compact = [0u8; 32];
    let mut index = 0;
    for (offset, byte) in raw.iter().copied().enumerate() {
        if [8, 13, 18, 23].contains(&offset) {
            if byte != b'-' {
                return Err(EngineError::StorageFailure);
            }
        } else {
            if !byte.is_ascii_hexdigit() {
                return Err(EngineError::StorageFailure);
            }
            compact[index] = byte.to_ascii_lowercase();
            index += 1;
        }
    }
    Ok(parse_job(OsStr::from_bytes(&compact))?.0)
}
#[allow(deprecated)]
pub(super) fn boot_time() -> Result<BootTime> {
    let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
    if unsafe { libc::mach_timebase_info(&mut info) } != 0 || info.numer == 0 || info.denom == 0 {
        return Err(EngineError::StorageFailure);
    }
    let millis = continuous_millis(unsafe { mach_continuous_time() }, info.numer, info.denom)?;
    Ok(BootTime {
        boot_id: boot_id()?,
        millis,
    })
}
fn continuous_millis(ticks: u64, numer: u32, denom: u32) -> Result<u64> {
    if numer == 0 || denom == 0 {
        return Err(EngineError::StorageFailure);
    }
    u64::try_from(u128::from(ticks) * u128::from(numer) / u128::from(denom) / 1_000_000)
        .map_err(|_| EngineError::StorageFailure)
}
pub(super) fn process_identity(pid: i32) -> Result<ProcessIdentity> {
    if pid <= 0 {
        return Err(EngineError::WorkerUnavailable);
    }
    let mut info: libc::proc_bsdinfo = unsafe { zeroed() };
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    if count != size_of::<libc::proc_bsdinfo>() as i32
        || info.pbi_pid != pid as u32
        || info.pbi_uid != unsafe { libc::geteuid() }
        || info.pbi_ruid != unsafe { libc::getuid() }
    {
        return Err(EngineError::WorkerUnavailable);
    }
    let start_ticks = process_start(info.pbi_start_tvsec, info.pbi_start_tvusec)?;
    Ok(ProcessIdentity {
        boot_id: boot_id()?,
        pid,
        start_ticks,
    })
}
fn process_start(seconds: u64, micros: u64) -> Result<u64> {
    if micros >= 1_000_000 {
        return Err(EngineError::WorkerUnavailable);
    }
    seconds
        .checked_mul(1_000_000)
        .and_then(|v| v.checked_add(micros))
        .filter(|v| *v != 0)
        .ok_or(EngineError::WorkerUnavailable)
}

/// A full buffer may have been truncated. Retry with space, bounded in both size
/// and attempts; never treat a snapshot error as an empty descriptor list.
fn descriptors() -> Result<Vec<libc::proc_fdinfo>> {
    let mut capacity = 256usize;
    for _ in 0..12 {
        let mut entries: Vec<libc::proc_fdinfo> =
            (0..capacity).map(|_| unsafe { zeroed() }).collect();
        let bytes = capacity
            .checked_mul(size_of::<libc::proc_fdinfo>())
            .ok_or(EngineError::UnsafePath)?;
        let count = unsafe {
            libc::proc_pidinfo(
                std::process::id() as i32,
                libc::PROC_PIDLISTFDS,
                0,
                entries.as_mut_ptr().cast(),
                bytes as i32,
            )
        };
        if count <= 0
            || count as usize > bytes
            || count as usize % size_of::<libc::proc_fdinfo>() != 0
        {
            return Err(EngineError::UnsafePath);
        }
        if (count as usize) < bytes {
            entries.truncate(count as usize / size_of::<libc::proc_fdinfo>());
            if entries.iter().any(|v| v.proc_fd < 0) {
                return Err(EngineError::UnsafePath);
            }
            return Ok(entries);
        }
        capacity = capacity
            .checked_mul(2)
            .filter(|v| *v <= MAX_FDS)
            .ok_or(EngineError::UnsafePath)?;
    }
    Err(EngineError::UnsafePath)
}
pub(super) fn descriptor_count() -> Result<usize> {
    Ok(descriptors()?.len())
}
fn descriptor_path(fd: i32) -> Result<PathBuf> {
    let mut path = [0u8; libc::PATH_MAX as usize];
    if unsafe { libc::fcntl(fd, libc::F_GETPATH, path.as_mut_ptr()) } < 0 {
        return Err(EngineError::UnsafePath);
    }
    let end = path
        .iter()
        .position(|b| *b == 0)
        .ok_or(EngineError::UnsafePath)?;
    let path = PathBuf::from(OsString::from_vec(path[..end].to_vec()));
    if !path.is_absolute() {
        return Err(EngineError::UnsafePath);
    }
    Ok(path)
}
fn identity(stat: &libc::stat) -> (u64, u64) {
    (stat.st_dev as u64, stat.st_ino)
}
fn descriptor_stat(fd: i32) -> Result<libc::stat> {
    let mut stat = unsafe { zeroed() };
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        return Err(EngineError::UnsafePath);
    }
    Ok(stat)
}
fn open_directory(path: &Path) -> Result<File> {
    let name = CString::new(path.as_os_str().as_bytes()).map_err(|_| EngineError::UnsafePath)?;
    let fd = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(EngineError::UnsafePath);
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
/// Walk held parent descriptions, comparing inode identity at every level.
/// Kernel paths may spell APFS names differently or cross a firmlink alias.
fn ancestry_contains(mut parent: File, root: (u64, u64)) -> Result<bool> {
    for _ in 0..1024 {
        let before = descriptor_stat(parent.as_raw_fd())?;
        if identity(&before) == root {
            return Ok(true);
        }
        let next_fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                c"..".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next_fd < 0 {
            return Err(EngineError::UnsafePath);
        }
        let next = unsafe { File::from_raw_fd(next_fd) };
        let after = descriptor_stat(next.as_raw_fd())?;
        // Reopen the current parent by its kernel path as a directory only, so
        // a concurrent directory move cannot silently change this ancestry.
        let named = open_directory(&descriptor_path(parent.as_raw_fd())?)?;
        if identity(&descriptor_stat(named.as_raw_fd())?) != identity(&before) {
            return Err(EngineError::UnsafePath);
        }
        let verify_fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                c"..".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if verify_fd < 0 {
            return Err(EngineError::UnsafePath);
        }
        let verify_parent = unsafe { File::from_raw_fd(verify_fd) };
        if identity(&descriptor_stat(verify_parent.as_raw_fd())?) != identity(&after) {
            return Err(EngineError::UnsafePath);
        }
        if identity(&before) == identity(&after) {
            return Ok(false);
        }
        parent = next;
    }
    Err(EngineError::UnsafePath)
}
pub(super) fn verify_descriptors(_path: &Path, root: &File, jobs: &File) -> Result<()> {
    let root_id = identity(&descriptor_stat(root.as_raw_fd())?);
    let jobs_id = identity(&descriptor_stat(jobs.as_raw_fd())?);
    let canonical_root = open_directory(&descriptor_path(root.as_raw_fd())?)?;
    if identity(&descriptor_stat(canonical_root.as_raw_fd())?) != root_id {
        return Err(EngineError::UnsafePath);
    }
    // Compare known state-file inodes as well as path ancestry. fstatat does
    // not open/close regular files or disturb SQLite's process-scoped locks.
    let mut known = std::collections::BTreeSet::new();
    for name in DirectoryEntries::open(root)? {
        let name = CString::new(name?.as_bytes()).map_err(|_| EngineError::UnsafePath)?;
        let mut stat: libc::stat = unsafe { zeroed() };
        if unsafe {
            libc::fstatat(
                root.as_raw_fd(),
                name.as_ptr(),
                &mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(EngineError::UnsafePath);
        }
        known.insert(identity(&stat));
    }
    for entry in descriptors()? {
        if entry.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
            // Unknown/new descriptor types require an explicit platform review.
            if ![
                libc::PROX_FDTYPE_ATALK,
                libc::PROX_FDTYPE_SOCKET,
                libc::PROX_FDTYPE_PSHM,
                libc::PROX_FDTYPE_PSEM,
                libc::PROX_FDTYPE_KQUEUE,
                libc::PROX_FDTYPE_PIPE,
                libc::PROX_FDTYPE_FSEVENTS,
                libc::PROX_FDTYPE_NETPOLICY,
                libc::PROX_FDTYPE_CHANNEL,
                libc::PROX_FDTYPE_NEXUS,
            ]
            .iter()
            .any(|kind| entry.proc_fdtype == *kind as u32)
            {
                return Err(EngineError::UnsafePath);
            }
            continue;
        }
        let before = descriptor_stat(entry.proc_fd)?;
        let path = descriptor_path(entry.proc_fd)?;
        let flags = unsafe { libc::fcntl(entry.proc_fd, libc::F_GETFD) };
        if flags < 0 {
            return Err(EngineError::UnsafePath);
        }
        if path == Path::new("/") {
            if before.st_mode & libc::S_IFMT != libc::S_IFDIR
                || identity(&descriptor_stat(open_directory(&path)?.as_raw_fd())?)
                    != identity(&before)
            {
                return Err(EngineError::UnsafePath);
            }
            continue;
        }
        let parent_path = path.parent().ok_or(EngineError::UnsafePath)?;
        // canonicalize resolves only directory aliases; never open a regular
        // SQLite file, whose close could release this process's POSIX locks.
        let parent = open_directory(
            &parent_path
                .canonicalize()
                .map_err(|_| EngineError::UnsafePath)?,
        )?;
        let name = CString::new(path.file_name().ok_or(EngineError::UnsafePath)?.as_bytes())
            .map_err(|_| EngineError::UnsafePath)?;
        let mut named: libc::stat = unsafe { zeroed() };
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                &mut named,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
            || identity(&named) != identity(&before)
        {
            return Err(EngineError::UnsafePath);
        }
        let relevant = identity(&before) == root_id
            || identity(&before) == jobs_id
            || known.contains(&identity(&before))
            || ancestry_contains(parent, root_id)?;
        if relevant && flags & libc::FD_CLOEXEC == 0 {
            return Err(EngineError::UnsafePath);
        }
        if identity(&descriptor_stat(entry.proc_fd)?) != identity(&before)
            || descriptor_path(entry.proc_fd)? != path
        {
            return Err(EngineError::UnsafePath);
        }
    }
    Ok(())
}
pub(super) fn prepare_spawn() -> Result<()> {
    let mut limit: libc::rlimit = unsafe { zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0
        || limit.rlim_cur > FD_CEILING as u64
        || descriptors()?
            .iter()
            .any(|entry| entry.proc_fd >= FD_CEILING)
    {
        return Err(EngineError::WorkerUnavailable);
    }
    // No Marmish code raises this limit. Linked SQLCipher/OpenSSL do not alter
    // RLIMIT_NOFILE; future limit changes require revisiting this invariant.
    Ok(())
}
pub(super) unsafe fn pre_exec() -> std::io::Result<()> {
    if unsafe { libc::setsid() } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    unsafe {
        libc::umask(0o077);
    }
    for fd in 3..FD_CEILING {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags < 0 {
            if unsafe { *libc::__error() } == libc::EBADF {
                continue;
            }
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uuid_clock_and_process_bounds() {
        assert_eq!(size_of::<libc::proc_bsdinfo>(), 136);
        assert!(parse_boot_uuid(b"ABCDEF00-1234-5678-9ABC-DEF012345678\0").is_ok());
        assert!(parse_boot_uuid(b"abcdef00-1234-5678-9abc-def012345678\0\0").is_err());
        assert!(parse_boot_uuid(b"abcdef00123456789abcdef012345678").is_err());
        assert_eq!(continuous_millis(3_000_000, 2, 3).unwrap(), 2);
        assert!(continuous_millis(u64::MAX, u32::MAX, 1).is_err());
        assert!(continuous_millis(1, 1, 0).is_err());
        assert!(process_start(1, 1_000_000).is_err());
        assert!(process_start(u64::MAX, 0).is_err());
        assert!(process_start(0, 0).is_err());
    }
    #[test]
    fn native_descriptors_and_storage_cloexec() {
        let tmp = tempfile::tempdir().unwrap();
        let root = File::open(tmp.path()).unwrap();
        validate_storage(&root).unwrap();
        verify_descriptors(tmp.path(), &root, &root).unwrap();
        assert!(!descriptors().unwrap().is_empty());
        let flags = unsafe { libc::fcntl(root.as_raw_fd(), libc::F_GETFD) };
        assert_eq!(
            unsafe { libc::fcntl(root.as_raw_fd(), libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
            0
        );
        let result = verify_descriptors(tmp.path(), &root, &root);
        unsafe {
            libc::fcntl(root.as_raw_fd(), libc::F_SETFD, flags);
        }
        assert!(matches!(result, Err(EngineError::UnsafePath)));
    }
    #[test]
    fn exclusive_publication_never_clobbers_and_directory_aliases_keep_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let root = File::open(tmp.path()).unwrap();
        validate_storage(&root).unwrap();
        std::fs::write(tmp.path().join(".engine.key.pending"), b"first").unwrap();
        publish_key(&root).unwrap();
        std::fs::write(tmp.path().join(".engine.key.pending"), b"second").unwrap();
        assert!(publish_key(&root).is_err());
        assert_eq!(
            std::fs::read(tmp.path().join("engine.key")).unwrap(),
            b"first"
        );
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(tmp.path(), &alias).unwrap();
        let parent = open_directory(&alias.canonicalize().unwrap()).unwrap();
        assert!(ancestry_contains(
            parent,
            identity(&descriptor_stat(root.as_raw_fd()).unwrap())
        )
        .unwrap());
        let child = tmp.path().join("MiXeD-case");
        std::fs::create_dir(&child).unwrap();
        let spelling = tmp.path().join("mixed-CASE");
        if spelling.is_dir() {
            assert!(ancestry_contains(
                open_directory(&spelling).unwrap(),
                identity(&descriptor_stat(root.as_raw_fd()).unwrap())
            )
            .unwrap());
        } else {
            // Case-sensitive APFS still checks membership by inode ancestry.
            assert!(ancestry_contains(
                open_directory(&child).unwrap(),
                identity(&descriptor_stat(root.as_raw_fd()).unwrap())
            )
            .unwrap());
        }
    }
    #[test]
    fn cloexec_sweep_closes_unmarked_descriptor_at_exec() {
        use std::os::unix::process::CommandExt;
        let file = tempfile::tempfile().unwrap();
        let fd = file.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
            0
        );
        prepare_spawn().unwrap();
        let started = std::time::Instant::now();
        let mut command = std::process::Command::new("/usr/bin/perl");
        command
            .arg("-e")
            .arg(format!("exit(open(my $f, '<&={fd}') ? 9 : 0)"));
        unsafe {
            command.pre_exec(|| pre_exec());
        }
        let status = command.status();
        unsafe {
            libc::fcntl(fd, libc::F_SETFD, flags);
        }
        assert!(status.unwrap().success());
        eprintln!(
            "Darwin CLOEXEC sweep + exec latency: {:?}",
            started.elapsed()
        );
    }
    #[test]
    fn inherited_descriptor_above_lowered_limit_is_rejected() {
        if std::env::var_os("MARMISH_TEST_DARWIN_FD_LIMIT").is_some() {
            let mut limit: libc::rlimit = unsafe { zeroed() };
            assert_eq!(
                unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
                0
            );
            assert!(
                limit.rlim_max > FD_CEILING as u64,
                "native runner hard FD limit cannot exercise inherited-high-FD gate"
            );
            limit.rlim_cur = FD_CEILING as u64 + 1;
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
            assert!(prepare_spawn().is_err());
            let file = File::open("/dev/null").unwrap();
            let high = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, FD_CEILING) };
            assert!(
                high >= FD_CEILING,
                "native runner cannot allocate descriptor at scan ceiling"
            );
            let high = unsafe { File::from_raw_fd(high) };
            limit.rlim_cur = 512;
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
            assert!(prepare_spawn().is_err());
            drop(high);
            prepare_spawn().unwrap();
            return;
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "platform::macos::tests::inherited_descriptor_above_lowered_limit_is_rejected",
                "--nocapture",
            ])
            .env("MARMISH_TEST_DARWIN_FD_LIMIT", "1")
            .status()
            .unwrap();
        assert!(status.success());
    }
}
