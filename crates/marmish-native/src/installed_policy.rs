//! Installed policy qualification. Records describe evidence; they confer no authority.
use super::super::command_check::EngineTree;
use super::{
    benchmark, cleanup, fixture,
    matrix::{self, Daemon},
    prepare,
    process::Environment,
    records::{self, Context, IdentityReady, Installation, ROLES},
};
use crate::{
    Error, Result,
    command_client::{self, CommandHandle},
    command_local::LocalResult,
    command_wire::{self as wire, RequestOp},
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
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::UnixStream,
    signal::unix::{Signal, SignalKind},
    time::Instant,
};
const CASES: [&str; 5] = [
    "deep/main",
    "other/main",
    "denied",
    "default-denied",
    "main",
];
fn bad() -> Error {
    Error::Invalid("installed_policy")
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Failure {
    Setup,
    ZeroGrant,
    Activation,
    Denial,
    Retry,
    Output,
    Conflict,
    Evidence,
    Cleanup,
    Interrupted,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProbeDiagnostic {
    RoomPending,
    LocalTimeout,
    RemotePending,
    LocalPending,
}
fn probe_diagnostic(bytes: &[u8]) -> Result<ProbeDiagnostic> {
    let text = std::str::from_utf8(bytes).map_err(|_| bad())?;
    match text
        .lines()
        .rev()
        .find(|line| !line.is_empty())
        .ok_or_else(bad)?
    {
        "marmish: command_room_pending" => Ok(ProbeDiagnostic::RoomPending),
        "marmish: local_timeout" => Ok(ProbeDiagnostic::LocalTimeout),
        "marmish: remote_pending" => Ok(ProbeDiagnostic::RemotePending),
        "marmish: local_pending" => Ok(ProbeDiagnostic::LocalPending),
        _ => Err(bad()),
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ZeroGrant {
    probe_account: String,
    attempted: bool,
    exit_code: u16,
    diagnostic: ProbeDiagnostic,
    // This is an unconfirmed CLI failure, not an authenticated policy refusal.
    unconfirmed: bool,
    ledger_rows: u8,
    receipts: u8,
    sessions: Vec<String>,
    server_stop_complete: bool,
    client_stop_complete: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    label: String,
    nonce: String,
    handle: CommandHandle,
    request_id: String,
    request_sha256: String,
    group_submit_sha256: String,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    refusal: Option<wire::ErrorCode>,
    receipt_absent: bool,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    receipt: Option<fixture::Receipt>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PolicyResult {
    schema: u8,
    kind: String,
    context_sha256: String,
    identity_ready_sha256: String,
    role: String,
    installation: Installation,
    route: String,
    relays: Vec<String>,
    context_age_seconds: u64,
    context_remaining_seconds: u64,
    fixture_root: String,
    fixture_sha256: String,
    config_sha256: BTreeMap<String, String>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    zero_grant: Option<ZeroGrant>,
    cases: Vec<Option<Case>>,
    identical_retry: bool,
    local_conflict: bool,
    evidence: Vec<serde_json::Value>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    final_seq: Option<u64>,
    engine_cleanup_complete: bool,
    server_stop_complete: bool,
    client_stop_complete: bool,
    workspace_removed: bool,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    failure: Option<Failure>,
    complete: bool,
}
impl PolicyResult {
    pub(super) fn is_complete(&self) -> bool {
        self.complete
    }
}
fn new_result(ctx: &Context, id: &IdentityReady, now: u64) -> Result<PolicyResult> {
    Ok(PolicyResult {
        schema: 1,
        kind: "policy-result".into(),
        context_sha256: ctx.digest()?,
        identity_ready_sha256: records::digest(&records::canonical(id)?),
        role: id.role.clone(),
        installation: id.installation.clone(),
        route: "direct".into(),
        relays: ctx.relays.clone(),
        context_age_seconds: now.checked_sub(ctx.created_at).ok_or_else(bad)?,
        context_remaining_seconds: ctx.expires_at.checked_sub(now).ok_or_else(bad)?,
        fixture_root: String::new(),
        fixture_sha256: String::new(),
        config_sha256: BTreeMap::new(),
        zero_grant: None,
        cases: (0..5).map(|_| None).collect(),
        identical_retry: false,
        local_conflict: false,
        evidence: Vec::new(),
        final_seq: None,
        engine_cleanup_complete: false,
        server_stop_complete: false,
        client_stop_complete: false,
        workspace_removed: false,
        failure: None,
        complete: false,
    })
}
fn nonce(ctx: &Context, role: &str, label: &str) -> Result<String> {
    Ok(records::digest(&records::canonical(&(
        "installed-policy/1",
        ctx.digest()?,
        role,
        label,
    ))?))
}
fn executable(root: &Path, index: usize) -> PathBuf {
    match index {
        0 => root.join("ok/deep/main"),
        1 => root.join("other/main"),
        2 => root.join("ok/denied"),
        3 => root.join("ok/default-denied"),
        _ => root.join("ok/main"),
    }
}
fn remaining(ctx: &Context, minimum: u64) -> Result<()> {
    if ctx
        .expires_at
        .checked_sub(super::now()?)
        .is_none_or(|n| n < minimum)
    {
        return Err(Error::Timeout);
    }
    Ok(())
}
fn account(s: &str) -> Result<wire::Account> {
    hex::decode(s)
        .map_err(|_| bad())?
        .try_into()
        .map_err(|_| bad())
}
fn id(s: &str) -> Result<wire::Id> {
    hex::decode(s)
        .map_err(|_| bad())?
        .try_into()
        .map_err(|_| bad())
}
fn fresh() -> Result<String> {
    crate::foundation::fresh_id(super::now()?)
}
fn interrupted(signal: &mut Signal) -> Result<()> {
    use std::task::{Context as TaskContext, Poll, Waker};
    let mut cx = TaskContext::from_waker(Waker::noop());
    if matches!(signal.poll_recv(&mut cx), Poll::Ready(_)) {
        return Err(Error::Invalid("installed_interrupted"));
    }
    Ok(())
}
async fn execute(
    w: &Path,
    args: &[OsString],
    env: &Environment,
    input: Vec<u8>,
    end: Instant,
    signal: &mut Signal,
) -> Result<super::process::Output> {
    interrupted(signal)?;
    benchmark::execute(w, args, env, input, end, async {
        signal.recv().await;
    })
    .await
}
fn config_value(
    mut v: serde_json::Value,
    ctx: &Context,
    name: &str,
    peer: &str,
    root: &Path,
) -> Result<serde_json::Value> {
    let server = name == "server";
    let peers = serde_json::json!([peer]);
    let empty = serde_json::json!([]);
    if !matches!(name, "server" | "client")
        || v["role"] != name
        || v["tor"] != "off"
        || v["relays"] != serde_json::json!(ctx.relays)
        || v["aliases"] != serde_json::json!({})
        || v["command_policy"] != serde_json::json!({"version":1})
        || v["execution_clients"] != if server { peers.clone() } else { empty.clone() }
        || v["trusted_remotes"] != if server { empty } else { peers }
    {
        return Err(bad());
    }
    if server {
        let r = root.to_str().ok_or_else(bad)?;
        v["command_policy"] = serde_json::json!({"version":1,"defaults":{"allow_exec":[format!("{r}/**")],
            "deny_exec":[format!("{r}/ok/default-denied")]},"keys":[
            {"principal":peer,"restrictions":{"allow_exec":[format!("{r}/ok/*")]}},
            {"principal":peer,"restrictions":{"allow_exec":[format!("{r}/**")],"deny_exec":[format!("{r}/ok/denied")]}}]});
        v["limits"] = serde_json::json!({"max_commands":2,"max_uptime":1800});
    } else {
        v["limits"] = serde_json::json!({"max_uptime":1800});
    }
    v["cwd"] = serde_json::json!(root.to_str().ok_or_else(bad)?);
    let mut c = crate::command_config::CommandConfig::from_bytes(&records::canonical(&v)?)?;
    c.apply_limit_overrides(Default::default())?;
    Ok(v)
}
fn write_config(w: &Path, ctx: &Context, name: &str, peer: &str, root: &Path) -> Result<String> {
    let p = w.join("profiles").join(name).join("config.json");
    let raw = records::read_bytes(&p, 1048576)?;
    let original = serde_json::from_slice(&raw).map_err(|_| bad())?;
    let v = config_value(original, ctx, name, peer, root)?;
    let bytes = records::canonical(&v)?;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&p)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    if records::read_bytes(&p, 1048576)? != bytes {
        return Err(bad());
    }
    Ok(records::digest(&bytes))
}
fn copy_fixtures(w: &Path, ctx: &Context, index: usize) -> Result<PathBuf> {
    let tool = records::verify_tools(ctx, index)?;
    let raw = records::read_bytes(
        &tool
            .parent()
            .ok_or_else(bad)?
            .join("marmish-installed-fixture"),
        268435456,
    )?;
    if records::digest(&raw) != ctx.tooling[index].files.fixture.sha256 {
        return Err(bad());
    }
    let root = w.join("policy-bin");
    for d in [
        &root,
        &root.join("ok"),
        &root.join("ok/deep"),
        &root.join("other"),
    ] {
        fs::DirBuilder::new().mode(0o700).create(d)?;
    }
    for d in ["ok", "ok/deep", "other"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join(d).join("receipts"))?;
    }
    for i in 0..5 {
        let p = executable(&root, i);
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&p)?;
        f.write_all(&raw)?;
        f.sync_all()?;
        if records::digest(&records::read_bytes(&p, 268435456)?)
            != ctx.tooling[index].files.fixture.sha256
        {
            return Err(bad());
        }
    }
    if root.canonicalize()? != root
        || root
            .to_str()
            .ok_or_else(bad)?
            .contains(['*', '?', '[', ']', '{', '}', '\\'])
    {
        return Err(bad());
    }
    Ok(root)
}
fn validate_setup(s: &prepare::Setup, name: &str, fresh: bool) -> Result<()> {
    if s.schema != 1
        || s.identity != name
        || s.backend != "file"
        || s.role != if name == "server" { "server" } else { "client" }
        || s.inbound_enabled != (name == "server")
        || s.identity_created != fresh
        || s.owner_opened != fresh
        || !s.engine_stop_complete
        || !s.mdk_stop_complete
        || !s.runtime_cleanup_complete
        || !s.config_changed
    {
        return Err(bad());
    }
    Ok(())
}
async fn setup(
    w: &Path,
    ctx: &Context,
    name: &str,
    peer: Option<&str>,
    env: &Environment,
    end: Instant,
    signal: &mut Signal,
) -> Result<String> {
    let mut args = matrix::base(w, name, &w.join("unused.sock"), &ctx.relays);
    args.push("setup".into());
    if name == "client-probe" && peer.is_none() {
        args.extend(["--role".into(), "client".into(), "--no-grants".into()]);
    } else {
        args.extend([
            "--update".into(),
            if name == "server" {
                "--grant"
            } else {
                "--trust"
            }
            .into(),
            nostr::key::PublicKey::from_hex(peer.ok_or_else(bad)?)
                .map_err(|_| bad())?
                .to_bech32()
                .map_err(|_| bad())?
                .into(),
        ]);
    }
    let out = execute(w, &args, env, Vec::new(), end, signal).await?;
    let s: prepare::Setup = serde_json::from_slice(&out.stdout).map_err(|_| bad())?;
    let fresh = name == "client-probe" && peer.is_none();
    validate_setup(&s, name, fresh)?;
    if !out.status.success() {
        return Err(bad());
    }
    Ok(hex::encode(crate::command_config::account(&s.npub)?))
}
async fn entries(
    daemon: &mut Daemon,
    end: Instant,
) -> Result<Vec<crate::command_local::InvocationEntry>> {
    let mut after = None;
    let mut result = Vec::new();
    loop {
        let LocalResult::Invocations { entries, next } = cleanup::command_request(
            &mut daemon.child,
            &daemon.socket,
            &ipc::Request::CommandInvocations { after, limit: 64 },
            end,
        )
        .await?
        else {
            return Err(bad());
        };
        let mut previous = after.unwrap_or(0);
        if entries.len() > 64 {
            return Err(bad());
        }
        for e in entries {
            if e.cursor <= previous || result.len() >= 64 {
                return Err(bad());
            }
            previous = e.cursor;
            result.push(e);
        }
        if next.is_some_and(|n| n != previous || Some(n) == after) {
            return Err(bad());
        }
        after = next;
        if after.is_none() {
            return Ok(result);
        }
    }
}
async fn prepare_binding(
    daemon: &mut Daemon,
    ctx: &Context,
    identity: &IdentityReady,
    end: Instant,
    signal: &mut Signal,
) -> Result<wire::Binding> {
    loop {
        interrupted(signal)?;
        match cleanup::command_request(
            &mut daemon.child,
            &daemon.socket,
            &ipc::Request::CommandPrepare {
                remote: identity.server_account.clone(),
                relays: ctx.relays.clone(),
            },
            end,
        )
        .await?
        {
            LocalResult::Ready {
                session,
                local_account,
            } if local_account == identity.client_account => {
                return Ok(wire::Binding {
                    session: id(&session)?,
                    client: account(&identity.client_account)?,
                    server: account(&identity.server_account)?,
                });
            }
            LocalResult::LocalBusy => {}
            _ => return Err(bad()),
        }
        if Instant::now() >= end {
            return Err(Error::Timeout);
        }
        tokio::time::sleep_until(end.min(Instant::now() + Duration::from_millis(100))).await;
    }
}
fn submit(exe: &Path, nonce: &str) -> Result<wire::Submit> {
    let mut argv = vec![exe.as_os_str().as_bytes().to_vec()];
    argv.extend(
        fixture::arguments(nonce, 0)?
            .iter()
            .map(|a| a.as_bytes().to_vec()),
    );
    Ok(wire::Submit {
        argv,
        invocation: id(&fresh()?)?,
        stdin: fixture::bytes(nonce, 0)?.0,
    })
}
fn submit_request(
    ctx: &Context,
    binding: &wire::Binding,
    rid: &str,
    value: wire::Submit,
) -> ipc::Request {
    ipc::Request::CommandSubmit {
        remote: hex::encode(binding.server),
        relays: ctx.relays.clone(),
        session: hex::encode(binding.session),
        request_id: rid.into(),
        submit: value,
    }
}
fn queued(result: LocalResult, handle: &CommandHandle, rid: &str) -> Result<()> {
    match result {
        LocalResult::LocalQueued { handle: h } if h == *handle => Ok(()),
        LocalResult::RemotePending {
            handle: h,
            request_id,
        } if h == *handle && request_id == rid => Ok(()),
        _ => Err(bad()),
    }
}
async fn refusal(
    daemon: &mut Daemon,
    handle: &CommandHandle,
    request: &wire::Request,
    end: Instant,
    signal: &mut Signal,
) -> Result<(String, String)> {
    let rid = hex::encode(request.id);
    loop {
        interrupted(signal)?;
        match cleanup::command_request(
            &mut daemon.child,
            &daemon.socket,
            &ipc::Request::CommandResponse {
                handle: handle.clone(),
                request_id: rid.clone(),
            },
            end,
        )
        .await?
        {
            LocalResult::RemotePending {
                handle: h,
                request_id,
            } if h == *handle && request_id == rid => {}
            LocalResult::Answer {
                handle: h,
                request_id,
                response_base64,
                response_sha256,
            } => {
                let bytes = STANDARD.decode(&response_base64).map_err(|_| bad())?;
                let response = wire::decode_response(&bytes)?;
                let digest = wire::request_digest(&wire::encode_request(request)?);
                if h != *handle
                    || request_id != rid
                    || records::digest(&bytes) != response_sha256
                    || STANDARD.encode(&bytes) != response_base64
                    || response.binding != request.binding
                    || response.id != request.id
                    || response.op != wire::Op::Submit
                    || response.request_sha256 != digest
                    || !matches!(
                        response.result,
                        Err(wire::WireError {
                            code: wire::ErrorCode::CommandDenied,
                            detail: None
                        })
                    )
                {
                    return Err(bad());
                }
                // Lookup BEFORE consumption, from the independently cached client row.
                let group =
                    cleanup::client_group_submit(&mut daemon.child, &daemon.socket, handle, end)
                        .await?;
                let consumed = cleanup::command_request(
                    &mut daemon.child,
                    &daemon.socket,
                    &ipc::Request::CommandConsume {
                        handle: handle.clone(),
                        request_id: rid,
                        response_sha256,
                        expected_cursors: None,
                    },
                    end,
                )
                .await?;
                if !matches!(consumed, LocalResult::Consumed { .. }) {
                    return Err(bad());
                }
                return Ok((hex::encode(digest), group));
            }
            _ => return Err(bad()),
        }
        if Instant::now() >= end {
            return Err(Error::Timeout);
        }
        tokio::time::sleep_until(end.min(Instant::now() + Duration::from_millis(100))).await;
    }
}
async fn write_then_disconnect(
    daemon: &mut Daemon,
    request: &ipc::Request,
    end: Instant,
) -> Result<()> {
    // Peer identity is checked against the retained unreaped child before writing.
    if daemon.child.try_wait()?.is_some() {
        return Err(bad());
    }
    let pid = daemon.child.id().ok_or_else(bad)?;
    tokio::time::timeout_at(end.min(Instant::now() + Duration::from_secs(15)), async {
        let mut stream = UnixStream::connect(&daemon.socket).await?;
        let peer = stream.peer_cred()?;
        if peer.uid() != unsafe { libc::geteuid() } || peer.pid() != Some(pid as _) {
            return Err(bad());
        }
        let bytes = serde_json::to_vec(request).map_err(|_| bad())?;
        if bytes.len() > 65536 {
            return Err(bad());
        }
        stream.write_u32(bytes.len() as u32).await?;
        stream.write_all(&bytes).await?;
        stream.shutdown().await?;
        Ok::<_, Error>(())
    })
    .await
    .map_err(|_| Error::Timeout)??;
    if daemon.child.try_wait()?.is_some() {
        return Err(bad());
    }
    Ok(())
}
struct Runtime {
    server: Option<Daemon>,
    client: Option<Daemon>,
    client_name: String,
    receipts: BTreeMap<String, cleanup::ProfileReceipt>,
    tree: Option<EngineTree>,
    expected: cleanup::ExpectedRows,
    sockets: PathBuf,
    stage: Failure,
    deadline: Instant,
}
impl Runtime {
    fn uptime(&self) -> u64 {
        self.deadline
            .saturating_duration_since(Instant::now())
            .as_secs()
            .min(1800)
    }
    fn end(&self, seconds: u64) -> Instant {
        self.deadline
            .min(Instant::now() + Duration::from_secs(seconds))
    }
    fn verify(&self, w: &Path, name: &str) -> Result<()> {
        self.receipts
            .get(name)
            .ok_or_else(bad)?
            .verify(&w.join("profiles").join(name))
    }
    async fn no_engine_stop(&mut self, w: &Path, end: Instant) -> Result<(bool, bool)> {
        let proof = end.checked_sub(Duration::from_secs(60)).ok_or_else(bad)?;
        let mut server_proved = false;
        let mut client_proved = false;
        if let Some(server) = self.server.as_mut() {
            server_proved = self
                .receipts
                .get("server")
                .ok_or_else(bad)?
                .verify(&w.join("profiles/server"))
                .is_ok()
                && cleanup::verify_no_engine(
                    &mut server.child,
                    &w.join("profiles/server"),
                    &server.socket,
                    proof,
                )
                .await
                .is_ok();
        }
        if let Some(client) = self.client.as_mut() {
            client_proved = self
                .receipts
                .get(&self.client_name)
                .ok_or_else(bad)?
                .verify(&w.join("profiles").join(&self.client_name))
                .is_ok()
                && cleanup::verify_no_engine(
                    &mut client.child,
                    &w.join("profiles").join(&self.client_name),
                    &client.socket,
                    proof,
                )
                .await
                .is_ok();
        }
        let server = self.server.take();
        let client = self.client.take();
        let (s, c) = tokio::join!(
            async {
                match server {
                    Some(s) => s.stop_no_engine(end).await,
                    None => Err(bad()),
                }
            },
            async {
                match client {
                    Some(c) => c.stop_no_engine(end).await,
                    None => Err(bad()),
                }
            }
        );
        let s = server_proved
            && s.is_ok()
            && self.verify(w, "server").is_ok()
            && cleanup::require_no_engine(&w.join("profiles/server")).is_ok();
        let c = client_proved
            && c.is_ok()
            && self.verify(w, &self.client_name).is_ok()
            && cleanup::require_no_engine(&w.join("profiles").join(&self.client_name)).is_ok();
        Ok((s, c))
    }
    async fn close(&mut self, w: &Path, result: &mut PolicyResult, end: Instant) -> Result<()> {
        let proof = end.checked_sub(Duration::from_secs(60)).ok_or_else(bad)?;
        let mut removed = false;
        if let (Some(server), Some(tree)) = (self.server.as_mut(), self.tree.take())
            && self.expected.len() == 5
        {
            if result.failure.is_some() {
                let _ = cleanup::cancel_policy_execution(
                    &mut server.child,
                    &server.socket,
                    &self.expected,
                    proof,
                )
                .await;
            }
            if let Ok(mut guard) = cleanup::prepare_policy(
                &mut server.child,
                &w.join("profiles/server"),
                &server.socket,
                self.expected.clone(),
                proof,
            )
            .await
            {
                result.final_seq = Some(guard.final_seq());
                removed = tree.remove_installed(&mut guard).await.is_ok();
                drop(guard);
                if removed {
                    result.evidence = cleanup::policy_evidence(
                        &mut server.child,
                        &server.socket,
                        result.final_seq.ok_or_else(bad)?,
                        &self.expected,
                        proof,
                    )
                    .await?;
                }
            }
        }
        result.engine_cleanup_complete = removed;
        let client_proved = if let Some(client) = self.client.as_mut() {
            self.receipts
                .get("client")
                .ok_or_else(bad)?
                .verify(&w.join("profiles/client"))?;
            cleanup::verify_no_engine(
                &mut client.child,
                &w.join("profiles/client"),
                &client.socket,
                proof,
            )
            .await
            .is_ok()
        } else {
            false
        };
        let server = self.server.take();
        let client = self.client.take();
        let (s, c) = tokio::join!(
            async {
                match server {
                    Some(s) => {
                        if removed {
                            s.stop(end).await
                        } else {
                            s.ordinary_stop(end).await
                        }
                    }
                    None => Err(bad()),
                }
            },
            async {
                match client {
                    Some(c) => c.stop_no_engine(end).await,
                    None => Err(bad()),
                }
            }
        );
        result.server_stop_complete = s.is_ok()
            && self.verify(w, "server").is_ok()
            && (!removed || cleanup::require_no_engine(&w.join("profiles/server")).is_ok());
        result.client_stop_complete = c.is_ok()
            && client_proved
            && self.verify(w, "client").is_ok()
            && cleanup::require_no_engine(&w.join("profiles/client")).is_ok();
        if !removed || !result.server_stop_complete || !result.client_stop_complete {
            return Err(bad());
        }
        Ok(())
    }
}
async fn zero_grant(
    ctx: &Context,
    w: &Path,
    identity: &IdentityReady,
    env: &Environment,
    root: &Path,
    state: &mut Runtime,
    signal: &mut Signal,
) -> Result<ZeroGrant> {
    let setup_end = state.end(300);
    let probe = setup(w, ctx, "client-probe", None, env, setup_end, signal).await?;
    if probe == identity.client_account || probe == identity.server_account {
        return Err(bad());
    }
    if setup(
        w,
        ctx,
        "client-probe",
        Some(&identity.server_account),
        env,
        setup_end,
        signal,
    )
    .await?
        != probe
    {
        return Err(bad());
    }
    for name in ["server", "client-probe"] {
        let p = w.join("profiles").join(name);
        cleanup::require_no_engine(&p)?;
        state
            .receipts
            .insert(name.into(), cleanup::ProfileReceipt::capture(&p)?);
    }
    let raw = records::read_bytes(&w.join("profiles/server/config.json"), 1048576)?;
    let v: serde_json::Value = serde_json::from_slice(&raw).map_err(|_| bad())?;
    if v["execution_clients"] != serde_json::json!([])
        || v["role"] != "server"
        || v["relays"] != serde_json::json!(ctx.relays)
        || v["tor"] != "off"
    {
        return Err(bad());
    }
    remaining(ctx, 2100)?;
    let start = Instant::now();
    state.stage = Failure::ZeroGrant;
    state.client_name = "client-probe".into();
    state.verify(w, "server")?;
    state.verify(w, "client-probe")?;
    state.server = Some(Daemon::start(
        w,
        "server",
        state.sockets.join("server"),
        env,
        &ctx.relays,
        state.uptime(),
    )?);
    state.client = Some(Daemon::start(
        w,
        "client-probe",
        state.sockets.join("probe"),
        env,
        &ctx.relays,
        state.uptime(),
    )?);
    matrix::ready(
        state.server.as_mut().ok_or_else(bad)?,
        (start + Duration::from_secs(150)).min(state.deadline),
    )
    .await?;
    matrix::ready(
        state.client.as_mut().ok_or_else(bad)?,
        (start + Duration::from_secs(150)).min(state.deadline),
    )
    .await?;
    for (name, daemon) in [
        ("server", state.server.as_mut().ok_or_else(bad)?),
        ("client-probe", state.client.as_mut().ok_or_else(bad)?),
    ] {
        state
            .receipts
            .get(name)
            .ok_or_else(bad)?
            .verify(&w.join("profiles").join(name))?;
        cleanup::verify_no_engine(
            &mut daemon.child,
            &w.join("profiles").join(name),
            &daemon.socket,
            (start + Duration::from_secs(150)).min(state.deadline),
        )
        .await?;
    }
    let n = nonce(ctx, &identity.role, "zero-grant")?;
    let end = state.end(45);
    let mut args = matrix::base(
        w,
        "client-probe",
        &state.client.as_ref().ok_or_else(bad)?.socket,
        &ctx.relays,
    );
    args.extend([
        "--no-autostart".into(),
        "--stdin".into(),
        "--wait-seconds".into(),
        "20".into(),
        nostr::key::PublicKey::from_hex(&identity.server_account)
            .map_err(|_| bad())?
            .to_bech32()
            .map_err(|_| bad())?
            .into(),
        "--".into(),
        executable(root, 1).into_os_string(),
    ]);
    args.extend(fixture::arguments(&n, 0)?);
    let out = execute(w, &args, env, fixture::bytes(&n, 0)?.0, end, signal).await?;
    if !matches!(out.status.code(), Some(254 | 255))
        || !out.stdout.is_empty()
        || fs::symlink_metadata(fixture::receipt_path(&executable(root, 1), &n)?).is_ok()
    {
        return Err(bad());
    }
    let sessions = entries(state.client.as_mut().ok_or_else(bad)?, end)
        .await?
        .into_iter()
        .map(|e| hex::encode(e.handle.session()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let (s, c) = state.no_engine_stop(w, state.end(120)).await?;
    if !s || !c {
        return Err(bad());
    }
    Ok(ZeroGrant {
        probe_account: probe,
        attempted: true,
        unconfirmed: true,
        exit_code: out.status.code().ok_or_else(bad)? as u16,
        diagnostic: probe_diagnostic(&out.stderr)?,
        ledger_rows: 0,
        receipts: 0,
        sessions,
        server_stop_complete: s,
        client_stop_complete: c,
    })
}
async fn denied_case(
    ctx: &Context,
    identity: &IdentityReady,
    root: &Path,
    index: usize,
    client: &mut Daemon,
    end: Instant,
    signal: &mut Signal,
) -> Result<Case> {
    let binding = prepare_binding(client, ctx, identity, end, signal).await?;
    let n = nonce(ctx, &identity.role, CASES[index])?;
    let value = submit(&executable(root, index), &n)?;
    let rid = fresh()?;
    let prepared = command_client::prepare_operation(
        binding.clone(),
        id(&rid)?,
        RequestOp::Submit(value.clone()),
    )?;
    let handle = CommandHandle::for_submit("client", &prepared)?;
    queued(
        cleanup::command_request(
            &mut client.child,
            &client.socket,
            &submit_request(ctx, &binding, &rid, value),
            end,
        )
        .await?,
        &handle,
        &rid,
    )?;
    let (request_sha256, group_submit_sha256) =
        refusal(client, &handle, &prepared.request, end, signal).await?;
    if !matches!(fs::symlink_metadata(fixture::receipt_path(&executable(root,index),&n)?),Err(e) if e.kind()==std::io::ErrorKind::NotFound)
    {
        return Err(bad());
    }
    Ok(Case {
        label: CASES[index].into(),
        nonce: n,
        handle,
        request_id: rid,
        request_sha256,
        group_submit_sha256,
        refusal: Some(wire::ErrorCode::CommandDenied),
        receipt_absent: true,
        receipt: None,
    })
}
struct ExecutionContext<'a> {
    ctx: &'a Context,
    w: &'a Path,
    identity: &'a IdentityReady,
    root: &'a Path,
    env: &'a Environment,
}
async fn executed_case(
    context: ExecutionContext<'_>,
    client: &mut Daemon,
    end: Instant,
    signal: &mut Signal,
    result: &mut PolicyResult,
    expected: &mut cleanup::ExpectedRows,
) -> Result<Case> {
    let ExecutionContext {
        ctx,
        w,
        identity,
        root,
        env,
    } = context;
    let binding = prepare_binding(client, ctx, identity, end, signal).await?;
    let n = nonce(ctx, &identity.role, CASES[4])?;
    let value = submit(&executable(root, 4), &n)?;
    let rid = fresh()?;
    let prepared = command_client::prepare_operation(
        binding.clone(),
        id(&rid)?,
        RequestOp::Submit(value.clone()),
    )?;
    let handle = CommandHandle::for_submit("client", &prepared)?;
    let request = submit_request(ctx, &binding, &rid, value.clone());
    let retry_end = end.min(Instant::now() + Duration::from_secs(30));
    write_then_disconnect(client, &request, retry_end).await?;
    loop {
        interrupted(signal)?;
        if entries(client, retry_end)
            .await?
            .iter()
            .filter(|e| e.handle == handle)
            .count()
            == 1
        {
            break;
        }
        if Instant::now() >= retry_end {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Preserve the exact possibly executed key before any retry or wait can fail.
    let group_submit_sha256 =
        cleanup::client_group_submit(&mut client.child, &client.socket, &handle, end).await?;
    retain_execution(expected, identity, &handle, &group_submit_sha256)?;
    queued(
        cleanup::command_request(&mut client.child, &client.socket, &request, retry_end).await?,
        &handle,
        &rid,
    )?;
    if entries(client, retry_end)
        .await?
        .iter()
        .filter(|e| e.handle == handle)
        .count()
        != 1
    {
        return Err(bad());
    }
    result.identical_retry = true;
    let mut args = matrix::base(w, "client", &client.socket, &[]);
    args.extend([
        "--no-autostart".into(),
        "--wait-seconds".into(),
        "170".into(),
        "wait".into(),
        "--handle".into(),
        std::str::from_utf8(&handle.encode()?)
            .map_err(|_| bad())?
            .into(),
    ]);
    let out = execute(
        w,
        &args,
        env,
        Vec::new(),
        end.min(Instant::now() + Duration::from_secs(180)),
        signal,
    )
    .await;
    let out = out?;
    let (_, stdout, stderr) = fixture::bytes(&n, 0)?;
    if out.status.code() != Some(7) || out.stdout != stdout || out.stderr != stderr {
        return Err(bad());
    }
    let receipt: fixture::Receipt =
        records::read(&fixture::receipt_path(&executable(root, 4), &n)?, 65536)?;
    let index = ROLES
        .iter()
        .position(|r| *r == identity.role)
        .ok_or_else(bad)?;
    if receipt
        != fixture::expected_receipt(
            &executable(root, 4),
            &n,
            0,
            &ctx.tooling[index].files.fixture.sha256,
        )?
    {
        return Err(bad());
    }
    let mut changed = value;
    changed.stdin[0] ^= 1;
    let conflict = cleanup::command_request(
        &mut client.child,
        &client.socket,
        &submit_request(ctx, &binding, &fresh()?, changed),
        end.min(Instant::now() + Duration::from_secs(15)),
    )
    .await?;
    if !matches!(conflict,LocalResult::Failure {category,handle:None} if category=="command_handle_binding")
    {
        return Err(bad());
    }
    let rows = entries(client, end).await?;
    if rows.len() != 5 || rows.iter().filter(|e| e.handle == handle).count() != 1 {
        return Err(bad());
    }
    result.local_conflict = true;
    Ok(Case {
        label: CASES[4].into(),
        nonce: n,
        handle,
        request_id: rid,
        request_sha256: hex::encode(prepared.digest),
        group_submit_sha256,
        refusal: None,
        receipt_absent: false,
        receipt: Some(receipt),
    })
}
fn retain_execution(
    expected: &mut cleanup::ExpectedRows,
    identity: &IdentityReady,
    handle: &CommandHandle,
    digest: &str,
) -> Result<()> {
    if expected.len() != 4 || !records::hexadecimal(digest, 64) {
        return Err(bad());
    }
    if expected
        .insert(
            (
                identity.client_account.clone(),
                hex::encode(handle.invocation()),
            ),
            (
                (hex::encode(handle.session()), digest.into()),
                cleanup::ExpectedDisposition::ExecutedFixture,
            ),
        )
        .is_some()
    {
        return Err(bad());
    }
    Ok(())
}
fn expected_rows(result: &PolicyResult, identity: &IdentityReady) -> Result<cleanup::ExpectedRows> {
    if result.cases.iter().any(Option::is_none) {
        return Err(bad());
    }
    expected_rows_partial(result, identity)
}
fn expected_rows_partial(
    result: &PolicyResult,
    identity: &IdentityReady,
) -> Result<cleanup::ExpectedRows> {
    let mut rows = BTreeMap::new();
    for (index, case) in result.cases.iter().enumerate() {
        let Some(case) = case.as_ref() else { continue };
        if rows
            .insert(
                (
                    identity.client_account.clone(),
                    hex::encode(case.handle.invocation()),
                ),
                (
                    (
                        hex::encode(case.handle.session()),
                        case.group_submit_sha256.clone(),
                    ),
                    if index < 4 {
                        cleanup::ExpectedDisposition::Refused(wire::ErrorCode::CommandDenied)
                    } else {
                        cleanup::ExpectedDisposition::ExecutedFixture
                    },
                ),
            )
            .is_some()
        {
            return Err(bad());
        }
    }
    Ok(rows)
}
async fn work(
    ctx: &Context,
    w: &Path,
    identity: &IdentityReady,
    result: &mut PolicyResult,
    state: &mut Runtime,
    env: &Environment,
    signal: &mut Signal,
) -> Result<()> {
    remaining(ctx, 2400)?;
    let index = ROLES
        .iter()
        .position(|r| *r == identity.role)
        .ok_or_else(bad)?;
    let root = copy_fixtures(w, ctx, index)?;
    result.fixture_root = root.to_str().ok_or_else(bad)?.into();
    result.fixture_sha256 = ctx.tooling[index].files.fixture.sha256.clone();
    result.zero_grant = Some(zero_grant(ctx, w, identity, env, &root, state, signal).await?);
    state.stage = Failure::Setup;
    let end = state.end(300);
    if setup(
        w,
        ctx,
        "server",
        Some(&identity.client_account),
        env,
        end,
        signal,
    )
    .await?
        != identity.server_account
        || setup(
            w,
            ctx,
            "client",
            Some(&identity.server_account),
            env,
            end,
            signal,
        )
        .await?
            != identity.client_account
    {
        return Err(bad());
    }
    for (name, peer) in [
        ("server", &identity.client_account),
        ("client", &identity.server_account),
    ] {
        result
            .config_sha256
            .insert(name.into(), write_config(w, ctx, name, peer, &root)?);
    }
    cleanup::require_no_engine(&w.join("profiles/server"))?;
    cleanup::require_no_engine(&w.join("profiles/client"))?;
    state.receipts.insert(
        "client".into(),
        cleanup::ProfileReceipt::capture(&w.join("profiles/client"))?,
    );
    remaining(ctx, 1500)?;
    state.stage = Failure::Activation;
    state.client_name = "client".into();
    state.verify(w, "server")?;
    state.verify(w, "client")?;
    let start = Instant::now();
    state.server = Some(Daemon::start(
        w,
        "server",
        state.sockets.join("server"),
        env,
        &ctx.relays,
        state.uptime(),
    )?);
    state.client = Some(Daemon::start(
        w,
        "client",
        state.sockets.join("client"),
        env,
        &ctx.relays,
        state.uptime(),
    )?);
    matrix::ready(
        state.server.as_mut().ok_or_else(bad)?,
        (start + Duration::from_secs(150)).min(state.deadline),
    )
    .await?;
    state.tree = Some(EngineTree::capture_path(&w.join("profiles/server"))?);
    matrix::ready(
        state.client.as_mut().ok_or_else(bad)?,
        (start + Duration::from_secs(150)).min(state.deadline),
    )
    .await?;
    state.verify(w, "client")?;
    let client = state.client.as_mut().ok_or_else(bad)?;
    cleanup::verify_no_engine(
        &mut client.child,
        &w.join("profiles/client"),
        &client.socket,
        (start + Duration::from_secs(150)).min(state.deadline),
    )
    .await?;
    let work_end = state.end(600);
    state.stage = Failure::Denial;
    for i in 0..4 {
        interrupted(signal)?;
        result.cases[i] = Some(
            denied_case(
                ctx,
                identity,
                &root,
                i,
                state.client.as_mut().ok_or_else(bad)?,
                work_end.min(Instant::now() + Duration::from_secs(90)),
                signal,
            )
            .await?,
        );
    }
    state.expected = expected_rows_partial(result, identity)?;
    state.stage = Failure::Retry;
    result.cases[4] = Some(
        executed_case(
            ExecutionContext {
                ctx,
                w,
                identity,
                root: &root,
                env,
            },
            state.client.as_mut().ok_or_else(bad)?,
            work_end,
            signal,
            result,
            &mut state.expected,
        )
        .await?,
    );
    state.expected = expected_rows(result, identity)?;
    let probe = &result.zero_grant.as_ref().ok_or_else(bad)?.sessions;
    if result
        .cases
        .iter()
        .flatten()
        .any(|c| probe.contains(&hex::encode(c.handle.session())))
    {
        return Err(bad());
    }
    // Exact receipt inventory: four denied paths cannot hide a duplicate execution.
    let wanted = fixture::receipt_path(
        &executable(&root, 4),
        &nonce(ctx, &identity.role, CASES[4])?,
    )?;
    let mut actual = BTreeSet::new();
    for d in ["ok", "ok/deep", "other"] {
        for e in fs::read_dir(root.join(d).join("receipts"))? {
            let p = e?.path();
            let m = fs::symlink_metadata(&p)?;
            if !m.is_file() || m.file_type().is_symlink() {
                return Err(bad());
            }
            actual.insert(p);
        }
    }
    if actual != BTreeSet::from([wanted]) {
        return Err(bad());
    }
    state.stage = Failure::Evidence;
    Ok(())
}
pub(super) async fn run(ctx: &Context, w: &Path, output: &Path) -> Result<PolicyResult> {
    ctx.validate()?;
    ctx.require_cohort("policy")?;
    let identity = records::own_identity(w, ctx)?;
    crate::identity::check_dir(w)?;
    crate::identity::check_dir(output)?;
    if output.parent() != Some(w) || w.canonicalize()? != w || output.canonicalize()? != output {
        return Err(bad());
    }
    let now = super::now()?;
    let mut result = new_result(ctx, &identity, now)?;
    let budget = ctx.expires_at.checked_sub(now).ok_or_else(bad)?;
    let global = Instant::now() + Duration::from_secs(budget.saturating_sub(300));
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| Error::Identity("random"))?;
    let sockets = std::env::temp_dir()
        .canonicalize()?
        .join(format!("mp-{}", hex::encode(random)));
    fs::DirBuilder::new().mode(0o700).create(&sockets)?;
    let env = Environment {
        home: w.join("home"),
        temporary: w.join("tmp"),
        utilities: w.join("utilities"),
    };
    let mut state = Runtime {
        server: None,
        client: None,
        client_name: "client-probe".into(),
        receipts: BTreeMap::new(),
        tree: None,
        expected: BTreeMap::new(),
        sockets,
        stage: Failure::Setup,
        deadline: global,
    };
    let mut signal = tokio::signal::unix::signal(SignalKind::terminate())?;
    let outcome = work(
        ctx,
        w,
        &identity,
        &mut result,
        &mut state,
        &env,
        &mut signal,
    )
    .await;
    if outcome.is_err() {
        result.failure = Some(
            if matches!(outcome, Err(Error::Invalid("installed_interrupted"))) {
                Failure::Interrupted
            } else {
                state.stage
            },
        );
    }
    let close_end = (Instant::now() + Duration::from_secs(300)).min(global);
    let closed = if state.tree.is_some() {
        state.close(w, &mut result, close_end).await.is_ok()
    } else if state.server.is_some() || state.client.is_some() {
        state
            .no_engine_stop(w, close_end)
            .await
            .is_ok_and(|(s, c)| s && c)
    } else {
        false
    };
    // Always join both owned leaders, including failures during proof collection.
    // Ordinary stop preserves any unproved engine/worker state and remains red.
    let (server, client) = (state.server.take(), state.client.take());
    let emergency = (Instant::now() + Duration::from_secs(60)).min(global);
    let _ = tokio::join!(
        async {
            if let Some(d) = server {
                let _ = d.ordinary_stop(emergency).await;
            }
        },
        async {
            if let Some(d) = client {
                let _ = d.ordinary_stop(emergency).await;
            }
        }
    );
    let no_signal = interrupted(&mut signal).is_ok();
    if outcome.is_ok() && closed && no_signal {
        let verified = state
            .receipts
            .iter()
            .all(|(name, r)| r.verify(&w.join("profiles").join(name)).is_ok());
        if verified
            && super::autoexit::remove_private_workspace(
                w,
                output,
                &["server", "client", "client-probe"],
            )
            .is_ok()
        {
            result.workspace_removed = true;
        }
    }
    result.complete = outcome.is_ok() && closed && no_signal && result.workspace_removed;
    if !result.complete && result.failure.is_none() {
        result.failure = Some(if no_signal {
            Failure::Cleanup
        } else {
            Failure::Interrupted
        });
    }
    let _ = fs::remove_dir(&state.sockets);
    validate_record(&result, ctx, &identity.role, &identity)?;
    Ok(result)
}
fn validate_cases(result: &PolicyResult, ctx: &Context, identity: &IdentityReady) -> Result<()> {
    if result.cases.len() != 5 {
        return Err(bad());
    }
    if result.cases.iter().any(Option::is_some)
        && (result.fixture_root.is_empty()
            || result.config_sha256.len() != 2
            || result.zero_grant.is_none())
    {
        return Err(bad());
    }
    let root = Path::new(&result.fixture_root);
    let mut seen = BTreeSet::new();
    let mut session = None;
    let mut ended = false;
    for (index, case) in result.cases.iter().enumerate() {
        let Some(case) = case else {
            ended = true;
            continue;
        };
        let expected_nonce = nonce(ctx, &identity.role, CASES[index])?;
        let value = submit_for_validation(
            &executable(root, index),
            &expected_nonce,
            case.handle.invocation(),
        )?;
        let prepared = command_client::prepare_operation(
            wire::Binding {
                session: case.handle.session(),
                client: account(&identity.client_account)?,
                server: account(&identity.server_account)?,
            },
            id(&case.request_id)?,
            RequestOp::Submit(value),
        )?;
        if ended
            || case.label != CASES[index]
            || case.nonce != expected_nonce
            || case.handle.profile() != "client"
            || case.handle.remote() != account(&identity.server_account)?
            || case.handle != CommandHandle::for_submit("client", &prepared)?
            || case.request_sha256 != hex::encode(prepared.digest)
            || !records::hexadecimal(&case.group_submit_sha256, 64)
            || case.group_submit_sha256 == case.request_sha256
            || !seen.insert(case.handle.invocation())
            || session.is_some_and(|v| v != case.handle.session())
            || result
                .zero_grant
                .as_ref()
                .is_some_and(|z| z.sessions.contains(&hex::encode(case.handle.session())))
        {
            return Err(bad());
        }
        session = Some(case.handle.session());
        if index < 4 {
            if case.refusal != Some(wire::ErrorCode::CommandDenied)
                || !case.receipt_absent
                || case.receipt.is_some()
            {
                return Err(bad());
            }
        } else if case.refusal.is_some()
            || case.receipt_absent
            || case.receipt.as_ref()
                != Some(&fixture::expected_receipt(
                    &executable(root, 4),
                    &expected_nonce,
                    0,
                    &result.fixture_sha256,
                )?)
        {
            return Err(bad());
        }
    }
    Ok(())
}
fn submit_for_validation(exe: &Path, n: &str, invocation: wire::Id) -> Result<wire::Submit> {
    let mut argv = vec![exe.as_os_str().as_bytes().to_vec()];
    argv.extend(
        fixture::arguments(n, 0)?
            .iter()
            .map(|a| a.as_bytes().to_vec()),
    );
    Ok(wire::Submit {
        argv,
        invocation,
        stdin: fixture::bytes(n, 0)?.0,
    })
}
pub(super) fn validate_record(
    result: &PolicyResult,
    ctx: &Context,
    role: &str,
    identity: &IdentityReady,
) -> Result<()> {
    ctx.validate()?;
    ctx.require_cohort("policy")?;
    identity.validate(ctx, role)?;
    let index = ROLES.iter().position(|r| *r == role).ok_or_else(bad)?;
    if result.schema != 1
        || result.kind != "policy-result"
        || result.context_sha256 != ctx.digest()?
        || result.role != role
        || result.identity_ready_sha256 != records::digest(&records::canonical(identity)?)
        || result.route != "direct"
        || result.relays != ctx.relays
        || records::canonical(&result.installation)? != records::canonical(&identity.installation)?
        || result
            .context_age_seconds
            .checked_add(result.context_remaining_seconds)
            != Some(ctx.expires_at - ctx.created_at)
        || records::canonical(result)?.len() > 65536
        || result.complete == result.failure.is_some()
        || result.config_sha256.len() > 2
        || result.config_sha256.iter().any(|(k, v)| {
            !matches!(k.as_str(), "server" | "client") || !records::hexadecimal(v, 64)
        })
    {
        return Err(bad());
    }
    if !result.fixture_root.is_empty() {
        let path = Path::new(&result.fixture_root);
        if !path.is_absolute()
            || path.file_name().and_then(|s| s.to_str()) != Some("policy-bin")
            || result
                .fixture_root
                .contains(['*', '?', '[', ']', '{', '}', '\\'])
            || result.fixture_sha256 != ctx.tooling[index].files.fixture.sha256
        {
            return Err(bad());
        }
    } else if !result.fixture_sha256.is_empty() {
        return Err(bad());
    }
    if let Some(zero) = &result.zero_grant
        && (!records::hexadecimal(&zero.probe_account, 64)
            || zero.probe_account == identity.client_account
            || zero.probe_account == identity.server_account
            || !matches!(zero.exit_code, 254 | 255)
            || !zero.attempted
            || !zero.unconfirmed
            || zero.ledger_rows != 0
            || zero.receipts != 0
            || !zero.server_stop_complete
            || !zero.client_stop_complete
            || zero.sessions.len() > 64
            || zero.sessions.iter().any(|s| !records::hexadecimal(s, 32))
            || zero.sessions.iter().collect::<BTreeSet<_>>().len() != zero.sessions.len())
    {
        return Err(bad());
    }
    validate_cases(result, ctx, identity)?;
    if !result.evidence.is_empty() {
        cleanup::validate_policy_evidence(
            &result.evidence,
            result.final_seq.ok_or_else(bad)?,
            &expected_rows(result, identity)?,
        )?;
    } else if result.final_seq.is_some() && result.engine_cleanup_complete {
        return Err(bad());
    }
    if result.complete
        && (result.context_remaining_seconds < 2400
            || result.zero_grant.is_none()
            || result.cases.iter().any(Option::is_none)
            || result.config_sha256.len() != 2
            || !result.identical_retry
            || !result.local_conflict
            || result.evidence.len() != 5
            || !result.engine_cleanup_complete
            || !result.server_stop_complete
            || !result.client_stop_complete
            || !result.workspace_removed)
    {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn validate(
    result: &PolicyResult,
    ctx: &Context,
    role: &str,
    identity: &IdentityReady,
) -> Result<()> {
    validate_record(result, ctx, role, identity)?;
    if !result.complete {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn failed_record_for(role: &str) -> (Context, IdentityReady, PolicyResult) {
        let (mut ctx, mut identity, _) = benchmark::tests::failed_record_for(role);
        ctx.cohort = "policy".into();
        identity.context_sha256 = ctx.digest().unwrap();
        let mut result = new_result(&ctx, &identity, ctx.created_at).unwrap();
        result.failure = Some(Failure::Setup);
        (ctx, identity, result)
    }
    #[test]
    fn setup_reports_match_fresh_owner_open_and_update_no_open() {
        for (name, fresh) in [
            ("client-probe", true),
            ("client-probe", false),
            ("server", false),
            ("client", false),
        ] {
            let mut setup = prepare::Setup {
                schema: 1,
                identity: name.into(),
                npub: String::new(),
                backend: "file".into(),
                role: if name == "server" { "server" } else { "client" }.into(),
                inbound_enabled: name == "server",
                config_changed: true,
                identity_created: fresh,
                owner_opened: fresh,
                engine_stop_complete: true,
                mdk_stop_complete: true,
                runtime_cleanup_complete: true,
            };
            validate_setup(&setup, name, fresh).unwrap();
            setup.owner_opened = !fresh;
            assert!(validate_setup(&setup, name, fresh).is_err());
            setup.owner_opened = fresh;
            setup.identity_created = !fresh;
            assert!(validate_setup(&setup, name, fresh).is_err());
        }
    }
    #[test]
    fn known_execution_cleanup_binding_survives_failed_wait_and_conflict() {
        let (_, identity, mut result) = complete();
        let executed = result.cases[4].take().unwrap();
        result.failure = Some(Failure::Retry);
        let mut expected = expected_rows_partial(&result, &identity).unwrap();
        assert_eq!(expected.len(), 4);
        retain_execution(
            &mut expected,
            &identity,
            &executed.handle,
            &executed.group_submit_sha256,
        )
        .unwrap();
        assert_eq!(expected.len(), 5);
        assert!(result.cases[4].is_none());
        assert!(
            matches!(expected.get(&(identity.client_account.clone(), hex::encode(executed.handle.invocation()))),
            Some(((session, digest), cleanup::ExpectedDisposition::ExecutedFixture))
            if session == &hex::encode(executed.handle.session()) && digest == &executed.group_submit_sha256)
        );
        assert!(
            retain_execution(
                &mut expected,
                &identity,
                &executed.handle,
                &executed.group_submit_sha256
            )
            .is_err()
        );
    }
    #[test]
    fn genuine_failed_records_are_preserved_for_every_role() {
        for role in ROLES {
            let (ctx, identity, result) = failed_record_for(role);
            validate_record(&result, &ctx, role, &identity).unwrap();
            assert!(validate(&result, &ctx, role, &identity).is_err());
            assert!(!result.is_complete());
            let encoded = records::canonical(&result).unwrap();
            let roundtrip: PolicyResult = serde_json::from_slice(&encoded).unwrap();
            validate_record(&roundtrip, &ctx, role, &identity).unwrap();
            for key in ["zero_grant", "final_seq", "failure"] {
                let mut value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
                value.as_object_mut().unwrap().remove(key);
                assert!(serde_json::from_value::<PolicyResult>(value).is_err());
            }
        }
    }
    fn base(ctx: &Context, peer: &str) -> serde_json::Value {
        serde_json::json!({"version":1,"cwd":null,"limits":{},"role":"server","tor":"off","relays":ctx.relays,
            "aliases":{},"command_policy":{"version":1},"execution_clients":[peer],"trusted_remotes":[]})
    }
    #[test]
    fn actual_engine_checks_additive_default_and_each_key_layer() {
        let (ctx, identity, _) = failed_record_for(ROLES[0]);
        let root = Path::new("/owned/policy-bin");
        let mut value = config_value(
            base(&ctx, &identity.client_account),
            &ctx,
            "server",
            &identity.client_account,
            root,
        )
        .unwrap();
        for reverse in [false, true] {
            if reverse {
                value["command_policy"]["keys"]
                    .as_array_mut()
                    .unwrap()
                    .reverse();
            }
            let config = crate::command_config::CommandConfig::from_bytes(
                &records::canonical(&value).unwrap(),
            )
            .unwrap();
            for index in 0..5 {
                let argv = vec![executable(root, index).as_os_str().as_bytes().to_vec()];
                assert_eq!(
                    config
                        .check_exec_restrictions(account(&identity.client_account).unwrap(), &argv)
                        .is_ok(),
                    index == 4,
                    "case {index}"
                );
            }
        }
        let mut bad_grant = base(&ctx, &identity.client_account);
        bad_grant["execution_clients"] = serde_json::json!([]);
        assert!(config_value(bad_grant, &ctx, "server", &identity.client_account, root).is_err());
    }
    #[test]
    fn partial_cases_cannot_invent_an_empty_grant_or_fixture_binding() {
        let (ctx, identity, mut result) = failed_record_for(ROLES[0]);
        let root = Path::new("/owned/policy-bin");
        let n = nonce(&ctx, &identity.role, CASES[0]).unwrap();
        let invocation = id(&crate::foundation::fresh_id(ctx.created_at).unwrap()).unwrap();
        let request_id = crate::foundation::fresh_id(ctx.created_at).unwrap();
        let prepared = command_client::prepare_operation(
            wire::Binding {
                session: invocation,
                client: account(&identity.client_account).unwrap(),
                server: account(&identity.server_account).unwrap(),
            },
            id(&request_id).unwrap(),
            RequestOp::Submit(submit_for_validation(&executable(root, 0), &n, invocation).unwrap()),
        )
        .unwrap();
        result.cases[0] = Some(Case {
            label: CASES[0].into(),
            nonce: n,
            handle: CommandHandle::for_submit("client", &prepared).unwrap(),
            request_id,
            request_sha256: hex::encode(prepared.digest),
            group_submit_sha256: "f".repeat(64),
            refusal: Some(wire::ErrorCode::CommandDenied),
            receipt_absent: true,
            receipt: None,
        });
        assert!(validate_record(&result, &ctx, &identity.role, &identity).is_err());
        result.fixture_root = root.to_str().unwrap().into();
        result.fixture_sha256 = ctx.tooling[0].files.fixture.sha256.clone();
        result.config_sha256 = BTreeMap::from([
            ("server".into(), "a".repeat(64)),
            ("client".into(), "b".repeat(64)),
        ]);
        assert!(validate_record(&result, &ctx, &identity.role, &identity).is_err());
    }
    fn complete() -> (Context, IdentityReady, PolicyResult) {
        let (ctx, identity, mut result) = failed_record_for(ROLES[0]);
        let root = Path::new("/owned/policy-bin");
        result.failure = None;
        result.fixture_root = root.to_str().unwrap().into();
        result.fixture_sha256 = ctx.tooling[0].files.fixture.sha256.clone();
        result.config_sha256 = BTreeMap::from([
            ("server".into(), "a".repeat(64)),
            ("client".into(), "b".repeat(64)),
        ]);
        result.zero_grant = Some(ZeroGrant {
            probe_account: "c".repeat(64),
            attempted: true,
            exit_code: 254,
            diagnostic: ProbeDiagnostic::LocalPending,
            unconfirmed: true,
            ledger_rows: 0,
            receipts: 0,
            sessions: vec![],
            server_stop_complete: true,
            client_stop_complete: true,
        });
        let session = id(&crate::foundation::fresh_id(ctx.created_at).unwrap()).unwrap();
        for (index, label) in CASES.iter().enumerate() {
            let n = nonce(&ctx, &identity.role, label).unwrap();
            let invocation = id(&crate::foundation::fresh_id(ctx.created_at).unwrap()).unwrap();
            let request_id = crate::foundation::fresh_id(ctx.created_at).unwrap();
            let prepared = command_client::prepare_operation(
                wire::Binding {
                    session,
                    client: account(&identity.client_account).unwrap(),
                    server: account(&identity.server_account).unwrap(),
                },
                id(&request_id).unwrap(),
                RequestOp::Submit(
                    submit_for_validation(&executable(root, index), &n, invocation).unwrap(),
                ),
            )
            .unwrap();
            let denied = index < 4;
            let case = Case {
                label: CASES[index].into(),
                nonce: n.clone(),
                handle: CommandHandle::for_submit("client", &prepared).unwrap(),
                request_id,
                request_sha256: hex::encode(prepared.digest),
                group_submit_sha256: records::digest(&[index as u8]),
                refusal: denied.then_some(wire::ErrorCode::CommandDenied),
                receipt_absent: denied,
                receipt: if denied {
                    None
                } else {
                    Some(
                        fixture::expected_receipt(
                            &executable(root, index),
                            &n,
                            0,
                            &result.fixture_sha256,
                        )
                        .unwrap(),
                    )
                },
            };
            let stream = if denied {
                serde_json::json!({"produced":0,"closed":false,"gaps":0,"sha256":null})
            } else {
                serde_json::json!({"produced":131072,"closed":true,"gaps":0,"sha256":null})
            };
            result.evidence.push(serde_json::json!({"cursor":index+1,"principal":identity.client_account,"group_sha256":"d".repeat(64),"session":hex::encode(session),"invocation":hex::encode(invocation),"submit_sha256":case.group_submit_sha256,"engine_admitted":if denied{0}else{1},"refusal":case.refusal,"outcome":if denied{serde_json::Value::Null}else{serde_json::json!({"kind":"code","value":7})},"stdout":stream,"stderr":stream,"authority_valid":true,"worker_state":if denied{"unknown"}else{"exited"},"observed_seq":9}));
            result.cases[index] = Some(case);
        }
        result.identical_retry = true;
        result.local_conflict = true;
        result.final_seq = Some(9);
        result.engine_cleanup_complete = true;
        result.server_stop_complete = true;
        result.client_stop_complete = true;
        result.workspace_removed = true;
        result.complete = true;
        (ctx, identity, result)
    }
    #[test]
    fn complete_record_binds_all_rows_receipts_and_refusal_dispositions() {
        let (ctx, identity, result) = complete();
        validate(&result, &ctx, &identity.role, &identity).unwrap();
        let encoded = records::canonical(&result).unwrap();
        for change in [
            "extra_row",
            "missing_row",
            "wrong_refusal",
            "admitted_refusal",
            "refused_worker",
            "wrong_binding",
            "stale",
            "receipt",
            "duplicate_case",
            "request_digest",
            "group_digest",
            "probe_session",
            "probe_exit",
            "cleanup",
        ] {
            let mut r: PolicyResult = serde_json::from_slice(&encoded).unwrap();
            match change {
                "extra_row" => r.evidence.push(r.evidence[0].clone()),
                "missing_row" => {
                    r.evidence.pop();
                }
                "wrong_refusal" => r.evidence[0]["refusal"] = "not_found".into(),
                "admitted_refusal" => r.evidence[0]["engine_admitted"] = 1.into(),
                "refused_worker" => r.evidence[0]["worker_state"] = "exited".into(),
                "wrong_binding" => r.evidence[0]["submit_sha256"] = "f".repeat(64).into(),
                "stale" => r.evidence[0]["observed_seq"] = 8.into(),
                "receipt" => {
                    r.cases[4]
                        .as_mut()
                        .unwrap()
                        .receipt
                        .as_mut()
                        .unwrap()
                        .stdout_bytes = 0
                }
                "duplicate_case" => {
                    r.cases[1] = Some(
                        serde_json::from_value(
                            serde_json::to_value(r.cases[0].as_ref().unwrap()).unwrap(),
                        )
                        .unwrap(),
                    )
                }
                "request_digest" => r.cases[4].as_mut().unwrap().request_sha256 = "f".repeat(64),
                "group_digest" => {
                    let c = r.cases[4].as_mut().unwrap();
                    c.group_submit_sha256 = c.request_sha256.clone();
                }
                "probe_session" => r
                    .zero_grant
                    .as_mut()
                    .unwrap()
                    .sessions
                    .push(hex::encode(r.cases[0].as_ref().unwrap().handle.session())),
                "probe_exit" => r.zero_grant.as_mut().unwrap().exit_code = 0,
                _ => r.engine_cleanup_complete = false,
            }
            assert!(
                validate(&r, &ctx, &identity.role, &identity).is_err(),
                "{change}"
            );
        }
    }
}
