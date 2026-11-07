//! Installed qualified rsync signal edges. Receipts never grant signal authority.
use super::{
    copy_interrupt::Binding,
    edge_fixture, process,
    records::{self, Context, IdentityReady},
    streaming_cases::Endpoint,
    streaming_cleanup::{self, Entry},
    streaming_prerequisites,
    streaming_records::ServerReady,
    transfer_cases,
};
use crate::{Error, Result};
use marmish_stream::wire::Terminal;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ffi::OsString, path::Path, process::ExitStatus, time::Duration};
use tokio::{io::AsyncReadExt, process::Child, task::JoinHandle, time::Instant};
fn remote_operand(account: &str, nonce: &str) -> Result<String> {
    Ok(format!(
        "{}:edge-rsync-{nonce}.destination",
        records::public_destination(account)?
    ))
}

fn bad() -> Error {
    Error::Invalid("installed_edge_rsync")
}

#[derive(Clone, Copy)]
enum Case {
    Interrupt,
    Terminate,
    Hangup,
    GroupInterrupt,
    Timeout,
    Failure,
}
impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Interrupt => "rsync-sigint",
            Self::Terminate => "rsync-sigterm",
            Self::Hangup => "rsync-sighup",
            Self::GroupInterrupt => "rsync-group-sigint",
            Self::Timeout => "rsync-timeout-usr1",
            Self::Failure => "rsync-independent-failure",
        }
    }
    fn signal(self) -> Option<i32> {
        match self {
            Self::Interrupt | Self::GroupInterrupt => Some(libc::SIGINT),
            Self::Terminate => Some(libc::SIGTERM),
            Self::Hangup => Some(libc::SIGHUP),
            _ => None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub case: String,
    pub wall_ns: u64,
    pub binding: Binding,
    pub marker_sha256: String,
    pub fixture_argv_sha256: String,
    pub local_rsync_sha256: String,
    pub remote_rsync_sha256: String,
    pub local_exit: i32,
    pub stderr_sha256: String,
    pub cancellation_confirmed: bool,
    pub remote_terminal: Terminal,
    pub cleanup: streaming_cleanup::StreamStatus,
}
impl Observation {
    pub(super) fn validate(&self) -> Result<()> {
        self.cleanup.validate_idle()?;
        if self.wall_ns == 0
            || self.wall_ns > 240_000_000_000
            || !matches!(self.local_exit, 1..=255)
            || [
                &self.binding.stream_sha256,
                &self.binding.session_sha256,
                &self.binding.open_digest,
                &self.binding.group_sha256,
                &self.marker_sha256,
                &self.fixture_argv_sha256,
                &self.local_rsync_sha256,
                &self.remote_rsync_sha256,
                &self.stderr_sha256,
            ]
            .into_iter()
            .any(|s| !records::hexadecimal(s, 64))
        {
            return Err(bad());
        }
        if self.case == "rsync-timeout-usr1" && self.local_exit != 30 {
            return Err(bad());
        }
        match self.case.as_str() {
            "rsync-sigint" | "rsync-sigterm" | "rsync-sighup" | "rsync-group-sigint"
            | "rsync-timeout-usr1" => {
                if !self.cancellation_confirmed || self.remote_terminal != Terminal::Cancelled {
                    return Err(bad());
                }
            }
            "rsync-independent-failure" => {
                // Preserve a natural helper failure even when rsync subsequently forwards USR1.
                if self.remote_terminal != (Terminal::Exited { code: 23 }) {
                    return Err(bad());
                }
            }
            _ => return Err(bad()),
        }
        Ok(())
    }
}
pub(super) struct Cohort<'a, 'b> {
    pub context: &'a Context,
    pub identities: &'a BTreeMap<String, IdentityReady>,
    pub role: &'a str,
    pub ready: &'a ServerReady,
    pub client: &'a Endpoint<'b>,
    pub server: &'a Endpoint<'b>,
    pub deadline: Instant,
}

/// Only the live, unreaped Child grants PID/PGID authority; never a receipt PID.
type Drain = JoinHandle<std::io::Result<(Vec<u8>, bool)>>;

struct OwnedRsync {
    child: Child,
    stream: String,
    prefix: Vec<u8>,
    stdout: Option<Drain>,
    stderr: Option<Drain>,
}
struct Finished {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
fn stream_line(bytes: &[u8]) -> Result<Option<String>> {
    let label = b"marmish stream: ";
    let Some(offset) = bytes.windows(label.len()).position(|b| b == label) else {
        return Ok(None);
    };
    let value = &bytes[offset + label.len()..];
    if value.len() != 33 || value[32] != b'\n' {
        return Err(bad());
    }
    let id = std::str::from_utf8(&value[..32]).map_err(|_| bad())?;
    if !records::hexadecimal(id, 32) {
        return Err(bad());
    }
    Ok(Some(id.into()))
}
impl OwnedRsync {
    async fn start(
        program: &Path,
        args: &[OsString],
        ep: &Endpoint<'_>,
        deadline: Instant,
    ) -> Result<Self> {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let mut child = process::command(program, args, ep.workspace, ep.environment).spawn()?;
        let out = child.stdout.take().ok_or_else(bad)?;
        let mut err = child.stderr.take().ok_or_else(bad)?;
        let stdout = tokio::spawn(process::drain(out, 65536));
        let parsed = tokio::time::timeout_at(deadline, async {
            let mut prefix = Vec::new();
            let mut line = Vec::new();
            loop {
                let byte = err.read_u8().await?;
                prefix.push(byte);
                line.push(byte);
                if prefix.len() > 8192 {
                    return Err(bad());
                }
                if byte == b'\n' {
                    if let Some(id) = stream_line(&line)? {
                        return Ok((id, prefix));
                    }
                    line.clear();
                }
            }
        })
        .await;
        let (stream, prefix) = match parsed {
            Ok(Ok(value)) => value,
            other => {
                let cleanup = Self::cleanup_child(&mut child).await;
                stdout.abort();
                cleanup?;
                return Err(match other {
                    Err(_) => Error::Timeout,
                    Ok(Err(e)) => e,
                    _ => bad(),
                });
            }
        };
        Ok(Self {
            child,
            stream,
            prefix,
            stdout: Some(stdout),
            stderr: Some(tokio::spawn(process::drain(err, 65536))),
        })
    }
    fn exited(child: &Child) -> Result<bool> {
        let pid = child.id().ok_or_else(bad)?;
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { info.si_pid() } != 0)
    }
    async fn cleanup_child(child: &mut Child) -> Result<ExitStatus> {
        let pid = child.id().ok_or_else(bad)?;
        // WNOWAIT checks the retained Child relationship without releasing the
        // leader PID/PGID fence, even if it has already exited.
        Self::exited(child)?;
        let pgid = i32::try_from(pid).map_err(|_| bad())?;
        cleanup_owned_group(
            || group_signal(pgid, libc::SIGKILL),
            || async {
                tokio::time::timeout(Duration::from_secs(3), child.wait())
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(Into::into)
            },
            || group_signal(pgid, 0),
        )
        .await
    }
    fn signal(&mut self, signal: i32, group: bool) -> Result<()> {
        if !matches!(signal, libc::SIGINT | libc::SIGTERM | libc::SIGHUP)
            || Self::exited(&self.child)?
        {
            return Err(bad());
        }
        let pid = i32::try_from(self.child.id().ok_or_else(bad)?).map_err(|_| bad())?;
        if unsafe { libc::kill(if group { -pid } else { pid }, signal) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    async fn abort(&mut self) -> Result<()> {
        let cleanup = if self.child.id().is_some() {
            reap_owned_group(&mut self.child).await.map(|_| ())
        } else {
            Ok(())
        };
        if let Some(out) = self.stdout.take() {
            out.abort();
        }
        if let Some(err) = self.stderr.take() {
            err.abort();
        }
        cleanup
    }
    async fn finish(&mut self, deadline: Instant) -> Result<Finished> {
        // Retain the unreaped leader while draining adapter descendants. Reap
        // only after group cleanup, so a deadline cannot leak an orphan rsh.
        let observed = wait_unreaped(&mut self.child, deadline).await;
        if let Err(error) = observed {
            self.abort().await?;
            return Err(error);
        }
        let pipes = tokio::time::timeout_at(deadline, async {
            let out = self
                .stdout
                .as_mut()
                .ok_or_else(bad)?
                .await
                .map_err(|_| bad())??;
            let err = self
                .stderr
                .as_mut()
                .ok_or_else(bad)?
                .await
                .map_err(|_| bad())??;
            Ok::<_, Error>((out, err))
        })
        .await;
        let ((stdout, out_over), (stderr, err_over)) = match pipes {
            Ok(Ok(pipes)) => pipes,
            other => {
                self.abort().await?;
                return Err(match other {
                    Err(_) => Error::Timeout,
                    Ok(Err(e)) => e,
                    _ => bad(),
                });
            }
        };
        let status = reap_owned_group(&mut self.child).await?;
        self.stdout = None;
        self.stderr = None;
        if out_over || err_over {
            return Err(bad());
        }
        let mut all = self.prefix.clone();
        all.extend(stderr);
        Ok(Finished {
            status,
            stdout,
            stderr: all,
        })
    }
}
fn group_signal(group: i32, signal: i32) -> std::io::Result<()> {
    if unsafe { libc::kill(-group, signal) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
/// The kill runs while the owned leader still reserves its group identity.
/// EPERM alone proves nothing: after exact-child reap, only fresh ESRCH proves
/// group absence. The post-reap operation never sends a nonzero signal.
async fn cleanup_owned_group<W>(
    kill: impl FnOnce() -> std::io::Result<()>,
    wait: impl FnOnce() -> W,
    probe: impl FnOnce() -> std::io::Result<()>,
) -> Result<ExitStatus>
where
    W: std::future::Future<Output = Result<ExitStatus>>,
{
    let denied = match kill() {
        Ok(()) => false,
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => false,
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => true,
        Err(error) => return Err(error.into()),
    };
    let status = wait().await?;
    if denied {
        match probe() {
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {}
            Err(error) => return Err(error.into()),
            Ok(()) => return Err(Error::Invalid("installed_owned_group_present")),
        }
    }
    Ok(status)
}

#[cfg(test)]
#[path = "installed_owned_group_cleanup_tests.rs"]
mod cleanup_tests;

/// Probe this Child without reaping it or granting authority from a numeric PID.
pub(super) fn leader_exited(child: &Child) -> Result<bool> {
    OwnedRsync::exited(child)
}
/// Observe this owned leader without releasing its PID/PGID fence.
pub(super) async fn wait_unreaped(child: &mut Child, deadline: Instant) -> Result<()> {
    tokio::time::timeout_at(deadline, async {
        while !leader_exited(child)? {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    })
    .await
    .map_err(|_| Error::Timeout)?
}
/// Kill the still-owned group, then reap; never signal an already reaped PID.
pub(super) async fn reap_owned_group(child: &mut Child) -> Result<ExitStatus> {
    OwnedRsync::cleanup_child(child).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    schema: u8,
    nonce: String,
    executable_sha256: String,
    argv_sha256: String,
}
async fn entry(c: &Cohort<'_, '_>, stream: &str) -> Result<Option<Entry>> {
    Ok(
        streaming_cleanup::entries(*c.server.daemon.lock().await, c.server.socket, c.deadline)
            .await?
            .into_iter()
            .find(|e| e.stream == stream),
    )
}
fn principal(c: &Cohort<'_, '_>, e: &Entry) -> bool {
    e.direction == "server"
        && c.identities
            .get(c.role)
            .is_some_and(|identity| e.client == identity.client_account)
        && e.server == c.ready.server_account
}
async fn started(
    c: &Cohort<'_, '_>,
    stream: &str,
    nonce: &str,
    case: Case,
) -> Result<(Entry, String, String)> {
    let marker = edge_fixture::marker(c.server.workspace, nonce)?;
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        if let Some(e) = entry(c, stream).await? {
            if !principal(c, &e) || e.open_digest.is_none() {
                return Err(bad());
            }
            if marker.try_exists()? {
                let m: Marker = records::read(&marker, 8192)?;
                if m.schema != 1
                    || m.nonce != nonce
                    || m.executable_sha256 != c.ready.fixture_sha256
                    || !records::hexadecimal(&m.argv_sha256, 64)
                {
                    return Err(bad());
                }
                if !matches!(case, Case::Failure) {
                    if e.state != "running"
                        || !e.active
                        || e.cancel_requested
                        || e.terminal.is_some()
                    {
                        return Err(bad());
                    }
                    let status = streaming_cleanup::status(
                        *c.server.daemon.lock().await,
                        c.server.socket,
                        c.deadline,
                    )
                    .await?;
                    if status.active != 1 || status.unfinished_runners != 1 || status.uncertain != 0
                    {
                        return Err(bad());
                    }
                }
                return Ok((
                    e,
                    records::digest(&records::read_bytes(&marker, 8192)?),
                    m.argv_sha256,
                ));
            }
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(50))).await;
    }
}
async fn terminal(
    c: &Cohort<'_, '_>,
    initial: &Entry,
    expected: Terminal,
) -> Result<streaming_cleanup::StreamStatus> {
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        if let Some(e) = entry(c, &initial.stream).await? {
            if !principal(c, &e)
                || e.session != initial.session
                || e.group_sha256 != initial.group_sha256
                || e.open_digest != initial.open_digest
            {
                return Err(bad());
            }
            if let Some(t) = e.terminal {
                if t != expected || (expected == Terminal::Cancelled && !e.cancel_requested) {
                    return Err(bad());
                }
                if !e.active {
                    let status = streaming_cleanup::status(
                        *c.server.daemon.lock().await,
                        c.server.socket,
                        c.deadline,
                    )
                    .await?;
                    if status.validate_idle().is_ok() {
                        return Ok(status);
                    }
                }
            }
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(50))).await;
    }
}
async fn one(c: &Cohort<'_, '_>, case: Case) -> Result<Observation> {
    let local = streaming_prerequisites::rsync(c.context, c.client.workspace)?.ok_or_else(bad)?;
    let remote = c.ready.rsync.as_ref().ok_or_else(bad)?;
    streaming_prerequisites::verify(remote)?;
    let nonce = records::digest(&records::canonical(&(
        "installed-rsync-edge/1",
        c.context.digest()?,
        c.role,
        case.name(),
    ))?);
    edge_fixture::prepare_gates(c.server.workspace)?;
    let marker = edge_fixture::marker(c.server.workspace, &nonce)?;
    if marker.try_exists()? {
        return Err(bad());
    }
    records::write(
        &c.server.workspace.join("edge-gates/rsync.json"),
        &edge_fixture::RsyncConfig {
            schema: 1,
            nonce: nonce.clone(),
            mode: if matches!(case, Case::Failure) {
                "fail"
            } else {
                "hold"
            }
            .into(),
            rsync: remote.clone(),
        },
    )?;
    let source = c
        .client
        .workspace
        .join(format!("edge-rsync-{nonce}.source"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source)?;
    use std::io::Write;
    file.write_all(b"installed-rsync-edge\n")?;
    file.sync_all()?;
    drop(file);
    let mut args: Vec<OsString> = vec![
        "-as".into(),
        "--partial".into(),
        "-e".into(),
        transfer_cases::rsh(c.client, c.context, false)?,
        "--rsync-path=./fixture/main".into(),
    ];
    if matches!(case, Case::Timeout) {
        // Allow the authenticated transport to start before exercising rsync's
        // own idle timeout. The original family deadline remains authoritative.
        args.push("--timeout=30".into());
    }
    args.extend([
        "--".into(),
        source.into_os_string(),
        remote_operand(&c.ready.server_account, &nonce)?.into(),
    ]);
    let start = Instant::now();
    let mut child = OwnedRsync::start(Path::new(&local.path), &args, c.client, c.deadline).await?;
    let result = async {
        let (initial, marker_sha256, fixture_argv_sha256) =
            started(c, &child.stream, &nonce, case).await?;
        if let Some(signal) = case.signal() {
            child.signal(signal, matches!(case, Case::GroupInterrupt))?;
        }
        let done = child.finish(c.deadline).await?;
        if !done.stdout.is_empty()
            || (matches!(case, Case::Timeout)
                && (done.status.code() != Some(30)
                    || !done
                        .stderr
                        .windows(b"io timeout".len())
                        .any(|bytes| bytes == b"io timeout")))
        {
            return Err(bad());
        }
        let expected = if matches!(case, Case::Failure) {
            Terminal::Exited { code: 23 }
        } else {
            Terminal::Cancelled
        };
        let cleanup = terminal(c, &initial, expected).await?;
        let observation = Observation {
            case: case.name().into(),
            wall_ns: u64::try_from(start.elapsed().as_nanos()).map_err(|_| bad())?,
            binding: Binding {
                stream_sha256: records::digest(initial.stream.as_bytes()),
                session_sha256: records::digest(initial.session.as_bytes()),
                open_digest: initial.open_digest.ok_or_else(bad)?,
                group_sha256: initial.group_sha256,
            },
            marker_sha256,
            fixture_argv_sha256,
            local_rsync_sha256: local.sha256,
            remote_rsync_sha256: remote.sha256.clone(),
            local_exit: done.status.code().ok_or_else(bad)?,
            stderr_sha256: records::digest(&done.stderr),
            cancellation_confirmed: done
                .stderr
                .windows(b"terminal state confirmed".len())
                .any(|s| s == b"terminal state confirmed"),
            remote_terminal: expected,
            cleanup,
        };
        observation.validate()?;
        // All remote work is physically finished before replacing the gate for
        // the next case. Immutable publication refuses any pre-existing gate.
        std::fs::remove_file(c.server.workspace.join("edge-gates/rsync.json"))?;
        Ok(observation)
    }
    .await;
    if result.is_err() {
        child.abort().await?;
    }
    result
}
pub(super) async fn run(mut c: Cohort<'_, '_>) -> Result<Vec<Observation>> {
    c.deadline = c.deadline.min(Instant::now() + Duration::from_secs(1140));
    let mut observations = Vec::new();
    for case in [
        Case::Interrupt,
        Case::Terminate,
        Case::Hangup,
        Case::GroupInterrupt,
        Case::Timeout,
        Case::Failure,
    ] {
        observations.push(one(&c, case).await?);
    }
    validate(&observations)?;
    Ok(observations)
}
/// Reject incomplete/duplicated families when reconciling serialized evidence.
pub(super) fn validate(observations: &[Observation]) -> Result<()> {
    let expected = [
        "rsync-sigint",
        "rsync-sigterm",
        "rsync-sighup",
        "rsync-group-sigint",
        "rsync-timeout-usr1",
        "rsync-independent-failure",
    ];
    if observations.len() != expected.len() {
        return Err(bad());
    }
    let mut streams = std::collections::BTreeSet::new();
    let mut wall = 0u64;
    for (observation, case) in observations.iter().zip(expected) {
        observation.validate()?;
        if observation.case != case || !streams.insert(&observation.binding.stream_sha256) {
            return Err(bad());
        }
        wall = wall.checked_add(observation.wall_ns).ok_or_else(bad)?;
    }
    if wall > 1_140_000_000_000 {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation() -> Observation {
        Observation {
            case: "rsync-sigint".into(),
            wall_ns: 1,
            binding: Binding {
                stream_sha256: "a".repeat(64),
                session_sha256: "b".repeat(64),
                open_digest: "c".repeat(64),
                group_sha256: "d".repeat(64),
            },
            marker_sha256: "e".repeat(64),
            fixture_argv_sha256: "f".repeat(64),
            local_rsync_sha256: "1".repeat(64),
            remote_rsync_sha256: "2".repeat(64),
            local_exit: 20,
            stderr_sha256: "3".repeat(64),
            cancellation_confirmed: true,
            remote_terminal: Terminal::Cancelled,
            cleanup: streaming_cleanup::StreamStatus {
                active: 0,
                cancelling: 0,
                uncertain: 0,
                owned_runners: 1,
                unfinished_runners: 0,
                observation_pending: false,
                cleanup_ready: true,
                stop_complete: true,
            },
        }
    }
    #[test]
    fn cancellation_requires_confirmation_terminal_and_physical_cleanup() {
        for case in [
            "rsync-sigint",
            "rsync-sigterm",
            "rsync-sighup",
            "rsync-group-sigint",
            "rsync-timeout-usr1",
        ] {
            let mut o = observation();
            o.case = case.into();
            if case == "rsync-timeout-usr1" {
                o.local_exit = 30;
            }
            o.validate().unwrap();
            o.cancellation_confirmed = false;
            assert!(o.validate().is_err());
            o.cancellation_confirmed = true;
            o.remote_terminal = Terminal::Exited { code: 23 };
            assert!(o.validate().is_err());
            o.remote_terminal = Terminal::Cancelled;
            o.cleanup.unfinished_runners = 1;
            assert!(o.validate().is_err());
        }
    }
    #[test]
    fn independent_failure_preserves_natural_terminal_and_closed_evidence() {
        let mut o = observation();
        o.case = "rsync-independent-failure".into();
        assert!(o.validate().is_err());
        o.remote_terminal = Terminal::Exited { code: 23 };
        o.cancellation_confirmed = false;
        o.validate().unwrap();
        o.cancellation_confirmed = true;
        o.validate().unwrap();
        o.remote_terminal = Terminal::Exited { code: 0 };
        assert!(o.validate().is_err());
        o.remote_terminal = Terminal::Exited { code: 23 };
        for mutation in 0..7 {
            let mut invalid = o.clone();
            match mutation {
                0 => invalid.case = "unknown".into(),
                1 => invalid.wall_ns = 0,
                2 => invalid.wall_ns = 240_000_000_001,
                3 => invalid.local_exit = 0,
                4 => invalid.binding.open_digest.clear(),
                5 => invalid.cleanup.uncertain = 1,
                _ => invalid.cleanup.stop_complete = false,
            }
            assert!(invalid.validate().is_err());
        }
        let mut encoded = serde_json::to_value(&o).unwrap();
        encoded
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), true.into());
        assert!(serde_json::from_value::<Observation>(encoded).is_err());
    }
    #[test]
    fn family_requires_exact_order_distinct_streams_and_total_budget() {
        let mut family: Vec<_> = [
            "rsync-sigint",
            "rsync-sigterm",
            "rsync-sighup",
            "rsync-group-sigint",
            "rsync-timeout-usr1",
            "rsync-independent-failure",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, case)| {
            let mut o = observation();
            o.case = case.into();
            o.binding.stream_sha256 = format!("{index:064x}");
            if index == 4 {
                o.local_exit = 30;
            }
            if index == 5 {
                o.remote_terminal = Terminal::Exited { code: 23 };
            }
            o
        })
        .collect();
        validate(&family).unwrap();
        assert!(validate(&family[..5]).is_err());
        family.swap(0, 1);
        assert!(validate(&family).is_err());
        family.swap(0, 1);
        family[1].binding.stream_sha256 = family[0].binding.stream_sha256.clone();
        assert!(validate(&family).is_err());
        family[1].binding.stream_sha256 = format!("{:064x}", 1);
        family[0].wall_ns = 240_000_000_000;
        validate(&family).unwrap();
        for observation in &mut family {
            observation.wall_ns = 240_000_000_000;
        }
        assert!(validate(&family).is_err());
    }
    #[tokio::test]
    async fn retained_leader_fences_group_cleanup_after_natural_exit() {
        let root = crate::journal::private_test_directory();
        let environment = process::Environment {
            home: root.path().into(),
            temporary: root.path().into(),
            utilities: root.path().into(),
        };
        let args = vec![
            OsString::from("-c"),
            OsString::from("/bin/sleep 30 & exit 23"),
        ];
        let mut child = process::command(Path::new("/bin/sh"), &args, root.path(), &environment)
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        wait_unreaped(&mut child, Instant::now() + Duration::from_secs(3))
            .await
            .unwrap();
        assert!(child.id().is_some());
        // A background descendant retains these pipes until owned-group cleanup.
        let mut drained = tokio::spawn(async move {
            let out = process::drain(stdout, 1024).await.unwrap();
            let err = process::drain(stderr, 1024).await.unwrap();
            (out, err)
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut drained)
                .await
                .is_err()
        );
        let status = reap_owned_group(&mut child).await.unwrap();
        assert_eq!(status.code(), Some(23));
        assert!(child.id().is_none());
        tokio::time::timeout(Duration::from_secs(3), drained)
            .await
            .unwrap()
            .unwrap();
        assert!(reap_owned_group(&mut child).await.is_err());
    }
    #[test]
    fn rsync_stream_label_allows_prefix_but_rejects_malformed_identity() {
        let id = "a".repeat(32);
        assert_eq!(
            stream_line(format!("rsync: marmish stream: {id}\n").as_bytes()).unwrap(),
            Some(id)
        );
        assert!(
            stream_line(b"rsync: unrelated diagnostic\n")
                .unwrap()
                .is_none()
        );
        assert!(stream_line(b"marmish stream: invalid\n").is_err());
        assert!(
            stream_line(format!("marmish stream: {} trailing\n", "a".repeat(32)).as_bytes())
                .is_err()
        );
    }
}

#[cfg(test)]
mod public_destination_tests {
    use super::*;
    #[test]
    fn public_destination_builder_obeys_product_contract() {
        let account = "ab".repeat(32);
        let remote = remote_operand(&account, "nonce").unwrap();
        let (target, path) = remote.split_once(':').unwrap();
        records::assert_public_target(target.as_ref(), &account);
        assert_eq!(path, "edge-rsync-nonce.destination");
        assert!(remote_operand("bad", "nonce").is_err());
    }
}
