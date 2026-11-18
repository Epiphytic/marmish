//! Ownership and termios restoration for the foreground terminal.

use super::{Error, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;

const TTY: Error = Error::Invalid("terminal_tty");
const FOREGROUND: Error = Error::Invalid("terminal_foreground");
const GEOMETRY: Error = Error::Invalid("terminal_geometry");
const RESTORE: Error = Error::Invalid("terminal_termios_restore");

pub(super) struct Tty {
    file: File,
    saved: libc::termios,
    needs_restore: bool,
}

impl Tty {
    pub(super) fn open() -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open("/dev/tty")
            .map_err(|_| TTY)?;
        check_terminal_identity(file.as_raw_fd())?;
        let saved = get_termios(file.as_raw_fd())?;
        let tty = Self {
            file,
            saved,
            needs_restore: false,
        };
        tty.geometry()?;
        Ok(tty)
    }

    pub(super) fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    pub(super) fn geometry(&self) -> Result<(u16, u16)> {
        self.foreground()?;
        let mut size = MaybeUninit::<libc::winsize>::zeroed();
        if unsafe { libc::ioctl(self.fd(), libc::TIOCGWINSZ, size.as_mut_ptr()) } != 0 {
            return Err(GEOMETRY);
        }
        let size = unsafe { size.assume_init() };
        if !(2..=250).contains(&size.ws_row) || !(2..=500).contains(&size.ws_col) {
            return Err(GEOMETRY);
        }
        Ok((size.ws_row, size.ws_col))
    }

    pub(super) fn foreground(&self) -> Result<()> {
        let group = unsafe { libc::getpgrp() };
        if group > 0 && unsafe { libc::tcgetpgrp(self.fd()) } == group {
            Ok(())
        } else {
            Err(FOREGROUND)
        }
    }

    pub(super) fn raw(&mut self) -> Result<()> {
        self.foreground()?;
        let mut termios = self.saved;
        unsafe { libc::cfmakeraw(&mut termios) };
        termios.c_iflag &= !(libc::IXON | libc::IXOFF);
        termios.c_cc[libc::VMIN] = 1;
        termios.c_cc[libc::VTIME] = 0;
        self.needs_restore = true;
        set_termios(self.fd(), &termios)
    }

    pub(super) fn restore(&mut self) -> Result<()> {
        if !self.needs_restore {
            return Ok(());
        }
        self.foreground()?;
        set_termios(self.fd(), &self.saved).map_err(|_| RESTORE)?;
        let actual = get_termios(self.fd()).map_err(|_| RESTORE)?;
        if !termios_equal(&actual, &self.saved) {
            return Err(RESTORE);
        }
        self.needs_restore = false;
        Ok(())
    }

    pub(super) fn read(&mut self, bytes: &mut [u8]) -> Result<Option<usize>> {
        self.foreground()?;
        nonblocking(self.file.read(bytes))
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> Result<Option<usize>> {
        self.foreground()?;
        nonblocking(self.file.write(bytes))
    }
}

impl Drop for Tty {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn nonblocking(result: std::io::Result<usize>) -> Result<Option<usize>> {
    match result {
        Ok(n) => Ok(Some(n)),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) =>
        {
            Ok(None)
        }
        Err(_) => Err(TTY),
    }
}

fn check_terminal_identity(alias: RawFd) -> Result<()> {
    let stdin = libc::STDIN_FILENO;
    let stdout = libc::STDOUT_FILENO;
    let input = terminal_stat(stdin)?;
    let output = terminal_stat(stdout)?;
    if (input.st_dev, input.st_ino) != (output.st_dev, output.st_ino) {
        return Err(TTY);
    }

    let group = unsafe { libc::getpgrp() };
    if group <= 0
        || [stdin, stdout, alias]
            .into_iter()
            .any(|fd| unsafe { libc::tcgetpgrp(fd) } != group)
    {
        return Err(FOREGROUND);
    }

    #[cfg(target_os = "linux")]
    {
        let session = unsafe { libc::getsid(0) };
        if session <= 0
            || [stdin, stdout, alias]
                .into_iter()
                .any(|fd| unsafe { libc::tcgetsid(fd) } != session)
        {
            return Err(FOREGROUND);
        }
        let mut device = 0u32;
        if unsafe { libc::ioctl(alias, libc::TIOCGDEV, &mut device) } != 0
            || u64::from(device) != input.st_rdev
        {
            return Err(TTY);
        }
    }
    Ok(())
}

fn terminal_stat(fd: RawFd) -> Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::isatty(fd) } != 1 || unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(TTY);
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFCHR {
        return Err(TTY);
    }
    Ok(stat)
}

fn get_termios(fd: RawFd) -> Result<libc::termios> {
    let mut termios = MaybeUninit::zeroed();
    if unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } != 0 {
        return Err(TTY);
    }
    Ok(unsafe { termios.assume_init() })
}

fn set_termios(fd: RawFd, termios: &libc::termios) -> Result<()> {
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, termios) } != 0 {
        return Err(TTY);
    }
    Ok(())
}

fn termios_equal(a: &libc::termios, b: &libc::termios) -> bool {
    #[cfg(target_os = "linux")]
    if a.c_line != b.c_line {
        return false;
    }
    a.c_iflag == b.c_iflag
        && a.c_oflag == b.c_oflag
        && a.c_cflag == b.c_cflag
        && a.c_lflag == b.c_lflag
        && a.c_cc == b.c_cc
        && unsafe { libc::cfgetispeed(a) == libc::cfgetispeed(b) }
        && unsafe { libc::cfgetospeed(a) == libc::cfgetospeed(b) }
}
