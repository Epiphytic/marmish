//! Four installed same-platform commands; private build tooling, never a runtime API.
use super::{
    cleanup, fixture,
    matrix::{self, Daemon},
    process::{self, Environment},
    records::{self, Context, IdentityReady, Installation, ROLES},
};
use crate::{Error, Result, command_client::CommandHandle};
use nostr::nips::nip19::ToBech32;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, time::Instant};

fn bad() -> Error {
    Error::Invalid("installed_benchmark")
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Failure {
    Preparation,
    Activation,
    Interrupted,
    Command,
    UnknownAdmission,
    Output,
    Receipt,
    Cleanup,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Temperature {
    Cold,
    Warm,
}
impl Temperature {
    fn label(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
        }
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Disposition {
    NotRun,
    Failed,
    Passed,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Handle {
    format: u8,
    profile: String,
    remote: String,
    session: String,
    invocation: String,
    submit_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    index: u8,
    temperature: Temperature,
    disposition: Disposition,
    failure: Option<Failure>,
    nonce: String,
    vector: u8,
    handle: Option<Handle>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    group_submit_sha256: Option<String>,
    receipt: Option<fixture::Receipt>,
    wall_ns: Option<u64>,
    goodput_bytes_per_second: Option<f64>,
    qualified: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchmarkResult {
    schema: u8,
    kind: String,
    context_sha256: String,
    role: String,
    identity_ready_sha256: String,
    context_age_seconds: u64,
    context_remaining_seconds: u64,
    installation: Installation,
    route: String,
    relays: Vec<String>,
    fixture_path: String,
    fixture_sha256: String,
    planned_samples: u8,
    successful_samples: u8,
    samples: Vec<Sample>,
    failure: Option<Failure>,
    engine_cleanup_complete: bool,
    server_stop_complete: bool,
    client_stop_complete: bool,
    complete: bool,
}
impl BenchmarkResult {
    pub(super) fn is_complete(&self) -> bool {
        self.complete
    }
}
fn nonce(ctx: &Context, id: &IdentityReady, index: u8) -> Result<String> {
    Ok(records::digest(&records::canonical(&(
        "installed-benchmark/1",
        ctx.digest()?,
        &id.role,
        &id.client_account,
        &id.server_account,
        index,
    ))?))
}
fn remaining_at(ctx: &Context, now: u64, minimum: u64) -> Result<()> {
    if ctx.expires_at.checked_sub(now).is_none_or(|n| n < minimum) {
        return Err(Error::Timeout);
    }
    Ok(())
}
fn deadlines(launch: Instant) -> (Instant, Instant, Instant) {
    (
        launch + Duration::from_secs(150),
        launch + Duration::from_secs(1350),
        launch + Duration::from_secs(1770),
    )
}
fn goodput(ns: u64) -> Result<f64> {
    if ns == 0 || ns > 330_000_000_000 {
        return Err(bad());
    }
    Ok(262144.0 * 1_000_000_000.0 / ns as f64)
}
fn parsed_handle<'a>(stderr: &'a [u8], remote: &str) -> Result<(Handle, &'a [u8])> {
    let rest = stderr.strip_prefix(b"marmish handle: ").ok_or_else(bad)?;
    let end = rest.iter().position(|b| *b == b'\n').ok_or_else(bad)?;
    let h: Handle = serde_json::from_slice(&rest[..end]).map_err(|_| bad())?;
    if h.format != 1
        || h.profile != "client"
        || h.remote != remote
        || !records::hexadecimal(&h.session, 32)
        || !records::hexadecimal(&h.invocation, 32)
        || !records::hexadecimal(&h.submit_sha256, 64)
    {
        return Err(bad());
    }
    Ok((h, &rest[end + 1..]))
}
fn wait_arguments(workspace: &Path, socket: &Path, encoded: &[u8]) -> Result<Vec<OsString>> {
    let mut args = matrix::base(workspace, "client", socket, &[]);
    args.extend([
        "--no-autostart".into(),
        "--wait-seconds".into(),
        "300".into(),
        "wait".into(),
        "--handle".into(),
        std::str::from_utf8(encoded).map_err(|_| bad())?.into(),
    ]);
    Ok(args)
}
fn configure_value(
    mut v: serde_json::Value,
    name: &str,
    ctx: &Context,
    peer: &str,
    fixture: &Path,
    server_ceiling: u64,
) -> Result<serde_json::Value> {
    if !matches!(server_ceiling, 3 | 5) {
        return Err(bad());
    }
    let server = name == "server";
    let peers = serde_json::json!([peer]);
    let empty = serde_json::json!([]);
    if !matches!(name, "server" | "client")
        || v["role"] != name
        || v["tor"] != "off"
        || v["aliases"] != serde_json::json!({})
        || v["command_policy"] != serde_json::json!({"version":1})
        || v["relays"] != serde_json::json!(ctx.relays)
        || v["execution_clients"] != if server { &peers } else { &empty }.clone()
        || v["trusted_remotes"] != if server { &empty } else { &peers }.clone()
    {
        return Err(bad());
    }
    v["command_policy"]["defaults"]["allow_exec"] = if server {
        serde_json::json!([fixture.to_str().ok_or_else(bad)?])
    } else {
        empty
    };
    v["cwd"] = serde_json::json!(fixture.parent().and_then(Path::to_str));
    v["limits"] = if server {
        serde_json::json!({"max_commands":server_ceiling,"max_uptime":1800})
    } else {
        serde_json::json!({"max_uptime":1800})
    };
    let mut config = crate::command_config::CommandConfig::from_bytes(&records::canonical(&v)?)?;
    config.apply_limit_overrides(Default::default())?;
    Ok(v)
}
pub(super) async fn configure(
    w: &Path,
    ctx: &Context,
    id: &IdentityReady,
    fixture: &Path,
    env: &Environment,
    server_ceiling: u64,
    deadline: Instant,
) -> Result<()> {
    for name in ["server", "client"] {
        let (peer, own) = if name == "server" {
            (&id.client_account, &id.server_account)
        } else {
            (&id.server_account, &id.client_account)
        };
        let mut args = matrix::base(w, name, &w.join("setup-unused.sock"), &ctx.relays);
        args.extend([
            "setup".into(),
            "--update".into(),
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
        let output = process::run(&w.join("bin/marmish"), &args, w, env, deadline, 65536).await?;
        let setup: super::prepare::Setup =
            serde_json::from_slice(&output.stdout).map_err(|_| bad())?;
        if !output.status.success()
            || setup.schema != 1
            || setup.identity != name
            || setup.backend != "file"
            || setup.role != name
            || setup.inbound_enabled != (name == "server")
            || !setup.config_changed
            || setup.identity_created
            || setup.owner_opened
            || !setup.engine_stop_complete
            || !setup.mdk_stop_complete
            || !setup.runtime_cleanup_complete
            || hex::encode(crate::command_config::account(&setup.npub)?) != *own
        {
            return Err(bad());
        }
        let path = w.join("profiles").join(name).join("config.json");
        let old = records::read_bytes(&path, 1048576)?;
        crate::command_config::CommandConfig::from_bytes(&old)?;
        let v = configure_value(
            serde_json::from_slice(&old).map_err(|_| bad())?,
            name,
            ctx,
            peer,
            fixture,
            server_ceiling,
        )?;
        let bytes = records::canonical(&v)?;
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
    }
    Ok(())
}
pub(super) fn copy_fixture(w: &Path, ctx: &Context, index: usize) -> Result<PathBuf> {
    let tool = records::verify_tools(ctx, index)?;
    let source = tool
        .parent()
        .ok_or_else(bad)?
        .join("marmish-installed-fixture");
    let bytes = records::read_bytes(&source, 268435456)?;
    if records::digest(&bytes) != ctx.tooling[index].files.fixture.sha256 {
        return Err(bad());
    }
    let dir = w.join("fixture");
    fs::DirBuilder::new().mode(0o700).create(&dir)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(dir.join("receipts"))?;
    let path = dir.join("main");
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o700)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    let path = path.canonicalize()?;
    if path
        .to_str()
        .ok_or_else(bad)?
        .contains(['*', '?', '[', ']', '{', '}', '\\'])
        || records::digest(&records::read_bytes(&path, 268435456)?)
            != ctx.tooling[index].files.fixture.sha256
    {
        return Err(bad());
    }
    Ok(path)
}

// Keep child and all tasks owned even when SIGTERM or the absolute deadline wins.
pub(super) async fn execute(
    w: &Path,
    args: &[OsString],
    env: &Environment,
    input: Vec<u8>,
    deadline: Instant,
    cancelled: impl std::future::Future<Output = ()>,
) -> Result<process::Output> {
    let started = Instant::now();
    if started >= deadline {
        return Err(Error::Timeout);
    }
    let mut command = process::command(&w.join("bin/marmish"), args, w, env);
    command.stdin(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(bad)?;
    let mut writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    });
    let mut out = tokio::spawn(process::drain(child.stdout.take().ok_or_else(bad)?, 524288));
    let mut err = tokio::spawn(process::drain(child.stderr.take().ok_or_else(bad)?, 524288));
    let status = tokio::select! {
        biased;
        _ = cancelled => Err(Error::Invalid("installed_interrupted")),
        _ = tokio::time::sleep_until(deadline) => Err(Error::Timeout),
        s = child.wait() => s.map_err(Error::from),
    };
    let status = match status {
        Ok(s) => s,
        Err(e) => {
            let _ = process::terminate(&mut child).await;
            writer.abort();
            out.abort();
            err.abort();
            let _ = writer.await;
            let _ = out.await;
            let _ = err.await;
            return Err(e);
        }
    };
    let collected = tokio::time::timeout_at(deadline, async {
        (&mut writer).await.map_err(|_| bad())??;
        let stdout = (&mut out).await.map_err(|_| bad())??;
        let stderr = (&mut err).await.map_err(|_| bad())??;
        if stdout.1 || stderr.1 {
            return Err(bad());
        }
        Ok((stdout.0, stderr.0))
    })
    .await
    .map_err(|_| Error::Timeout)
    .and_then(|r| r);
    let collected = match collected {
        Ok(value) => value,
        Err(error) => {
            writer.abort();
            out.abort();
            err.abort();
            // A completed JoinHandle must not be polled twice.
            if !writer.is_finished() {
                let _ = writer.await;
            }
            if !out.is_finished() {
                let _ = out.await;
            }
            if !err.is_finished() {
                let _ = err.await;
            }
            return Err(error);
        }
    };
    Ok(process::Output {
        status,
        stdout: collected.0,
        stderr: collected.1,
    })
}
#[derive(Default)]
struct Runtime {
    server: Option<Daemon>,
    client: Option<Daemon>,
    tree: Option<super::super::command_check::EngineTree>,
    unknown_admission: bool,
    client_profile: Option<cleanup::ProfileReceipt>,
}
impl Runtime {
    async fn close(
        &mut self,
        w: &Path,
        expected: BTreeMap<(String, String), (String, String)>,
        cancel: bool,
        deadline: Instant,
    ) -> (bool, bool, bool) {
        let cleanup_deadline = deadline
            .checked_sub(Duration::from_secs(60))
            .unwrap_or(deadline);
        let plan = cleanup_plan(self.unknown_admission, cancel);
        if plan.cancel
            && let Some(s) = self.server.as_mut()
        {
            let _ =
                cleanup::cancel_fixture(&mut s.child, &s.socket, &expected, 4, cleanup_deadline)
                    .await;
        }
        let engine = if plan.prepare
            && let (Some(s), Some(tree)) = (self.server.as_mut(), self.tree.take())
        {
            match cleanup::prepare(
                &mut s.child,
                &w.join("profiles/server"),
                &s.socket,
                expected,
                cleanup_deadline,
            )
            .await
            {
                Ok(mut guard) => tree.remove_installed(&mut guard).await.is_ok(),
                Err(_) => false,
            }
        } else {
            false
        };
        let client = self.client.take();
        let server = self.server.take();
        let (client, server) = tokio::join!(
            async {
                match client {
                    Some(mut d) => {
                        let profile = w.join("profiles/client");
                        let receipt = self.client_profile.as_ref();
                        let original = receipt.is_some_and(|r| r.verify(&profile).is_ok());
                        let proven = original
                            && cleanup::verify_no_engine(
                                &mut d.child,
                                &profile,
                                &d.socket,
                                cleanup_deadline,
                            )
                            .await
                            .is_ok();
                        let stopped = d.stop_no_engine(deadline).await.is_ok();
                        proven
                            && stopped
                            && receipt.is_some_and(|r| r.verify(&profile).is_ok())
                            && cleanup::require_no_engine(&profile).is_ok()
                    }
                    None => false,
                }
            },
            async {
                match server {
                    Some(d) => d.stop(deadline).await.is_ok(),
                    None => false,
                }
            },
        );
        (engine, server, client)
    }
}
fn new_result(ctx: &Context, id: &IdentityReady, now: u64) -> Result<BenchmarkResult> {
    Ok(BenchmarkResult {
        schema: 1,
        kind: "benchmark-result".into(),
        context_sha256: ctx.digest()?,
        role: id.role.clone(),
        identity_ready_sha256: records::digest(&records::canonical(id)?),
        context_age_seconds: now.checked_sub(ctx.created_at).ok_or_else(bad)?,
        context_remaining_seconds: ctx.expires_at.checked_sub(now).ok_or_else(bad)?,
        installation: id.installation.clone(),
        route: "direct".into(),
        relays: ctx.relays.clone(),
        fixture_path: String::new(),
        fixture_sha256: ctx.tooling[ROLES.iter().position(|r| *r == id.role).ok_or_else(bad)?]
            .files
            .fixture
            .sha256
            .clone(),
        planned_samples: 4,
        successful_samples: 0,
        samples: (0..4)
            .map(|i| {
                Ok(Sample {
                    index: i,
                    temperature: if i == 0 {
                        Temperature::Cold
                    } else {
                        Temperature::Warm
                    },
                    disposition: Disposition::NotRun,
                    failure: None,
                    nonce: nonce(ctx, id, i)?,
                    vector: 0,
                    handle: None,
                    group_submit_sha256: None,
                    receipt: None,
                    wall_ns: None,
                    goodput_bytes_per_second: None,
                    qualified: false,
                })
            })
            .collect::<Result<_>>()?,
        failure: None,
        engine_cleanup_complete: false,
        server_stop_complete: false,
        client_stop_complete: false,
        complete: false,
    })
}
struct RunState {
    runtime: Runtime,
    expected: BTreeMap<(String, String), (String, String)>,
    cleanup_deadline: Instant,
    sockets: Option<PathBuf>,
    stage: Failure,
}
struct Client<'a> {
    workspace: &'a Path,
    context: &'a Context,
    identity: &'a IdentityReady,
    environment: &'a Environment,
    executable: &'a Path,
    socket: &'a Path,
    deadline: Instant,
}
fn exact_output(
    output: &process::Output,
    stdout: &[u8],
    actual_stderr: &[u8],
    stderr: &[u8],
) -> Result<()> {
    if output.status.code() != Some(7) || output.stdout != stdout || actual_stderr != stderr {
        return Err(bad());
    }
    Ok(())
}
struct Executed {
    handle: Handle,
    locator: CommandHandle,
    outcome: Result<process::Output>,
    elapsed: Duration,
}
impl Client<'_> {
    async fn command(
        &self,
        args: &[OsString],
        input: Vec<u8>,
        expected: (&[u8], &[u8]),
        signal: &mut tokio::signal::unix::Signal,
    ) -> Result<Executed> {
        let (stdout, stderr) = expected;
        let started = Instant::now();
        let deadline = self.deadline.min(started + Duration::from_secs(330));
        let detached = execute(
            self.workspace,
            args,
            self.environment,
            input,
            deadline,
            async {
                signal.recv().await;
            },
        )
        .await?;
        let (handle, tail) = parsed_handle(&detached.stderr, &self.identity.server_account)?;
        if detached.status.code() != Some(0) || !detached.stdout.is_empty() || !tail.is_empty() {
            return Err(bad());
        }
        // Preserve the exact CLI bytes: record canonicalization has a different order.
        let encoded = detached
            .stderr
            .strip_prefix(b"marmish handle: ")
            .and_then(|bytes| bytes.strip_suffix(b"\n"))
            .ok_or_else(bad)?;
        let locator: CommandHandle = serde_json::from_slice(encoded).map_err(|_| bad())?;
        let args = wait_arguments(self.workspace, self.socket, encoded)?;
        let waited = match pending_signal(signal).await {
            Err(error) => Err(error),
            Ok(()) => execute(
                self.workspace,
                &args,
                self.environment,
                Vec::new(),
                deadline,
                async {
                    signal.recv().await;
                },
            )
            .await
            .and_then(|output| {
                exact_output(&output, stdout, &output.stderr, stderr)?;
                Ok(output)
            }),
        };
        Ok(Executed {
            handle,
            locator,
            outcome: waited,
            elapsed: started.elapsed(),
        })
    }

    async fn sample(
        &self,
        sample: &mut Sample,
        signal: &mut tokio::signal::unix::Signal,
        daemon: &mut Daemon,
    ) -> Result<()> {
        sample.disposition = Disposition::Failed;
        sample.failure = Some(Failure::Command);
        let (input, stdout, stderr) = fixture::bytes(&sample.nonce, 0)?;
        let mut args = matrix::base(self.workspace, "client", self.socket, &self.context.relays);
        args.extend([
            "--no-autostart".into(),
            "--stdin".into(),
            "--detach".into(),
            "--wait-seconds".into(),
            "300".into(),
            nostr::key::PublicKey::from_hex(&self.identity.server_account)
                .map_err(|_| bad())?
                .to_bech32()
                .map_err(|_| bad())?
                .into(),
            "--".into(),
            self.executable.as_os_str().into(),
        ]);
        args.extend(fixture::arguments(&sample.nonce, 0)?);
        sample.failure = Some(Failure::UnknownAdmission);
        let executed = self
            .command(&args, input, (&stdout, &stderr), signal)
            .await?;
        sample.handle = Some(executed.handle);
        sample.failure = Some(Failure::Output);
        // Even a failed wait may have an admitted invocation; bind it before cleanup.
        sample.group_submit_sha256 = Some(
            cleanup::client_group_submit(
                &mut daemon.child,
                self.socket,
                &executed.locator,
                self.deadline,
            )
            .await?,
        );
        executed.outcome?;
        sample.failure = Some(Failure::Receipt);
        let receipt: fixture::Receipt = records::read(
            &fixture::receipt_path(self.executable, &sample.nonce)?,
            65536,
        )?;
        let expected = fixture::expected_receipt(
            self.executable,
            &sample.nonce,
            0,
            &self.context.tooling[ROLES
                .iter()
                .position(|r| *r == self.identity.role)
                .ok_or_else(bad)?]
            .files
            .fixture
            .sha256,
        )?;
        if receipt != expected {
            return Err(bad());
        }
        let ns = u64::try_from(executed.elapsed.as_nanos()).map_err(|_| bad())?;
        sample.goodput_bytes_per_second = Some(goodput(ns)?);
        sample.wall_ns = Some(ns);
        sample.receipt = Some(receipt);
        sample.qualified = true;
        sample.disposition = Disposition::Passed;
        sample.failure = None;
        Ok(())
    }
}
#[derive(PartialEq, Eq)]
struct CleanupPlan {
    cancel: bool,
    prepare: bool,
}
fn cleanup_plan(unknown: bool, cancel: bool) -> CleanupPlan {
    CleanupPlan {
        cancel: cancel && !unknown,
        prepare: !unknown,
    }
}
fn record_sample(
    sample: &mut Sample,
    principal: &str,
    expected: &mut BTreeMap<(String, String), (String, String)>,
    unknown: &mut bool,
    outcome: Result<()>,
) -> Result<()> {
    let binding =
        if let (Some(handle), Some(digest)) = (&sample.handle, &sample.group_submit_sha256) {
            let key = (principal.to_owned(), handle.invocation.clone());
            if expected.keys().any(|(_, id)| id == &handle.invocation) {
                *unknown = true;
                Err(bad())
            } else {
                let changed = expected
                    .values()
                    .any(|(session, _)| session != &handle.session);
                expected.insert(key, (handle.session.clone(), digest.clone()));
                if changed { Err(bad()) } else { Ok(()) }
            }
        } else {
            *unknown = true;
            Err(bad())
        };
    if binding.is_err() {
        sample.qualified = false;
        sample.disposition = Disposition::Failed;
        sample.failure = Some(if *unknown {
            Failure::UnknownAdmission
        } else {
            Failure::Output
        });
        sample.wall_ns = None;
        sample.goodput_bytes_per_second = None;
    }
    binding.and(outcome)
}
async fn pending_signal(signal: &mut tokio::signal::unix::Signal) -> Result<()> {
    let pending =
        std::future::poll_fn(|cx| std::task::Poll::Ready(signal.poll_recv(cx).is_ready())).await;
    if pending {
        Err(Error::Invalid("installed_interrupted"))
    } else {
        Ok(())
    }
}
async fn work(
    ctx: &Context,
    workspace: &Path,
    id: &IdentityReady,
    result: &mut BenchmarkResult,
    state: &mut RunState,
    signal: &mut tokio::signal::unix::Signal,
) -> Result<()> {
    remaining_at(ctx, super::now()?, 2220)?;
    let environment = Environment {
        home: workspace.join("home"),
        temporary: workspace.join("tmp"),
        utilities: workspace.join("utilities"),
    };
    let index = ROLES.iter().position(|r| *r == id.role).ok_or_else(bad)?;
    let executable = copy_fixture(workspace, ctx, index)?;
    result.fixture_path = executable.to_str().ok_or_else(bad)?.into();
    result.fixture_sha256 = ctx.tooling[index].files.fixture.sha256.clone();
    cleanup::require_no_engine(&workspace.join("profiles/client"))?;
    state.runtime.client_profile = Some(cleanup::ProfileReceipt::capture(
        &workspace.join("profiles/client"),
    )?);
    configure(
        workspace,
        ctx,
        id,
        &executable,
        &environment,
        5,
        Instant::now() + Duration::from_secs(150),
    )
    .await?;
    state
        .runtime
        .client_profile
        .as_ref()
        .ok_or_else(bad)?
        .verify(&workspace.join("profiles/client"))?;
    pending_signal(signal).await?;
    remaining_at(ctx, super::now()?, 2070)?;
    match fs::symlink_metadata(workspace.join("profiles/server/commands")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(bad()),
    }
    let mut random = [0; 8];
    getrandom::fill(&mut random).map_err(|_| Error::Identity("random"))?;
    let directory = std::env::temp_dir()
        .canonicalize()?
        .join(format!("mb-{}", hex::encode(random)));
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    state.sockets = Some(directory.clone());
    let (ready_deadline, work_deadline, cleanup_deadline) = deadlines(Instant::now());
    state.cleanup_deadline = cleanup_deadline;
    state.stage = Failure::Activation;
    state.runtime.server = Some(Daemon::start(
        workspace,
        "server",
        directory.join("s"),
        &environment,
        &ctx.relays,
        1800,
    )?);
    cleanup::require_no_engine(&workspace.join("profiles/client"))?;
    state.runtime.client = Some(Daemon::start(
        workspace,
        "client",
        directory.join("c"),
        &environment,
        &ctx.relays,
        1800,
    )?);
    let activation = async {
        matrix::ready(
            state.runtime.server.as_mut().ok_or_else(bad)?,
            ready_deadline,
        )
        .await?;
        state.runtime.tree = Some(super::super::command_check::EngineTree::capture_path(
            &workspace.join("profiles/server"),
        )?);
        matrix::ready(
            state.runtime.client.as_mut().ok_or_else(bad)?,
            ready_deadline,
        )
        .await?;
        let client = state.runtime.client.as_mut().ok_or_else(bad)?;
        state
            .runtime
            .client_profile
            .as_ref()
            .ok_or_else(bad)?
            .verify(&workspace.join("profiles/client"))?;
        cleanup::verify_no_engine(
            &mut client.child,
            &workspace.join("profiles/client"),
            &client.socket,
            ready_deadline,
        )
        .await
    };
    tokio::select! {
        biased;
        _ = signal.recv() => return Err(Error::Invalid("installed_interrupted")),
        result = activation => result?,
    }
    let client_socket = directory.join("c");
    let client = Client {
        workspace,
        context: ctx,
        identity: id,
        environment: &environment,
        executable: &executable,
        socket: &client_socket,
        deadline: work_deadline,
    };
    for sample in &mut result.samples {
        pending_signal(signal).await?;
        let outcome = client
            .sample(
                sample,
                signal,
                state.runtime.client.as_mut().ok_or_else(bad)?,
            )
            .await;
        let outcome = record_sample(
            sample,
            &id.client_account,
            &mut state.expected,
            &mut state.runtime.unknown_admission,
            outcome,
        );
        state.stage = sample.failure.unwrap_or(Failure::Command);
        outcome?;
        result.successful_samples += 1;
    }
    Ok(())
}
pub(super) async fn run(ctx: &Context, workspace: &Path, output: &Path) -> Result<()> {
    ctx.validate()?;
    ctx.require_cohort("benchmark")?;
    let id = records::own_identity(workspace, ctx)?;
    crate::identity::check_dir(workspace)?;
    crate::identity::check_dir(output.parent().ok_or_else(bad)?)?;
    let mut result = new_result(ctx, &id, super::now()?)?;
    let mut state = RunState {
        runtime: Runtime::default(),
        expected: BTreeMap::new(),
        cleanup_deadline: Instant::now(),
        sockets: None,
        stage: Failure::Preparation,
    };
    let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let outcome = work(ctx, workspace, &id, &mut result, &mut state, &mut signal).await;
    if outcome.is_err() {
        result.failure = Some(
            if matches!(&outcome, Err(Error::Invalid("installed_interrupted"))) {
                Failure::Interrupted
            } else {
                state.stage
            },
        );
    }
    let interrupted_after_work = pending_signal(&mut signal).await.is_err();
    let flags = state
        .runtime
        .close(
            workspace,
            state.expected,
            outcome.is_err(),
            state.cleanup_deadline,
        )
        .await;
    result.engine_cleanup_complete = flags.0;
    result.server_stop_complete = flags.1;
    result.client_stop_complete = flags.2;
    let interrupted = interrupted_after_work || pending_signal(&mut signal).await.is_err();
    result.complete = outcome.is_ok() && !interrupted && flags.0 && flags.1 && flags.2;
    if outcome.is_ok() && !result.complete {
        result.failure = Some(if interrupted {
            Failure::Interrupted
        } else {
            Failure::Cleanup
        });
    }
    if let Some(directory) = state.sockets {
        let _ = fs::remove_dir(directory);
    }
    records::write(output, &result)?;
    if result.complete {
        validate(&result, ctx, &id)
    } else {
        Err(bad())
    }
}
fn validate_samples(r: &BenchmarkResult, ctx: &Context, id: &IdentityReady) -> Result<()> {
    if r.samples.len() != 4 {
        return Err(bad());
    }
    let mut invocations = BTreeSet::new();
    let mut session = None;
    let mut ended = false;
    let mut failed = false;
    let mut passed = 0;
    for (index, sample) in r.samples.iter().enumerate() {
        if sample.index as usize != index
            || sample.temperature
                != if index == 0 {
                    Temperature::Cold
                } else {
                    Temperature::Warm
                }
            || sample.vector != 0
            || sample.nonce != nonce(ctx, id, sample.index)?
        {
            return Err(bad());
        }
        if let Some(handle) = &sample.handle
            && (handle.format != 1
                || handle.profile != "client"
                || handle.remote != id.server_account
                || !records::hexadecimal(&handle.session, 32)
                || !records::hexadecimal(&handle.invocation, 32)
                || !records::hexadecimal(&handle.submit_sha256, 64))
        {
            return Err(bad());
        }
        if sample
            .group_submit_sha256
            .as_ref()
            .is_some_and(|digest| !records::hexadecimal(digest, 64))
            || (sample.group_submit_sha256.is_some() && sample.handle.is_none())
        {
            return Err(bad());
        }
        if let Some(receipt) = &sample.receipt
            && (r.fixture_path.is_empty()
                || *receipt
                    != fixture::expected_receipt(
                        Path::new(&r.fixture_path),
                        &sample.nonce,
                        0,
                        &r.fixture_sha256,
                    )?)
        {
            return Err(bad());
        }
        match sample.disposition {
            Disposition::Passed => {
                let handle = sample.handle.as_ref().ok_or_else(bad)?;
                if ended
                    || !sample.qualified
                    || sample.failure.is_some()
                    || sample.receipt.is_none()
                    || sample.group_submit_sha256.is_none()
                    || !invocations.insert(&handle.invocation)
                    || session.is_some_and(|v| v != &handle.session)
                    || sample.goodput_bytes_per_second
                        != Some(goodput(sample.wall_ns.ok_or_else(bad)?)?)
                {
                    return Err(bad());
                }
                session = Some(&handle.session);
                passed += 1;
            }
            Disposition::Failed => {
                if ended
                    || failed
                    || sample.qualified
                    || sample.failure.is_none()
                    || sample.wall_ns.is_some()
                    || sample.goodput_bytes_per_second.is_some()
                {
                    return Err(bad());
                }
                failed = true;
                ended = true;
            }
            Disposition::NotRun => {
                if sample.qualified
                    || sample.failure.is_some()
                    || sample.handle.is_some()
                    || sample.group_submit_sha256.is_some()
                    || sample.receipt.is_some()
                    || sample.wall_ns.is_some()
                    || sample.goodput_bytes_per_second.is_some()
                {
                    return Err(bad());
                }
                ended = true;
            }
        }
    }
    if r.successful_samples != passed {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn validate_record(
    r: &BenchmarkResult,
    ctx: &Context,
    id: &IdentityReady,
) -> Result<()> {
    ctx.validate()?;
    ctx.require_cohort("benchmark")?;
    id.validate(ctx, &id.role)?;
    let i = ROLES.iter().position(|v| *v == id.role).ok_or_else(bad)?;
    if r.schema != 1
        || r.kind != "benchmark-result"
        || r.context_sha256 != ctx.digest()?
        || r.role != id.role
        || r.identity_ready_sha256 != records::digest(&records::canonical(id)?)
        || records::canonical(&r.installation)? != records::canonical(&id.installation)?
        || r.context_age_seconds
            .checked_add(r.context_remaining_seconds)
            != ctx.expires_at.checked_sub(ctx.created_at)
        || r.route != "direct"
        || r.relays != ctx.relays
        || r.fixture_sha256 != ctx.tooling[i].files.fixture.sha256
        || (!Path::new(&r.fixture_path).is_absolute()
            && !(r.fixture_path.is_empty()
                && r.successful_samples == 0
                && r.failure == Some(Failure::Preparation)))
        || r.planned_samples != 4
        || r.complete == r.failure.is_some()
    {
        return Err(bad());
    }
    validate_samples(r, ctx, id)?;
    if r.complete
        && (r.context_remaining_seconds < 2220
            || r.successful_samples != 4
            || !r.engine_cleanup_complete
            || !r.server_stop_complete
            || !r.client_stop_complete)
    {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn validate(r: &BenchmarkResult, ctx: &Context, id: &IdentityReady) -> Result<()> {
    validate_record(r, ctx, id)?;
    if !r.complete {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn markdown_summary(r: &BenchmarkResult) -> Result<String> {
    if r.samples.len() != 4 {
        return Err(bad());
    }
    let mut text = format!(
        "### {}\n\nRoute: direct. Relays: {}. Wall time includes detached submit and wait subprocesses; excludes cached binding lookup, receipt validation and cleanup. It is not CPU time or network RTT. Per command: 16384 stdin bytes; 131072 stdout and 131072 stderr bytes.\n\n| Sample | Qualification | Submit + wait wall milliseconds | Goodput bytes/s |\n| --- | --- | ---: | ---: |\n",
        r.role,
        r.relays.join(", ")
    );
    for sample in &r.samples {
        let (qualification, wall, speed) = if sample.qualified {
            let ns = sample.wall_ns.ok_or_else(bad)?;
            (
                "qualified",
                format!("{:.3}", ns as f64 / 1e6),
                format!("{:.3}", goodput(ns)?),
            )
        } else {
            ("unqualified", "null".into(), "null".into())
        };
        text.push_str(&format!(
            "| {} {} | {} | {} | {} |\n",
            sample.temperature.label(),
            sample.index,
            qualification,
            wall,
            speed
        ));
    }
    text.push_str(&format!(
        "Failure category: `{}`. Complete: `{}`.\n\n",
        serde_json::to_string(&r.failure).map_err(|_| bad())?,
        r.complete
    ));
    let cold = usize::from(r.samples[0].qualified);
    let warm_count = r.samples[1..].iter().filter(|s| s.qualified).count();
    text.push_str(&format!(
        "\nCold n={cold}. Warm n={warm_count}. No percentiles.\n"
    ));
    if warm_count == 3 {
        let mut warm = r.samples[1..]
            .iter()
            .map(|s| s.wall_ns.ok_or_else(bad))
            .collect::<Result<Vec<_>>>()?;
        warm.sort();
        text.push_str(&format!(
            "Warm n=3: wall min/median/max {:.3}/{:.3}/{:.3} ms; range {:.3} ms.\n",
            warm[0] as f64 / 1e6,
            warm[1] as f64 / 1e6,
            warm[2] as f64 / 1e6,
            (warm[2] - warm[0]) as f64 / 1e6
        ));
        text.push_str(&format!(
            "Warm goodput min/median/max {:.3}/{:.3}/{:.3} bytes/s; range {:.3} bytes/s.\n",
            goodput(warm[2])?,
            goodput(warm[1])?,
            goodput(warm[0])?,
            goodput(warm[0])? - goodput(warm[2])?
        ));
    } else {
        text.push_str("Warm min/median/max/range: null (requires all three qualified samples).\n");
    }
    Ok(text)
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::records::{FilePin, TARGETS};
    use super::*;
    fn inputs() -> (Context, IdentityReady) {
        let mut ctx = records::tests::ctx();
        ctx.cohort = "benchmark".into();
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
                files: [
                    "THIRD-PARTY-LICENSES",
                    "marmish",
                    "marmish-worker",
                    "marmishd",
                ]
                .into_iter()
                .map(|name| {
                    (
                        name.into(),
                        FilePin {
                            sha256: "c".repeat(64),
                            bytes: 1,
                        },
                    )
                })
                .collect(),
            },
        };
        (ctx, id)
    }
    fn complete() -> (Context, IdentityReady, BenchmarkResult) {
        let (ctx, id) = inputs();
        let mut r = new_result(&ctx, &id, ctx.created_at).unwrap();
        r.fixture_path = "/private/fixture/main".into();
        r.fixture_sha256 = ctx.tooling[0].files.fixture.sha256.clone();
        for s in &mut r.samples {
            s.group_submit_sha256 = Some("4".repeat(64));
            s.handle = Some(Handle {
                format: 1,
                profile: "client".into(),
                remote: id.server_account.clone(),
                session: "1".repeat(32),
                invocation: format!("{:032x}", s.index),
                submit_sha256: "2".repeat(64),
            });
            s.receipt = Some(
                fixture::expected_receipt(
                    Path::new(&r.fixture_path),
                    &s.nonce,
                    0,
                    &r.fixture_sha256,
                )
                .unwrap(),
            );
            s.wall_ns = Some((u64::from(s.index) + 1) * 1_000_000_000);
            s.goodput_bytes_per_second = Some(goodput(s.wall_ns.unwrap()).unwrap());
            s.qualified = true;
            s.disposition = Disposition::Passed;
        }
        r.successful_samples = 4;
        r.engine_cleanup_complete = true;
        r.server_stop_complete = true;
        r.client_stop_complete = true;
        r.complete = true;
        (ctx, id, r)
    }
    pub(crate) fn failed_record() -> (Context, IdentityReady, BenchmarkResult) {
        failed_record_for(ROLES[0])
    }
    pub(crate) fn failed_record_for(role: &str) -> (Context, IdentityReady, BenchmarkResult) {
        let (ctx, mut id) = inputs();
        let index = ROLES.iter().position(|v| *v == role).unwrap();
        id.role = role.into();
        id.installation.target = TARGETS[index].into();
        id.installation.manifest_sha256 = ctx.manifests[role].clone();
        let mut r = new_result(&ctx, &id, ctx.created_at).unwrap();
        r.failure = Some(Failure::Preparation);
        (ctx, id, r)
    }
    #[test]
    fn report_recomputes_receipts_bindings_and_same_room() {
        let (ctx, id, r) = complete();
        validate(&r, &ctx, &id).unwrap();
        let canonical = records::canonical(&r).unwrap();
        for path in [
            "receipt",
            "group_digest",
            "session",
            "invocation",
            "nonce",
            "count",
            "fixture",
            "cleanup",
            "speed",
            "unknown",
        ] {
            let mut changed: BenchmarkResult = serde_json::from_slice(&canonical).unwrap();
            match path {
                "group_digest" => changed.samples[1].group_submit_sha256 = Some("A".repeat(64)),
                "receipt" => {
                    changed.samples[1].receipt.as_mut().unwrap().stdout_sha256 = "0".repeat(64)
                }
                "session" => changed.samples[1].handle.as_mut().unwrap().session = "3".repeat(32),
                "invocation" => {
                    changed.samples[1].handle.as_mut().unwrap().invocation = changed.samples[0]
                        .handle
                        .as_ref()
                        .unwrap()
                        .invocation
                        .clone()
                }
                "nonce" => changed.samples[1].nonce = "4".repeat(64),
                "count" => {
                    changed.samples.pop();
                }
                "fixture" => changed.fixture_sha256 = "5".repeat(64),
                "cleanup" => changed.engine_cleanup_complete = false,
                "speed" => changed.samples[0].goodput_bytes_per_second = Some(1.0),
                _ => {
                    changed.samples[0].qualified = false;
                    changed.samples[0].failure = Some(Failure::UnknownAdmission);
                }
            }
            assert!(validate(&changed, &ctx, &id).is_err(), "{path}");
        }
        let mut object: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
        let mut missing: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
        missing["samples"][0]
            .as_object_mut()
            .unwrap()
            .remove("group_submit_sha256");
        assert!(serde_json::from_value::<BenchmarkResult>(missing).is_err());
        object["extra"] = true.into();
        assert!(serde_json::from_value::<BenchmarkResult>(object).is_err());
    }
    #[test]
    fn failed_records_keep_all_slots_null_metrics_and_bound_details() {
        let (ctx, id, mut r) = complete();
        let pristine = records::canonical(&r).unwrap();
        r.samples[1].qualified = false;
        r.samples[1].disposition = Disposition::Failed;
        r.samples[1].failure = Some(Failure::Output);
        r.samples[1].wall_ns = None;
        r.samples[1].goodput_bytes_per_second = None;
        let fresh = new_result(&ctx, &id, ctx.created_at).unwrap();
        r.samples[2..].swap_with_slice(&mut fresh.samples.into_iter().skip(2).collect::<Vec<_>>());
        r.successful_samples = 1;
        r.complete = false;
        r.failure = Some(Failure::Output);
        validate_record(&r, &ctx, &id).unwrap();
        assert!(validate(&r, &ctx, &id).is_err());
        let summary = markdown_summary(&r).unwrap();
        assert_eq!(summary.matches("| unqualified | null | null |").count(), 3);
        assert!(summary.contains("Warm n=0"));
        assert!(!summary.contains("Warm n=3:"));
        r.samples[2].nonce = "0".repeat(64);
        assert!(validate_record(&r, &ctx, &id).is_err());
        let mut v: serde_json::Value = serde_json::from_slice(&pristine).unwrap();
        v["samples"][0]["disposition"] = "imaginary".into();
        assert!(serde_json::from_value::<BenchmarkResult>(v).is_err());
        let (_, _, early) = failed_record();
        validate_record(&early, &ctx, &id).unwrap();
    }
    #[test]
    fn sample_binding_stops_on_unknown_duplicate_or_changed_room() {
        let (_, id, mut r) = complete();
        let mut expected = BTreeMap::new();
        let mut unknown = false;
        record_sample(
            &mut r.samples[0],
            &id.client_account,
            &mut expected,
            &mut unknown,
            Ok(()),
        )
        .unwrap();
        let duplicate = r.samples[0].handle.as_ref().unwrap().invocation.clone();
        r.samples[1].handle.as_mut().unwrap().invocation = duplicate;
        let mut attempts = 0;
        for sample in &mut r.samples[1..] {
            attempts += 1;
            if record_sample(
                sample,
                &id.client_account,
                &mut expected,
                &mut unknown,
                Ok(()),
            )
            .is_err()
            {
                break;
            }
        }
        assert_eq!(attempts, 1);
        assert!(unknown);
        assert!(r.samples[1].wall_ns.is_none());
        assert!(!cleanup_plan(unknown, true).cancel);
        assert!(!cleanup_plan(unknown, true).prepare);
        let (_, id, mut r) = complete();
        let mut expected = BTreeMap::new();
        let mut unknown = false;
        record_sample(
            &mut r.samples[0],
            &id.client_account,
            &mut expected,
            &mut unknown,
            Ok(()),
        )
        .unwrap();
        r.samples[1].handle.as_mut().unwrap().session = "3".repeat(32);
        assert!(
            record_sample(
                &mut r.samples[1],
                &id.client_account,
                &mut expected,
                &mut unknown,
                Ok(())
            )
            .is_err()
        );
        assert!(!unknown);
        assert_eq!(expected.len(), 2);
        assert!(r.samples[1].failure == Some(Failure::Output));
        r.samples[2].handle = None;
        assert!(
            record_sample(
                &mut r.samples[2],
                &id.client_account,
                &mut expected,
                &mut unknown,
                Err(Error::Timeout)
            )
            .is_err()
        );
        assert!(unknown);
        assert!(!cleanup_plan(unknown, false).prepare);
    }
    #[test]
    fn nonce_and_small_sample_summary_have_no_percentile_claim() {
        let (ctx, id, r) = complete();
        let values = (0..4)
            .map(|i| nonce(&ctx, &id, i).unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(values.len(), 4);
        let mut other = ctx.clone();
        other.nonce = "f".repeat(64);
        assert_ne!(nonce(&other, &id, 0).unwrap(), nonce(&ctx, &id, 0).unwrap());
        let text = markdown_summary(&r).unwrap();
        assert!(text.contains("Cold n=1. Warm n=3"));
        assert!(text.contains("2000.000/3000.000/4000.000"));
        assert!(!text.contains("p95"));
        assert_eq!(goodput(1_000_000_000).unwrap(), 262144.0);
        assert!(goodput(0).is_err());
        assert!(goodput(330_000_000_001).is_err());
    }
    #[test]
    fn launch_deadlines_keep_cleanup_inside_original_uptime() {
        let (ctx, _) = inputs();
        assert!(remaining_at(&ctx, ctx.expires_at - 2220, 2220).is_ok());
        assert!(remaining_at(&ctx, ctx.expires_at - 2219, 2220).is_err());
        assert!(remaining_at(&ctx, ctx.expires_at - 2070, 2070).is_ok());
        assert!(remaining_at(&ctx, ctx.expires_at - 2069, 2070).is_err());
        let launch = Instant::now();
        let (ready, work, cleanup) = deadlines(launch);
        assert_eq!(ready - launch, Duration::from_secs(150));
        assert_eq!(work - launch, Duration::from_secs(1350));
        assert_eq!(cleanup - launch, Duration::from_secs(1770));
        assert_eq!(
            cleanup - Duration::from_secs(60) - launch,
            Duration::from_secs(1710)
        );
        assert!(work < cleanup && cleanup < launch + Duration::from_secs(1800));
    }
    #[test]
    fn configured_roles_keep_setup_trust_and_only_server_has_count_ceiling() {
        let (ctx, id) = inputs();
        for (name, peer) in [
            ("server", &id.client_account),
            ("client", &id.server_account),
        ] {
            let server = name == "server";
            let before = serde_json::json!({"version":1,"role":name,"relays":ctx.relays,"tor":"off","execution_clients":if server {vec![peer]} else {vec![]},
                "trusted_remotes":if server {vec![]} else {vec![peer]},"command_policy":{"version":1},"cwd":null,"limits":{},"aliases":{}});
            let mut unexpected = before.clone();
            unexpected["command_policy"]["keys"] = serde_json::json!([]);
            assert!(
                configure_value(
                    unexpected,
                    name,
                    &ctx,
                    peer,
                    Path::new("/private/fixture/main"),
                    5
                )
                .is_err()
            );
            let after = configure_value(
                before.clone(),
                name,
                &ctx,
                peer,
                Path::new("/private/fixture/main"),
                5,
            )
            .unwrap();
            for field in [
                "version",
                "role",
                "relays",
                "tor",
                "execution_clients",
                "trusted_remotes",
                "aliases",
            ] {
                assert_eq!(after[field], before[field]);
            }
            let mut config = crate::command_config::CommandConfig::from_bytes(
                &records::canonical(&after).unwrap(),
            )
            .unwrap();
            config.apply_limit_overrides(Default::default()).unwrap();
            assert_eq!(
                config.limits().max_commands.map(std::num::NonZeroU64::get),
                if server { Some(5) } else { None }
            );
            assert_eq!(config.limits().max_uptime, Some(Duration::from_secs(1800)));
            let mut wrong = before;
            wrong[if server {
                "execution_clients"
            } else {
                "trusted_remotes"
            }] = serde_json::json!([]);
            assert!(
                configure_value(
                    wrong,
                    name,
                    &ctx,
                    peer,
                    Path::new("/private/fixture/main"),
                    5
                )
                .is_err()
            );
        }
    }
    #[test]
    fn only_first_exact_handle_line_is_removed() {
        let (_, id, r) = complete();
        let h = r.samples[0].handle.as_ref().unwrap();
        let mut stderr = b"marmish handle: ".to_vec();
        stderr.extend(serde_json::to_vec(h).unwrap());
        stderr.push(b'\n');
        stderr.extend([0, 255, b'\n']);
        let (parsed, rest) = parsed_handle(&stderr, &id.server_account).unwrap();
        assert_eq!(parsed.invocation, h.invocation);
        assert_eq!(rest, [0, 255, b'\n']);
        assert!(parsed_handle(&stderr, &id.client_account).is_err());
        assert!(parsed_handle(b"local_busy", &id.server_account).is_err());
    }
    #[test]
    fn exact_output_rejects_exit_or_either_changed_stream() {
        use std::os::unix::process::ExitStatusExt;
        let mut output = process::Output {
            status: std::process::ExitStatus::from_raw(7 << 8),
            stdout: vec![0, 255],
            stderr: vec![],
        };
        assert!(exact_output(&output, &[0, 255], &[255, 0], &[255, 0]).is_ok());
        assert!(exact_output(&output, &[0], &[255, 0], &[255, 0]).is_err());
        assert!(exact_output(&output, &[0, 255], &[255], &[255, 0]).is_err());
        output.status = std::process::ExitStatus::from_raw(0);
        assert!(exact_output(&output, &[0, 255], &[255, 0], &[255, 0]).is_err());
    }
    #[tokio::test]
    async fn detached_submit_and_wait_preserve_exact_handle_bytes_and_one_deadline() {
        let temporary = tempfile::tempdir().unwrap();
        let w = temporary.path();
        for name in ["bin", "home", "tmp", "utilities"] {
            fs::create_dir(w.join(name)).unwrap();
        }
        let environment = Environment {
            home: w.join("home"),
            temporary: w.join("tmp"),
            utilities: w.join("utilities"),
        };
        let (ctx, id, result) = complete();
        let encoded = serde_json::to_string(result.samples[0].handle.as_ref().unwrap()).unwrap();
        let mut signal =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
        for (detach_exit, wait_body, good, known) in [
            (
                0,
                "printf '\\000\\377'; printf '\\376\\001' >&2; exit 7",
                true,
                true,
            ),
            (254, "exit 7", false, false),
            (0, "printf wrong >&2; exit 7", false, true),
            (0, "/bin/sleep 1; exit 7", false, true),
        ] {
            let script = format!(
                "#!/bin/sh\nif [ \"$1\" = submit ]; then\n/bin/cat >/dev/null\nprintf s >> calls\nprintf '%s\\n' 'marmish handle: {encoded}' >&2\n/bin/sleep 0.03\nexit {detach_exit}\nfi\nprintf w >> calls\nfound=0\nfor arg; do [ \"$arg\" = --relay ] && exit 99; [ \"$arg\" = '{encoded}' ] && found=1; done\n[ \"$found\" = 1 ] || exit 98\n{wait_body}\n"
            );
            let executable = w.join("bin/marmish");
            fs::write(&executable, script).unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            let client = Client {
                workspace: w,
                context: &ctx,
                identity: &id,
                environment: &environment,
                executable: &executable,
                socket: Path::new("/private/socket"),
                deadline: Instant::now()
                    + Duration::from_millis(if wait_body.contains("sleep 1") {
                        80
                    } else {
                        1000
                    }),
            };
            let output = client
                .command(
                    &["submit".into()],
                    vec![0, 255],
                    (&[0, 255], &[254, 1]),
                    &mut signal,
                )
                .await;
            assert_eq!(output.is_ok(), known);
            if let Ok(output) = output {
                assert_eq!(output.outcome.is_ok(), good);
                assert_eq!(output.locator.encode().unwrap(), encoded.as_bytes());
                assert!(output.elapsed >= Duration::from_millis(30));
                if wait_body.contains("sleep 1") {
                    assert!(matches!(output.outcome, Err(Error::Timeout)));
                    assert!(output.elapsed < Duration::from_millis(500));
                }
            }
            assert_eq!(
                fs::read(w.join("calls")).unwrap(),
                if known {
                    b"sw".as_slice()
                } else {
                    b"s".as_slice()
                }
            );
            fs::remove_file(w.join("calls")).unwrap();
        }
    }
    #[tokio::test]
    async fn cli_child_deadline_and_cancellation_are_owned_and_bounded() {
        let temporary = tempfile::tempdir().unwrap();
        let w = temporary.path();
        for name in ["bin", "home", "tmp", "utilities"] {
            fs::create_dir(w.join(name)).unwrap();
        }
        let environment = Environment {
            home: w.join("home"),
            temporary: w.join("tmp"),
            utilities: w.join("utilities"),
        };
        let executable = w.join("bin/marmish");
        // System tools are copied solely into this disposable offline test fixture.
        fs::copy("/bin/sleep", &executable).unwrap();
        let args = [OsString::from("10")];
        let started = Instant::now();
        assert!(matches!(
            execute(
                w,
                &args,
                &environment,
                vec![],
                started + Duration::from_millis(30),
                std::future::pending()
            )
            .await,
            Err(Error::Timeout)
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            execute(
                w,
                &args,
                &environment,
                vec![],
                Instant::now() + Duration::from_secs(2),
                std::future::ready(())
            )
            .await,
            Err(Error::Invalid("installed_interrupted"))
        ));
        fs::copy("/usr/bin/printf", &executable).unwrap();
        let output = execute(
            w,
            &[OsString::from("binary:%s"), OsString::from("ok")],
            &environment,
            vec![],
            Instant::now() + Duration::from_secs(2),
            std::future::pending(),
        )
        .await
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"binary:ok");
        assert!(output.stderr.is_empty());
        // An expired deadline does not even spawn a missing executable.
        fs::remove_file(executable).unwrap();
        assert!(matches!(
            execute(
                w,
                &[],
                &environment,
                vec![],
                Instant::now(),
                std::future::pending()
            )
            .await,
            Err(Error::Timeout)
        ));
    }
}
