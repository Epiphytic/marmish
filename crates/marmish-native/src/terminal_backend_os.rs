//! Local PTY and single-thread descriptor setup primitives.
use crate::{Error, Result};
use std::ffi::CStr;
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};

pub(super) const DESCRIPTORS: Error = Error::Invalid("terminal_descriptors");
#[cfg(target_os = "linux")]
const INVENTORY: &CStr = c"/proc/self/fd";
#[cfg(target_os = "macos")]
const INVENTORY: &CStr = c"/dev/fd";

pub(super) fn pty(uid: libc::uid_t) -> Result<(File, File)> {
    // SAFETY: the return value is a new descriptor.
    let fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(DESCRIPTORS);
    }
    // SAFETY: posix_openpt just returned the exclusively owned descriptor.
    let master = unsafe { File::from_raw_fd(fd) };
    // SAFETY: both calls operate on the held PTY master.
    if unsafe { libc::grantpt(fd) } != 0 || unsafe { libc::unlockpt(fd) } != 0 {
        return Err(DESCRIPTORS);
    }
    let slave = open_slave(fd, uid)?;
    nonblocking(fd)?;
    Ok((master, slave))
}

#[cfg(target_os = "linux")]
fn open_slave(master: RawFd, uid: libc::uid_t) -> Result<File> {
    // SAFETY: TIOCGPTPEER opens the kernel-associated slave on the live master.
    let fd = unsafe {
        libc::ioctl(
            master,
            libc::TIOCGPTPEER,
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(DESCRIPTORS);
    }
    // SAFETY: the successful ioctl exclusively returned a new descriptor.
    let slave = unsafe { File::from_raw_fd(fd) };
    tty_fd(slave.as_raw_fd(), uid)?;
    Ok(slave)
}

#[cfg(target_os = "macos")]
fn open_slave(master: RawFd, uid: libc::uid_t) -> Result<File> {
    let mut path = [0u8; 128];
    // SAFETY: TIOCPTYGNAME writes at most the fixed 128 bytes to the supplied buffer.
    if unsafe {
        libc::ioctl(
            master,
            libc::c_ulong::from(libc::TIOCPTYGNAME),
            path.as_mut_ptr(),
        )
    } != 0
    {
        return Err(DESCRIPTORS);
    }
    let path = CStr::from_bytes_until_nul(&path).map_err(|_| DESCRIPTORS)?;
    if !path.to_bytes().starts_with(b"/dev/tty") || path.to_bytes()[5..].contains(&b'/') {
        return Err(DESCRIPTORS);
    }
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: path is NUL-terminated and metadata is valid writable storage.
    if unsafe { libc::lstat(path.as_ptr(), metadata.as_mut_ptr()) } != 0 {
        return Err(DESCRIPTORS);
    }
    // SAFETY: successful lstat initialized metadata.
    let expected = unsafe { metadata.assume_init() };
    if expected.st_mode & libc::S_IFMT != libc::S_IFCHR || expected.st_uid != uid {
        return Err(DESCRIPTORS);
    }
    // SAFETY: the pathname was built by the kernel, checked above and is NUL-terminated.
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(DESCRIPTORS);
    }
    // SAFETY: successful open returned this exclusively owned descriptor.
    let slave = unsafe { File::from_raw_fd(fd) };
    let actual = tty_fd(fd, uid)?;
    if (
        expected.st_dev,
        expected.st_ino,
        expected.st_rdev,
        expected.st_mode,
        expected.st_uid,
    ) != (
        actual.st_dev,
        actual.st_ino,
        actual.st_rdev,
        actual.st_mode,
        actual.st_uid,
    ) {
        return Err(DESCRIPTORS);
    }
    Ok(slave)
}

pub(super) fn status_pipe() -> Result<(File, File)> {
    let mut pair = [-1; 2];
    #[cfg(target_os = "linux")]
    // SAFETY: pair has writable storage for both descriptor numbers.
    let status = unsafe { libc::pipe2(pair.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(target_os = "macos")]
    // SAFETY: pair has writable storage for both descriptor numbers.
    let status = unsafe { libc::pipe(pair.as_mut_ptr()) };
    if status != 0 {
        return Err(DESCRIPTORS);
    }
    // SAFETY: the successful pipe call returned two exclusively owned descriptors.
    let (read, write) = unsafe { (File::from_raw_fd(pair[0]), File::from_raw_fd(pair[1])) };
    #[cfg(target_os = "macos")]
    for fd in pair {
        // SAFETY: both pipe descriptors remain live and exclusive to this launch.
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
            return Err(DESCRIPTORS);
        }
    }
    nonblocking(read.as_raw_fd())?;
    Ok((read, write))
}

fn nonblocking(fd: RawFd) -> Result<()> {
    // SAFETY: both calls operate on the held descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
        return Err(DESCRIPTORS);
    }
    Ok(())
}

fn metadata(fd: RawFd) -> Result<libc::stat> {
    let mut value = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat receives a valid output slot.
    if unsafe { libc::fstat(fd, value.as_mut_ptr()) } != 0 {
        return Err(DESCRIPTORS);
    }
    // SAFETY: successful fstat initialized the output.
    Ok(unsafe { value.assume_init() })
}

fn tty_fd(fd: RawFd, uid: libc::uid_t) -> Result<libc::stat> {
    let value = metadata(fd)?;
    // SAFETY: both queries only inspect fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if value.st_mode & libc::S_IFMT != libc::S_IFCHR
        || value.st_uid != uid
        || flags < 0
        || flags & libc::O_ACCMODE != libc::O_RDWR
        || unsafe { libc::isatty(fd) } != 1
    {
        return Err(DESCRIPTORS);
    }
    Ok(value)
}

fn writable_fd(fd: RawFd) -> bool {
    // SAFETY: F_GETFL just queries the descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    flags >= 0 && flags & libc::O_ACCMODE == libc::O_WRONLY
}

pub(super) fn worker_endpoints(uid: libc::uid_t) -> Result<()> {
    tty_fd(0, uid)?;
    let status = metadata(1)?;
    let error = metadata(2)?;
    let null = File::open("/dev/null").map_err(|_| DESCRIPTORS)?;
    let expected = metadata(null.as_raw_fd())?;
    if status.st_mode & libc::S_IFMT != libc::S_IFIFO
        || !writable_fd(1)
        || error.st_mode & libc::S_IFMT != libc::S_IFCHR
        || !writable_fd(2)
        || (error.st_dev, error.st_ino, error.st_rdev)
            != (expected.st_dev, expected.st_ino, expected.st_rdev)
    {
        return Err(DESCRIPTORS);
    }
    Ok(())
}

pub(super) fn reset_signals() -> Result<()> {
    // SAFETY: this runs in the single-thread worker. Every slot is initialized
    // before the relevant syscall; no signal handler uses Rust state.
    unsafe {
        let mut action = std::mem::zeroed::<libc::sigaction>();
        action.sa_sigaction = libc::SIG_DFL;
        if libc::sigemptyset(&mut action.sa_mask) != 0 {
            return Err(DESCRIPTORS);
        }
        for signal in [
            libc::SIGHUP,
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGTERM,
            libc::SIGPIPE,
            libc::SIGCHLD,
            libc::SIGTSTP,
            libc::SIGTTIN,
            libc::SIGTTOU,
            libc::SIGWINCH,
            libc::SIGUSR1,
            libc::SIGUSR2,
            libc::SIGALRM,
        ] {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(DESCRIPTORS);
            }
        }
        if libc::sigprocmask(libc::SIG_SETMASK, &action.sa_mask, std::ptr::null_mut()) != 0 {
            return Err(DESCRIPTORS);
        }
    }
    Ok(())
}

pub(super) fn duplicate_status() -> Result<RawFd> {
    // SAFETY: F_DUPFD_CLOEXEC duplicates the verified pipe atomically.
    let fd = unsafe { libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3) };
    if fd < 0 { Err(DESCRIPTORS) } else { Ok(fd) }
}

pub(super) fn close_unrelated(status: RawFd) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let status = status as u32;
        // SAFETY: these run in the dedicated process and exclude its status fd.
        let close = |first: u32, last: u32| unsafe {
            libc::syscall(libc::SYS_close_range, first, last, 0_u32)
        };
        for (first, last) in [(3, status - 1), (status + 1, u32::MAX)] {
            if first > last || close(first, last) == 0 {
                continue;
            }
            return match io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOSYS | libc::EPERM) => close_inventory(status as RawFd),
                _ => Err(DESCRIPTORS),
            };
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    close_inventory(status)
}

fn errno_location() -> *mut libc::c_int {
    #[cfg(target_os = "linux")]
    // SAFETY: libc exposes the current thread's errno slot.
    unsafe {
        libc::__errno_location()
    }
    #[cfg(target_os = "macos")]
    // SAFETY: libc exposes the current thread's errno slot.
    unsafe {
        libc::__error()
    }
}

pub(super) fn close_inventory(status: RawFd) -> Result<()> {
    // SAFETY: INVENTORY is a static NUL-terminated path.
    let directory = unsafe { libc::opendir(INVENTORY.as_ptr()) };
    if directory.is_null() {
        return Err(DESCRIPTORS);
    }
    let result = inventory_fds(directory, status);
    // SAFETY: opendir returned this single owned directory stream.
    let closed = unsafe { libc::closedir(directory) };
    if closed != 0 {
        return Err(DESCRIPTORS);
    }
    let descriptors = result?;
    for fd in descriptors {
        // SAFETY: exact single-thread inventory captured each inherited descriptor.
        if unsafe { libc::close(fd) } != 0 {
            return Err(DESCRIPTORS);
        }
    }
    Ok(())
}

pub(super) fn inventory_fds(directory: *mut libc::DIR, status: RawFd) -> Result<Vec<RawFd>> {
    // SAFETY: directory is a live unique stream until close_inventory closes it.
    let own = unsafe { libc::dirfd(directory) };
    let (mut found_status, mut found_own) = (false, false);
    let mut descriptors = Vec::new();
    let errno = errno_location();
    loop {
        // SAFETY: this thread owns errno; readdir owns each entry until next read.
        let entry = unsafe {
            *errno = 0;
            libc::readdir(directory)
        };
        if entry.is_null() {
            // SAFETY: inspect the same live errno slot after readdir.
            return if unsafe { *errno } == 0 && found_status && found_own {
                Ok(descriptors)
            } else {
                Err(DESCRIPTORS)
            };
        }
        // SAFETY: libc provides a NUL-terminated d_name within this live entry.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if matches!(name, b"." | b"..") {
            continue;
        }
        let fd = std::str::from_utf8(name)
            .ok()
            .and_then(|value| value.parse::<RawFd>().ok())
            .filter(|value| *value >= 0)
            .ok_or(DESCRIPTORS)?;
        if fd == status {
            found_status = true;
        } else if fd == own {
            found_own = true;
        } else if fd > 2 {
            descriptors.push(fd);
        }
    }
}

pub(super) fn map_slave() -> Result<()> {
    // SAFETY: fd 0 is the checked owned slave; old fds 1 and 2 are expendable.
    if unsafe { libc::dup2(0, 1) } != 1 || unsafe { libc::dup2(0, 2) } != 2 {
        return Err(DESCRIPTORS);
    }
    Ok(())
}

pub(super) fn controlling_terminal() -> Result<()> {
    #[cfg(target_os = "linux")]
    let request = libc::TIOCSCTTY;
    #[cfg(target_os = "macos")]
    let request = libc::c_ulong::from(libc::TIOCSCTTY);
    // SAFETY: only this dedicated worker alters its own session and owned slave.
    if unsafe { libc::setsid() } < 0 || unsafe { libc::ioctl(0, request, 0) } != 0 {
        return Err(DESCRIPTORS);
    }
    Ok(())
}

pub(super) fn resize(fd: RawFd, rows: u16, cols: u16) -> Result<()> {
    if rows == 0 || cols == 0 {
        return Err(Error::Invalid("terminal_size"));
    }
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: fd is the held PTY and size is valid for the duration of ioctl.
    if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) } != 0 {
        return Err(DESCRIPTORS);
    }
    Ok(())
}
