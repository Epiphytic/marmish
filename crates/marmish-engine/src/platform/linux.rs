//! Linux primitives retain open-file-description locks and close_range.
use super::*;
use std::io::Read;
use std::path::Path;

fn flock() -> libc::flock {
    libc::flock {
        l_type: libc::F_WRLCK as i16,
        l_whence: libc::SEEK_SET as i16,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    }
}
pub(super) fn lock_exclusive(file: &File) -> Result<()> {
    let lock = flock();
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_SETLK, &lock) } == -1 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn lock_is_busy(file: &File) -> Result<bool> {
    let mut lock = flock();
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_GETLK, &mut lock) } == -1 {
        return Err(EngineError::StorageFailure);
    }
    Ok(lock.l_type != libc::F_UNLCK as i16)
}

pub(super) fn boot_time() -> Result<BootTime> {
    let boot_id = boot_id()?;
    let mut clock: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut clock) } == -1 || clock.tv_sec < 0 {
        return Err(EngineError::StorageFailure);
    }
    let millis = (clock.tv_sec as u64)
        .checked_mul(1000)
        .and_then(|n| n.checked_add(clock.tv_nsec as u64 / 1_000_000))
        .ok_or(EngineError::StorageFailure)?;
    Ok(BootTime { boot_id, millis })
}
fn boot_id() -> Result<[u8; 16]> {
    let raw = std::fs::read("/proc/sys/kernel/random/boot_id").map_err(io_error)?;
    let compact: Vec<_> = raw
        .into_iter()
        .filter(|b| *b != b'-' && *b != b'\n')
        .collect();
    Ok(parse_job(OsStr::from_bytes(&compact))?.0)
}
pub(super) fn process_identity(pid: i32) -> Result<ProcessIdentity> {
    if pid <= 0 {
        return Err(EngineError::WorkerUnavailable);
    }
    let mut raw = Vec::new();
    File::open(format!("/proc/{pid}/stat"))
        .map_err(|_| EngineError::WorkerUnavailable)?
        .take(16385)
        .read_to_end(&mut raw)
        .map_err(|_| EngineError::WorkerUnavailable)?;
    if raw.len() > 16384 {
        return Err(EngineError::WorkerUnavailable);
    }
    let start_ticks = parse_proc_stat(&raw, pid)?;
    Ok(ProcessIdentity {
        boot_id: boot_id()?,
        pid,
        start_ticks,
    })
}
pub(crate) fn parse_proc_stat(raw: &[u8], pid: i32) -> Result<u64> {
    let text = std::str::from_utf8(raw).map_err(|_| EngineError::WorkerUnavailable)?;
    let (before, after) = text
        .rsplit_once(") ")
        .ok_or(EngineError::WorkerUnavailable)?;
    if !before.starts_with(&format!("{pid} (")) {
        return Err(EngineError::WorkerUnavailable);
    }
    let fields: Vec<_> = after.split_whitespace().collect();
    if fields.len() < 50 || fields[0].len() != 1 {
        return Err(EngineError::WorkerUnavailable);
    }
    for field in &fields[1..] {
        if field.parse::<i128>().is_err() {
            return Err(EngineError::WorkerUnavailable);
        }
    }
    fields[19]
        .parse()
        .map_err(|_| EngineError::WorkerUnavailable)
}

pub(super) fn publish_key(directory: &File) -> Result<()> {
    if unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            c".engine.key.pending".as_ptr(),
            directory.as_raw_fd(),
            c"engine.key".as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(EngineError::StorageFailure);
    }
    Ok(())
}
pub(super) fn verify_descriptors(path: &Path, _root: &File, _jobs: &File) -> Result<()> {
    // Never open/close extra database fds: POSIX locks are process-wide.
    for entry in std::fs::read_dir("/proc/self/fd").map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        if target.starts_with(path) {
            let Some(fd) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<i32>().ok())
            else {
                continue;
            };
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags == -1 || flags & libc::FD_CLOEXEC == 0 {
                return Err(EngineError::UnsafePath);
            }
        }
    }
    Ok(())
}
pub(super) fn validate_storage(_root: &File) -> Result<()> {
    Ok(())
}
pub(super) fn prepare_spawn() -> Result<()> {
    Ok(())
}
pub(super) fn sync(file: &File) -> Result<()> {
    file.sync_all().map_err(io_error)
}
pub(super) unsafe fn pre_exec() -> std::io::Result<()> {
    if unsafe { libc::setsid() } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    unsafe {
        libc::umask(0o077);
    }
    if unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    } == -1
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
pub(super) fn descriptor_count() -> Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir("/proc/self/fd").map_err(io_error)? {
        entry.map_err(io_error)?;
        count += 1;
        if count > 262_144 {
            return Err(EngineError::QuotaExceeded);
        }
    }
    Ok(count)
}
