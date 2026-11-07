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
#[derive(Clone, Default, Serialize)]
pub(super) struct DuplexChecks {
    pub client_role: Option<bool>,
    pub server_role: Option<bool>,
    pub transfer_absent: Option<bool>,
    pub case: Option<bool>,
    pub complete: Option<bool>,
    pub failure_absent: Option<bool>,
    pub nonce: Option<bool>,
    pub stream_hex: Option<bool>,
    pub session_hex: Option<bool>,
    pub open_digest_hex: Option<bool>,
    pub group_sha256_hex: Option<bool>,
    pub stdin_count: Option<bool>,
    pub stdout_count: Option<bool>,
    pub stderr_count: Option<bool>,
    pub exit7: Option<bool>,
    pub wall_ns: Option<bool>,
    pub stdin_hash: Option<bool>,
    pub stdout_hash: Option<bool>,
    pub stderr_hash: Option<bool>,
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
        self.check_duplex(ctx, ids).1
    }
    pub(super) fn check_duplex(
        &self,
        ctx: &Context,
        ids: &BTreeMap<String, IdentityReady>,
    ) -> (DuplexChecks, Result<()>) {
        let mut checks = DuplexChecks::default();
        let result = (|| {
            macro_rules! check {
                ($field:ident, $value:expr) => {
                    let valid = $value;
                    checks.$field = Some(valid);
                    if !valid {
                        return Err(bad());
                    }
                };
            }
            check!(client_role, ROLES.contains(&self.client.as_str()));
            check!(server_role, ROLES.contains(&self.server.as_str()));
            check!(transfer_absent, self.transfer.is_none());
            check!(case, self.case == "duplex");
            check!(complete, self.complete);
            check!(failure_absent, self.failure.is_none());
            check!(
                nonce,
                self.nonce == nonce(ctx, ids, &self.client, &self.server, "duplex")?
            );
            check!(stream_hex, records::hexadecimal(&self.stream, 64));
            check!(session_hex, records::hexadecimal(&self.session, 64));
            check!(open_digest_hex, records::hexadecimal(&self.open_digest, 64));
            check!(
                group_sha256_hex,
                records::hexadecimal(&self.group_sha256, 64)
            );
            check!(
                stdin_count,
                self.stdin_bytes == streaming_fixture::STDIN_BYTES
            );
            check!(
                stdout_count,
                self.stdout_bytes == streaming_fixture::STDOUT_BYTES
            );
            check!(
                stderr_count,
                self.stderr_bytes == streaming_fixture::STDERR_BYTES
            );
            check!(exit7, self.exit == 7);
            check!(wall_ns, self.wall_ns > 0 && self.wall_ns <= 330_000_000_000);
            check!(
                stdin_hash,
                self.stdin_sha256
                    == records::digest(&streaming_fixture::bytes(
                        &self.nonce,
                        7,
                        self.stdin_bytes
                    )?)
            );
            check!(
                stdout_hash,
                self.stdout_sha256
                    == records::digest(&streaming_fixture::bytes(
                        &self.nonce,
                        43,
                        self.stdout_bytes
                    )?)
            );
            check!(
                stderr_hash,
                self.stderr_sha256
                    == records::digest(&streaming_fixture::bytes(
                        &self.nonce,
                        149,
                        self.stderr_bytes
                    )?)
            );
            Ok(())
        })();
        (checks, result)
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

#[cfg(test)]
mod duplex_observation_tests {
    use super::*;
    fn fixture() -> (Context, BTreeMap<String, IdentityReady>, Pair) {
        let ctx = records::tests::ctx();
        let ids = ROLES
            .into_iter()
            .map(|role| {
                (
                    role.into(),
                    IdentityReady {
                        schema: 1,
                        kind: "identity-ready".into(),
                        context_sha256: String::new(),
                        role: role.into(),
                        nonce: "a".repeat(64),
                        client_account: "b".repeat(64),
                        server_account: "c".repeat(64),
                        installation: records::Installation {
                            version: String::new(),
                            target: String::new(),
                            source_commit: String::new(),
                            manifest_sha256: String::new(),
                            bootstrap_sha256: String::new(),
                            files: BTreeMap::new(),
                        },
                        identity_ready_at: 0,
                    },
                )
            })
            .collect();
        let nonce = nonce(&ctx, &ids, ROLES[0], ROLES[2], "duplex").unwrap();
        let pair = Pair {
            client: ROLES[0].into(),
            server: ROLES[2].into(),
            case: "duplex".into(),
            nonce: nonce.clone(),
            stream: "a".repeat(64),
            session: "b".repeat(64),
            open_digest: "c".repeat(64),
            group_sha256: "d".repeat(64),
            stdin_bytes: streaming_fixture::STDIN_BYTES,
            stdout_bytes: streaming_fixture::STDOUT_BYTES,
            stderr_bytes: streaming_fixture::STDERR_BYTES,
            stdin_sha256: records::digest(
                &streaming_fixture::bytes(&nonce, 7, streaming_fixture::STDIN_BYTES).unwrap(),
            ),
            stdout_sha256: records::digest(
                &streaming_fixture::bytes(&nonce, 43, streaming_fixture::STDOUT_BYTES).unwrap(),
            ),
            stderr_sha256: records::digest(
                &streaming_fixture::bytes(&nonce, 149, streaming_fixture::STDERR_BYTES).unwrap(),
            ),
            exit: 7,
            wall_ns: 1,
            complete: true,
            failure: None,
            transfer: None,
        };
        (ctx, ids, pair)
    }
    #[test]
    fn shared_checks_pin_every_predicate_short_circuit_and_serialized_pair_parity() {
        let (ctx, ids, pair) = fixture();
        let original = serde_json::to_value(&pair).unwrap();
        assert!(pair.validate_duplex(&ctx, &ids).is_ok());
        let (checks, result) = pair.check_duplex(&ctx, &ids);
        assert!(result.is_ok());
        assert!(
            serde_json::to_value(checks)
                .unwrap()
                .as_object()
                .unwrap()
                .values()
                .all(|v| v == true)
        );
        assert_eq!(serde_json::to_value(&pair).unwrap(), original);
        let cases = [
            ("client", "client_role", serde_json::json!("invalid")),
            ("server", "server_role", serde_json::json!("invalid")),
            (
                "transfer",
                "transfer_absent",
                serde_json::json!({"client_items":[],"controls":[],"items":[],"payload_bytes":0,"rsync":null,"transfer_sha256":null,"checkpoint":null}),
            ),
            ("case", "case", serde_json::json!("other")),
            ("complete", "complete", serde_json::json!(false)),
            ("failure", "failure_absent", serde_json::json!("private")),
            ("nonce", "nonce", serde_json::json!("x")),
            ("stream_sha256", "stream_hex", serde_json::json!("x")),
            ("session_sha256", "session_hex", serde_json::json!("x")),
            ("open_digest", "open_digest_hex", serde_json::json!("x")),
            ("group_sha256", "group_sha256_hex", serde_json::json!("x")),
            ("stdin_bytes", "stdin_count", serde_json::json!(usize::MAX)),
            (
                "stdout_bytes",
                "stdout_count",
                serde_json::json!(usize::MAX),
            ),
            (
                "stderr_bytes",
                "stderr_count",
                serde_json::json!(usize::MAX),
            ),
            ("exit", "exit7", serde_json::json!(0)),
            ("wall_ns", "wall_ns", serde_json::json!(0)),
            ("stdin_sha256", "stdin_hash", serde_json::json!("x")),
            ("stdout_sha256", "stdout_hash", serde_json::json!("x")),
            ("stderr_sha256", "stderr_hash", serde_json::json!("x")),
        ];
        let order: Vec<_> = cases.iter().map(|(_, check, _)| *check).collect();
        for (index, (field, check, value)) in cases.into_iter().enumerate() {
            let mut value_pair = original.clone();
            value_pair[field] = value;
            let changed: Pair = serde_json::from_value(value_pair).unwrap();
            let (checks, result) = changed.check_duplex(&ctx, &ids);
            assert!(
                matches!(result, Err(Error::Invalid("installed_stream_record"))),
                "{field}"
            );
            assert!(matches!(
                changed.validate_duplex(&ctx, &ids),
                Err(Error::Invalid("installed_stream_record"))
            ));
            let checks = serde_json::to_value(checks).unwrap();
            assert_eq!(checks[check], false, "{field}");
            for later in &order[index + 1..] {
                assert!(
                    checks[*later].is_null(),
                    "later predicate {later} after {field}"
                );
            }
            assert_eq!(
                checks
                    .as_object()
                    .unwrap()
                    .values()
                    .filter(|v| **v == false)
                    .count(),
                1
            );
        }
    }
    #[test]
    fn early_failures_do_not_index_missing_identity_or_generate_untrusted_sizes() {
        let (ctx, ids, mut pair) = fixture();
        pair.client = "invalid".into();
        let (checks, result) = pair.check_duplex(&ctx, &BTreeMap::new());
        assert!(result.is_err());
        assert_eq!(checks.server_role, None);
        assert_eq!(checks.nonce, None);
        pair.client = ROLES[0].into();
        pair.stdin_bytes = usize::MAX;
        let (checks, result) = pair.check_duplex(&ctx, &ids);
        assert!(result.is_err());
        assert_eq!(checks.stdin_count, Some(false));
        assert_eq!(checks.stdin_hash, None);
        assert_eq!(checks.stdout_count, None);
        pair.stdin_bytes = streaming_fixture::STDIN_BYTES;
        let mut invalid_ctx = ctx;
        invalid_ctx.schema = 0;
        assert!(matches!(
            pair.check_duplex(&invalid_ctx, &ids).1,
            Err(Error::Invalid("installed_stream_record"))
        ));
    }
}
