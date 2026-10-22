//! Offline MDK compatibility adapter. No network-capable production runner exists.
//! Synthetic plans and parser fixtures establish neither provenance nor authentication.
#![cfg(unix)]

use marmish_core::{Limits, RelaySet, Retention};
use std::ffi::OsString;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use thiserror::Error;

const OUTPUT_LIMIT: usize = Limits::NDJSON_BYTES;
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("an explicit absolute executable file is required")]
    InvalidExecutable,
    #[error("configured executable is unavailable")]
    MissingExecutable,
    #[error("subprocess deadline exceeded")]
    Timeout,
    #[error("response exceeds the fixed size limit")]
    OutputLimit,
    #[error("response is malformed or truncated")]
    InvalidResponse,
    #[error("subprocess failed")]
    SubprocessFailure,
    #[error("strict configured-relay egress is unverified; operation cannot start")]
    LiveEgressUnverified,
}

/// Bounded observation only: no source provenance or capability approval.
#[derive(Debug)]
pub struct VersionObservation {
    pub stdout: String,
    pub stderr: String,
}

pub struct RealWn {
    executable: PathBuf,
}

impl RealWn {
    pub fn new(executable: PathBuf) -> Result<Self, AdapterError> {
        if !executable.is_absolute() {
            return Err(AdapterError::InvalidExecutable);
        }
        let executable = executable
            .canonicalize()
            .map_err(|_| AdapterError::MissingExecutable)?;
        if !executable.is_file() {
            return Err(AdapterError::InvalidExecutable);
        }
        Ok(Self { executable })
    }

    /// The sole production subprocess operation; inherits no environment.
    pub fn version(&self) -> Result<VersionObservation, AdapterError> {
        let output = observe(
            &self.executable,
            &[OsString::from("--version")],
            PROBE_TIMEOUT,
        )?;
        Ok(VersionObservation {
            stdout: version_text(output.0)?,
            stderr: version_text(output.1)?,
        })
    }
}

fn version_text(bytes: Vec<u8>) -> Result<String, AdapterError> {
    let value = String::from_utf8(bytes).map_err(|_| AdapterError::InvalidResponse)?;
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        return Err(AdapterError::InvalidResponse);
    }
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    OfflineVersion,
    NetworkCapable,
}

/// Inert CLI documentation. Cannot carry secrets or message payloads, or be executed.
#[derive(Debug)]
pub struct CommandPlan {
    class: CommandClass,
    arguments: Vec<OsString>,
    relays: Vec<String>,
}

impl CommandPlan {
    pub fn version() -> Self {
        Self {
            class: CommandClass::OfflineVersion,
            arguments: vec!["--version".into()],
            relays: vec![],
        }
    }

    pub fn daemon_start(state: PathBuf, relays: &RelaySet) -> Self {
        let joined = relays.as_slice().join(",");
        Self::network(
            state,
            relays,
            vec![
                "daemon".into(),
                "start".into(),
                "--discovery-relays".into(),
                joined.clone().into(),
                "--default-account-relays".into(),
                joined.into(),
            ],
        )
    }

    pub fn create_room(
        state: PathBuf,
        relays: &RelaySet,
        name: String,
        members: &[String],
        retention: Retention,
    ) -> Self {
        let mut args = vec!["groups".into(), "create".into(), name.into()];
        args.extend(members.iter().map(OsString::from));
        if let Some(seconds) = retention.seconds() {
            args.extend(["--retention".into(), format!("{seconds}s").into()]);
        }
        Self::network(state, relays, args)
    }

    pub fn sweep_expired(state: PathBuf, relays: &RelaySet) -> Self {
        Self::network(
            state,
            relays,
            vec!["messages".into(), "sweep-expired".into()],
        )
    }

    fn network(state: PathBuf, relays: &RelaySet, tail: Vec<OsString>) -> Self {
        let mut arguments = vec!["--home".into(), state.into_os_string(), "--json".into()];
        arguments.extend(tail);
        Self {
            class: CommandClass::NetworkCapable,
            arguments,
            relays: relays.as_slice().to_vec(),
        }
    }

    pub fn class(&self) -> CommandClass {
        self.class
    }
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }
    pub fn relays(&self) -> &[String] {
        &self.relays
    }

    pub fn require_live_egress(&self) -> Result<(), AdapterError> {
        match self.class {
            CommandClass::OfflineVersion => Ok(()),
            CommandClass::NetworkCapable => Err(AdapterError::LiveEgressUnverified),
        }
    }
}

// Keep the process-group leader unreaped until after killpg. Its reserved PID
// prevents cleanup from ever signalling an unrelated, reused process group.
// A descendant that deliberately calls setsid/setpgid can escape this cleanup;
// this probe is not a sandbox for an untrusted executable. Pipes remain bounded
// by the deadline, but escaped processes require external containment.
struct ProbeChild {
    child: Child,
    reaped: bool,
}

impl ProbeChild {
    fn has_exited(&self) -> Result<bool, AdapterError> {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == -1 {
            return if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                Ok(false)
            } else {
                Err(AdapterError::SubprocessFailure)
            };
        }
        Ok(unsafe { info.si_pid() } != 0)
    }

    fn finish(&mut self) -> Result<std::process::ExitStatus, AdapterError> {
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let status = self
            .child
            .wait()
            .map_err(|_| AdapterError::SubprocessFailure)?;
        self.reaped = true;
        Ok(status)
    }
}

impl Drop for ProbeChild {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.finish();
        }
    }
}

fn observe(
    executable: &std::path::Path,
    args: &[OsString],
    timeout: Duration,
) -> Result<(Vec<u8>, Vec<u8>), AdapterError> {
    let child = Command::new(executable)
        .args(args)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|_| AdapterError::SubprocessFailure)?;
    let mut child = ProbeChild {
        child,
        reaped: false,
    };
    let mut stdout = child
        .child
        .stdout
        .take()
        .ok_or(AdapterError::SubprocessFailure)?;
    let mut stderr = child
        .child
        .stderr
        .take()
        .ok_or(AdapterError::SubprocessFailure)?;
    set_nonblocking(&stdout)?;
    set_nonblocking(&stderr)?;
    let start = Instant::now();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_eof = false;
    let mut err_eof = false;
    loop {
        if start.elapsed() >= timeout {
            return Err(AdapterError::Timeout);
        }
        if !out_eof {
            out_eof = drain(&mut stdout, &mut out)?;
        }
        if !err_eof {
            err_eof = drain(&mut stderr, &mut err)?;
        }
        if out_eof && err_eof && child.has_exited()? {
            return if child.finish()?.success() {
                Ok((out, err))
            } else {
                Err(AdapterError::SubprocessFailure)
            };
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn set_nonblocking(file: &impl AsRawFd) -> Result<(), AdapterError> {
    let fd = file.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return Err(AdapterError::SubprocessFailure);
        }
    }
    Ok(())
}

// One read per pipe per tick keeps floods on either channel fair and bounded.
fn drain(file: &mut impl Read, output: &mut Vec<u8>) -> Result<bool, AdapterError> {
    let mut chunk = [0u8; 4096];
    match file.read(&mut chunk) {
        Ok(0) => Ok(true),
        Ok(n) => {
            if output.len() + n > OUTPUT_LIMIT {
                return Err(AdapterError::OutputLimit);
            }
            output.extend_from_slice(&chunk[..n]);
            Ok(false)
        }
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        Err(_) => Err(AdapterError::SubprocessFailure),
    }
}

/// Bounded structural JSON decoding, without assumptions about authenticated senders.
#[cfg(test)]
pub(crate) fn parse_json(bytes: &[u8]) -> Result<serde_json::Value, AdapterError> {
    if bytes.len() > OUTPUT_LIMIT {
        return Err(AdapterError::OutputLimit);
    }
    serde_json::from_slice(bytes).map_err(|_| AdapterError::InvalidResponse)
}

/// Synthetic subscription contract: at most 200 newline-terminated records.
/// This is not a cursor, catch-up mechanism, or authentication validator.
#[cfg(test)]
pub(crate) fn parse_ndjson(
    mut reader: impl std::io::BufRead,
) -> Result<Vec<serde_json::Value>, AdapterError> {
    let mut records = Vec::new();
    let mut line = Vec::new();
    loop {
        let buffer = reader
            .fill_buf()
            .map_err(|_| AdapterError::InvalidResponse)?;
        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(records)
            } else {
                Err(AdapterError::InvalidResponse)
            };
        }
        let end = buffer.iter().position(|b| *b == b'\n');
        let count = end.map_or(buffer.len(), |n| n + 1);
        if line.len() + count > OUTPUT_LIMIT {
            return Err(AdapterError::OutputLimit);
        }
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            if records.len() >= 200 {
                return Err(AdapterError::OutputLimit);
            }
            records.push(parse_json(&line)?);
            line.clear();
        }
    }
}

#[cfg(test)]
mod tests;
