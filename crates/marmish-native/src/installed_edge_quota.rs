//! Peer quota saturation uses two owned fixture processes, never inferred load.
use super::edge_observation::{Case, Family, Stage, Trace};
use super::streaming_cases::{cancel_args, hold_args};
use super::{
    copy_interrupt::Binding, edge_fixture, edge_process::Started, edge_signals::Cohort, fixture,
    matrix, process, records, streaming_cleanup,
};
use crate::{Error, Result};
use marmish_stream::wire::Terminal;
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, time::Duration};
use tokio::time::Instant;

fn bad() -> Error {
    Error::Invalid("installed_edge_quota")
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub wall_ns: u64,
    pub held: [Binding; 2],
    pub refusal: Refusal,
    pub marker_sha256: [String; 2],
    pub active_at_refusal: u32,
    pub unfinished_at_refusal: u32,
    pub third_exit: i32,
    pub cleanup: streaming_cleanup::StreamStatus,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Refusal {
    pub binding: Binding,
    pub server_state: String,
    pub server_open_digest: Option<String>,
    pub server_active: bool,
    pub server_terminal: Option<Terminal>,
    pub server_cancel_requested: bool,
    pub server_child_disposition: Option<crate::stream_service::ChildDisposition>,
    pub marker_absent: bool,
    pub owned_runners_before: u32,
    pub owned_runners_after: u32,
}
impl Refusal {
    fn validate(&self) -> Result<()> {
        if self.server_state != "hello"
            || self.server_open_digest.is_some()
            || self.server_active
            || self.server_terminal.is_some()
            || self.server_cancel_requested
            || self.server_child_disposition.is_some()
            || !self.marker_absent
            || self.owned_runners_before < 2
            || self.owned_runners_before != self.owned_runners_after
            || self.owned_runners_after > 32
            || [
                &self.binding.stream_sha256,
                &self.binding.session_sha256,
                &self.binding.open_digest,
                &self.binding.group_sha256,
            ]
            .into_iter()
            .any(|v| !records::hexadecimal(v, 64))
        {
            return Err(bad());
        }
        Ok(())
    }
}
impl Observation {
    pub(super) fn validate(&self) -> Result<()> {
        self.cleanup.validate_idle()?;
        self.refusal.validate()?;
        if self.wall_ns == 0
            || self.wall_ns > 120_000_000_000
            || self.active_at_refusal != 2
            || self.unfinished_at_refusal != 2
            || self.third_exit != 255
            || self.held[0].stream_sha256 == self.held[1].stream_sha256
            || self
                .held
                .iter()
                .any(|e| e.stream_sha256 == self.refusal.binding.stream_sha256)
            || self
                .marker_sha256
                .iter()
                .any(|v| !records::hexadecimal(v, 64))
            || self.held.iter().any(|e| {
                [
                    &e.stream_sha256,
                    &e.session_sha256,
                    &e.open_digest,
                    &e.group_sha256,
                ]
                .iter()
                .any(|v| !records::hexadecimal(v, 64))
            })
        {
            return Err(bad());
        }
        Ok(())
    }
}
fn args(c: &Cohort<'_, '_>, nonce: &str) -> Result<Vec<OsString>> {
    let mut args = matrix::base(
        c.client.workspace,
        "client",
        c.client.socket,
        &c.context.relays,
    );
    args.extend(hold_args(&c.ready.server_account, nonce, c.role)?);
    Ok(args)
}
fn read_started_marker(path: &std::path::Path) -> Result<Option<serde_json::Value>> {
    records::read_published(path, 8192)
}
async fn started(c: &Cohort<'_, '_>, stream: &str, nonce: &str) -> Result<(Binding, String)> {
    let path = edge_fixture::marker(c.server.workspace, nonce)?;
    let argv = fixture::argv_digest(
        &c.server.workspace.join("fixture/main"),
        &["edge-v1".into(), "hold".into(), nonce.into(), c.role.into()],
    )?;
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        let entries =
            streaming_cleanup::entries(*c.server.daemon.lock().await, c.server.socket, c.deadline)
                .await?;
        let selected = entries.into_iter().find(|e| e.stream == stream);
        let phase = streaming_cleanup::startup_phase(
            selected.as_ref(),
            &c.identities[c.role].client_account,
            &c.ready.server_account,
            false,
        )?;
        if phase == streaming_cleanup::StartupPhase::Running
            && path.try_exists()?
            && let Some(marker) = read_started_marker(&path)?
        {
            let e = selected.ok_or_else(bad)?;
            edge_fixture::validate_marker(&marker, nonce, &c.ready.fixture_sha256, &argv)?;
            return Ok((
                Binding {
                    stream_sha256: records::digest(e.stream.as_bytes()),
                    session_sha256: records::digest(e.session.as_bytes()),
                    open_digest: e.open_digest.ok_or_else(bad)?,
                    group_sha256: e.group_sha256,
                },
                records::digest(&records::read_bytes(&path, 8192)?),
            ));
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(50))).await;
    }
}
async fn cancel(c: &Cohort<'_, '_>, stream: &str) -> Result<()> {
    let mut args = matrix::base(
        c.client.workspace,
        "client",
        c.client.socket,
        &c.context.relays,
    );
    args.extend(cancel_args(&c.ready.server_account, stream)?);
    let out = process::run(
        &c.client.workspace.join("bin/marmish"),
        &args,
        c.client.workspace,
        c.client.environment,
        c.deadline,
        65536,
    )
    .await?;
    if out.status.code() != Some(130)
        || !out.stdout.is_empty()
        || !out
            .stderr
            .windows(b"terminal state confirmed".len())
            .any(|b| b == b"terminal state confirmed")
    {
        return Err(bad());
    }
    Ok(())
}
pub(super) async fn run(c: Cohort<'_, '_>) -> Result<Observation> {
    let trace = Trace::new(c.role, Family::Quota, Case::TwoChildrenQuota);
    let result = run_inner(c, &trace).await;
    trace.finish(&result);
    result
}
async fn run_inner(c: Cohort<'_, '_>, trace: &Trace<'_>) -> Result<Observation> {
    trace.stage(Stage::Prerequisite);
    let start = Instant::now();
    let mut owned = Vec::new();
    let result = async {
        let mut bindings = Vec::new();
        let mut markers = Vec::new();
        for ordinal in 0..2 {
            let nonce = records::digest(&records::canonical(&(
                "installed-peer2/1",
                c.context.digest()?,
                c.role,
                ordinal,
            ))?);
            if edge_fixture::marker(c.server.workspace, &nonce)?.try_exists()? {
                return Err(bad());
            }
            trace.stage(Stage::Attachment);
            let child = Started::start(
                &c.client.workspace.join("bin/marmish"),
                &args(&c, &nonce)?,
                c.client.workspace,
                c.client.environment,
                c.deadline,
                false,
            )
            .await?;
            owned.push(child);
            trace.stage(Stage::Startup);
            let (binding, marker) =
                started(&c, &owned.last().ok_or_else(bad)?.stream, &nonce).await?;
            bindings.push(binding);
            markers.push(marker);
        }
        let nonce = records::digest(&records::canonical(&(
            "installed-peer2/1",
            c.context.digest()?,
            c.role,
            2,
        ))?);
        let third_marker = edge_fixture::marker(c.server.workspace, &nonce)?;
        if third_marker.try_exists()? {
            return Err(bad());
        }
        trace.stage(Stage::Baseline);
        let before =
            streaming_cleanup::status(*c.server.daemon.lock().await, c.server.socket, c.deadline)
                .await?;
        streaming_cleanup::validate_charged(&before, 2)?;
        trace.stage(Stage::Attachment);
        let third = process::run(
            &c.client.workspace.join("bin/marmish"),
            &args(&c, &nonce)?,
            c.client.workspace,
            c.client.environment,
            c.deadline,
            65536,
        )
        .await?;
        trace.stage(Stage::Prefix);
        let (third_stream, diagnostic) = super::streaming_cases::stderr(&third.stderr)?;
        if !third.stdout.is_empty()
            || !diagnostic
                .windows(8)
                .any(|b| b.eq_ignore_ascii_case(b"capacity"))
            || third_marker.try_exists()?
        {
            return Err(bad());
        }
        let status =
            streaming_cleanup::status(*c.server.daemon.lock().await, c.server.socket, c.deadline)
                .await?;
        streaming_cleanup::validate_charged(&status, 2)?;
        trace.stage(Stage::JournalCheck);
        let server_entries =
            streaming_cleanup::entries(*c.server.daemon.lock().await, c.server.socket, c.deadline)
                .await?;
        let client_entries =
            streaming_cleanup::entries(*c.client.daemon.lock().await, c.client.socket, c.deadline)
                .await?;
        let refused_server = server_entries
            .iter()
            .find(|e| e.stream == third_stream)
            .ok_or_else(bad)?;
        let refused_client = client_entries
            .iter()
            .find(|e| e.stream == third_stream)
            .ok_or_else(bad)?;
        if refused_server.direction != "server"
            || refused_client.direction != "client"
            || refused_server.client != c.identities[c.role].client_account
            || refused_server.server != c.ready.server_account
            || refused_client.client != refused_server.client
            || refused_client.server != refused_server.server
            || refused_client.session != refused_server.session
            || refused_client.group_sha256 != refused_server.group_sha256
            || bindings.iter().any(|binding| {
                binding.session_sha256 != records::digest(refused_server.session.as_bytes())
                    || binding.group_sha256 != refused_server.group_sha256
            })
        {
            return Err(bad());
        }
        let expected_open = hex::encode(
            marmish_stream::wire::open_digest(
                &[
                    b"./fixture/main".to_vec(),
                    b"edge-v1".to_vec(),
                    b"hold".to_vec(),
                    nonce.as_bytes().to_vec(),
                    c.role.as_bytes().to_vec(),
                ],
                marmish_stream::codec::Codec::None,
            )
            .map_err(|_| bad())?,
        );
        if refused_client.open_digest.as_ref() != Some(&expected_open) {
            return Err(bad());
        }
        for (child, binding) in owned.iter().zip(&bindings) {
            let e = server_entries
                .iter()
                .find(|e| e.stream == child.stream)
                .ok_or_else(bad)?;
            if e.direction != "server"
                || e.client != c.identities[c.role].client_account
                || e.server != c.ready.server_account
                || e.state != "running"
                || !e.active
                || e.cancel_requested
                || e.terminal.is_some()
                || records::digest(e.session.as_bytes()) != binding.session_sha256
                || e.open_digest.as_ref() != Some(&binding.open_digest)
                || e.group_sha256 != binding.group_sha256
            {
                return Err(bad());
            }
        }
        let refusal = Refusal {
            binding: Binding {
                stream_sha256: records::digest(third_stream.as_bytes()),
                session_sha256: records::digest(refused_server.session.as_bytes()),
                open_digest: refused_client.open_digest.clone().ok_or_else(bad)?,
                group_sha256: refused_server.group_sha256.clone(),
            },
            server_state: refused_server.state.clone(),
            server_open_digest: refused_server.open_digest.clone(),
            server_active: refused_server.active,
            server_terminal: refused_server.terminal,
            server_cancel_requested: refused_server.cancel_requested,
            server_child_disposition: refused_server.child_disposition,
            marker_absent: !third_marker.try_exists()?,
            owned_runners_before: before.owned_runners,
            owned_runners_after: status.owned_runners,
        };
        refusal.validate()?;
        for child in &owned {
            trace.stage(Stage::Cancel);
            cancel(&c, &child.stream).await?;
        }
        for child in &mut owned {
            let ended = child.finish(c.deadline).await?;
            if ended.status.code() != Some(130) || !ended.stdout.is_empty() {
                return Err(bad());
            }
        }
        let cleanup = loop {
            if Instant::now() >= c.deadline {
                return Err(Error::Timeout);
            }
            let entries = streaming_cleanup::entries(
                *c.server.daemon.lock().await,
                c.server.socket,
                c.deadline,
            )
            .await?;
            if owned.iter().all(|child| {
                entries.iter().any(|e| {
                    e.stream == child.stream
                        && !e.active
                        && e.cancel_requested
                        && e.terminal == Some(Terminal::Cancelled)
                })
            }) {
                let s = streaming_cleanup::status(
                    *c.server.daemon.lock().await,
                    c.server.socket,
                    c.deadline,
                )
                .await?;
                if s.validate_idle().is_ok() {
                    break s;
                }
            }
            tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(50)))
                .await;
        };
        let observation = Observation {
            wall_ns: u64::try_from(start.elapsed().as_nanos()).map_err(|_| bad())?,
            held: bindings.try_into().map_err(|_| bad())?,
            refusal,
            marker_sha256: markers.try_into().map_err(|_| bad())?,
            active_at_refusal: status.active,
            unfinished_at_refusal: status.unfinished_runners,
            third_exit: third.status.code().ok_or_else(bad)?,
            cleanup,
        };
        trace.stage(Stage::Validation);
        observation.validate()?;
        Ok(observation)
    }
    .await;
    trace.stage(Stage::Cleanup);
    let mut cleanup_error = None;
    for child in &mut owned {
        if let Err(error) = child.abort().await {
            cleanup_error = Some(error);
        }
    }
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marker_source_contract_defers_initial_read_but_keeps_digest_strict() {
        // Wiring assertions supplement filesystem seams; they do not execute live IPC.
        let source = include_str!("installed_edge_quota.rs");
        let body = source
            .split("async fn started(")
            .nth(1)
            .unwrap()
            .split("async fn cancel(")
            .next()
            .unwrap();
        let helper = source
            .split("fn read_started_marker(")
            .nth(1)
            .unwrap()
            .split("async fn started(")
            .next()
            .unwrap();
        assert!(helper.contains("records::read_published(path, 8192)"));
        assert!(body.contains("read_started_marker(&path)?"));
        assert!(body.contains("records::digest(&records::read_bytes("));
        assert!(
            body.find("read_started_marker(&path)?").unwrap()
                < body.find("records::digest(&records::read_bytes(").unwrap()
        );
    }

    #[test]
    fn live_marker_defers_publication_then_reads_or_rejects_completed_json() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("marker");
        let staging = root.path().join("staging");
        std::fs::write(&path, b"malformed until publication completes").unwrap();
        std::fs::hard_link(&path, &staging).unwrap();
        assert!(read_started_marker(&path).unwrap().is_none());
        assert!(records::read::<serde_json::Value>(&path, 8192).is_err());
        std::fs::remove_file(&staging).unwrap();
        assert!(read_started_marker(&path).is_err());
        std::fs::write(
            &path,
            br#"{"schema":1,"nonce":"n","executable_sha256":"e","argv_sha256":"a"}"#,
        )
        .unwrap();
        std::fs::hard_link(&path, &staging).unwrap();
        assert!(read_started_marker(&path).unwrap().is_none());
        std::fs::remove_file(&staging).unwrap();
        assert!(read_started_marker(&path).unwrap().is_some());
    }

    #[test]
    fn quota_proof_requires_two_distinct_held_bindings_and_real_refusal() {
        let binding = |n| Binding {
            stream_sha256: hex::encode([n; 32]),
            session_sha256: "d".repeat(64),
            open_digest: "e".repeat(64),
            group_sha256: "f".repeat(64),
        };
        let mut value = Observation {
            wall_ns: 1,
            held: [binding(1), binding(2)],
            refusal: Refusal {
                binding: binding(3),
                server_state: "hello".into(),
                server_open_digest: None,
                server_active: false,
                server_terminal: None,
                server_cancel_requested: false,
                server_child_disposition: None,
                marker_absent: true,
                owned_runners_before: 2,
                owned_runners_after: 2,
            },
            marker_sha256: ["b".repeat(64), "c".repeat(64)],
            active_at_refusal: 2,
            unfinished_at_refusal: 2,
            third_exit: 255,
            cleanup: streaming_cleanup::StreamStatus {
                active: 0,
                cancelling: 0,
                uncertain: 0,
                owned_runners: 2,
                unfinished_runners: 0,
                observation_pending: false,
                cleanup_ready: true,
                stop_complete: true,
            },
        };
        value.validate().unwrap();
        value.third_exit = 0;
        assert!(value.validate().is_err());
        value.third_exit = 255;
        value.refusal.server_open_digest = Some("a".repeat(64));
        assert!(value.validate().is_err());
        value.refusal.server_open_digest = None;
        value.refusal.owned_runners_after = 3;
        assert!(value.validate().is_err());
        value.refusal.owned_runners_after = 2;
        value.refusal.marker_absent = false;
        assert!(value.validate().is_err());
        value.refusal.marker_absent = true;
        for corrupt in 0..7 {
            let mut invalid = value.clone();
            match corrupt {
                0 => invalid.refusal.server_state = "running".into(),
                1 => invalid.refusal.server_active = true,
                2 => invalid.refusal.server_terminal = Some(Terminal::Cancelled),
                3 => invalid.refusal.server_cancel_requested = true,
                4 => invalid.active_at_refusal = 1,
                5 => invalid.refusal.binding.open_digest = "invalid".into(),
                _ => invalid.third_exit = 130,
            }
            assert!(invalid.validate().is_err());
        }
        value.held[1] = value.held[0].clone();
        assert!(value.validate().is_err());
    }
}

#[cfg(test)]
mod public_destination_tests {
    use super::*;
    #[test]
    fn public_destination_builder_obeys_product_contract() {
        let account = "ab".repeat(32);
        let args = hold_args(&account, "nonce", "role").unwrap();
        records::assert_public_target(&args[2], &account);
        assert_eq!(
            args[3..],
            ["--", "./fixture/main", "edge-v1", "hold", "nonce", "role"]
        );
        let cancel = cancel_args(&account, "stream").unwrap();
        records::assert_public_target(&cancel[4], &account);
        assert_eq!(
            cancel[..4],
            ["--no-autostart", "cancel", "--stream", "stream"]
        );
    }
}
