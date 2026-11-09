//! Darwin APIs; native runner tests remain the authority for filesystem support.
use super::*;
use crate::process::diagnostic::{Context, DarwinStage as Ds, Failure, Staged};
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
pub(super) fn publish_file(directory: &File, from: &CStr, to: &CStr) -> Result<()> {
    if unsafe {
        libc::renameatx_np(
            directory.as_raw_fd(),
            from.as_ptr(),
            directory.as_raw_fd(),
            to.as_ptr(),
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
// Apple sys/proc_info.h: PROC_PGRP_ONLY. libc does not export this selector.
const PROC_PGRP_ONLY: u32 = 2;
// Current-main XNU bsd/sys/proc_info.h: PROC_FLAG_INEXIT. The diagnostic
// workflow checks the selected SDK declaration; this is not kernel proof.
// https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/sys/proc_info.h
#[cfg(debug_assertions)]
const PROC_FLAG_INEXIT: u32 = 4;
const _: () = assert!(size_of::<libc::proc_bsdinfo>() == 136);

fn bsd_info(pid: i32, include_zombie: bool) -> Result<libc::proc_bsdinfo> {
    bsd_info_staged(pid, include_zombie).map_err(|failure| failure.error)
}
fn bsd_info_staged(pid: i32, include_zombie: bool) -> Staged<libc::proc_bsdinfo> {
    if pid <= 0 {
        return Err(Failure::darwin(
            EngineError::WorkerUnavailable,
            Ds::InvalidPid,
            None,
        ));
    }
    let mut info: libc::proc_bsdinfo = unsafe { zeroed() };
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            u64::from(include_zombie),
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    if count != size_of::<libc::proc_bsdinfo>() as i32 {
        let errno = if count <= 0 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        return Err(Failure::darwin(
            EngineError::WorkerUnavailable,
            Ds::BsdRead,
            errno,
        ));
    }
    if info.pbi_pid != pid as u32
        || info.pbi_uid != unsafe { libc::geteuid() }
        || info.pbi_ruid != unsafe { libc::getuid() }
    {
        return Err(Failure::darwin(
            EngineError::WorkerUnavailable,
            Ds::BsdMetadata,
            None,
        ));
    }
    process_start(info.pbi_start_tvsec, info.pbi_start_tvusec)
        .map_err(|error| Failure::darwin(error, Ds::StartTime, None))?;
    Ok(info)
}
fn info_identity(info: &libc::proc_bsdinfo) -> Result<ProcessIdentity> {
    info_identity_staged(info).map_err(|failure| failure.error)
}
fn info_identity_staged(info: &libc::proc_bsdinfo) -> Staged<ProcessIdentity> {
    Ok(ProcessIdentity {
        boot_id: boot_id().map_err(|error| Failure::darwin(error, Ds::BootId, None))?,
        pid: info.pbi_pid as i32,
        start_ticks: process_start(info.pbi_start_tvsec, info.pbi_start_tvusec)
            .map_err(|error| Failure::darwin(error, Ds::StartTime, None))?,
    })
}
pub(super) fn process_identity(pid: i32) -> Result<ProcessIdentity> {
    info_identity(&bsd_info(pid, false)?)
}
pub(super) fn process_identity_staged(pid: i32) -> Staged<ProcessIdentity> {
    info_identity_staged(&bsd_info_staged(pid, false)?)
}
pub(super) fn owned_child_identity_and_group(pid: i32) -> Result<(ProcessIdentity, i32)> {
    owned_child_identity_and_group_staged(pid).map_err(|failure| failure.error)
}
pub(super) fn owned_child_identity_and_group_staged(pid: i32) -> Staged<(ProcessIdentity, i32)> {
    let info = bsd_info_staged(pid, true)?;
    let group = i32::try_from(info.pbi_pgid)
        .map_err(|_| Failure::darwin(EngineError::WorkerUnavailable, Ds::ChildGroup, None))?;
    if info.pbi_ppid != std::process::id() || group <= 0 {
        return Err(Failure::darwin(
            EngineError::WorkerUnavailable,
            Ds::ChildParent,
            None,
        ));
    }
    Ok((info_identity_staged(&info)?, group))
}
#[derive(PartialEq, Eq)]
struct ZombieIdentity {
    pid: u32,
    parent: u32,
    group: u32,
    uid: u32,
    ruid: u32,
    seconds: u64,
    micros: u64,
}
fn zombie_identity(pid: i32, group: i32) -> Staged<ZombieIdentity> {
    let info = bsd_info_staged(pid, true)?;
    zombie_identity_from_info(&info, group)
}
fn zombie_identity_from_info(info: &libc::proc_bsdinfo, group: i32) -> Staged<ZombieIdentity> {
    let stage = if info.pbi_pgid != group as u32 {
        Some(Ds::ZombieGroup)
    } else if info.pbi_status != libc::SZOMB {
        Some(Ds::ZombieStatus)
    } else {
        None
    };
    if let Some(stage) = stage {
        let failure = Failure::darwin(EngineError::RecoveryUncertain, stage, None);
        #[cfg(debug_assertions)]
        let failure = {
            use crate::process::diagnostic::{MemberKind, ZombieDetail};
            failure.with_zombie_detail(ZombieDetail {
                member: if info.pbi_pid == group as u32 {
                    MemberKind::Leader
                } else {
                    MemberKind::Other
                },
                group_zero: info.pbi_pgid == 0,
                status: info.pbi_status,
                in_exit: info.pbi_flags & PROC_FLAG_INEXIT != 0,
            })
        };
        return Err(failure);
    }
    Ok(ZombieIdentity {
        pid: info.pbi_pid,
        parent: info.pbi_ppid,
        group: info.pbi_pgid,
        uid: info.pbi_uid,
        ruid: info.pbi_ruid,
        seconds: info.pbi_start_tvsec,
        micros: info.pbi_start_tvusec,
    })
}
fn held_exit(pid: i32) -> Staged<(i32, i32, i32)> {
    let mut info: libc::siginfo_t = unsafe { zeroed() };
    if unsafe {
        libc::waitid(
            libc::P_PID,
            pid as u32,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } != 0
    {
        let errno = std::io::Error::last_os_error().raw_os_error();
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::HeldWait,
            errno,
        ));
    }
    if unsafe { info.si_pid() } != pid
        || !matches!(
            info.si_code,
            libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
        )
    {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::HeldStatus,
            None,
        ));
    }
    Ok((pid, info.si_code, unsafe { info.si_status() }))
}
fn zombie_members(group: i32) -> Staged<Vec<ZombieIdentity>> {
    let mut pids = [0i32; 257];
    let capacity = std::mem::size_of_val(&pids) as i32;
    let count = unsafe {
        libc::proc_listpids(
            PROC_PGRP_ONLY,
            group as u32,
            pids.as_mut_ptr().cast(),
            capacity,
        )
    };
    if count <= 0 {
        let errno = std::io::Error::last_os_error().raw_os_error();
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::MembersRead,
            errno,
        ));
    }
    if count >= capacity || !(count as usize).is_multiple_of(size_of::<i32>()) {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::MembersCount,
            None,
        ));
    }
    let pids = &mut pids[..count as usize / size_of::<i32>()];
    pids.sort_unstable();
    if pids[0] <= 0
        || pids.windows(2).any(|p| p[0] == p[1])
        || pids.iter().filter(|pid| **pid == group).count() != 1
    {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::MembersIds,
            None,
        ));
    }
    pids.iter()
        .map(|pid| zombie_identity(*pid, group))
        .collect()
}
/// EPERM is acceptable only for an unchanged, entirely terminal owned group.
/// This proof neither signals nor reaps; the held child reserves its PGID.
#[cfg(test)]
pub(super) fn owned_zombie_group(identity: &ProcessIdentity, group: i32) -> Result<()> {
    owned_zombie_group_staged(identity, group).map_err(|failure| failure.error)
}
pub(super) fn owned_zombie_group_staged(identity: &ProcessIdentity, group: i32) -> Staged<()> {
    let uid = unsafe { libc::getuid() };
    if group <= 1 || group != identity.pid || uid == 0 || uid != unsafe { libc::geteuid() } {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::ProofGuard,
            None,
        ));
    }
    let exit = held_exit(group).map_err(|failure| failure.context(Context::ProofHeld))?;
    if owned_child_identity_and_group_staged(group)
        .map_err(|failure| failure.context(Context::ProofOwner))?
        != (identity.clone(), group)
    {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::ProofIdentity,
            None,
        ));
    }
    let leader =
        zombie_identity(group, group).map_err(|failure| failure.context(Context::ProofLeader))?;
    let first =
        zombie_members(group).map_err(|failure| failure.context(Context::ProofFirstMembers))?;
    if !first.iter().any(|member| member == &leader) {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::ProofLeaderMissing,
            None,
        ));
    }
    if first
        != zombie_members(group).map_err(|failure| failure.context(Context::ProofSecondMembers))?
    {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::ProofMembersChanged,
            None,
        ));
    }
    if exit != held_exit(group).map_err(|failure| failure.context(Context::ProofFinalHeld))? {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::ProofExitChanged,
            None,
        ));
    }
    if leader
        != zombie_identity(group, group)
            .map_err(|failure| failure.context(Context::ProofFinalLeader))?
    {
        return Err(Failure::darwin(
            EngineError::RecoveryUncertain,
            Ds::ProofLeaderChanged,
            None,
        ));
    }
    Ok(())
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
            || !(count as usize).is_multiple_of(size_of::<libc::proc_fdinfo>())
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
pub(super) fn initialize_process_limits() -> Result<()> {
    let mut before: libc::rlimit = unsafe { zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut before) } != 0 {
        return Err(EngineError::WorkerUnavailable);
    }
    let expected_soft = before.rlim_cur.min(FD_CEILING as libc::rlim_t);
    if before.rlim_cur != expected_soft {
        let normalized = libc::rlimit {
            rlim_cur: expected_soft,
            rlim_max: before.rlim_max,
        };
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &normalized) } != 0 {
            return Err(EngineError::WorkerUnavailable);
        }
    }
    let mut after: libc::rlimit = unsafe { zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut after) } != 0
        || after.rlim_cur != expected_soft
        || after.rlim_max != before.rlim_max
    {
        return Err(EngineError::WorkerUnavailable);
    }
    Ok(())
}
pub(super) fn prepare_spawn() -> Result<()> {
    prepare_spawn_below(FD_CEILING)
}
// The production ceiling is fixed; the parameter lets the isolated Darwin
// regression exercise real inherited descriptors below kernel allocation caps.
fn prepare_spawn_below(ceiling: i32) -> Result<()> {
    let mut limit: libc::rlimit = unsafe { zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0
        || ceiling <= 0
        || limit.rlim_cur > ceiling as u64
        || descriptors()?.iter().any(|entry| entry.proc_fd >= ceiling)
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
    // Pure post-read validation: the real BSD reader retains its identity checks.
    fn zombie_info() -> libc::proc_bsdinfo {
        let mut info: libc::proc_bsdinfo = unsafe { zeroed() };
        info.pbi_pid = 41;
        info.pbi_ppid = 7;
        info.pbi_pgid = 41;
        info.pbi_uid = 501;
        info.pbi_ruid = 502;
        info.pbi_start_tvsec = 10;
        info.pbi_start_tvusec = 20;
        info.pbi_status = libc::SZOMB;
        info
    }
    #[test]
    fn zombie_validator_retains_identity_and_rejects_group_or_live_status() {
        let mut info = zombie_info();
        let identity = zombie_identity_from_info(&info, 41)
            .unwrap_or_else(|_| panic!("matching zombie rejected"));
        assert_eq!((identity.pid, identity.parent, identity.group), (41, 7, 41));
        assert_eq!(
            (
                identity.uid,
                identity.ruid,
                identity.seconds,
                identity.micros
            ),
            (501, 502, 10, 20)
        );
        for (group, status) in [(41, 2), (0, libc::SZOMB), (42, libc::SZOMB), (42, 2)] {
            info.pbi_pgid = group;
            info.pbi_status = status;
            assert!(matches!(
                zombie_identity_from_info(&info, 41).map_err(|failure| failure.error),
                Err(EngineError::RecoveryUncertain)
            ));
        }
    }
    #[cfg(debug_assertions)]
    #[test]
    fn zombie_validator_reports_order_member_group_status_and_exit_flag() {
        use crate::process::diagnostic::{MemberKind, ZombieDetail};
        for pid in [41, 42] {
            for (group, status, stage) in [
                (41, 2, "darwin-zombie-status"),
                (0, libc::SZOMB, "darwin-zombie-group"),
                (42, libc::SZOMB, "darwin-zombie-group"),
                (42, 2, "darwin-zombie-group"),
            ] {
                for flags in [0, PROC_FLAG_INEXIT, 8, PROC_FLAG_INEXIT | 8] {
                    let mut info = zombie_info();
                    info.pbi_pid = pid;
                    info.pbi_pgid = group;
                    info.pbi_status = status;
                    info.pbi_flags = flags;
                    let failure = match zombie_identity_from_info(&info, 41) {
                        Err(failure) => failure,
                        Ok(_) => panic!("invalid zombie accepted"),
                    };
                    assert_eq!(
                        failure.diagnostic_detail(),
                        (
                            stage,
                            Some(ZombieDetail {
                                member: if pid == 41 {
                                    MemberKind::Leader
                                } else {
                                    MemberKind::Other
                                },
                                group_zero: group == 0,
                                status,
                                in_exit: flags & PROC_FLAG_INEXIT != 0,
                            })
                        )
                    );
                    assert!(matches!(failure.error, EngineError::RecoveryUncertain));
                }
            }
        }
    }
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
        publish_file(&root, c".engine.key.pending", c"engine.key").unwrap();
        std::fs::write(tmp.path().join(".engine.key.pending"), b"second").unwrap();
        assert!(publish_file(&root, c".engine.key.pending", c"engine.key").is_err());
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
            assert!(matches!(
                prepare_spawn(),
                Err(EngineError::WorkerUnavailable)
            ));
            assert_eq!(FD_CEILING, 65_536);
            let file = File::open("/dev/null").unwrap();
            let baseline_max = descriptors()
                .unwrap()
                .iter()
                .map(|entry| entry.proc_fd)
                .max()
                .unwrap_or(2);
            let ceiling = baseline_max.checked_add(1).unwrap().max(64);
            assert!(
                ceiling < FD_CEILING - 1 && (ceiling as u64 + 1) <= limit.rlim_max,
                "unsupported test budget: baseline={baseline_max}, ceiling={ceiling}, hard={}",
                limit.rlim_max
            );
            let mut set_soft = |soft: u64| {
                limit.rlim_cur = soft;
                assert_eq!(
                    unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) },
                    0,
                    "set limit: baseline={baseline_max}, ceiling={ceiling}, soft={soft}, hard={}",
                    limit.rlim_max
                );
                let mut actual: libc::rlimit = unsafe { zeroed() };
                assert_eq!(
                    unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut actual) },
                    0
                );
                assert_eq!(actual.rlim_cur, soft);
            };
            set_soft(ceiling as u64);
            prepare_spawn().unwrap();
            prepare_spawn_below(ceiling).unwrap();
            set_soft(ceiling as u64 + 1);
            let high = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, ceiling) };
            assert_eq!(
                high, ceiling,
                "native runner cannot allocate test descriptor"
            );
            let high = unsafe { File::from_raw_fd(high) };
            assert!(matches!(
                prepare_spawn_below(ceiling),
                Err(EngineError::WorkerUnavailable)
            ));
            set_soft(ceiling as u64 - 1);
            assert!(matches!(
                prepare_spawn_below(ceiling),
                Err(EngineError::WorkerUnavailable)
            ));
            drop(high);
            prepare_spawn_below(ceiling).unwrap();
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
