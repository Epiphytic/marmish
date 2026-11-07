//! Installed natural-exit qualification. Public measurements never authorize cleanup.
use super::super::command_check::EngineTree;
use super::{
    cleanup, fixture, prepare,
    process::{self, Environment},
    records::{self, Context, IdentityReady, Installation, ROLES, TARGETS},
};
use crate::{
    Error, Result,
    command_client::CommandHandle,
    command_local::{Cursors, LocalResult},
    command_wire::{self as wire, RequestOp, ResponseResult},
    ipc,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nostr::nips::nip19::ToBech32;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Child, task::JoinHandle, time::Instant};
const STREAM: usize = 131072;
const RECORD_CAP: u64 = 65536;
const CLIENTS: [&str; 3] = ["client", "client-2", "client-3"];
fn bad() -> Error {
    Error::Invalid("installed_autoexit")
}
fn null<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
macro_rules! closed {($n:ident{$($v:ident),+})=>{#[derive(Clone,Copy,PartialEq,Eq,Serialize,Deserialize)] #[serde(rename_all="snake_case")] pub(super) enum $n{$($v),+}};}
closed!(Stage {
    Prepare,
    Setup,
    ServerStart,
    Clients,
    AwaitExit,
    Restart,
    Evidence,
    Remove,
    Stop,
    Complete
});
closed!(Category {
    Setup,
    Readiness,
    Submission,
    ResponseBinding,
    Output,
    Terminal,
    CountCutoff,
    NaturalExit,
    Evidence,
    Cleanup,
    Deadline,
    Native
});
closed!(FinishDisposition {
    NotRequested,
    Pending,
    Confirmed,
    Rejected
});
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OriginalStop {
    schema: u8,
    engine_quiesced: bool,
    engine_stop_complete: bool,
    mdk_stop_complete: bool,
    runtime_cleanup_complete: bool,
}
impl OriginalStop {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let mut lines = bytes.split(|v| *v == b'\n').filter(|s| !s.is_empty());
        let v: Self = serde_json::from_slice(lines.next().ok_or_else(bad)?).map_err(|_| bad())?;
        if lines.next().is_some() || !v.joined() {
            return Err(bad());
        }
        Ok(v)
    }
    fn joined(&self) -> bool {
        self.schema == 1
            && self.engine_stop_complete
            && self.mdk_stop_complete
            && self.runtime_cleanup_complete
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigPin {
    profile: String,
    sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Terminal {
    group_submit_sha256: String,
    client_account: String,
    server_account: String,
    session: String,
    invocation: String,
    submit_sha256: String,
    terminal_poll_request_id: String,
    terminal_poll_request_sha256: String,
    terminal_response_sha256: String,
    stdout_bytes: usize,
    stdout_sha256: String,
    stderr_bytes: usize,
    stderr_sha256: String,
    outcome: ExitOutcome,
    gap_free: bool,
    authority_valid: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExitOutcome {
    kind: String,
    value: u8,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    client_account: String,
    invocation: String,
    session: String,
    submit_sha256: String,
    engine_admitted: u8,
    outcome: ExitOutcome,
    observed_seq: u64,
    authority_valid: bool,
    worker_exited: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cleanup {
    original_server_reaped: bool,
    clients_joined: bool,
    engine_removed: bool,
    post_unlink_verified: bool,
    cleanup_instance_joined: bool,
    drains_joined: bool,
    workspace_removed: bool,
}
impl Cleanup {
    fn complete(&self) -> bool {
        self.original_server_reaped
            && self.clients_joined
            && self.engine_removed
            && self.post_unlink_verified
            && self.cleanup_instance_joined
            && self.drains_joined
            && self.workspace_removed
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CohortResult {
    schema: u8,
    kind: String,
    context_sha256: String,
    identity_ready_sha256: String,
    role: String,
    cohort: String,
    installation: Installation,
    tooling_manifest_sha256: String,
    complete: bool,
    stage: Stage,
    #[serde(deserialize_with = "null")]
    category: Option<Category>,
    context_age_at_entry_ms: u64,
    work_budget_at_entry_ms: u64,
    config_sha256: Vec<ConfigPin>,
    elapsed_ms: u64,
    #[serde(deserialize_with = "null")]
    launch_to_ready_ms: Option<u64>,
    #[serde(deserialize_with = "null")]
    natural_exit_ms: Option<u64>,
    #[serde(deserialize_with = "null")]
    uptime_exit_upper_ms: Option<u64>,
    #[serde(deserialize_with = "null")]
    original_stop: Option<OriginalStop>,
    signal_sent: bool,
    count_exit_ok: bool,
    uptime_exit_ok: bool,
    planned_admissions: u8,
    terminals: Vec<Option<Terminal>>,
    receipt_sha256: Vec<Option<String>>,
    evidence: Vec<Evidence>,
    #[serde(deserialize_with = "null")]
    final_seq: Option<u64>,
    finish: Vec<FinishDisposition>,
    cleanup: Cleanup,
}
fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}
fn hex(s: &str, n: usize) -> bool {
    records::hexadecimal(s, n)
}
fn budget(cohort: &str, remaining: u64, setup: bool) -> Result<()> {
    let need = match (cohort, setup) {
        ("autoexit", true) => 2925,
        ("autoexit", false) => 2325,
        ("uptime", true) => 1865,
        ("uptime", false) => 1265,
        _ => return Err(bad()),
    };
    if remaining < need {
        return Err(Error::Timeout);
    }
    Ok(())
}
impl CohortResult {
    pub(super) fn is_complete(&self) -> bool {
        self.complete
    }
    fn new(ctx: &Context, id: &IdentityReady, now: u64) -> Result<Self> {
        let count = if ctx.cohort == "autoexit" {
            3
        } else if ctx.cohort == "uptime" {
            0
        } else {
            return Err(bad());
        };
        Ok(Self {
            schema: 1,
            kind: "cohort-result".into(),
            context_sha256: ctx.digest()?,
            identity_ready_sha256: records::digest(&records::canonical(id)?),
            role: id.role.clone(),
            cohort: ctx.cohort.clone(),
            installation: id.installation.clone(),
            tooling_manifest_sha256: ctx.tooling_manifests[&id.role].clone(),
            complete: false,
            stage: Stage::Prepare,
            category: None,
            context_age_at_entry_ms: now.saturating_sub(ctx.created_at) * 1000,
            work_budget_at_entry_ms: ctx.expires_at.saturating_sub(now) * 1000,
            config_sha256: Vec::new(),
            elapsed_ms: 0,
            launch_to_ready_ms: None,
            natural_exit_ms: None,
            uptime_exit_upper_ms: None,
            original_stop: None,
            signal_sent: false,
            count_exit_ok: false,
            uptime_exit_ok: false,
            planned_admissions: count,
            terminals: vec![None; count as usize],
            receipt_sha256: vec![None; count as usize],
            evidence: Vec::new(),
            final_seq: None,
            finish: vec![FinishDisposition::NotRequested; count as usize],
            cleanup: Cleanup::default(),
        })
    }
    fn fail(&mut self, c: Category) {
        if self.category.is_none() {
            self.category = Some(c)
        }
    }
}
/// Validate the actual uploaded identity, not a self-declared digest in a result.
pub(super) fn validate_record(
    r: &CohortResult,
    ctx: &Context,
    role: &str,
    id: &IdentityReady,
) -> Result<()> {
    ctx.validate()?;
    id.validate(ctx, role)?;
    if !matches!(ctx.cohort.as_str(), "autoexit" | "uptime")
        || r.schema != 1
        || r.kind != "cohort-result"
        || r.context_sha256 != ctx.digest()?
        || r.identity_ready_sha256 != records::digest(&records::canonical(id)?)
        || r.role != role
        || r.cohort != ctx.cohort
        || records::canonical(&r.installation)? != records::canonical(&id.installation)?
        || r.tooling_manifest_sha256 != ctx.tooling_manifests[role]
        || records::canonical(r)?.len() > RECORD_CAP as usize
    {
        return Err(bad());
    }
    let n = if r.cohort == "autoexit" { 3 } else { 0 };
    if r.planned_admissions != n
        || r.terminals.len() != n as usize
        || r.receipt_sha256.len() != n as usize
        || r.finish.len() != n as usize
        || r.evidence.len() > n as usize
        || r.final_seq == Some(0)
        || [
            Some(r.context_age_at_entry_ms),
            Some(r.work_budget_at_entry_ms),
            Some(r.elapsed_ms),
            r.launch_to_ready_ms,
            r.natural_exit_ms,
            r.uptime_exit_upper_ms,
        ]
        .into_iter()
        .flatten()
        .any(|v| v > 3300000)
    {
        return Err(bad());
    }
    let expected = if n == 3 {
        vec!["client", "client-2", "client-3", "server"]
    } else {
        vec!["client", "server"]
    };
    if r.config_sha256.len() > expected.len()
        || r.config_sha256
            .iter()
            .map(|p| p.profile.as_str())
            .ne(expected.iter().copied().take(r.config_sha256.len()))
        || r.config_sha256.iter().any(|p| !hex(&p.sha256, 64))
        || r.receipt_sha256.iter().flatten().any(|h| !hex(h, 64))
    {
        return Err(bad());
    }
    let mut peers = BTreeSet::from([id.server_account.clone()]);
    let mut keys = BTreeSet::new();
    for (i, t) in r
        .terminals
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.as_ref().map(|t| (i, t)))
    {
        let nonce = nonce(ctx, role, &t.client_account, &t.server_account, i as u8)?;
        let (_, out, err) = fixture::bytes(&nonce, i as u8)?;
        if !hex(&t.client_account, 64)
            || t.server_account != id.server_account
            || !peers.insert(t.client_account.clone())
            || !hex(&t.session, 32)
            || !hex(&t.invocation, 32)
            || !keys.insert(t.invocation.clone())
            || !hex(&t.submit_sha256, 64)
            || !hex(&t.group_submit_sha256, 64)
            || t.group_submit_sha256 == t.submit_sha256
            || !hex(&t.terminal_poll_request_id, 32)
            || !hex(&t.terminal_poll_request_sha256, 64)
            || !hex(&t.terminal_response_sha256, 64)
            || t.stdout_bytes != STREAM
            || t.stderr_bytes != STREAM
            || t.stdout_sha256 != records::digest(&out)
            || t.stderr_sha256 != records::digest(&err)
            || t.outcome.kind != "exit"
            || t.outcome.value != 7
            || !t.gap_free
            || !t.authority_valid
        {
            return Err(bad());
        }
    }
    if r.terminals
        .first()
        .and_then(Option::as_ref)
        .is_some_and(|t| t.client_account != id.client_account)
    {
        return Err(bad());
    }
    let mut last = None;
    for e in &r.evidence {
        let key = (&e.client_account, &e.invocation);
        if last.is_some_and(|l| l >= key)
            || e.engine_admitted != 1
            || e.outcome.kind != "exit"
            || e.outcome.value != 7
            || !e.authority_valid
            || !e.worker_exited
            || e.observed_seq == 0
            || r.final_seq.is_none_or(|f| e.observed_seq < f)
            || !r.terminals.iter().flatten().any(|t| {
                t.client_account == e.client_account
                    && t.invocation == e.invocation
                    && t.session == e.session
                    && t.group_submit_sha256 == e.submit_sha256
            })
        {
            return Err(bad());
        }
        last = Some(key);
    }
    if r.complete {
        let lifetime = ctx.expires_at.checked_sub(ctx.created_at).ok_or_else(bad)? * 1000;
        if r.work_budget_at_entry_ms < if n == 3 { 2925000 } else { 1865000 }
            || r.context_age_at_entry_ms
                .checked_add(r.work_budget_at_entry_ms)
                != Some(lifetime)
            || r.context_age_at_entry_ms
                .checked_add(r.elapsed_ms)
                .is_none_or(|v| v > lifetime)
        {
            return Err(bad());
        }
        let natural = r.natural_exit_ms.ok_or_else(bad)?;
        let ready = r
            .launch_to_ready_ms
            .filter(|v| *v <= 150000)
            .ok_or_else(bad)?;
        let upper = ready + if n == 3 { 1235000 } else { 215000 };
        if r.stage != Stage::Complete
            || r.category.is_some()
            || r.signal_sent
            || !r.cleanup.complete()
            || r.original_stop.as_ref().is_none_or(|s| !s.joined())
            || r.config_sha256.len() != expected.len()
            || r.uptime_exit_upper_ms != Some(upper)
            || natural > upper
            || r.final_seq.is_none()
        {
            return Err(bad());
        }
        if n == 3 {
            if !r.count_exit_ok
                || r.uptime_exit_ok
                || natural > 600000
                || r.terminals.iter().any(Option::is_none)
                || r.receipt_sha256.iter().any(Option::is_none)
                || r.evidence.len() != 3
                || r.finish.iter().any(|f| {
                    !matches!(f, FinishDisposition::Pending | FinishDisposition::Confirmed)
                })
            {
                return Err(bad());
            }
        } else if !r.uptime_exit_ok || r.count_exit_ok || natural < 180000 || !r.evidence.is_empty()
        {
            return Err(bad());
        }
    } else if r.category.is_none() {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn validate(
    r: &CohortResult,
    ctx: &Context,
    role: &str,
    id: &IdentityReady,
) -> Result<()> {
    validate_record(r, ctx, role, id)?;
    if !r.complete {
        return Err(bad());
    }
    Ok(())
}
fn nonce(ctx: &Context, role: &str, client: &str, server: &str, index: u8) -> Result<String> {
    Ok(records::digest(&records::canonical(&(
        "installed-autoexit/1",
        ctx.digest()?,
        role,
        client,
        server,
        index,
    ))?))
}
fn base(workspace: &Path, name: &str, socket: &Path, ctx: &Context) -> Vec<OsString> {
    super::matrix::base(workspace, name, socket, &ctx.relays)
}
fn require_new_engine(profile: &Path) -> Result<()> {
    cleanup::require_no_engine(profile)
}
type PipeTask = JoinHandle<std::io::Result<(Vec<u8>, bool)>>;
async fn drain_pair(
    out: &mut Option<PipeTask>,
    err: &mut Option<PipeTask>,
    deadline: Instant,
) -> Result<(Vec<u8>, Vec<u8>)> {
    async fn joined(slot: &mut Option<PipeTask>, deadline: Instant) -> Result<Vec<u8>> {
        let mut handle = slot.take().ok_or_else(bad)?;
        match tokio::time::timeout_at(deadline, &mut handle).await {
            Ok(value) => {
                let (bytes, overflow) = value.map_err(|_| bad())??;
                if overflow {
                    return Err(bad());
                }
                Ok(bytes)
            }
            Err(_) => {
                handle.abort();
                let _ = handle.await;
                Err(Error::Timeout)
            }
        }
    }
    let (out, err) = tokio::join!(joined(out, deadline), joined(err, deadline));
    Ok((out?, err?))
}
struct Helper {
    child: Child,
    out: Option<PipeTask>,
    err: Option<PipeTask>,
    joined: bool,
}
#[derive(Clone, Default)]
struct Helpers(Arc<Mutex<Vec<Arc<tokio::sync::Mutex<Helper>>>>>);
impl Helpers {
    async fn run(
        &self,
        executable: &Path,
        args: &[OsString],
        cwd: &Path,
        env: &Environment,
        deadline: Instant,
        input: Option<&[u8]>,
    ) -> Result<process::Output> {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let mut command = process::command(executable, args, cwd, env);
        if input.is_some() {
            command.stdin(std::process::Stdio::piped());
        }
        let mut child = command.spawn()?;
        let out = tokio::spawn(process::drain(child.stdout.take().ok_or_else(bad)?, 65536));
        let err = tokio::spawn(process::drain(child.stderr.take().ok_or_else(bad)?, 65536));
        let owned = Arc::new(tokio::sync::Mutex::new(Helper {
            child,
            out: Some(out),
            err: Some(err),
            joined: false,
        }));
        self.0.lock().map_err(|_| bad())?.push(owned.clone());
        let mut helper = owned.lock().await;
        let result = tokio::time::timeout_at(deadline, async {
            if let Some(input) = input {
                let mut stdin = helper.child.stdin.take().ok_or_else(bad)?;
                stdin.write_all(input).await?;
                stdin.shutdown().await?;
            }
            helper.child.wait().await.map_err(Error::from)
        })
        .await;
        let status = match result {
            Ok(Ok(status)) => Ok(status),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(Error::Timeout),
        };
        if status.is_err() {
            tokio::time::timeout_at(deadline, process::terminate(&mut helper.child))
                .await
                .map_err(|_| Error::Timeout)??;
        }
        let Helper { out, err, .. } = &mut *helper;
        let (stdout, stderr) = drain_pair(out, err, deadline).await?;
        helper.joined = true;
        Ok(process::Output {
            status: status?,
            stdout,
            stderr,
        })
    }
    async fn stop(&self, deadline: Instant) -> bool {
        let entries = match self.0.lock() {
            Ok(entries) => entries.clone(),
            Err(_) => return false,
        };
        let mut okay = true;
        for entry in entries {
            let mut helper = entry.lock().await;
            if helper.joined {
                continue;
            }
            let reaped = matches!(
                tokio::time::timeout_at(deadline, process::terminate(&mut helper.child)).await,
                Ok(Ok(()))
            );
            let Helper { out, err, .. } = &mut *helper;
            let drained = drain_pair(out, err, deadline).await.is_ok();
            helper.joined = reaped && drained;
            okay &= helper.joined;
        }
        okay
    }
}
struct Daemon {
    child: Child,
    out: Option<PipeTask>,
    err: Option<PipeTask>,
    socket: PathBuf,
    profile: PathBuf,
    launch: Instant,
}
impl Daemon {
    fn start(
        w: &Path,
        name: &str,
        socket: PathBuf,
        ctx: &Context,
        env: &Environment,
        uptime: u64,
        count: Option<u64>,
    ) -> Result<Self> {
        if socket.as_os_str().as_bytes().len() > 104 {
            return Err(bad());
        }
        let mut a = base(w, name, &socket, ctx);
        a.extend(["--max-uptime".into(), uptime.to_string().into()]);
        if let Some(c) = count {
            a.extend(["--max-commands".into(), c.to_string().into()])
        }
        a.extend(["--foreground".into(), "daemon".into()]);
        let launch = Instant::now();
        let mut child = process::command(&w.join("bin/marmishd"), &a, w, env).spawn()?;
        let out = tokio::spawn(process::drain(child.stdout.take().ok_or_else(bad)?, 65536));
        let err = tokio::spawn(process::drain(child.stderr.take().ok_or_else(bad)?, 65536));
        Ok(Self {
            child,
            out: Some(out),
            err: Some(err),
            socket,
            profile: w.join("profiles").join(name),
            launch,
        })
    }
    async fn ready(&mut self, deadline: Instant) -> Result<()> {
        loop {
            if self.child.try_wait()?.is_some() || Instant::now() >= deadline {
                return Err(bad());
            }
            if let Ok(v) = cleanup::request::<serde_json::Value>(
                &mut self.child,
                &self.socket,
                &ipc::Request::Status {},
                deadline,
            )
            .await
                && v["state"] == "active"
            {
                return Ok(());
            }
            tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(100)))
                .await;
        }
    }
    async fn drains(&mut self, deadline: Instant) -> Result<Vec<u8>> {
        drain_pair(&mut self.out, &mut self.err, deadline)
            .await
            .map(|pair| pair.0)
    }
    async fn force(&mut self, deadline: Instant) -> Result<()> {
        let result = tokio::time::timeout_at(deadline, process::terminate(&mut self.child))
            .await
            .map_err(|_| Error::Timeout)?;
        let drains = self.drains(deadline).await;
        result?;
        drains?;
        Ok(())
    }
    async fn stop_no_engine(
        &mut self,
        receipt: &cleanup::ProfileReceipt,
        deadline: Instant,
    ) -> Result<()> {
        receipt.verify(&self.profile)?;
        cleanup::verify_no_engine(&mut self.child, &self.profile, &self.socket, deadline).await?;
        cleanup::stop(
            &mut self.child,
            deadline.min(Instant::now() + Duration::from_secs(30)),
        )
        .await?;
        let bytes = self.drains(deadline).await?;
        cleanup::no_engine_stop_report(&bytes)?;
        receipt.verify(&self.profile)?;
        cleanup::require_no_engine(&self.profile)
    }
    async fn stop(&mut self, deadline: Instant) -> Result<()> {
        cleanup::stop(
            &mut self.child,
            deadline.min(Instant::now() + Duration::from_secs(30)),
        )
        .await?;
        let bytes = self.drains(deadline).await?;
        cleanup::stop_report(&bytes)
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(h) = &self.out {
            h.abort()
        }
        if let Some(h) = &self.err {
            h.abort()
        }
    }
}
pub(super) fn socket(w: &Path, name: &str, env: &Environment) -> Result<PathBuf> {
    crate::identity::validate_name(name)?;
    crate::identity::check_dir(&env.temporary)?;
    if !env.temporary.is_absolute() || env.temporary.canonicalize()? != env.temporary {
        return Err(bad());
    }
    let p: serde_json::Value =
        records::read(&w.join("profiles").join(name).join("profile.json"), 65536)?;
    let id = p["profile_id"]
        .as_str()
        .filter(|s| hex(s, 32))
        .ok_or_else(bad)?;
    let runtime = env
        .temporary
        .join(format!("mr2-{}-{id}", unsafe { libc::geteuid() }));
    let path = runtime.join("s");
    if runtime.as_os_str().as_bytes().len() + 10 >= 104 || path.as_os_str().as_bytes().len() >= 104
    {
        eprintln!("marmish-installed-check: socket_path_too_long");
        return Err(Error::Invalid("installed_socket_path_too_long"));
    }
    Ok(path)
}
fn setup_valid(s: &prepare::Setup, name: &str, new: bool, granted: bool) -> bool {
    s.schema == 1
        && s.identity == name
        && s.backend == "file"
        && s.role == if name == "server" { "server" } else { "client" }
        && s.inbound_enabled == (name == "server" && granted)
        && s.config_changed
        && s.identity_created == new
        && s.owner_opened == new
        && s.engine_stop_complete
        && s.mdk_stop_complete
        && s.runtime_cleanup_complete
}
async fn setup(
    w: &Path,
    ctx: &Context,
    env: &Environment,
    name: &str,
    peers: Option<&[String]>,
    helpers: &Helpers,
    deadline: Instant,
) -> Result<String> {
    let mut args = base(w, name, &w.join("unused.sock"), ctx);
    args.push("setup".into());
    if let Some(peers) = peers {
        args.push("--update".into());
        for peer in peers {
            args.extend([
                if name == "server" {
                    "--grant"
                } else {
                    "--trust"
                }
                .into(),
                nostr::key::PublicKey::from_hex(peer)
                    .map_err(|_| bad())?
                    .to_bech32()
                    .map_err(|_| bad())?
                    .into(),
            ]);
        }
    } else {
        args.extend(["--role".into(), "client".into(), "--no-grants".into()]);
    }
    let out = helpers
        .run(&w.join("bin/marmish"), &args, w, env, deadline, None)
        .await?;
    let s: prepare::Setup = serde_json::from_slice(&out.stdout).map_err(|_| bad())?;
    if !out.status.success() || !setup_valid(&s, name, peers.is_none(), peers.is_some()) {
        return Err(bad());
    }
    Ok(hex::encode(crate::command_config::account(&s.npub)?))
}
fn configure(
    w: &Path,
    ctx: &Context,
    name: &str,
    peers: &[String],
    executable: &Path,
) -> Result<ConfigPin> {
    let path = w.join("profiles").join(name).join("config.json");
    let raw = records::read_bytes(&path, 1048576)?;
    crate::command_config::CommandConfig::from_bytes(&raw)?;
    let original: serde_json::Value = serde_json::from_slice(&raw).map_err(|_| bad())?;
    let mut value = original.clone();
    let server = name == "server";
    let mut peers = peers.to_vec();
    peers.sort();
    if peers.windows(2).any(|p| p[0] == p[1])
        || value["role"] != if server { "server" } else { "client" }
        || value["relays"] != serde_json::json!(ctx.relays)
        || value["tor"] != "off"
        || value["aliases"] != serde_json::json!({})
        || value["execution_clients"]
            != serde_json::json!(if server { peers.clone() } else { vec![] })
        || value["trusted_remotes"] != serde_json::json!(if server { vec![] } else { peers })
    {
        return Err(bad());
    }
    let text = executable.to_str().ok_or_else(bad)?;
    if !executable.is_absolute() || text.bytes().any(|b| b"*?[]{}\\".contains(&b)) {
        return Err(bad());
    }
    value["command_policy"]["defaults"]["allow_exec"] =
        serde_json::json!(if server { vec![text] } else { vec![] });
    value["cwd"] = serde_json::json!(executable.parent().ok_or_else(bad)?);
    value["limits"] = serde_json::json!({});
    let bytes = records::canonical(&value)?;
    crate::command_config::CommandConfig::from_bytes(&bytes)?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    if records::read_bytes(&path, 1048576)? != bytes {
        return Err(bad());
    }
    Ok(ConfigPin {
        profile: name.into(),
        sha256: records::digest(&bytes),
    })
}
fn copy_fixture(ctx: &Context, index: usize, w: &Path) -> Result<PathBuf> {
    let own = records::verify_tools(ctx, index)?;
    let source = own
        .parent()
        .ok_or_else(bad)?
        .join("marmish-installed-fixture");
    let bytes = records::read_bytes(&source, 268435456)?;
    let root = w.join("fixture");
    fs::DirBuilder::new().mode(0o700).create(&root)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(root.join("receipts"))?;
    let path = root.join("main");
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o700)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    if records::digest(&records::read_bytes(&path, 268435456)?)
        != ctx.tooling[index].files.fixture.sha256
    {
        return Err(bad());
    }
    path.canonicalize().map_err(Into::into)
}
fn id() -> Result<String> {
    crate::foundation::fresh_id(super::now()?)
}
fn fixed<const N: usize>(s: &str) -> Result<[u8; N]> {
    if !hex(s, N * 2) {
        return Err(bad());
    }
    hex::decode(s)
        .map_err(|_| bad())?
        .try_into()
        .map_err(|_| bad())
}
async fn local(socket: &Path, request: &ipc::Request, deadline: Instant) -> Result<LocalResult> {
    tokio::time::timeout_at(
        deadline.min(Instant::now() + Duration::from_secs(15)),
        ipc::command_request(socket, request),
    )
    .await
    .map_err(|_| Error::Timeout)?
}
fn retryable(v: &LocalResult) -> bool {
    matches!(
        v,
        LocalResult::LocalBusy | LocalResult::RemotePending { .. }
    ) || matches!(v,LocalResult::Failure{category,..} if matches!(category.as_str(),"local_busy"|"command_poll_busy"|"command_slot_capacity"|"command_slot_principal_capacity"|"remote_pending"))
}
fn answer(
    v: LocalResult,
    h: &CommandHandle,
    client: wire::Account,
    rid: &str,
    op: wire::Op,
    request_sha: Option<wire::Digest>,
) -> Result<Option<(wire::Response, String)>> {
    if let LocalResult::RemotePending { handle, request_id } = &v
        && (handle != h || request_id != rid)
    {
        return Err(bad());
    }
    if retryable(&v) {
        return Ok(None);
    }
    let LocalResult::Answer {
        handle,
        request_id,
        response_base64,
        response_sha256,
    } = v
    else {
        return Err(bad());
    };
    if handle != *h
        || request_id != rid
        || response_base64.len() > 43692
        || !hex(&response_sha256, 64)
    {
        return Err(bad());
    }
    let bytes = STANDARD.decode(&response_base64).map_err(|_| bad())?;
    if bytes.len() > wire::MAX_CONTENT
        || STANDARD.encode(&bytes) != response_base64
        || records::digest(&bytes) != response_sha256
    {
        return Err(bad());
    }
    let r = wire::decode_response(&bytes)?;
    if r.id != fixed(rid)?
        || r.binding.client != client
        || r.binding.server != h.remote()
        || r.binding.session != h.session()
        || r.op != op
        || request_sha.is_some_and(|s| s != r.request_sha256)
        || r.result
            .as_ref()
            .ok()
            .and_then(ResponseResult::status)
            .is_some_and(|s| s.invocation != h.invocation())
    {
        return Err(bad());
    }
    Ok(Some((r, response_sha256)))
}
fn terminal_status(s: &wire::Status) -> bool {
    s.authority_valid
        && s.engine_known
        && s.state == wire::State::Exited
        && s.outcome
            .as_ref()
            .is_some_and(|o| o.kind == wire::OutcomeKind::Code && o.value == Some(7))
        && s.stdout.closed
        && s.stderr.closed
        && s.stdout.produced == STREAM as u64
        && s.stderr.produced == STREAM as u64
        && s.stdout.gaps.is_empty()
        && s.stderr.gaps.is_empty()
        && s.warning.is_none()
}
fn append(bytes: &mut Vec<u8>, chunk: &wire::Chunk, start: u64, expected: &[u8]) -> Result<()> {
    let end = start
        .checked_add(chunk.bytes.len() as u64)
        .ok_or_else(bad)?;
    if start != bytes.len() as u64
        || end > STREAM as u64
        || chunk.next_cursor != end
        || expected.get(start as usize..end as usize) != Some(chunk.bytes.as_slice())
    {
        return Err(bad());
    }
    bytes.extend_from_slice(&chunk.bytes);
    Ok(())
}
// Detachment is local acceptance only; ambiguous254 is never execution proof.
fn detached_handle(output: &process::Output) -> Result<CommandHandle> {
    if !output.status.success() || !output.stdout.is_empty() {
        return Err(bad());
    }
    let line = output
        .stderr
        .strip_prefix(b"marmish handle: ")
        .and_then(|s| s.strip_suffix(b"\n"))
        .ok_or_else(bad)?;
    CommandHandle::decode(line)
}
struct ClientResult {
    terminal: Terminal,
    finish: FinishDisposition,
    receipt: String,
}
struct ClientPlan<'a> {
    w: &'a Path,
    ctx: &'a Context,
    env: &'a Environment,
    executable: &'a Path,
    output: &'a Path,
    role: &'a str,
    account: &'a str,
    server: &'a str,
    index: u8,
    helpers: &'a Helpers,
}
async fn controller(
    plan: ClientPlan<'_>,
    daemon: &mut Daemon,
    mut exited: tokio::sync::watch::Receiver<bool>,
    deadline: Instant,
) -> Result<ClientResult> {
    let name = CLIENTS[plan.index as usize];
    let nonce = nonce(plan.ctx, plan.role, plan.account, plan.server, plan.index)?;
    let (input, expected_out, expected_err) = fixture::bytes(&nonce, plan.index)?;
    let fixture_args = fixture::arguments(&nonce, plan.index)?;
    let mut args = base(plan.w, name, &daemon.socket, plan.ctx);
    args.extend([
        "--no-autostart".into(),
        "--detach".into(),
        "--stdin".into(),
        "--wait-seconds".into(),
        "300".into(),
        nostr::key::PublicKey::from_hex(plan.server)
            .map_err(|_| bad())?
            .to_bech32()
            .map_err(|_| bad())?
            .into(),
        "--".into(),
        plan.executable.into(),
    ]);
    args.extend(fixture_args);
    let detached = plan
        .helpers
        .run(
            &plan.w.join("bin/marmish"),
            &args,
            plan.w,
            plan.env,
            deadline,
            Some(&input),
        )
        .await?;
    let h = detached_handle(&detached)?;
    if h.profile() != name || h.remote() != fixed(plan.server)? {
        return Err(bad());
    }
    let inv = local(
        &daemon.socket,
        &ipc::Request::CommandInvocations {
            after: None,
            limit: 64,
        },
        deadline,
    )
    .await?;
    let LocalResult::Invocations { entries, next } = inv else {
        return Err(bad());
    };
    if next.is_some()
        || entries.len() != 1
        || entries[0].handle != h
        || !entries[0].authority_valid
        || entries[0].refusal.is_some()
    {
        return Err(bad());
    }
    let initial_group_digest = entries[0].group_submit_sha256.clone();
    let binding = wire::Binding {
        client: fixed(plan.account)?,
        server: h.remote(),
        session: h.session(),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut terminal = None;
    let mut finish: Option<String> = None;
    let mut discovered_finish_sha: Option<String> = None;
    let mut finish_rounds = 0;
    loop {
        if Instant::now() >= deadline {
            return if let Some(t) = terminal {
                finish_client(plan, t, FinishDisposition::Pending)
            } else {
                Err(Error::Timeout)
            };
        }
        if *exited.borrow_and_update() && terminal.is_some() {
            return finish_client(plan, terminal.ok_or_else(bad)?, FinishDisposition::Pending);
        }
        if daemon.child.try_wait()?.is_some() {
            return Err(bad());
        }
        if let Some(rid) = finish.clone() {
            let v = local(
                &daemon.socket,
                &ipc::Request::CommandResponse {
                    handle: h.clone(),
                    request_id: rid.clone(),
                },
                deadline,
            )
            .await;
            if let Ok(v) = v
                && let Some((r, sha)) = answer(v, &h, binding.client, &rid, wire::Op::Finish, None)?
            {
                if discovered_finish_sha
                    .as_ref()
                    .is_some_and(|expected| *expected != sha)
                {
                    return Err(bad());
                }
                let disposition = match r.result {
                    Ok(ResponseResult::Finish { finished: true }) => FinishDisposition::Confirmed,
                    Err(e) if e.code == wire::ErrorCode::Busy => FinishDisposition::Pending,
                    _ => FinishDisposition::Rejected,
                };
                let consumed = local(
                    &daemon.socket,
                    &ipc::Request::CommandConsume {
                        handle: h.clone(),
                        request_id: rid,
                        response_sha256: sha,
                        expected_cursors: None,
                    },
                    deadline,
                )
                .await?;
                if !matches!(consumed, LocalResult::Consumed { .. }) {
                    return Err(bad());
                }
                if disposition != FinishDisposition::Pending || finish_rounds >= 4 {
                    return finish_client(plan, terminal.ok_or_else(bad)?, disposition);
                }
                finish = None;
                discovered_finish_sha = None;
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        }
        if terminal.is_some() && finish.is_none() {
            // A lost final Consume reply can leave an automatic Finish with an unknown ID.
            if let Ok(LocalResult::Entries { entries }) = local(
                &daemon.socket,
                &ipc::Request::CommandUnconsumed {
                    handle: h.clone(),
                    limit: 64,
                },
                deadline,
            )
            .await
            {
                if entries.len() > 64 {
                    return Err(bad());
                }
                if let Some(entry) = entries.into_iter().find(|e| e.op == wire::Op::Finish) {
                    discovered_finish_sha = Some(entry.sha256);
                    finish = Some(entry.request_id);
                    finish_rounds += 1;
                    continue;
                }
            }
        }
        let cursors = Cursors {
            stdout: stdout.len() as u64,
            stderr: stderr.len() as u64,
        };
        let rid = id()?;
        let op = RequestOp::Poll {
            invocation: h.invocation(),
            stdout_cursor: cursors.stdout,
            stderr_cursor: cursors.stderr,
            max_bytes: wire::MAX_BYTES as u32,
        };
        let request = wire::Request {
            binding: binding.clone(),
            id: fixed(&rid)?,
            op,
        };
        let digest = wire::request_digest(&wire::encode_request(&request)?);
        let response = loop {
            if Instant::now() >= deadline {
                return if let Some(t) = terminal {
                    finish_client(plan, t, FinishDisposition::Pending)
                } else {
                    Err(Error::Timeout)
                };
            }
            if *exited.borrow() && terminal.is_some() {
                return finish_client(plan, terminal.ok_or_else(bad)?, FinishDisposition::Pending);
            }
            let v = local(
                &daemon.socket,
                &ipc::Request::CommandPoll {
                    handle: h.clone(),
                    request_id: rid.clone(),
                    stdout_cursor: cursors.stdout,
                    stderr_cursor: cursors.stderr,
                    max_bytes: wire::MAX_BYTES as u32,
                },
                deadline,
            )
            .await;
            if let Ok(v) = v
                && let Some(a) = answer(v, &h, binding.client, &rid, wire::Op::Poll, Some(digest))?
            {
                break a;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let (r, response_sha) = response;
        let Ok(ResponseResult::Poll {
            status,
            stdout: out,
            stderr: err,
        }) = r.result
        else {
            return Err(bad());
        };
        if !status.authority_valid
            || !status.stdout.gaps.is_empty()
            || !status.stderr.gaps.is_empty()
            || status.warning.is_some()
        {
            return Err(bad());
        }
        append(&mut stdout, &out, cursors.stdout, &expected_out)?;
        append(&mut stderr, &err, cursors.stderr, &expected_err)?;
        let final_poll =
            terminal_status(&status) && stdout == expected_out && stderr == expected_err;
        // A Finish-Busy retry cannot downgrade the already-persisted terminal proof.
        if terminal.is_some() && !final_poll {
            return Err(bad());
        }
        if final_poll && terminal.is_none() {
            let group_submit_sha256 =
                cleanup::client_group_submit(&mut daemon.child, &daemon.socket, &h, deadline)
                    .await?;
            if group_submit_sha256 != initial_group_digest {
                return Err(bad());
            }
            let t = Terminal {
                client_account: plan.account.into(),
                server_account: plan.server.into(),
                session: hex::encode(h.session()),
                invocation: hex::encode(h.invocation()),
                submit_sha256: hex::encode(h.submit_sha256()),
                group_submit_sha256,
                terminal_poll_request_id: rid.clone(),
                terminal_poll_request_sha256: hex::encode(digest),
                terminal_response_sha256: response_sha.clone(),
                stdout_bytes: stdout.len(),
                stdout_sha256: records::digest(&stdout),
                stderr_bytes: stderr.len(),
                stderr_sha256: records::digest(&stderr),
                outcome: ExitOutcome {
                    kind: "exit".into(),
                    value: 7,
                },
                gap_free: true,
                authority_valid: true,
            };
            records::write(&plan.output.join(format!("{name}-terminal.json")), &t)?;
            terminal = Some(t);
        }
        let consumed = local(
            &daemon.socket,
            &ipc::Request::CommandConsume {
                handle: h.clone(),
                request_id: rid,
                response_sha256: response_sha,
                expected_cursors: Some(cursors),
            },
            deadline,
        )
        .await;
        match consumed {
            Ok(LocalResult::Consumed {
                stdout: out,
                stderr: err,
                finish_request_id,
                finish_confirmed,
            }) if out == stdout.len() as u64 && err == stderr.len() as u64 => {
                if final_poll && finish_confirmed {
                    return finish_client(
                        plan,
                        terminal.ok_or_else(bad)?,
                        FinishDisposition::Confirmed,
                    );
                }
                if let Some(rid) = finish_request_id {
                    if !final_poll {
                        return Err(bad());
                    }
                    discovered_finish_sha = None;
                    finish = Some(rid);
                    finish_rounds += 1;
                }
            }
            Err(_) if final_poll => {
                // Never retry a potentially successful final Consume.
                while Instant::now() < deadline && !*exited.borrow() {
                    if let Ok(LocalResult::Entries { entries }) = local(
                        &daemon.socket,
                        &ipc::Request::CommandUnconsumed {
                            handle: h.clone(),
                            limit: 64,
                        },
                        deadline,
                    )
                    .await
                        && let Some(e) = entries.into_iter().find(|e| e.op == wire::Op::Finish)
                    {
                        discovered_finish_sha = Some(e.sha256);
                        finish = Some(e.request_id);
                        finish_rounds += 1;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                if finish.is_none() {
                    return finish_client(
                        plan,
                        terminal.ok_or_else(bad)?,
                        FinishDisposition::Pending,
                    );
                }
            }
            _ => return Err(bad()),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
fn finish_client(
    p: ClientPlan<'_>,
    terminal: Terminal,
    finish: FinishDisposition,
) -> Result<ClientResult> {
    let n = nonce(p.ctx, p.role, p.account, p.server, p.index)?;
    let receipt: fixture::Receipt =
        records::read(&fixture::receipt_path(p.executable, &n)?, 65536)?;
    let expected = fixture::expected_receipt(
        p.executable,
        &n,
        p.index,
        &p.ctx.tooling[ROLES.iter().position(|r| *r == p.role).ok_or_else(bad)?]
            .files
            .fixture
            .sha256,
    )?;
    if receipt != expected {
        return Err(bad());
    }
    Ok(ClientResult {
        terminal,
        finish,
        receipt: records::digest(&records::canonical(&receipt)?),
    })
}
struct ClientOwner {
    daemon: Daemon,
}
impl ClientOwner {
    async fn close(&mut self, receipt: &cleanup::ProfileReceipt, deadline: Instant) -> Result<()> {
        self.daemon.stop_no_engine(receipt, deadline).await
    }
}
struct Runtime {
    server: Option<Daemon>,
    clients: Vec<ClientOwner>,
    tree: Option<EngineTree>,
    helpers: Helpers,
    client_profiles: BTreeMap<String, cleanup::ProfileReceipt>,
}
impl Runtime {
    fn new() -> Self {
        Self {
            server: None,
            clients: Vec::new(),
            tree: None,
            helpers: Helpers::default(),
            client_profiles: BTreeMap::new(),
        }
    }
    async fn force_all(&mut self, deadline: Instant) -> bool {
        let server = async {
            if let Some(s) = &mut self.server {
                s.force(deadline).await.is_ok()
            } else {
                true
            }
        };
        let clients = async {
            async fn force(client: Option<&mut ClientOwner>, deadline: Instant) -> bool {
                match client {
                    Some(c) => c.daemon.force(deadline).await.is_ok(),
                    None => true,
                }
            }
            let mut clients = self.clients.iter_mut();
            let (a, b, c) = tokio::join!(
                force(clients.next(), deadline),
                force(clients.next(), deadline),
                force(clients.next(), deadline)
            );
            a && b && c
        };
        let (a, b, c) = tokio::join!(server, clients, self.helpers.stop(deadline));
        a && b && c
    }
}
struct Observation {
    natural_ms: u64,
    upper_ms: u64,
    stop: OriginalStop,
    count_ok: bool,
    uptime_ok: bool,
}
async fn observe(
    server: &mut Daemon,
    cohort: &str,
    ready: Instant,
    send: tokio::sync::watch::Sender<bool>,
    hard: Instant,
) -> Result<Observation> {
    let count = cohort == "autoexit";
    let upper = ready + Duration::from_secs(if count { 1235 } else { 215 });
    let cutoff = upper.min(hard);
    let mut count_ok = false;
    let waited = if count {
        match tokio::time::timeout_at(
            (server.launch + Duration::from_secs(600)).min(cutoff),
            server.child.wait(),
        )
        .await
        {
            Ok(s) => {
                count_ok = true;
                Ok(s?)
            }
            Err(_) => tokio::time::timeout_at(cutoff, server.child.wait())
                .await
                .map_err(|_| Error::Timeout)
                .and_then(|s| s.map_err(Into::into)),
        }
    } else {
        tokio::time::timeout_at(cutoff, server.child.wait())
            .await
            .map_err(|_| Error::Timeout)
            .and_then(|s| s.map_err(Into::into))
    };
    let observed = Instant::now();
    let elapsed = ms(observed.duration_since(server.launch));
    let _ = send.send(true);
    let status = waited?;
    if !status.success() {
        return Err(bad());
    }
    let bytes = server
        .drains((Instant::now() + Duration::from_secs(5)).min(hard))
        .await?;
    let stop = OriginalStop::parse(&bytes)?;
    Ok(Observation {
        natural_ms: elapsed,
        upper_ms: ms(upper.duration_since(server.launch)),
        stop,
        count_ok: count && count_ok && elapsed <= 600000,
        uptime_ok: !count && elapsed >= 180000 && observed <= cutoff,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceStream {
    produced: u64,
    closed: bool,
    gaps: u32,
    #[serde(deserialize_with = "null")]
    sha256: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceRow {
    cursor: u64,
    principal: String,
    group_sha256: String,
    session: String,
    invocation: String,
    submit_sha256: String,
    #[serde(deserialize_with = "null")]
    engine_admitted: Option<u8>,
    #[serde(deserialize_with = "null")]
    refusal: Option<wire::ErrorCode>,
    #[serde(deserialize_with = "null")]
    outcome: Option<wire::Outcome>,
    stdout: EvidenceStream,
    stderr: EvidenceStream,
    authority_valid: bool,
    worker_state: crate::journal::WorkerState,
    observed_seq: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidencePage {
    rows: Vec<EvidenceRow>,
    #[serde(deserialize_with = "null")]
    next: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanupStatus {
    active: u32,
    uncertain: u32,
    worker_running: u32,
    worker_unknown: u32,
    engine_present: bool,
    engine_quiesced: bool,
    observation_pending: bool,
    cleanup: String,
    #[serde(deserialize_with = "null")]
    final_seq: Option<u64>,
    #[serde(default, rename = "streams")]
    _streams: Option<super::streaming_cleanup::StreamStatus>,
}
async fn collect_evidence(
    server: &mut Daemon,
    terminals: &[Option<Terminal>],
    deadline: Instant,
) -> Result<(u64, Vec<Evidence>)> {
    let status: CleanupStatus = cleanup::request(
        &mut server.child,
        &server.socket,
        &ipc::Request::CommandCleanupStatus {},
        deadline,
    )
    .await?;
    let fence = status.final_seq.filter(|v| *v > 0).ok_or_else(bad)?;
    if !status.engine_present
        || status.active != 0
        || status.uncertain != 0
        || status.worker_running != 0
        || status.worker_unknown != 0
        || !status.engine_quiesced
        || status.observation_pending
        || status.cleanup != "quiesced"
    {
        return Err(bad());
    }
    let mut result = Vec::new();
    let mut after = None;
    loop {
        let p: EvidencePage = cleanup::request(
            &mut server.child,
            &server.socket,
            &ipc::Request::CommandEvidence { after, limit: 64 },
            deadline,
        )
        .await?;
        let mut previous = after.unwrap_or(0);
        if p.rows.len() > 64 {
            return Err(bad());
        }
        for row in p.rows {
            let t = terminals
                .iter()
                .flatten()
                .find(|t| t.client_account == row.principal && t.invocation == row.invocation)
                .ok_or_else(bad)?;
            if row.cursor <= previous
                || result.len() >= 3
                || !hex(&row.group_sha256, 64)
                || row.session != t.session
                || row.submit_sha256 != t.group_submit_sha256
                || row.refusal.is_some()
                || row.engine_admitted != Some(1)
                || !row.authority_valid
                || row.worker_state != crate::journal::WorkerState::Exited
                || row.observed_seq < fence
                || row
                    .outcome
                    .as_ref()
                    .is_none_or(|o| o.kind != wire::OutcomeKind::Code || o.value != Some(7))
                || !row.stdout.closed
                || !row.stderr.closed
                || row.stdout.produced != STREAM as u64
                || row.stderr.produced != STREAM as u64
                || row.stdout.gaps != 0
                || row.stderr.gaps != 0
                || row
                    .stdout
                    .sha256
                    .as_ref()
                    .is_some_and(|h| h != &t.stdout_sha256)
                || row
                    .stderr
                    .sha256
                    .as_ref()
                    .is_some_and(|h| h != &t.stderr_sha256)
            {
                return Err(bad());
            }
            previous = row.cursor;
            result.push(Evidence {
                client_account: row.principal,
                invocation: row.invocation,
                session: row.session,
                submit_sha256: row.submit_sha256,
                engine_admitted: 1,
                outcome: ExitOutcome {
                    kind: "exit".into(),
                    value: 7,
                },
                observed_seq: row.observed_seq,
                authority_valid: true,
                worker_exited: true,
            });
        }
        if p.next.is_some_and(|n| n != previous || Some(n) == after) {
            return Err(bad());
        }
        after = p.next;
        if after.is_none() {
            break;
        }
    }
    if result.len() != terminals.iter().flatten().count() {
        return Err(bad());
    }
    result.sort_by(|a, b| {
        (&a.client_account, &a.invocation).cmp(&(&b.client_account, &b.invocation))
    });
    Ok((fence, result))
}
struct WorkPlan<'a> {
    w: &'a Path,
    ctx: &'a Context,
    env: &'a Environment,
    exe: &'a Path,
    output: &'a Path,
    role: &'a str,
    accounts: &'a [String],
    deadline: Instant,
}
async fn client_work(
    runtime: &mut Runtime,
    plan: WorkPlan<'_>,
    r: &mut CohortResult,
) -> Result<()> {
    let WorkPlan {
        w,
        ctx,
        env,
        exe,
        output,
        role,
        accounts,
        deadline,
    } = plan;
    let server = runtime.server.as_mut().ok_or_else(bad)?;
    let ready = server.launch + Duration::from_millis(r.launch_to_ready_ms.ok_or_else(bad)?);
    let (send, recv) = tokio::sync::watch::channel(false);
    let plans = |i: usize| ClientPlan {
        w,
        ctx,
        env,
        executable: exe,
        output,
        role,
        account: &accounts[i],
        server: &accounts[3],
        index: i as u8,
        helpers: &runtime.helpers,
    };
    let [a, b, c] = runtime.clients.as_mut_slice() else {
        return Err(bad());
    };
    let work_deadline = (Instant::now() + Duration::from_secs(360))
        .min(server.launch + Duration::from_secs(600))
        .min(deadline);
    let clients = async {
        tokio::join!(
            controller(plans(0), &mut a.daemon, recv.clone(), work_deadline),
            controller(plans(1), &mut b.daemon, recv.clone(), work_deadline),
            controller(plans(2), &mut c.daemon, recv, work_deadline)
        )
    };
    let (observed, (a, b, c)) =
        tokio::join!(observe(server, "autoexit", ready, send, deadline), clients);
    match observed {
        Ok(o) => {
            r.natural_exit_ms = Some(o.natural_ms);
            r.uptime_exit_upper_ms = Some(o.upper_ms);
            r.original_stop = Some(o.stop);
            r.count_exit_ok = o.count_ok;
            r.cleanup.original_server_reaped = true;
            if !o.count_ok {
                r.category = Some(Category::CountCutoff)
            }
        }
        Err(e) => {
            r.fail(Category::NaturalExit);
            return Err(e);
        }
    }
    let mut success = true;
    for (i, result) in [a, b, c].into_iter().enumerate() {
        match result {
            Ok(c) => {
                r.terminals[i] = Some(c.terminal);
                r.receipt_sha256[i] = Some(c.receipt);
                r.finish[i] = c.finish;
                if c.finish == FinishDisposition::Rejected {
                    success = false
                }
            }
            Err(_) => success = false,
        }
    }
    if !success {
        r.fail(Category::Submission);
        return Err(bad());
    }
    Ok(())
}
fn receipt_inventory(
    executable: &Path,
    ctx: &Context,
    role: &str,
    accounts: &[String],
) -> Result<()> {
    let directory = executable.parent().ok_or_else(bad)?.join("receipts");
    let expected = if ctx.cohort == "autoexit" {
        (0..3)
            .map(|i| {
                fixture::receipt_path(
                    executable,
                    &nonce(ctx, role, &accounts[i], &accounts[3], i as u8)?,
                )
            })
            .collect::<Result<BTreeSet<_>>>()?
    } else {
        BTreeSet::new()
    };
    let actual = fs::read_dir(&directory)?
        .map(|entry| {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(bad());
            }
            Ok(path)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if actual != expected {
        return Err(bad());
    }
    Ok(())
}
fn expected(r: &CohortResult) -> BTreeMap<(String, String), (String, String)> {
    r.terminals
        .iter()
        .flatten()
        .map(|t| {
            (
                (t.client_account.clone(), t.invocation.clone()),
                (t.session.clone(), t.group_submit_sha256.clone()),
            )
        })
        .collect()
}
async fn close_clients(runtime: &mut Runtime, deadline: Instant) -> Result<()> {
    let [a, b, c] = runtime.clients.as_mut_slice() else {
        return Err(bad());
    };
    let receipts = &runtime.client_profiles;
    let (a, b, c) = tokio::join!(
        a.close(receipts.get(CLIENTS[0]).ok_or_else(bad)?, deadline),
        b.close(receipts.get(CLIENTS[1]).ok_or_else(bad)?, deadline),
        c.close(receipts.get(CLIENTS[2]).ok_or_else(bad)?, deadline)
    );
    a?;
    b?;
    c?;
    Ok(())
}
async fn cleanup_server(
    runtime: &mut Runtime,
    w: &Path,
    ctx: &Context,
    env: &Environment,
    deadline: Instant,
    r: &mut CohortResult,
) -> Result<()> {
    r.stage = Stage::Restart;
    let profile = w.join("profiles/server");
    let tree = runtime.tree.as_ref().ok_or_else(bad)?;
    tree.verify_path(&profile)?;
    if deadline.saturating_duration_since(Instant::now()) < Duration::from_secs(600) {
        return Err(Error::Timeout);
    }
    let socket = socket(w, "server", env)?;
    runtime.server = Some(Daemon::start(
        w,
        "server",
        socket,
        ctx,
        env,
        600,
        Some(if r.planned_admissions == 3 { 4 } else { 1 }),
    )?);
    let server = runtime.server.as_mut().ok_or_else(bad)?;
    let origin = server.launch;
    server.ready(origin + Duration::from_secs(150)).await?;
    tree.verify_path(&profile)?;
    r.stage = Stage::Evidence;
    let guard_deadline = (origin + Duration::from_secs(510)).min(deadline);
    let mut guard = cleanup::prepare(
        &mut server.child,
        &profile,
        &server.socket,
        expected(r),
        guard_deadline,
    )
    .await?;
    r.stage = Stage::Remove;
    runtime
        .tree
        .take()
        .ok_or_else(bad)?
        .remove_installed(&mut guard)
        .await?;
    drop(guard);
    r.cleanup.engine_removed = true;
    r.cleanup.post_unlink_verified = true;
    let (fence, evidence) = collect_evidence(server, &r.terminals, guard_deadline).await?;
    r.final_seq = Some(fence);
    r.evidence = evidence;
    r.stage = Stage::Stop;
    server
        .stop((origin + Duration::from_secs(570)).min(deadline))
        .await?;
    r.cleanup.cleanup_instance_joined = true;
    Ok(())
}
pub(super) fn remove_private_workspace(w: &Path, output: &Path, names: &[&str]) -> Result<()> {
    crate::identity::check_dir(w)?;
    crate::identity::check_dir(output)?;
    if output.parent() != Some(w) || w.canonicalize()? != w || output.canonicalize()? != output {
        return Err(bad());
    }
    // No recursive deletion may remove an engine whose sealed proof was not consumed.
    for name in names {
        cleanup::require_no_engine(&w.join("profiles").join(name))?;
    }
    for entry in fs::read_dir(w)? {
        let entry = entry?;
        let p = entry.path();
        if p == output {
            continue;
        }
        let m = fs::symlink_metadata(&p)?;
        if m.file_type().is_symlink() {
            return Err(bad());
        }
        if m.is_dir() {
            fs::remove_dir_all(p)?
        } else if m.is_file() {
            fs::remove_file(p)?
        } else {
            return Err(bad());
        }
    }
    let rest = fs::read_dir(w)?.collect::<std::io::Result<Vec<_>>>()?;
    if rest.len() != 1 || rest[0].path() != output {
        return Err(bad());
    }
    Ok(())
}
/// The caller writes the final public result last, into the retained output directory.
pub(super) async fn run(ctx: &Context, w: &Path, output: &Path) -> Result<CohortResult> {
    ctx.validate()?;
    if !matches!(ctx.cohort.as_str(), "autoexit" | "uptime") {
        return Err(bad());
    }
    crate::identity::check_dir(w)?;
    crate::identity::check_dir(output)?;
    if output.parent() != Some(w) || w.canonicalize()? != w {
        return Err(bad());
    }
    let index = TARGETS
        .iter()
        .position(|t| *t == crate::native_manifest::compiled_release_metadata().target)
        .ok_or_else(bad)?;
    let role = ROLES[index];
    let own = records::own_identity(w, ctx)?;
    records::verify_tools(ctx, index)?;
    let now = super::now()?;
    let began = Instant::now();
    let remaining = ctx.expires_at.checked_sub(now).ok_or_else(bad)?;
    budget(&ctx.cohort, remaining, true)?;
    let hard = began + Duration::from_secs(remaining);
    let cleanup_deadline = hard.checked_sub(Duration::from_secs(300)).ok_or_else(bad)?;
    let env = Environment {
        home: w.join("home"),
        temporary: w.join("tmp"),
        utilities: w.join("utilities"),
    };
    for p in [&env.home, &env.temporary, &env.utilities] {
        crate::identity::check_dir(p)?
    }
    let mut report = CohortResult::new(ctx, &own, now)?;
    let mut runtime = Runtime::new();
    let work = async {
        report.stage = Stage::Setup;
        let setup_deadline = (began + Duration::from_secs(600)).min(cleanup_deadline);
        let exe = copy_fixture(ctx, index, w)?;
        let mut accounts = vec![own.client_account.clone()];
        if ctx.cohort == "autoexit" {
            for name in &CLIENTS[1..] {
                accounts
                    .push(setup(w, ctx, &env, name, None, &runtime.helpers, setup_deadline).await?)
            }
        }
        accounts.push(own.server_account.clone());
        if accounts.iter().collect::<BTreeSet<_>>().len() != accounts.len() {
            return Err(bad());
        }
        let server = own.server_account.clone();
        let peers = accounts[..accounts.len() - 1].to_vec();
        if setup(
            w,
            ctx,
            &env,
            "server",
            Some(&peers),
            &runtime.helpers,
            setup_deadline,
        )
        .await?
            != server
        {
            return Err(bad());
        }
        let names = if ctx.cohort == "autoexit" {
            CLIENTS.as_slice()
        } else {
            &CLIENTS[..1]
        };
        for (i, name) in names.iter().enumerate() {
            if setup(
                w,
                ctx,
                &env,
                name,
                Some(std::slice::from_ref(&server)),
                &runtime.helpers,
                setup_deadline,
            )
            .await?
                != accounts[i]
            {
                return Err(bad());
            }
            report.config_sha256.push(configure(
                w,
                ctx,
                name,
                std::slice::from_ref(&server),
                &exe,
            )?);
        }
        for name in names {
            let profile = w.join("profiles").join(name);
            cleanup::require_no_engine(&profile)?;
            runtime.client_profiles.insert(
                (*name).to_owned(),
                cleanup::ProfileReceipt::capture(&profile)?,
            );
        }
        report
            .config_sha256
            .push(configure(w, ctx, "server", &peers, &exe)?);
        report
            .config_sha256
            .sort_by(|a, b| a.profile.cmp(&b.profile));
        budget(
            &ctx.cohort,
            cleanup_deadline
                .saturating_duration_since(Instant::now())
                .as_secs()
                + 300,
            false,
        )?;
        report.stage = Stage::ServerStart;
        let server_socket = socket(w, "server", &env)?;
        require_new_engine(&w.join("profiles/server"))?;
        runtime.server = Some(Daemon::start(
            w,
            "server",
            server_socket,
            ctx,
            &env,
            if ctx.cohort == "autoexit" { 1200 } else { 180 },
            if ctx.cohort == "autoexit" {
                Some(3)
            } else {
                None
            },
        )?);
        let server = runtime.server.as_mut().ok_or_else(bad)?;
        server
            .ready(server.launch + Duration::from_secs(150))
            .await?;
        report.launch_to_ready_ms = Some(ms(server.launch.elapsed()));
        runtime.tree = Some(EngineTree::capture_path(&server.profile)?);
        if ctx.cohort == "autoexit" {
            report.stage = Stage::Clients;
            for name in CLIENTS {
                runtime
                    .client_profiles
                    .get(name)
                    .ok_or_else(bad)?
                    .verify(&w.join("profiles").join(name))?;
                require_new_engine(&w.join("profiles").join(name))?;
                let d = Daemon::start(w, name, socket(w, name, &env)?, ctx, &env, 1800, None)?;
                runtime.clients.push(ClientOwner { daemon: d });
                let owner = runtime.clients.last_mut().ok_or_else(bad)?;
                owner
                    .daemon
                    .ready((owner.daemon.launch + Duration::from_secs(150)).min(hard))
                    .await?;
                runtime
                    .client_profiles
                    .get(name)
                    .ok_or_else(bad)?
                    .verify(&owner.daemon.profile)?;
                cleanup::verify_no_engine(
                    &mut owner.daemon.child,
                    &owner.daemon.profile,
                    &owner.daemon.socket,
                    hard,
                )
                .await?;
            }
            client_work(
                &mut runtime,
                WorkPlan {
                    w,
                    ctx,
                    env: &env,
                    exe: &exe,
                    output,
                    role,
                    accounts: &accounts,
                    deadline: hard,
                },
                &mut report,
            )
            .await?;
            report.stage = Stage::Stop;
            let deadline = (Instant::now() + Duration::from_secs(40)).min(cleanup_deadline);
            for owner in &runtime.clients {
                let name = owner
                    .daemon
                    .profile
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(bad)?;
                runtime
                    .client_profiles
                    .get(name)
                    .ok_or_else(bad)?
                    .verify(&owner.daemon.profile)?;
            }
            close_clients(&mut runtime, deadline).await?;
            for owner in &runtime.clients {
                let name = owner
                    .daemon
                    .profile
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(bad)?;
                runtime
                    .client_profiles
                    .get(name)
                    .ok_or_else(bad)?
                    .verify(&owner.daemon.profile)?;
            }
            report.cleanup.clients_joined = true;
        } else {
            report.stage = Stage::AwaitExit;
            let server = runtime.server.as_mut().ok_or_else(bad)?;
            let no_invocations = local(
                &server.socket,
                &ipc::Request::CommandInvocations {
                    after: None,
                    limit: 64,
                },
                hard,
            )
            .await?;
            if !matches!(no_invocations,LocalResult::Invocations{entries,next:None} if entries.is_empty())
            {
                return Err(bad());
            }
            let (tx, _) = tokio::sync::watch::channel(false);
            let ready =
                server.launch + Duration::from_millis(report.launch_to_ready_ms.ok_or_else(bad)?);
            let o = observe(server, "uptime", ready, tx, hard).await?;
            report.natural_exit_ms = Some(o.natural_ms);
            report.uptime_exit_upper_ms = Some(o.upper_ms);
            report.original_stop = Some(o.stop);
            report.uptime_exit_ok = o.uptime_ok;
            report.cleanup.original_server_reaped = true;
            report.cleanup.clients_joined = true;
            if !o.uptime_ok {
                report.fail(Category::NaturalExit)
            }
        }
        receipt_inventory(&exe, ctx, role, &accounts)?;
        cleanup_server(&mut runtime, w, ctx, &env, cleanup_deadline, &mut report).await?;
        report.cleanup.drains_joined = true;
        if report.category.is_none() {
            let names = if ctx.cohort == "autoexit" {
                vec!["server", "client", "client-2", "client-3"]
            } else {
                vec!["server", "client"]
            };
            for (name, receipt) in &runtime.client_profiles {
                let profile = w.join("profiles").join(name);
                receipt.verify(&profile)?;
                cleanup::require_no_engine(&profile)?;
            }
            remove_private_workspace(w, output, &names)?;
            report.cleanup.workspace_removed = true;
        }
        Ok::<_, Error>(())
    };
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let result = tokio::select! {r=tokio::time::timeout_at(cleanup_deadline,work)=>r.map_err(|_|Error::Timeout).and_then(|r|r),_=term.recv()=>Err(Error::Invalid("installed_interrupted")),_=tokio::signal::ctrl_c()=>Err(Error::Invalid("installed_interrupted"))};
    if let Err(error) = result {
        report.fail(match error {
            Error::Timeout => Category::Deadline,
            _ => match report.stage {
                Stage::Setup | Stage::Prepare => Category::Setup,
                Stage::ServerStart => Category::Readiness,
                Stage::Clients => Category::Submission,
                Stage::AwaitExit => Category::NaturalExit,
                _ => Category::Cleanup,
            },
        });
        if runtime
            .server
            .as_mut()
            .is_some_and(|s| s.child.try_wait().ok().flatten().is_none())
        {
            report.signal_sent = true;
        }
        // Failure never authorizes deletion; retained child termination and pipe joining are bounded.
        let joined = runtime
            .force_all((Instant::now() + Duration::from_secs(10)).min(hard))
            .await;
        report.cleanup.drains_joined = joined;
    }
    report.elapsed_ms = ms(began.elapsed());
    report.complete = report.category.is_none()
        && report.cleanup.complete()
        && if ctx.cohort == "autoexit" {
            report.count_exit_ok
        } else {
            report.uptime_exit_ok
        };
    if report.complete {
        report.stage = Stage::Complete
    } else {
        report.fail(Category::Native)
    }
    validate_record(&report, ctx, role, &own)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn admission_budgets_preserve_all_cleanup_reserves() {
        for (cohort, setup, minimum) in [
            ("autoexit", true, 2925),
            ("autoexit", false, 2325),
            ("uptime", true, 1865),
            ("uptime", false, 1265),
        ] {
            assert!(budget(cohort, minimum, setup).is_ok());
            assert!(matches!(
                budget(cohort, minimum - 1, setup),
                Err(Error::Timeout)
            ));
        }
        assert!(budget("matrix", 3300, true).is_err());
    }

    #[test]
    fn original_natural_stop_requires_joined_tasks_but_not_premature_quiescence() {
        let raw = br#"{"schema":1,"engine_quiesced":false,"engine_stop_complete":true,"mdk_stop_complete":true,"runtime_cleanup_complete":true}"#;
        assert!(OriginalStop::parse(raw).is_ok());
        for key in [
            "engine_stop_complete",
            "mdk_stop_complete",
            "runtime_cleanup_complete",
        ] {
            let mut value: serde_json::Value = serde_json::from_slice(raw).unwrap();
            value[key] = false.into();
            assert!(OriginalStop::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        assert!(OriginalStop::parse(&[raw.as_slice(), b"\n", raw.as_slice()].concat()).is_err());
        assert!(OriginalStop::parse(br#"{"schema":1,"engine_quiesced":false,"engine_stop_complete":true,"mdk_stop_complete":true,"runtime_cleanup_complete":true,"private":"no"}"#).is_err());
        assert!(OriginalStop::parse(br#"{"schema":1,"schema":1,"engine_quiesced":false,"engine_stop_complete":true,"mdk_stop_complete":true,"runtime_cleanup_complete":true}"#).is_err());
    }

    #[test]
    fn output_chunks_require_exact_contiguous_binary_contents() {
        let expected = (0..256).map(|i| i as u8).collect::<Vec<_>>();
        let mut bytes = Vec::new();
        append(
            &mut bytes,
            &wire::Chunk {
                bytes: expected[..128].to_vec(),
                next_cursor: 128,
            },
            0,
            &expected,
        )
        .unwrap();
        let original = bytes.clone();
        for chunk in [
            wire::Chunk {
                bytes: expected[128..].to_vec(),
                next_cursor: 257,
            },
            wire::Chunk {
                bytes: vec![0xff],
                next_cursor: 129,
            },
        ] {
            assert!(append(&mut bytes, &chunk, 128, &expected).is_err());
            assert_eq!(bytes, original);
        }
        assert!(
            append(
                &mut bytes,
                &wire::Chunk {
                    bytes: vec![],
                    next_cursor: 127
                },
                127,
                &expected
            )
            .is_err()
        );
        append(
            &mut bytes,
            &wire::Chunk {
                bytes: expected[128..].to_vec(),
                next_cursor: 256,
            },
            128,
            &expected,
        )
        .unwrap();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn workspace_cleanup_never_recursively_removes_an_unproved_engine() {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let w = root.path().canonicalize().unwrap();
        let output = w.join("output");
        fs::DirBuilder::new().mode(0o700).create(&output).unwrap();
        let engine = w.join("profiles/client/commands");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&engine)
            .unwrap();
        fs::write(engine.join("preserve"), b"owned").unwrap();
        assert!(remove_private_workspace(&w, &output, &["client"]).is_err());
        assert_eq!(fs::read(engine.join("preserve")).unwrap(), b"owned");
        fs::remove_dir_all(&engine).unwrap(); // fixture-only simulated completed sealed removal
        fs::write(output.join("retained.json"), b"{}").unwrap();
        remove_private_workspace(&w, &output, &["client"]).unwrap();
        assert_eq!(fs::read(output.join("retained.json")).unwrap(), b"{}");
        assert!(!w.join("profiles").exists());
    }

    #[tokio::test]
    async fn completed_output_drain_is_not_polled_again_if_other_drain_times_out() {
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        child.wait().await.unwrap();
        let out = tokio::spawn(async { Ok((b"public".to_vec(), false)) });
        let err = tokio::spawn(async {
            std::future::pending::<std::io::Result<(Vec<u8>, bool)>>().await
        });
        let mut daemon = Daemon {
            child,
            out: Some(out),
            err: Some(err),
            socket: PathBuf::new(),
            profile: PathBuf::new(),
            launch: Instant::now(),
        };
        assert!(matches!(
            daemon
                .drains(Instant::now() + Duration::from_millis(20))
                .await,
            Err(Error::Timeout)
        ));
        assert!(daemon.out.is_none() && daemon.err.is_none());
    }
    #[tokio::test]
    async fn cancelled_helper_retains_child_and_drains_until_explicit_cleanup() {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let env = Environment {
            home: root.path().into(),
            temporary: root.path().into(),
            utilities: root.path().into(),
        };
        let helpers = Helpers::default();
        let args = [
            OsString::from("-c"),
            OsString::from("printf public; exec /bin/sleep 10"),
        ];
        {
            let running = helpers.run(
                Path::new("/bin/sh"),
                &args,
                root.path(),
                &env,
                Instant::now() + Duration::from_secs(15),
                None,
            );
            tokio::pin!(running);
            tokio::select! {
                result=&mut running=>panic!("helper ended before cancellation: {}",result.is_ok()),
                _=tokio::time::sleep(Duration::from_millis(30))=>{}
            }
        }
        assert!(helpers.stop(Instant::now() + Duration::from_secs(2)).await);
        let entries = helpers.0.lock().unwrap().clone();
        assert_eq!(entries.len(), 1);
        let mut helper = entries[0].lock().await;
        assert!(helper.joined && helper.child.try_wait().unwrap().is_some());
        assert!(helper.out.is_none() && helper.err.is_none());
    }
    fn uptime_report() -> (Context, IdentityReady, CohortResult) {
        let mut ctx = records::tests::ctx();
        ctx.cohort = "uptime".into();
        ctx.expires_at = ctx.created_at + 3300;
        let id = IdentityReady {
            schema: 1,
            kind: "identity-ready".into(),
            context_sha256: ctx.digest().unwrap(),
            role: ROLES[0].into(),
            nonce: "9".repeat(64),
            client_account: "a".repeat(64),
            server_account: "b".repeat(64),
            identity_ready_at: ctx.created_at,
            installation: Installation {
                version: ctx.version.clone(),
                target: TARGETS[0].into(),
                source_commit: ctx.source_commit.clone(),
                manifest_sha256: ctx.manifests[ROLES[0]].clone(),
                bootstrap_sha256: ctx.bootstrap_sha256.clone(),
                files: crate::native_manifest::NAMES
                    .into_iter()
                    .map(|name| {
                        (
                            name.into(),
                            records::FilePin {
                                sha256: "c".repeat(64),
                                bytes: 1,
                            },
                        )
                    })
                    .collect(),
            },
        };
        let mut r = CohortResult::new(&ctx, &id, ctx.created_at).unwrap();
        r.complete = true;
        r.stage = Stage::Complete;
        r.elapsed_ms = 200000;
        r.launch_to_ready_ms = Some(0);
        r.natural_exit_ms = Some(180000);
        r.uptime_exit_upper_ms = Some(215000);
        r.uptime_exit_ok = true;
        r.final_seq = Some(1);
        r.original_stop = Some(OriginalStop {
            schema: 1,
            engine_quiesced: false,
            engine_stop_complete: true,
            mdk_stop_complete: true,
            runtime_cleanup_complete: true,
        });
        r.config_sha256 = ["client", "server"]
            .into_iter()
            .map(|name| ConfigPin {
                profile: name.into(),
                sha256: "d".repeat(64),
            })
            .collect();
        r.cleanup = Cleanup {
            original_server_reaped: true,
            clients_joined: true,
            engine_removed: true,
            post_unlink_verified: true,
            cleanup_instance_joined: true,
            drains_joined: true,
            workspace_removed: true,
        };
        (ctx, id, r)
    }
    #[test]
    fn rollup_requires_complete_identity_bound_natural_exit_and_cleanup() {
        let (ctx, id, r) = uptime_report();
        validate(&r, &ctx, &id.role, &id).unwrap();
        let original = records::canonical(&r).unwrap();
        for field in [
            "identity_ready_sha256",
            "count_exit_ok",
            "natural_exit_ms",
            "signal_sent",
            "cleanup",
            "complete",
            "cohort",
            "upper",
        ] {
            let mut changed: CohortResult = serde_json::from_slice(&original).unwrap();
            match field {
                "identity_ready_sha256" => changed.identity_ready_sha256 = "0".repeat(64),
                "count_exit_ok" => changed.count_exit_ok = true,
                "natural_exit_ms" => changed.natural_exit_ms = Some(179999),
                "signal_sent" => changed.signal_sent = true,
                "cleanup" => changed.cleanup.post_unlink_verified = false,
                "complete" => {
                    changed.complete = false;
                    changed.category = Some(Category::Deadline);
                }
                "cohort" => changed.cohort = "autoexit".into(),
                _ => changed.uptime_exit_upper_ms = Some(215001),
            }
            assert!(validate(&changed, &ctx, &id.role, &id).is_err(), "{field}");
        }
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value.as_object_mut().unwrap().remove("category");
        assert!(serde_json::from_value::<CohortResult>(value).is_err());
    }
    #[test]
    fn first_spawn_refuses_existing_or_symlink_engine_tree() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        require_new_engine(tmp.path()).unwrap();
        fs::create_dir(tmp.path().join("commands")).unwrap();
        assert!(require_new_engine(tmp.path()).is_err());
        fs::remove_dir(tmp.path().join("commands")).unwrap();
        symlink(tmp.path().join("absent"), tmp.path().join("commands")).unwrap();
        assert!(require_new_engine(tmp.path()).is_err());
    }
    fn autoexit_report() -> (Context, IdentityReady, CohortResult) {
        let (mut ctx, mut id, mut r) = uptime_report();
        ctx.cohort = "autoexit".into();
        id.context_sha256 = ctx.digest().unwrap();
        r.context_sha256 = ctx.digest().unwrap();
        r.identity_ready_sha256 = records::digest(&records::canonical(&id).unwrap());
        r.cohort = ctx.cohort.clone();
        r.count_exit_ok = true;
        r.uptime_exit_ok = false;
        r.planned_admissions = 3;
        r.natural_exit_ms = Some(500000);
        r.uptime_exit_upper_ms = Some(1235000);
        r.elapsed_ms = 1000000;
        r.config_sha256 = ["client", "client-2", "client-3", "server"]
            .into_iter()
            .map(|p| ConfigPin {
                profile: p.into(),
                sha256: "e".repeat(64),
            })
            .collect();
        r.finish = vec![
            FinishDisposition::Confirmed,
            FinishDisposition::Pending,
            FinishDisposition::Confirmed,
        ];
        r.receipt_sha256 = vec![Some("1".repeat(64)); 3];
        r.terminals.clear();
        for (i, account) in [id.client_account.clone(), "c".repeat(64), "d".repeat(64)]
            .into_iter()
            .enumerate()
        {
            let nonce = nonce(&ctx, &id.role, &account, &id.server_account, i as u8).unwrap();
            let (_, out, err) = fixture::bytes(&nonce, i as u8).unwrap();
            let terminal = Terminal {
                client_account: account,
                server_account: id.server_account.clone(),
                session: format!("{i:032x}"),
                invocation: format!("{:032x}", i + 10),
                submit_sha256: "2".repeat(64),
                group_submit_sha256: "4".repeat(64),
                terminal_poll_request_id: "3".repeat(32),
                terminal_poll_request_sha256: "4".repeat(64),
                terminal_response_sha256: "5".repeat(64),
                stdout_bytes: STREAM,
                stdout_sha256: records::digest(&out),
                stderr_bytes: STREAM,
                stderr_sha256: records::digest(&err),
                outcome: ExitOutcome {
                    kind: "exit".into(),
                    value: 7,
                },
                gap_free: true,
                authority_valid: true,
            };
            r.evidence.push(Evidence {
                client_account: terminal.client_account.clone(),
                invocation: terminal.invocation.clone(),
                session: terminal.session.clone(),
                submit_sha256: terminal.group_submit_sha256.clone(),
                engine_admitted: 1,
                outcome: terminal.outcome.clone(),
                observed_seq: 1,
                authority_valid: true,
                worker_exited: true,
            });
            r.terminals.push(Some(terminal));
        }
        (ctx, id, r)
    }
    #[test]
    fn detached_acceptance_requires_exit_zero_and_one_exact_handle() {
        use std::os::unix::process::ExitStatusExt;
        let handle: CommandHandle = serde_json::from_value(serde_json::json!({
            "format":1,"profile":"client","remote":"b".repeat(64),
            "session":"1".repeat(32),"invocation":"2".repeat(32),"submit_sha256":"3".repeat(64)
        }))
        .unwrap();
        let encoded = handle.encode().unwrap();
        let mut output = process::Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: [b"marmish handle: ".as_slice(), encoded.as_slice(), b"\n"].concat(),
        };
        assert!(detached_handle(&output).unwrap() == handle);
        for code in [254, 255, 7] {
            output.status = std::process::ExitStatus::from_raw(code << 8);
            assert!(detached_handle(&output).is_err());
        }
        output.status = std::process::ExitStatus::from_raw(0);
        output.stdout.push(0);
        assert!(detached_handle(&output).is_err());
        output.stdout.clear();
        output.stderr.extend_from_slice(b"another line\n");
        assert!(detached_handle(&output).is_err());
    }

    #[test]
    fn genuine_failed_record_is_retained_without_pass_qualification() {
        let (ctx, id, mut report) = autoexit_report();
        report.complete = false;
        report.stage = Stage::Clients;
        report.category = Some(Category::Submission);
        report.terminals = vec![None; 3];
        report.receipt_sha256 = vec![None; 3];
        report.finish = vec![FinishDisposition::NotRequested; 3];
        report.evidence.clear();
        report.final_seq = None;
        assert!(validate_record(&report, &ctx, &id.role, &id).is_ok());
        assert!(validate(&report, &ctx, &id.role, &id).is_err());
        report.identity_ready_sha256 = "0".repeat(64);
        assert!(validate_record(&report, &ctx, &id.role, &id).is_err());
    }

    #[test]
    fn autoexit_rollup_recomputes_slots_streams_budget_exit_and_exact_evidence() {
        let (ctx, id, r) = autoexit_report();
        validate(&r, &ctx, &id.role, &id).unwrap();
        let bytes = records::canonical(&r).unwrap();
        for case in 0..13 {
            let mut r: CohortResult = serde_json::from_slice(&bytes).unwrap();
            match case {
                0 => r.terminals[0].as_mut().unwrap().stdout_sha256 = "0".repeat(64),
                1 => r.terminals.swap(1, 2),
                2 => r.evidence[0].invocation = "f".repeat(32),
                3 => r.terminals[1].as_mut().unwrap().client_account = id.client_account.clone(),
                4 => r.finish[0] = FinishDisposition::NotRequested,
                5 => r.finish[0] = FinishDisposition::Rejected,
                6 => r.natural_exit_ms = Some(600001),
                7 => r.uptime_exit_ok = true,
                8 => r.receipt_sha256[0] = None,
                9 => r.work_budget_at_entry_ms = 0,
                10 => r.elapsed_ms = 3300001,
                11 => r.context_age_at_entry_ms = 1,
                _ => {
                    let t = r.terminals[0].as_mut().unwrap();
                    t.group_submit_sha256 = t.submit_sha256.clone();
                }
            }
            assert!(validate(&r, &ctx, &id.role, &id).is_err(), "case {case}");
        }
        let mut failed = r;
        failed.fail(Category::CountCutoff);
        failed.cleanup.engine_removed = true;
        assert!(validate(&failed, &ctx, &id.role, &id).is_err());
        assert!(failed.category == Some(Category::CountCutoff));
    }
    #[test]
    fn response_requires_exact_authenticated_binding_digest_and_canonical_bytes() {
        let h=CommandHandle::decode(br#"{"format":1,"profile":"client","remote":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","session":"11111111111111111111111111111111","invocation":"22222222222222222222222222222222","submit_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}"#).unwrap();
        let rid = "3".repeat(32);
        let response = wire::Response {
            binding: wire::Binding {
                client: [0xaa; 32],
                server: h.remote(),
                session: h.session(),
            },
            id: fixed(&rid).unwrap(),
            op: wire::Op::Poll,
            request_sha256: [4; 32],
            result: Err(wire::WireError {
                code: wire::ErrorCode::Busy,
                detail: None,
            }),
        };
        let value = |r: &wire::Response| {
            let bytes = wire::encode_response(r).unwrap();
            LocalResult::Answer {
                handle: h.clone(),
                request_id: rid.clone(),
                response_base64: STANDARD.encode(&bytes),
                response_sha256: records::digest(&bytes),
            }
        };
        assert!(
            answer(
                value(&response),
                &h,
                [0xaa; 32],
                &rid,
                wire::Op::Poll,
                Some([4; 32])
            )
            .unwrap()
            .is_some()
        );
        for case in 0..6 {
            let mut r = response.clone();
            match case {
                0 => r.id = [9; 16],
                1 => r.op = wire::Op::Status,
                2 => r.binding.client = [9; 32],
                3 => r.binding.server = [9; 32],
                4 => r.binding.session = [9; 16],
                _ => r.request_sha256 = [9; 32],
            }
            assert!(
                answer(
                    value(&r),
                    &h,
                    [0xaa; 32],
                    &rid,
                    wire::Op::Poll,
                    Some([4; 32])
                )
                .is_err()
            );
        }
        for case in 0..2 {
            let mut v = value(&response);
            if let LocalResult::Answer {
                response_base64,
                response_sha256,
                ..
            } = &mut v
            {
                if case == 0 {
                    response_base64.push('=');
                } else {
                    *response_sha256 = "0".repeat(64);
                }
            }
            assert!(answer(v, &h, [0xaa; 32], &rid, wire::Op::Poll, Some([4; 32])).is_err());
        }
        assert!(
            answer(
                LocalResult::RemotePending {
                    handle: h.clone(),
                    request_id: "9".repeat(32)
                },
                &h,
                [0xaa; 32],
                &rid,
                wire::Op::Poll,
                Some([4; 32])
            )
            .is_err()
        );
    }
}

#[cfg(test)]
#[path = "installed_socket_tests.rs"]
mod socket_tests;
