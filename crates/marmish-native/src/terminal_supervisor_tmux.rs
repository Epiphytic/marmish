//! Bounded management of one private tmux server per persistent shell.
use super::{CLEANUP, SlotId, UNAVAILABLE, backend, store::Slot};
use crate::identity;
use crate::{Error, Result};
use std::fs::{self, DirBuilder, File};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TMUX: Error = Error::Invalid("terminal_tmux_unavailable");
const SOCKET: Error = Error::Invalid("terminal_socket");
const VERSION: Error = Error::Invalid("terminal_tmux_version");
const MAX_OUTPUT: usize = 4096;
const TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(1);
const CLIENT_POLL: Duration = Duration::from_millis(10);
pub(super) const SESSION: &str = "shell";

pub(super) struct Manager {
    pub tmux: PathBuf,
    pub(super) root: PathBuf,
    pub(super) uid: libc::uid_t,
    user: backend::paths::User,
    pending: Option<Child>,
    reaper: std::sync::mpsc::SyncSender<backend::ChildState>,
    pub(super) profile: Option<std::sync::Arc<crate::identity::ProfileLease>>,
}

#[derive(Clone, Copy)]
pub(super) struct Inventory {
    pub session: u32,
    pub pid: i32,
    pub directory: (u64, u64),
    pub socket: (u64, u64),
}

#[derive(Clone, Copy)]
pub(super) enum ClientChange {
    Departing,
    Arriving,
}

impl Manager {
    pub fn new(tmux: &Path, profile: &Path) -> Result<Self> {
        let uid = backend::paths::checked_uid()?;
        let tmux = backend::paths::tmux(tmux, uid).map_err(|_| TMUX)?;
        let user = backend::paths::user(uid)?;
        backend::paths::tmux(&user.shell, uid)?;
        let home = user.home.canonicalize().map_err(|_| TMUX)?;
        if !home.is_dir() || !safe_shell_option(&user.shell) {
            return Err(TMUX);
        }
        let root = profile.join("t");
        validate_length(&root.join("0".repeat(32)).join("s"))?;
        identity::check_dir(profile)?;
        match DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => File::open(profile)?.sync_all()?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        check_directory(&root)?;
        let user = backend::paths::User { home, ..user };
        let mut manager = Self {
            tmux,
            root,
            uid,
            user,
            pending: None,
            reaper: backend::reserve_reaper()?,
            profile: None,
        };
        let mut command = manager.command()?;
        command.arg("-V");
        version(&manager.run(command)?).ok_or(VERSION)?;
        Ok(manager)
    }

    pub fn socket(&self, slot: SlotId) -> PathBuf {
        self.root.join(hex::encode(slot.0)).join("s")
    }

    pub fn create(&mut self, slot: Slot, rows: u16, cols: u16) -> Result<Inventory> {
        self.create_until(slot, rows, cols, Instant::now() + TIMEOUT * 2, || Ok(()))
    }

    pub fn create_until(
        &mut self,
        slot: Slot,
        rows: u16,
        cols: u16,
        deadline: Instant,
        admit: impl FnOnce() -> Result<()>,
    ) -> Result<Inventory> {
        if Instant::now() >= deadline {
            return Err(CLEANUP);
        }
        let socket = self.socket(slot.id);
        let directory = socket.parent().ok_or(SOCKET)?;
        check_directory(&self.root)?;
        DirBuilder::new().mode(0o700).create(directory)?;
        File::open(&self.root)?.sync_all()?;
        check_directory(directory)?;
        let mut command = self.command()?;
        command
            .arg("-S")
            .arg(&socket)
            .args([
                "-f",
                "/dev/null",
                "start-server",
                ";",
                "set-option",
                "-g",
                "status",
                "off",
                ";",
                "set-window-option",
                "-g",
                "window-size",
                "latest",
                ";",
                "set-option",
                "-s",
                "set-clipboard",
                "off",
                ";",
                "set-window-option",
                "-g",
                "allow-passthrough",
                "off",
                ";",
                "set-option",
                "-g",
                "history-limit",
                "2000",
                ";",
                "set-option",
                "-g",
                "default-shell",
            ])
            .arg(&self.user.shell)
            .args([
                ";",
                "new-session",
                "-d",
                "-s",
                SESSION,
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
                ";",
                "set-option",
                "-t",
                "$0",
                "@marmish-proof",
                &hex::encode(slot.marker),
            ]);
        if let Err(error) = admit() {
            // Only this owner created the private directory, and no native
            // process was started. A removal failure leaves it quarantined.
            if fs::remove_dir(directory).is_ok() {
                File::open(&self.root)?.sync_all()?;
            }
            return Err(error);
        }
        self.run_until(command, deadline.min(Instant::now() + TIMEOUT))?;
        self.inventory_until(slot, None, deadline)
    }

    pub fn inventory(&mut self, slot: Slot, native_id: Option<u32>) -> Result<Inventory> {
        self.inventory_until(slot, native_id, Instant::now() + TIMEOUT)
    }

    pub fn inventory_until(
        &mut self,
        slot: Slot,
        native_id: Option<u32>,
        deadline: Instant,
    ) -> Result<Inventory> {
        let socket = self.socket(slot.id);
        let parent = socket.parent().ok_or(SOCKET)?;
        let directory = directory_identity(parent)?;
        let before = socket_identity(&socket, self.uid)?;
        if let Some(proof) = slot.proof
            && (proof.directory != directory || proof.socket != before)
        {
            return Err(UNAVAILABLE);
        }
        let mut command = self.control(slot.id)?;
        command.args([
            "list-sessions",
            "-F",
            "#{session_name}|#{session_id}|#{@marmish-proof}|#{pid}",
        ]);
        let raw = self.run_until(command, deadline.min(Instant::now() + TIMEOUT))?;
        let (session, pid) = parse_inventory(&raw, slot.marker).ok_or(UNAVAILABLE)?;
        if native_id.is_some_and(|expected| expected != session)
            || slot.proof.is_some_and(|proof| proof.pid != pid)
            || socket_identity(&socket, self.uid)? != before
            || directory_identity(parent)? != directory
        {
            return Err(UNAVAILABLE);
        }
        Ok(Inventory {
            session,
            pid,
            directory,
            socket: before,
        })
    }

    fn client_count(
        &mut self,
        slot: SlotId,
        inventory: Inventory,
        deadline: Instant,
    ) -> Result<usize> {
        let mut command = self.verified_control(slot, inventory)?;
        command.args(["list-clients", "-F", "1"]);
        let raw = self.run_until(command, deadline)?;
        if raw.chunks(2).any(|record| record != b"1\n") {
            return Err(UNAVAILABLE);
        }
        Ok(raw.len() / 2)
    }

    pub fn wait_clients(
        &mut self,
        slot: SlotId,
        inventory: Inventory,
        expected: usize,
        change: ClientChange,
    ) -> Result<()> {
        self.wait_clients_until(
            slot,
            inventory,
            expected,
            change,
            Instant::now() + CLIENT_TIMEOUT,
        )
    }

    pub fn wait_clients_until(
        &mut self,
        slot: SlotId,
        inventory: Inventory,
        expected: usize,
        change: ClientChange,
        deadline: Instant,
    ) -> Result<()> {
        let deadline = deadline.min(Instant::now() + CLIENT_TIMEOUT);
        loop {
            if Instant::now() >= deadline {
                return Err(CLEANUP);
            }
            let count = self.client_count(slot, inventory, deadline)?;
            if count == expected {
                return Ok(());
            }
            let moving_toward_expected = match change {
                ClientChange::Departing => count > expected,
                ClientChange::Arriving => count < expected,
            };
            if !moving_toward_expected {
                return Err(CLEANUP);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(CLIENT_POLL.min(remaining));
        }
    }

    pub fn terminate(&mut self, slot: SlotId, inventory: Inventory) -> Result<()> {
        let socket = self.socket(slot);
        let until = Instant::now() + TIMEOUT;
        self.kill_session(slot, inventory, until)?;
        loop {
            match fs::symlink_metadata(&socket) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Ok(_) if socket_identity(&socket, self.uid)? == inventory.socket => (),
                _ => return Err(CLEANUP),
            }
            if connection_refused(&socket)? {
                if socket_identity(&socket, self.uid)? != inventory.socket {
                    return Err(CLEANUP);
                }
                fs::remove_file(&socket)?;
                break;
            }
            if Instant::now() >= until {
                return Err(CLEANUP);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::remove_dir(socket.parent().ok_or(SOCKET)?)?;
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }

    pub fn kill_session(
        &mut self,
        slot: SlotId,
        inventory: Inventory,
        until: Instant,
    ) -> Result<()> {
        let target = format!("${}", inventory.session);
        let mut command = self.verified_control(slot, inventory)?;
        command.args(["kill-session", "-t", &target]);
        self.run_until(command, until.min(Instant::now() + TIMEOUT))?;
        Ok(())
    }

    pub(super) fn control(&self, slot: SlotId) -> Result<Command> {
        let path = self.socket(slot);
        backend::paths::socket(&path, self.uid)?;
        let mut command = self.command()?;
        command
            .args(["-N", "-S"])
            .arg(path)
            .args(["-f", "/dev/null"]);
        Ok(command)
    }

    fn verified_control(&self, slot: SlotId, inventory: Inventory) -> Result<Command> {
        let socket = self.socket(slot);
        if socket_identity(&socket, self.uid)? != inventory.socket
            || directory_identity(socket.parent().ok_or(SOCKET)?)? != inventory.directory
        {
            return Err(UNAVAILABLE);
        }
        self.control(slot)
    }

    fn command(&self) -> Result<Command> {
        if backend::paths::tmux(&self.tmux, self.uid)? != self.tmux {
            return Err(TMUX);
        }
        let locale = if cfg!(target_os = "macos") {
            "en_US.UTF-8"
        } else {
            "C.UTF-8"
        };
        let mut command = Command::new(&self.tmux);
        command
            .arg("-u")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.user.home)
            .env("USER", &self.user.name)
            .env("SHELL", &self.user.shell)
            .env("TERM", "xterm-256color")
            .env("LC_ALL", locale)
            .current_dir(&self.user.home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        Ok(command)
    }

    pub(super) fn run(&mut self, command: Command) -> Result<Vec<u8>> {
        self.run_until(command, Instant::now() + TIMEOUT)
    }

    fn run_until(&mut self, mut command: Command, deadline: Instant) -> Result<Vec<u8>> {
        if let Some(child) = &mut self.pending {
            if child.try_wait()?.is_none() {
                return Err(CLEANUP);
            }
            self.pending = None;
        }
        if Instant::now() >= deadline {
            return Err(CLEANUP);
        }
        self.pending = Some(command.spawn().map_err(|_| TMUX)?);
        let child = self.pending.as_mut().ok_or(TMUX)?;
        let result = collect(child, deadline);
        if result.is_err() && !matches!(child.try_wait(), Ok(Some(_))) {
            let _ = child.kill();
        }
        if matches!(child.try_wait(), Ok(Some(_))) {
            self.pending = None;
        }
        result
    }

    pub(super) fn drain_pending(&mut self) -> bool {
        let Some(child) = &mut self.pending else {
            return true;
        };
        let _ = child.kill();
        if !matches!(child.try_wait(), Ok(Some(_))) {
            return false;
        }
        self.pending = None;
        true
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if let Some(mut child) = self.pending.take() {
            let _ = child.kill();
            if !matches!(child.try_wait(), Ok(Some(_))) {
                // Retain the exact child while it is being reaped. The owner
                // can shut down even if the kernel delays its waitable state.
                let mut child = backend::ChildState::new(child);
                child._profile = self.profile.take();
                if self.reaper.try_send(child).is_err() {
                    std::process::abort();
                }
            }
        }
    }
}

fn collect(child: &mut Child, until: Instant) -> Result<Vec<u8>> {
    let mut reader = child.stdout.take().ok_or(TMUX)?;
    backend::os::nonblocking(reader.as_raw_fd())?;
    let mut output = Vec::new();
    let mut buffer = [0; 512];
    loop {
        if Instant::now() >= until {
            return Err(UNAVAILABLE);
        }
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) if output.len() + count <= MAX_OUTPUT => {
                output.extend_from_slice(&buffer[..count])
            }
            Ok(_) => return Err(UNAVAILABLE),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let mut fds = [backend::poll_descriptor(reader.as_raw_fd(), libc::POLLIN)];
                backend::poll_fds(&mut fds, until)?;
            }
            Err(_) => return Err(UNAVAILABLE),
        }
    }
    while Instant::now() < until {
        if let Some(status) = child.try_wait()? {
            return status.success().then_some(output).ok_or(UNAVAILABLE);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(UNAVAILABLE)
}

pub(super) fn check_directory(path: &Path) -> Result<()> {
    identity::check_dir(path)?;
    if path.canonicalize()? != path {
        return Err(SOCKET);
    }
    for ancestor in path.ancestors().skip(1) {
        marmish_engine::validate_executable_ancestor(ancestor).map_err(|_| SOCKET)?;
    }
    Ok(())
}

fn directory_identity(path: &Path) -> Result<(u64, u64)> {
    check_directory(path)?;
    let metadata = fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

pub(super) fn connection_refused(path: &Path) -> Result<bool> {
    validate_length(path)?;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: a zeroed sockaddr_un represents an empty local address. Every
    // populated field below is initialized before passing it to connect.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (output, input) in address.sun_path.iter_mut().zip(bytes) {
        *output = *input as libc::c_char;
    }
    let length =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = length as u8;
    }
    #[cfg(target_os = "linux")]
    let flags = libc::SOCK_STREAM | libc::SOCK_CLOEXEC;
    #[cfg(target_os = "macos")]
    let flags = libc::SOCK_STREAM;
    // SAFETY: socket creates a new, initially unconnected local descriptor.
    let fd = unsafe { libc::socket(libc::AF_UNIX, flags, 0) };
    if fd < 0 {
        return Err(CLEANUP);
    }
    // SAFETY: successful socket returned an owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    #[cfg(target_os = "macos")]
    {
        // SAFETY: this only changes descriptor flags on the owned socket.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(CLEANUP);
        }
    }
    backend::os::nonblocking(fd.as_raw_fd())?;
    // SAFETY: the address is populated through its terminating zero and kept
    // alive for the duration of connect. The socket is nonblocking.
    if unsafe { libc::connect(fd.as_raw_fd(), (&raw const address).cast(), length) } == 0 {
        return Ok(false);
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(libc::ECONNREFUSED) => Ok(true),
        Some(libc::EAGAIN | libc::EINPROGRESS | libc::EINTR) => Ok(false),
        _ => Err(CLEANUP),
    }
}

fn socket_identity(path: &Path, uid: libc::uid_t) -> Result<(u64, u64)> {
    backend::paths::socket(path, uid).map_err(|_| UNAVAILABLE)?;
    let metadata = fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

pub(super) fn validate_length(path: &Path) -> Result<()> {
    if path.as_os_str().as_encoded_bytes().len() > 103 {
        return Err(Error::Invalid("terminal_socket_path_length"));
    }
    Ok(())
}

fn safe_shell_option(path: &Path) -> bool {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/_.+-".contains(byte))
}

pub(super) fn version(raw: &[u8]) -> Option<(u16, u16)> {
    let text = std::str::from_utf8(raw)
        .ok()?
        .strip_prefix("tmux ")?
        .strip_suffix('\n')?;
    let (major, rest) = text.split_once('.')?;
    let minor = rest.trim_end_matches(|c: char| c.is_ascii_lowercase());
    if rest.len() - minor.len() > 1 || rest.len() > 4 || major.len() > 3 {
        return None;
    }
    let version = (number(major)?, number(minor)?);
    (version >= (3, 3)).then_some(version)
}

fn number<T: std::str::FromStr>(digits: &str) -> Option<T> {
    if !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    digits.parse().ok()
}

pub(super) fn parse_inventory(raw: &[u8], marker: [u8; 16]) -> Option<(u32, i32)> {
    let raw = std::str::from_utf8(raw)
        .ok()?
        .strip_prefix("shell|$")?
        .strip_suffix('\n')?;
    let (id, rest) = raw.split_once('|')?;
    let (proof, pid) = rest.split_once('|')?;
    if proof != hex::encode(marker) {
        return None;
    }
    let pid: i32 = number(pid)?;
    (pid > 0).then_some((number(id)?, pid))
}
