//! Closed, finite evidence for installed stream qualification; never cleanup authority.
use super::{
    records::{self, Context, IdentityReady, ROLES},
    streaming_fixture, transfer_fixture,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
fn bad() -> Error {
    Error::Invalid("installed_stream_record")
}
pub(super) const COHORTS: [&str; 8] = [
    "stream-matrix",
    "rsync-matrix",
    "copy-matrix",
    "edge-stream",
    "edge-rsync",
    "edge-copy",
    "copy-interrupt-put",
    "copy-interrupt-get",
];
pub(super) fn is_streaming(name: &str) -> bool {
    COHORTS.contains(&name)
}
pub(super) fn cases(cohort: &str) -> Result<&'static [&'static str]> {
    match cohort {
        "stream-matrix" => Ok(&["duplex"]),
        "rsync-matrix" => Ok(&["push", "pull"]),
        "copy-matrix" => Ok(&["put", "get"]),
        "copy-interrupt-put" => Ok(&["interrupt-put"]),
        "copy-interrupt-get" => Ok(&["interrupt-get"]),
        _ => Err(bad()),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Budget {
    pub ready_by: u64,
    pub traffic_by: u64,
    pub stop_by: u64,
}
pub(super) fn budget(ctx: &Context, start: u64, now: u64) -> Result<Budget> {
    if !is_streaming(&ctx.cohort)
        || start < ctx.created_at
        || start > ctx.created_at.saturating_add(720)
        || ctx.expires_at.saturating_sub(start) < 1980
        || now < start
    {
        return Err(bad());
    }
    let ready_by = start.checked_add(180).ok_or_else(bad)?;
    let traffic_by =
        (ready_by.checked_add(1200).ok_or_else(bad)?).min(ctx.expires_at.saturating_sub(180));
    let stop_by = traffic_by.checked_add(180).ok_or_else(bad)?;
    if now >= ready_by || stop_by > start.checked_add(1800).ok_or_else(bad)? {
        return Err(bad());
    }
    Ok(Budget {
        ready_by,
        traffic_by,
        stop_by,
    })
}
pub(super) use super::matrix::pins;
pub(super) fn nonce(
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    client: &str,
    server: &str,
    case: &str,
) -> Result<String> {
    if !ROLES.contains(&client)
        || !ROLES.contains(&server)
        || !matches!(case, "duplex" | "push" | "pull" | "put" | "get")
    {
        return Err(bad());
    }
    Ok(records::digest(&records::canonical(&(
        "installed-stream-pair/1",
        ctx.digest()?,
        client,
        server,
        case,
        &ids[client].nonce,
        &ids[server].nonce,
    ))?))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RsyncPackage {
    pub formula: String,
    pub version: String,
    pub poured_from_bottle: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RsyncPin {
    pub path: String,
    pub version: String,
    pub sha256: String,
    pub acquisition: String,
    pub package: Option<RsyncPackage>,
}
impl RsyncPin {
    pub(super) fn validate(&self) -> Result<()> {
        if self.package.as_ref().is_some_and(|p| {
            p.formula != "rsync"
                || p.version.is_empty()
                || p.version.len() > 64
                || !p
                    .version
                    .bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'_'))
        }) || (self.acquisition == "homebrew") != self.package.is_some()
        {
            return Err(bad());
        }
        if !Path::new(&self.path).is_absolute()
            || self.path.len() > 4096
            || self.path.contains(['\n', '\r', '\0'])
            || !records::hexadecimal(&self.sha256, 64)
            || !maintained_version(&self.version)
            || !matches!(
                self.acquisition.as_str(),
                "hosted-prerequisite" | "homebrew"
            )
        {
            return Err(bad());
        }
        Ok(())
    }
}
pub(super) fn maintained_version(version: &str) -> bool {
    let Some(v) = version
        .split("version ")
        .nth(1)
        .and_then(|v| v.split_whitespace().next())
    else {
        return false;
    };
    let mut p = v.split('.');
    matches!((p.next(),p.next().and_then(|n|n.parse::<u32>().ok()),p.next().and_then(|n|n.parse::<u32>().ok()),p.next()),(Some("3"),Some(minor),Some(patch),None) if minor>2 || minor==2&&patch>=7)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServerReady {
    pub schema: u8,
    pub kind: String,
    pub context_sha256: String,
    pub role: String,
    pub identity_ready_sha256: BTreeMap<String, String>,
    pub server_account: String,
    pub started_at: u64,
    pub ready_at: u64,
    pub ready_by: u64,
    pub traffic_by: u64,
    pub stop_by: u64,
    pub fixture_sha256: String,
    pub root_label: String,
    pub rsync: Option<RsyncPin>,
}
impl ServerReady {
    pub(super) fn validate(
        &self,
        ctx: &Context,
        ids: &BTreeMap<String, IdentityReady>,
        role: &str,
    ) -> Result<()> {
        let b = budget(ctx, self.started_at, self.started_at)?;
        if !ROLES.contains(&role)
            || self.schema != 1
            || self.kind != "server-ready"
            || self.role != role
            || self.context_sha256 != ctx.digest()?
            || self.identity_ready_sha256 != pins(ids)?
            || self.server_account != ids[role].server_account
            || self.root_label != "transfer-v1"
            || !records::hexadecimal(&self.fixture_sha256, 64)
            || self.fixture_sha256
                != ctx.tooling[ROLES.iter().position(|r| *r == role).ok_or_else(bad)?]
                    .files
                    .fixture
                    .sha256
            || self.ready_at < self.started_at
            || self.ready_at >= self.ready_by
            || (self.ready_by, self.traffic_by, self.stop_by)
                != (b.ready_by, b.traffic_by, b.stop_by)
        {
            return Err(bad());
        }
        if let Some(pin) = &self.rsync {
            pin.validate()?;
        }
        if matches!(ctx.cohort.as_str(), "rsync-matrix" | "edge-rsync") != self.rsync.is_some() {
            return Err(bad());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pair {
    pub client: String,
    pub server: String,
    pub case: String,
    pub nonce: String,
    #[serde(rename = "stream_sha256")]
    pub stream: String,
    #[serde(rename = "session_sha256")]
    pub session: String,
    pub open_digest: String,
    pub group_sha256: String,
    pub stdin_bytes: usize,
    pub stdin_sha256: String,
    pub stdout_bytes: usize,
    pub stdout_sha256: String,
    pub stderr_bytes: usize,
    pub stderr_sha256: String,
    pub exit: u8,
    pub wall_ns: u64,
    pub complete: bool,
    pub failure: Option<String>,
    pub transfer: Option<TransferProof>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ControlProof {
    pub action: String,
    pub stream_sha256: String,
    pub session_sha256: String,
    pub open_digest: String,
    pub group_sha256: String,
    pub stdout_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TransferProof {
    pub client_items: Vec<transfer_fixture::Item>,
    pub controls: Vec<ControlProof>,
    pub items: Vec<transfer_fixture::Item>,
    pub payload_bytes: u64,
    pub rsync: Option<RsyncPin>,
    #[serde(rename = "transfer_sha256")]
    pub transfer_id: Option<String>,
    pub checkpoint: Option<super::copy_checkpoint::Proof>,
}
impl Pair {
    pub(super) fn validate(
        &self,
        ctx: &Context,
        ids: &BTreeMap<String, IdentityReady>,
    ) -> Result<()> {
        if ctx.cohort == "stream-matrix" {
            return self.validate_duplex(ctx, ids);
        }
        let cases = cases(&ctx.cohort)?;
        if !cases.contains(&self.case.as_str())
            || !ROLES.contains(&self.client.as_str())
            || !ROLES.contains(&self.server.as_str())
            || !self.complete
            || self.failure.is_some()
            || self.exit != 0
            || self.wall_ns == 0
            || self.wall_ns > 330_000_000_000
            || self.nonce != nonce(ctx, ids, &self.client, &self.server, &self.case)?
            || !records::hexadecimal(&self.stream, 64)
            || !records::hexadecimal(&self.session, 64)
            || !records::hexadecimal(&self.open_digest, 64)
            || !records::hexadecimal(&self.group_sha256, 64)
            || self.stdin_bytes != 0
            || self.stdin_sha256 != records::digest(&[])
            || self.stdout_bytes != 0
            || self.stderr_bytes != 0
            || self.stdout_sha256 != records::digest(&[])
            || self.stderr_sha256 != records::digest(&[])
        {
            return Err(bad());
        }
        let transfer = self.transfer.as_ref().ok_or_else(bad)?;
        if transfer.client_items != transfer.items
            || transfer.items != transfer_fixture::expected(&self.nonce, &self.case)?
            || transfer.payload_bytes != transfer.items.iter().map(|i| i.bytes).sum::<u64>()
            || (ctx.cohort == "rsync-matrix") != transfer.rsync.is_some()
            || (ctx.cohort == "copy-matrix") != transfer.transfer_id.is_some()
            || transfer
                .transfer_id
                .as_ref()
                .is_some_and(|id| !records::hexadecimal(id, 64))
        {
            return Err(bad());
        }
        if ctx.cohort == "copy-matrix" {
            let proof = transfer.checkpoint.as_ref().ok_or_else(bad)?;
            if proof.phase != "committed"
                || proof.size != transfer.payload_bytes
                || proof.durable_offset != proof.size
                || proof.prefix_sha256 != proof.sha256
                || transfer.items.len() != 1
                || transfer.items[0].sha256.as_ref() != Some(&proof.sha256)
                || Some(&proof.transfer_sha256) != transfer.transfer_id.as_ref()
                || proof.stream_sha256 != self.stream
                || !records::hexadecimal(&proof.manifest_sha256, 64)
            {
                return Err(bad());
            }
        } else if transfer.checkpoint.is_some() {
            return Err(bad());
        }
        let pre = if matches!(self.case.as_str(), "push" | "put") {
            "absent"
        } else {
            "init"
        };
        if transfer.controls.len() != 2
            || transfer.controls[0].action != pre
            || transfer.controls[1].action != "observe"
        {
            return Err(bad());
        }
        let fixture_hash = ctx.tooling[ROLES
            .iter()
            .position(|r| *r == self.server)
            .ok_or_else(bad)?]
        .files
        .fixture
        .sha256
        .clone();
        let receipt = transfer_fixture::Receipt {
            schema: 1,
            nonce: self.nonce.clone(),
            client: self.client.clone(),
            case: self.case.clone(),
            executable_sha256: fixture_hash,
            items: transfer.items.clone(),
            checkpoint: if self.case == "put" {
                transfer.checkpoint.clone()
            } else {
                None
            },
        };
        let observed_hash = records::digest(&records::canonical(&receipt)?);
        if transfer.controls[1].stdout_sha256 != observed_hash
            || transfer.controls[0].stdout_sha256
                != if pre == "absent" {
                    records::digest(&[])
                } else {
                    observed_hash
                }
        {
            return Err(bad());
        }
        for control in &transfer.controls {
            if [
                &control.stream_sha256,
                &control.session_sha256,
                &control.open_digest,
                &control.group_sha256,
                &control.stdout_sha256,
            ]
            .iter()
            .any(|v| !records::hexadecimal(v, 64))
            {
                return Err(bad());
            }
        }
        if let Some(pin) = &transfer.rsync {
            pin.validate()?;
        }
        Ok(())
    }
    pub(super) fn validate_duplex(
        &self,
        ctx: &Context,
        ids: &BTreeMap<String, IdentityReady>,
    ) -> Result<()> {
        if !ROLES.contains(&self.client.as_str())
            || !ROLES.contains(&self.server.as_str())
            || self.transfer.is_some()
            || self.case != "duplex"
            || !self.complete
            || self.failure.is_some()
            || self.nonce != nonce(ctx, ids, &self.client, &self.server, "duplex")?
            || !records::hexadecimal(&self.stream, 64)
            || !records::hexadecimal(&self.session, 64)
            || !records::hexadecimal(&self.open_digest, 64)
            || !records::hexadecimal(&self.group_sha256, 64)
            || self.stdin_bytes != streaming_fixture::STDIN_BYTES
            || self.stdout_bytes != streaming_fixture::STDOUT_BYTES
            || self.stderr_bytes != streaming_fixture::STDERR_BYTES
            || self.exit != 7
            || self.wall_ns == 0
            || self.wall_ns > 330_000_000_000
            || self.stdin_sha256
                != records::digest(&streaming_fixture::bytes(&self.nonce, 7, self.stdin_bytes)?)
            || self.stdout_sha256
                != records::digest(&streaming_fixture::bytes(
                    &self.nonce,
                    43,
                    self.stdout_bytes,
                )?)
            || self.stderr_sha256
                != records::digest(&streaming_fixture::bytes(
                    &self.nonce,
                    149,
                    self.stderr_bytes,
                )?)
        {
            return Err(bad());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FailedCell {
    pub server: String,
    pub case: String,
    pub status: String,
    pub category: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClientResult {
    pub schema: u8,
    pub kind: String,
    pub context_sha256: String,
    pub role: String,
    pub identity_ready_sha256: BTreeMap<String, String>,
    pub server_ready_sha256: BTreeMap<String, String>,
    pub pairs: Vec<Pair>,
    pub failed_cells: Vec<FailedCell>,
    pub interruption: Option<super::copy_interrupt::Evidence>,
    pub edges: Option<super::edges::Evidence>,
    pub complete: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServerReceipt {
    pub client: String,
    pub case: String,
    #[serde(rename = "stream_sha256")]
    pub stream: String,
    #[serde(rename = "session_sha256")]
    pub session: String,
    pub open_digest: String,
    pub group_sha256: String,
    pub fixture: Option<streaming_fixture::Receipt>,
    pub transfer: Option<transfer_fixture::Receipt>,
    pub controls: Vec<ControlProof>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServerResult {
    pub schema: u8,
    pub kind: String,
    pub context_sha256: String,
    pub role: String,
    pub identity_ready_sha256: BTreeMap<String, String>,
    pub server_ready_sha256: String,
    pub receipts: Vec<ServerReceipt>,
    pub interruption: Option<super::copy_interrupt::Evidence>,
    pub edges: Option<super::edges::Evidence>,
    pub stream_cleanup_complete: bool,
    pub server_stop_complete: bool,
    pub client_stop_complete: bool,
    pub complete: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rsync_minimum_rejects_bundled_old_or_malformed_versions() {
        for v in [
            "rsync version 3.2.7 protocol version 31",
            "rsync version 3.4.1 protocol version 32",
        ] {
            assert!(maintained_version(v));
        }
        for v in [
            "rsync version 2.6.9",
            "rsync version 3.2.6",
            "rsync version 4.0.0",
            "3.2.7",
            "rsync version 3.2.7.bad",
        ] {
            assert!(!maintained_version(v));
        }
    }
    #[test]
    fn server_budget_preserves_readiness_traffic_and_physical_cleanup() {
        let mut c = records::tests::ctx();
        c.cohort = "stream-matrix".into();
        c.created_at = 1000;
        c.expires_at = 4300;
        assert_eq!(
            budget(&c, 1720, 1720).unwrap(),
            Budget {
                ready_by: 1900,
                traffic_by: 3100,
                stop_by: 3280
            }
        );
        assert!(budget(&c, 1721, 1721).is_err());
        assert!(budget(&c, 1720, 1900).is_err());
        c.expires_at = 3699;
        assert!(budget(&c, 1720, 1720).is_err());
    }
}
