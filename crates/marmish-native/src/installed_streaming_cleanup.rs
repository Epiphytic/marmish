//! Same-UID, owned-daemon stream cleanup observations. Public records grant no authority.
use super::cleanup;
use crate::{
    Error, Result, ipc,
    stream_service::{StreamLocalRequest, StreamLocalResult},
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::{process::Child, time::Instant};
fn bad() -> Error {
    Error::Invalid("installed_stream_cleanup")
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StreamStatus {
    pub active: u32,
    pub cancelling: u32,
    pub uncertain: u32,
    pub owned_runners: u32,
    pub unfinished_runners: u32,
    pub observation_pending: bool,
    pub cleanup_ready: bool,
    pub stop_complete: bool,
}
impl StreamStatus {
    pub(super) fn validate_idle(&self) -> Result<()> {
        if self.active != 0
            || self.cancelling != 0
            || self.uncertain != 0
            || self.unfinished_runners != 0
            || self.owned_runners > 32
            || self.observation_pending
            || !self.cleanup_ready
            || !self.stop_complete
        {
            return Err(bad());
        }
        Ok(())
    }
}
pub(super) async fn status(
    child: &mut Child,
    socket: &Path,
    deadline: Instant,
) -> Result<StreamStatus> {
    let value: serde_json::Value = cleanup::request(
        child,
        socket,
        &ipc::Request::CommandCleanupStatus {},
        deadline,
    )
    .await?;
    let status: StreamStatus =
        serde_json::from_value(value.get("streams").cloned().ok_or_else(bad)?)
            .map_err(|_| bad())?;
    if status.active > 32
        || status.cancelling > 32
        || status.uncertain > 32
        || status.owned_runners > 32
        || status.unfinished_runners > status.owned_runners
    {
        return Err(bad());
    }
    Ok(status)
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub stream: String,
    pub session: String,
    pub client: String,
    pub server: String,
    pub direction: String,
    pub state: String,
    pub cancel_requested: bool,
    pub terminal: Option<marmish_stream::wire::Terminal>,
    pub child_disposition: Option<crate::stream_service::ChildDisposition>,
    pub open_digest: Option<String>,
    pub group_sha256: String,
    pub active: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StartupPhase {
    Wait,
    Running,
}
pub(super) fn startup_phase(
    entry: Option<&Entry>,
    client: &str,
    server: &str,
    established: bool,
) -> Result<StartupPhase> {
    let phase = match entry {
        None => StartupPhase::Wait,
        Some(e) => {
            if e.direction != "server"
                || e.client != client
                || e.server != server
                || e.cancel_requested
                || e.terminal.is_some()
                || e.child_disposition.is_some()
                || e.open_digest
                    .as_ref()
                    .is_some_and(|d| !super::records::hexadecimal(d, 64))
            {
                return Err(bad());
            }
            match e.state.as_str() {
                "hello" if !e.active && e.open_digest.is_none() => StartupPhase::Wait,
                "preparing" | "launching" if e.active && e.open_digest.is_some() => {
                    StartupPhase::Wait
                }
                "running" if e.active && e.open_digest.is_some() => StartupPhase::Running,
                _ => return Err(bad()),
            }
        }
    };
    if established && phase == StartupPhase::Wait {
        return Err(bad());
    }
    Ok(phase)
}
pub(super) fn validate_charged(status: &StreamStatus, expected: u32) -> Result<()> {
    if status.active != expected || status.unfinished_runners != expected || status.uncertain != 0 {
        return Err(bad());
    }
    Ok(())
}
pub(super) async fn entries(
    child: &mut Child,
    socket: &Path,
    deadline: Instant,
) -> Result<Vec<Entry>> {
    let response = cleanup::stream_request(
        child,
        socket,
        StreamLocalRequest::List { remote: None },
        deadline,
    )
    .await?;
    let StreamLocalResult::Entries { entries } = response else {
        return Err(bad());
    };
    if entries.len() > 1024 {
        return Err(bad());
    }
    entries
        .into_iter()
        .map(|entry| {
            serde_json::from_value(serde_json::to_value(entry).map_err(|_| bad())?)
                .map_err(|_| bad())
        })
        .collect()
}
pub(super) async fn cancel_all(child: &mut Child, socket: &Path, deadline: Instant) -> Result<()> {
    let mut cancel_failed = false;
    for entry in entries(child, socket, deadline).await? {
        if entry.direction == "server"
            && (entry.active || entry.terminal.is_none())
            && cleanup::stream_request(
                child,
                socket,
                StreamLocalRequest::AdminCancel {
                    stream: entry.stream,
                },
                deadline,
            )
            .await
            .is_err()
        {
            cancel_failed = true;
        }
    }
    loop {
        if status(child, socket, deadline)
            .await?
            .validate_idle()
            .is_ok()
        {
            return if cancel_failed { Err(bad()) } else { Ok(()) };
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep_until(
            deadline.min(Instant::now() + std::time::Duration::from_millis(100)),
        )
        .await;
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stop {
    schema: u8,
    #[serde(rename = "engine_quiesced")]
    _engine_quiesced: bool,
    engine_stop_complete: bool,
    mdk_stop_complete: bool,
    #[serde(default)]
    stream_stop_complete: Option<bool>,
    runtime_cleanup_complete: bool,
}
pub(super) fn stop_report(stdout: &[u8], require_server_proof: bool) -> Result<()> {
    let mut lines = stdout.split(|b| *b == b'\n').filter(|l| !l.is_empty());
    let report: Stop = serde_json::from_slice(lines.next().ok_or_else(bad)?).map_err(|_| bad())?;
    if lines.next().is_some()
        || report.schema != 1
        || !report.engine_stop_complete
        || !report.mdk_stop_complete
        || report.stream_stop_complete == Some(false)
        || (require_server_proof && report.stream_stop_complete != Some(true))
        || !report.runtime_cleanup_complete
    {
        return Err(bad());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_physical_or_binding_metadata_cannot_masquerade_as_a_pass() {
        let good = serde_json::json!({"active":0,"cancelling":0,"uncertain":0,"owned_runners":0,"unfinished_runners":0,"observation_pending":false,"cleanup_ready":true,"stop_complete":true});
        for name in [
            "active",
            "cancelling",
            "uncertain",
            "owned_runners",
            "unfinished_runners",
            "observation_pending",
            "cleanup_ready",
            "stop_complete",
        ] {
            let mut bad = good.clone();
            bad.as_object_mut().unwrap().remove(name);
            assert!(
                serde_json::from_value::<StreamStatus>(bad).is_err(),
                "missing {name}"
            );
        }
        let legacy = serde_json::json!({"stream":"a".repeat(32),"session":"b".repeat(32),"client":"c".repeat(64),"server":"d".repeat(64),"direction":"server","state":"terminal","cancel_requested":false,"terminal":{"kind":"exited","data":{"code":0}},"child_disposition":null});
        assert!(serde_json::from_value::<Entry>(legacy).is_err());
        let legacy_stop=br#"{"schema":1,"engine_quiesced":false,"engine_stop_complete":true,"mdk_stop_complete":true,"runtime_cleanup_complete":true}"#;
        assert!(stop_report(legacy_stop, false).is_ok());
        assert!(stop_report(legacy_stop, true).is_err());
        let mut qualified: serde_json::Value = serde_json::from_slice(legacy_stop).unwrap();
        qualified["stream_stop_complete"] = true.into();
        assert!(stop_report(&serde_json::to_vec(&qualified).unwrap(), true).is_ok());
        qualified["stream_stop_complete"] = false.into();
        assert!(stop_report(&serde_json::to_vec(&qualified).unwrap(), false).is_err());
    }
    #[test]
    fn physical_idle_proof_is_required_not_record_absence() {
        let good = StreamStatus {
            active: 0,
            cancelling: 0,
            uncertain: 0,
            owned_runners: 1,
            unfinished_runners: 0,
            observation_pending: false,
            cleanup_ready: true,
            stop_complete: true,
        };
        assert!(good.validate_idle().is_ok());
        for field in ["active", "cancelling", "uncertain", "unfinished_runners"] {
            let mut value = serde_json::to_value(&good).unwrap();
            value[field] = 1.into();
            assert!(
                serde_json::from_value::<StreamStatus>(value)
                    .unwrap()
                    .validate_idle()
                    .is_err()
            );
        }
        for field in ["cleanup_ready", "stop_complete"] {
            let mut value = serde_json::to_value(&good).unwrap();
            value[field] = false.into();
            assert!(
                serde_json::from_value::<StreamStatus>(value)
                    .unwrap()
                    .validate_idle()
                    .is_err()
            );
        }
        assert!(stop_report(br#"{"schema":1,"engine_quiesced":false,"engine_stop_complete":true,"mdk_stop_complete":true,"runtime_cleanup_complete":true}"#,true).is_err());
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    fn entry(state: &str) -> Entry {
        Entry {
            stream: "stream".into(),
            session: "session".into(),
            client: "client".into(),
            server: "server".into(),
            direction: "server".into(),
            state: state.into(),
            cancel_requested: false,
            terminal: None,
            child_disposition: None,
            open_digest: (state != "hello").then(|| "a".repeat(64)),
            group_sha256: "b".repeat(64),
            active: state != "hello",
        }
    }
    #[test]
    fn startup_progression_waits_but_established_observation_never_restarts() {
        assert_eq!(
            startup_phase(None, "client", "server", false).unwrap(),
            StartupPhase::Wait
        );
        assert!(startup_phase(None, "client", "server", true).is_err());
        for state in ["hello", "preparing", "launching", "running"] {
            let e = entry(state);
            let want = if state == "running" {
                StartupPhase::Running
            } else {
                StartupPhase::Wait
            };
            assert_eq!(
                startup_phase(Some(&e), "client", "server", false).unwrap(),
                want
            );
            assert_eq!(
                startup_phase(Some(&e), "client", "server", true).is_ok(),
                state == "running"
            );
        }
    }
    #[test]
    fn startup_rejects_inconsistent_or_untrusted_entries() {
        for state in ["hello", "preparing", "launching", "running"] {
            let good = entry(state);
            let mut invalid = good.clone();
            invalid.active = !invalid.active;
            assert!(startup_phase(Some(&invalid), "client", "server", false).is_err());
            let mut invalid = good.clone();
            invalid.open_digest = if state == "hello" {
                Some("a".repeat(64))
            } else {
                None
            };
            assert!(startup_phase(Some(&invalid), "client", "server", false).is_err());
            for digest in [
                "A".repeat(64),
                "x".repeat(64),
                "a".repeat(63),
                String::new(),
            ] {
                let mut invalid = good.clone();
                invalid.open_digest = Some(digest);
                assert!(startup_phase(Some(&invalid), "client", "server", false).is_err());
            }
            for field in ["direction", "client", "server"] {
                let mut invalid = good.clone();
                match field {
                    "direction" => invalid.direction = "client".into(),
                    "client" => invalid.client = "other".into(),
                    _ => invalid.server = "other".into(),
                }
                assert!(startup_phase(Some(&invalid), "client", "server", false).is_err());
            }
            let mut invalid = good.clone();
            invalid.cancel_requested = true;
            assert!(startup_phase(Some(&invalid), "client", "server", false).is_err());
            for terminal in [
                marmish_stream::wire::Terminal::Exited { code: 0 },
                marmish_stream::wire::Terminal::Signaled { signal: 15 },
                marmish_stream::wire::Terminal::Cancelled,
                marmish_stream::wire::Terminal::Failed {
                    code: marmish_stream::wire::WireErrorCode::Denied,
                },
            ] {
                let mut invalid = good.clone();
                invalid.terminal = Some(terminal);
                assert!(startup_phase(Some(&invalid), "client", "server", false).is_err());
            }
            for disposition in [
                crate::stream_service::ChildDisposition::Absent,
                crate::stream_service::ChildDisposition::PresentUnowned,
                crate::stream_service::ChildDisposition::Unknown,
            ] {
                let mut invalid = good.clone();
                invalid.child_disposition = Some(disposition);
                assert!(startup_phase(Some(&invalid), "client", "server", false).is_err());
            }
        }
        for state in [
            "",
            "other",
            "lost",
            "unknown_launch",
            "authority_revoked",
            "server_stopping",
            "server_stopped",
            "terminal",
        ] {
            assert!(startup_phase(Some(&entry(state)), "client", "server", false).is_err());
        }
    }
    #[test]
    fn running_phase_does_not_substitute_for_charged_physical_status() {
        for expected in [1, 2] {
            let good = StreamStatus {
                active: expected,
                cancelling: 0,
                uncertain: 0,
                owned_runners: expected,
                unfinished_runners: expected,
                observation_pending: true,
                cleanup_ready: false,
                stop_complete: false,
            };
            validate_charged(&good, expected).unwrap();
            for count in [expected - 1, expected + 1] {
                let mut invalid = good.clone();
                invalid.active = count;
                assert!(validate_charged(&invalid, expected).is_err());
                let mut invalid = good.clone();
                invalid.unfinished_runners = count;
                assert!(validate_charged(&invalid, expected).is_err());
            }
            let mut invalid = good;
            invalid.uncertain = 1;
            assert!(validate_charged(&invalid, expected).is_err());
        }
    }
}
