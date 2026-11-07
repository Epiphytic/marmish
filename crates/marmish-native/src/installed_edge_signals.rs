//! Installed stream cancellation and detach checks using owned gated fixtures.
use super::{
    copy_interrupt::Binding,
    edge_fixture,
    edge_process::Started,
    fixture, matrix, process,
    records::{self, Context, IdentityReady},
    streaming_cases::Endpoint,
    streaming_cleanup::{self, Entry},
    streaming_records::ServerReady,
};
use crate::{Error, Result};
use marmish_stream::wire::Terminal;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ffi::OsString, time::Duration};
use tokio::time::Instant;
fn bad() -> Error {
    Error::Invalid("installed_edge_signal")
}
#[derive(Clone, Copy)]
enum Case {
    Interrupt,
    SignalClosedOutput,
    Epipe,
    Kill,
}
impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Interrupt => "stream-sigint",
            Self::SignalClosedOutput => "stream-signal-closed-output",
            Self::Epipe => "stream-epipe-detach",
            Self::Kill => "stream-sigkill-detach",
        }
    }
    fn detach(self) -> bool {
        !matches!(self, Self::Interrupt | Self::SignalClosedOutput)
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
    pub local_exit: Option<i32>,
    pub local_signal: Option<i32>,
    pub detached_helper_live_and_charged: bool,
    pub detach_dwell: Option<DetachDwell>,
    pub second_client_cancel: bool,
    pub remote_terminal: Terminal,
    pub cleanup: streaming_cleanup::StreamStatus,
}
const DETACH_DWELL: Duration = Duration::from_secs(2);
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DetachDwell {
    pub wall_ns: u64,
    pub polls: u32,
    pub minimum_active: u32,
    pub minimum_unfinished_runners: u32,
}
impl DetachDwell {
    fn validate(&self) -> Result<()> {
        if self.wall_ns < 2_000_000_000
            || self.wall_ns > 180_000_000_000
            || self.polls < 3
            || self.polls > 2000
            || self.minimum_active != 1
            || self.minimum_unfinished_runners != 1
        {
            return Err(bad());
        }
        Ok(())
    }
}
impl Observation {
    pub(super) fn validate(&self) -> Result<()> {
        self.cleanup.validate_idle()?;
        if let Some(dwell) = &self.detach_dwell {
            dwell.validate()?;
            if self.wall_ns < dwell.wall_ns {
                return Err(bad());
            }
        }
        if self.detached_helper_live_and_charged != self.detach_dwell.is_some() {
            return Err(bad());
        }
        if self.wall_ns == 0
            || self.wall_ns > 180_000_000_000
            || self.remote_terminal != Terminal::Cancelled
            || [
                &self.binding.stream_sha256,
                &self.binding.session_sha256,
                &self.binding.open_digest,
                &self.binding.group_sha256,
                &self.marker_sha256,
                &self.fixture_argv_sha256,
            ]
            .into_iter()
            .any(|s| !records::hexadecimal(s, 64))
        {
            return Err(bad());
        }
        let valid = match self.case.as_str() {
            "stream-sigint" | "stream-signal-closed-output" => {
                self.local_exit == Some(130)
                    && self.local_signal.is_none()
                    && !self.detached_helper_live_and_charged
                    && !self.second_client_cancel
            }
            "stream-epipe-detach" => {
                self.local_exit == Some(255)
                    && self.local_signal.is_none()
                    && self.detached_helper_live_and_charged
                    && self.second_client_cancel
            }
            "stream-sigkill-detach" => {
                self.local_exit.is_none()
                    && self.local_signal == Some(libc::SIGKILL)
                    && self.detached_helper_live_and_charged
                    && self.second_client_cancel
            }
            _ => false,
        };
        if !valid {
            return Err(bad());
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
async fn entry(c: &Cohort<'_, '_>, stream: &str) -> Result<Option<Entry>> {
    Ok(
        streaming_cleanup::entries(*c.server.daemon.lock().await, c.server.socket, c.deadline)
            .await?
            .into_iter()
            .find(|e| e.stream == stream),
    )
}
fn principal(c: &Cohort<'_, '_>, entry: &Entry) -> bool {
    entry.direction == "server"
        && entry.client == c.identities[c.role].client_account
        && entry.server == c.ready.server_account
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    schema: u8,
    nonce: String,
    executable_sha256: String,
    argv_sha256: String,
}
async fn running(
    c: &Cohort<'_, '_>,
    stream: &str,
    marker: &std::path::Path,
    nonce: &str,
    argv: &str,
) -> Result<(Entry, String, streaming_cleanup::StreamStatus)> {
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        if let Some(e) = entry(c, stream).await? {
            if !principal(c, &e) || e.cancel_requested || e.terminal.is_some() || !e.active {
                return Err(bad());
            }
            if e.state == "running" && marker.try_exists()? {
                let m: Marker = records::read(marker, 8192)?;
                if m.schema != 1
                    || m.nonce != nonce
                    || m.executable_sha256 != c.ready.fixture_sha256
                    || m.argv_sha256 != argv
                {
                    return Err(bad());
                }
                let status = streaming_cleanup::status(
                    *c.server.daemon.lock().await,
                    c.server.socket,
                    c.deadline,
                )
                .await?;
                if status.active != 1 || status.unfinished_runners != 1 || status.uncertain != 0 {
                    return Err(bad());
                }
                return Ok((
                    e,
                    records::digest(&records::read_bytes(marker, 8192)?),
                    status,
                ));
            }
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(50))).await;
    }
}
async fn detached_dwell(
    c: &Cohort<'_, '_>,
    initial: &Entry,
    marker: &std::path::Path,
    nonce: &str,
    argv: &str,
    marker_sha256: &str,
) -> Result<DetachDwell> {
    let start = Instant::now();
    let mut observed = DetachDwell {
        wall_ns: 0,
        polls: 0,
        minimum_active: u32::MAX,
        minimum_unfinished_runners: u32::MAX,
    };
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        let (entry, marker_hash, status) = running(c, &initial.stream, marker, nonce, argv).await?;
        if entry.session != initial.session
            || entry.open_digest != initial.open_digest
            || entry.group_sha256 != initial.group_sha256
            || marker_hash != marker_sha256
        {
            return Err(bad());
        }
        observed.polls = observed.polls.checked_add(1).ok_or_else(bad)?;
        observed.minimum_active = observed.minimum_active.min(status.active);
        observed.minimum_unfinished_runners = observed
            .minimum_unfinished_runners
            .min(status.unfinished_runners);
        if start.elapsed() >= DETACH_DWELL && observed.polls >= 3 {
            break;
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(100))).await;
    }
    observed.wall_ns = u64::try_from(start.elapsed().as_nanos()).map_err(|_| bad())?;
    observed.validate()?;
    Ok(observed)
}
async fn cancelled(c: &Cohort<'_, '_>, stream: &str) -> Result<streaming_cleanup::StreamStatus> {
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        if let Some(e) = entry(c, stream).await? {
            if !principal(c, &e) {
                return Err(bad());
            }
            if e.state == "terminal"
                && !e.active
                && e.cancel_requested
                && e.terminal == Some(Terminal::Cancelled)
            {
                let status = streaming_cleanup::status(
                    *c.server.daemon.lock().await,
                    c.server.socket,
                    c.deadline,
                )
                .await?;
                if status.validate_idle().is_ok() {
                    return Ok(status);
                }
            } else if e.terminal.is_some() || matches!(e.state.as_str(), "lost" | "unknown_launch")
            {
                return Err(bad());
            }
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(50))).await;
    }
}
async fn second_cancel(c: &Cohort<'_, '_>, stream: &str) -> Result<bool> {
    let mut args = matrix::base(
        c.client.workspace,
        "client",
        c.client.socket,
        &c.context.relays,
    );
    args.extend([
        "--no-autostart".into(),
        "cancel".into(),
        "--stream".into(),
        stream.into(),
        c.ready.server_account.clone().into(),
    ]);
    let output = process::run(
        &c.client.workspace.join("bin/marmish"),
        &args,
        c.client.workspace,
        c.client.environment,
        c.deadline,
        65536,
    )
    .await?;
    if output.status.code() != Some(130)
        || !output.stdout.is_empty()
        || !output
            .stderr
            .windows(b"terminal state confirmed".len())
            .any(|b| b == b"terminal state confirmed")
    {
        return Err(bad());
    }
    Ok(true)
}
async fn one(c: &Cohort<'_, '_>, case: Case) -> Result<Observation> {
    let nonce = records::digest(&records::canonical(&(
        "installed-edge/1",
        c.context.digest()?,
        c.role,
        case.name(),
    ))?);
    let marker = edge_fixture::marker(c.server.workspace, &nonce)?;
    if marker.try_exists()? {
        return Err(bad());
    }
    let remote_args: Vec<OsString> = vec![
        "edge-v1".into(),
        if matches!(case, Case::Epipe | Case::SignalClosedOutput) {
            "flood".into()
        } else {
            "hold".into()
        },
        nonce.clone().into(),
        c.role.into(),
    ];
    let argv = fixture::argv_digest(&c.server.workspace.join("fixture/main"), &remote_args)?;
    let mut args = matrix::base(
        c.client.workspace,
        "client",
        c.client.socket,
        &c.context.relays,
    );
    args.extend([
        "--no-autostart".into(),
        "stream".into(),
        c.ready.server_account.clone().into(),
        "--".into(),
        "./fixture/main".into(),
    ]);
    args.extend(remote_args);
    let start = Instant::now();
    let mut child = Started::start(
        &c.client.workspace.join("bin/marmish"),
        &args,
        c.client.workspace,
        c.client.environment,
        c.deadline,
        matches!(case, Case::Epipe),
    )
    .await?;
    let result = async {
        let (initial, marker_sha256, _) = running(c, &child.stream, &marker, &nonce, &argv).await?;
        match case {
            Case::Interrupt => child.signal(libc::SIGINT, false)?,
            Case::SignalClosedOutput => {
                child.signal(libc::SIGINT, false)?;
                child.close_stdout().await;
            }
            Case::Kill => child.signal(libc::SIGKILL, true)?,
            Case::Epipe => {}
        }
        let finished = child.finish(c.deadline).await?;
        if !finished.stdout.is_empty() {
            return Err(bad());
        }
        let mut detach_dwell = None;
        let mut second_client_cancel = false;
        if case.detach() {
            // Observe continuation repeatedly after the local endpoint exits.
            // This proves only the recorded dwell, not pipe-window occupancy.
            detach_dwell =
                Some(detached_dwell(c, &initial, &marker, &nonce, &argv, &marker_sha256).await?);
            second_client_cancel = second_cancel(c, &child.stream).await?;
        } else if !finished
            .stderr
            .windows(b"terminal state confirmed".len())
            .any(|b| b == b"terminal state confirmed")
        {
            return Err(bad());
        }
        let cleanup = cancelled(c, &child.stream).await?;
        use std::os::unix::process::ExitStatusExt;
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
            fixture_argv_sha256: argv,
            local_exit: finished.status.code(),
            local_signal: finished.status.signal(),
            detached_helper_live_and_charged: detach_dwell.is_some(),
            detach_dwell,
            second_client_cancel,
            remote_terminal: Terminal::Cancelled,
            cleanup,
        };
        observation.validate()?;
        Ok(observation)
    }
    .await;
    if result.is_err() {
        child.abort().await?;
    }
    result
}
pub(super) async fn run(c: Cohort<'_, '_>) -> Result<Vec<Observation>> {
    let mut results = Vec::new();
    for case in [
        Case::Interrupt,
        Case::SignalClosedOutput,
        Case::Epipe,
        Case::Kill,
    ] {
        results.push(one(&c, case).await?);
    }
    Ok(results)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detach_evidence_cannot_substitute_for_confirmed_interrupt() {
        let mut e = Observation {
            case: "stream-sigint".into(),
            wall_ns: 1,
            binding: Binding {
                stream_sha256: "a".repeat(64),
                session_sha256: "b".repeat(64),
                open_digest: "c".repeat(64),
                group_sha256: "d".repeat(64),
            },
            marker_sha256: "e".repeat(64),
            fixture_argv_sha256: "f".repeat(64),
            local_exit: Some(130),
            local_signal: None,
            detached_helper_live_and_charged: false,
            detach_dwell: None,
            second_client_cancel: false,
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
        };
        e.validate().unwrap();
        e.case = "stream-epipe-detach".into();
        assert!(e.validate().is_err());
        e.local_exit = Some(255);
        e.detached_helper_live_and_charged = true;
        e.second_client_cancel = true;
        assert!(e.validate().is_err());
        e.wall_ns = 3_000_000_000;
        e.detach_dwell = Some(DetachDwell {
            wall_ns: 2_000_000_000,
            polls: 3,
            minimum_active: 1,
            minimum_unfinished_runners: 1,
        });
        e.validate().unwrap();
        e.detach_dwell.as_mut().unwrap().polls = 1;
        assert!(e.validate().is_err());
        e.detach_dwell.as_mut().unwrap().polls = 3;
        e.detach_dwell.as_mut().unwrap().wall_ns = 1_999_999_999;
        assert!(e.validate().is_err());
        e.detach_dwell.as_mut().unwrap().wall_ns = 2_000_000_000;
        for corrupt in 0..4 {
            let mut invalid = e.clone();
            match corrupt {
                0 => invalid.second_client_cancel = false,
                1 => invalid.detach_dwell.as_mut().unwrap().minimum_active = 0,
                2 => invalid.case = "stream-sigint".into(),
                _ => invalid.wall_ns = 1,
            }
            assert!(invalid.validate().is_err());
        }
        e.case = "stream-sigkill-detach".into();
        e.local_exit = None;
        e.local_signal = Some(libc::SIGKILL);
        e.validate().unwrap();
        e.cleanup.unfinished_runners = 1;
        assert!(e.validate().is_err());
    }
}
