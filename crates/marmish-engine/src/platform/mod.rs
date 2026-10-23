//! Host-only primitives. No platform identity is interpreted on another host.
use crate::launcher::{io_error, parse_job};
use crate::model::*;
use crate::store::{BootTime, ProcessIdentity};
use std::ffi::{CStr, OsStr, OsString};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
use linux as native;
#[cfg(target_os = "macos")]
use macos as native;

pub(crate) fn boot_time() -> Result<BootTime> {
    native::boot_time()
}
pub(crate) fn process_identity(pid: i32) -> Result<ProcessIdentity> {
    native::process_identity(pid)
}
pub(crate) fn lock_exclusive(file: &File) -> Result<()> {
    native::lock_exclusive(file)
}
pub(crate) fn lock_is_busy(file: &File) -> Result<bool> {
    native::lock_is_busy(file)
}
pub(crate) fn publish_key(directory: &File) -> Result<()> {
    native::publish_key(directory)
}
pub(crate) fn verify_descriptors(path: &Path, root: &File, jobs: &File) -> Result<()> {
    native::verify_descriptors(path, root, jobs)
}
pub(crate) fn validate_storage(root: &File) -> Result<()> {
    native::validate_storage(root)
}
pub(crate) fn prepare_spawn() -> Result<()> {
    native::prepare_spawn()
}
pub(crate) fn cleanup_descriptor_budget(jobs: usize) -> Result<()> {
    let count = native::descriptor_count()?;
    let needed = jobs
        .checked_mul(2)
        .and_then(|n| n.checked_add(count))
        .and_then(|n| n.checked_add(64))
        .ok_or(EngineError::QuotaExceeded)?;
    let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(EngineError::WorkerUnavailable);
    }
    if needed as u128 > limit.rlim_cur as u128 {
        return Err(EngineError::QuotaExceeded);
    }
    #[cfg(target_os = "macos")]
    if needed > 65_536 {
        return Err(EngineError::QuotaExceeded);
    }
    Ok(())
}
pub(crate) fn sync(file: &File) -> Result<()> {
    native::sync(file)
}
/// Called only by Command::pre_exec; native implementations allocate nothing.
pub(crate) unsafe fn pre_exec() -> std::io::Result<()> {
    unsafe { native::pre_exec() }
}

pub(crate) fn state_path(path: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // Canonicalize only the trusted existing parent, never the private leaf.
        let name = path.file_name().ok_or(EngineError::UnsafePath)?;
        if !path.is_absolute() {
            return Err(EngineError::UnsafePath);
        }
        let parent = path
            .parent()
            .ok_or(EngineError::UnsafePath)?
            .canonicalize()
            .map_err(io_error)?;
        Ok(parent.join(name))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(path.to_owned())
    }
}

/// A fork-independent directory offset, bounded to the existing traversal cap.
/// fdopendir takes ownership only on success; closedir closes exactly that fd.
pub(crate) struct DirectoryEntries {
    pointer: *mut libc::DIR,
    visits: usize,
    failed: bool,
}
impl DirectoryEntries {
    pub(crate) fn open(directory: &File) -> Result<Self> {
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let pointer = unsafe { libc::fdopendir(fd) };
        if pointer.is_null() {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let _ = file.into_raw_fd();
        Ok(Self {
            pointer,
            visits: 0,
            failed: false,
        })
    }
}
impl Iterator for DirectoryEntries {
    type Item = Result<OsString>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        loop {
            unsafe {
                *errno_pointer() = 0;
            }
            let entry = unsafe { libc::readdir(self.pointer) };
            if entry.is_null() {
                self.failed = true;
                return if unsafe { *errno_pointer() } == 0 {
                    None
                } else {
                    Some(Err(EngineError::StorageFailure))
                };
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            self.visits += 1;
            if self.visits > 100_256 {
                self.failed = true;
                return Some(Err(EngineError::QuotaExceeded));
            }
            return Some(Ok(OsString::from_vec(name.to_vec())));
        }
    }
}
impl Drop for DirectoryEntries {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.pointer);
        }
    }
}
unsafe fn errno_pointer() -> *mut libc::c_int {
    #[cfg(target_os = "linux")]
    {
        unsafe { libc::__errno_location() }
    }
    #[cfg(target_os = "macos")]
    {
        unsafe { libc::__error() }
    }
}

/// Checked suspend-inclusive deadline, failing closed on changed boot/clock.
pub(crate) struct Deadline {
    started: BootTime,
    budget_ms: u64,
}
impl Deadline {
    pub(crate) fn after_millis(budget_ms: u64) -> Result<Self> {
        Ok(Self {
            started: boot_time()?,
            budget_ms,
        })
    }
    pub(crate) fn expired(&self) -> Result<bool> {
        let now = boot_time()?;
        if now.boot_id != self.started.boot_id || now.millis < self.started.millis {
            return Err(EngineError::RecoveryUncertain);
        }
        Ok(now.millis - self.started.millis >= self.budget_ms)
    }
}

/// Host-only runner marker, never populated from remote command arguments.
pub(crate) fn execution_marker() -> Result<Option<OsString>> {
    let Some(value) = std::env::var_os("EPIPHYTIC_EXECUTION_ID") else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(b))
    {
        return Err(EngineError::InvalidPolicy);
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_iterations_have_independent_offsets_and_stay_on_held_inode() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("first"), []).unwrap();
        std::fs::write(tmp.path().join("second"), []).unwrap();
        let directory = File::open(tmp.path()).unwrap();
        let mut first = DirectoryEntries::open(&directory).unwrap();
        assert!(first.next().unwrap().is_ok());
        assert_eq!(DirectoryEntries::open(&directory).unwrap().count(), 2);
        assert_eq!(first.count(), 1);
        assert_eq!(DirectoryEntries::open(&directory).unwrap().count(), 2);
    }
    #[test]
    fn clock_and_self_identity_are_available() {
        let before = boot_time().unwrap();
        let identity = process_identity(std::process::id() as i32).unwrap();
        assert_eq!(identity.boot_id, before.boot_id);
        assert!(identity.start_ticks > 0);
        assert!(boot_time().unwrap().millis >= before.millis);
        assert!(process_identity(0).is_err());
        assert!(Deadline::after_millis(0).unwrap().expired().unwrap());
    }
}
