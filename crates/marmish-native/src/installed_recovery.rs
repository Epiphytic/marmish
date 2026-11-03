//! Installed running-worker recovery; build tooling with no product execution hook.
use super::{
    benchmark, cleanup, fixture,
    matrix::{self, Daemon},
    process::Environment,
    records::{self, Context, IdentityReady, Installation, ROLES},
};
use crate::{Error, Result, command_client::CommandHandle, command_wire as wire};
use nostr::nips::nip19::ToBech32;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{signal::unix::Signal, time::Instant};
fn bad() -> Error {
    Error::Invalid("installed_recovery")
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Failure {
    Clock,
    Preparation,
    Activation,
    Submit,
    Running,
    Stop,
    Reopen,
    Completion,
    Interrupted,
    Cleanup,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    index: u8,
    nonce: String,
    passed: bool,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    handle: Option<CommandHandle>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    group_submit_sha256: Option<String>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    receipt: Option<serde_json::Value>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecoveryResult {
    schema: u8,
    kind: String,
    context_sha256: String,
    role: String,
    identity_ready_sha256: String,
    installation: Installation,
    route: String,
    relays: Vec<String>,
    fixture_path: String,
    fixture_sha256: String,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    context_age_seconds: Option<u64>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    context_remaining_seconds: Option<u64>,
    samples: Vec<Sample>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    before_stop: Option<wire::Status>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    after_reopen: Option<wire::Status>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    marker: Option<fixture::MarkerIdentity>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    initial_ready_ns: Option<u64>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    ordinary_stop_ns: Option<u64>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    restart_ready_ns: Option<u64>,
    marker_unchanged: bool,
    ordinary_stop_joined: bool,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    ordinary_stop_engine_quiesced: Option<bool>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    restart_uptime_seconds: Option<u64>,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    final_seq: Option<u64>,
    engine_cleanup_complete: bool,
    server_stop_complete: bool,
    client_stop_complete: bool,
    #[serde(deserialize_with = "cleanup::required_nullable")]
    failure: Option<Failure>,
    complete: bool,
}
impl RecoveryResult {
    pub(super) fn is_complete(&self) -> bool {
        self.complete
    }
}
fn nonce(ctx: &Context, id: &IdentityReady, index: u8) -> Result<String> {
    Ok(records::digest(&records::canonical(&(
        "installed-recovery/1",
        ctx.digest()?,
        &id.role,
        &id.client_account,
        &id.server_account,
        index,
    ))?))
}
fn new_result(ctx: &Context, id: &IdentityReady, now: u64) -> Result<RecoveryResult> {
    let index = ROLES.iter().position(|r| *r == id.role).ok_or_else(bad)?;
    Ok(RecoveryResult {
        schema: 1,
        kind: "recovery-result".into(),
        context_sha256: ctx.digest()?,
        role: id.role.clone(),
        identity_ready_sha256: records::digest(&records::canonical(id)?),
        installation: id.installation.clone(),
        route: "direct".into(),
        relays: ctx.relays.clone(),
        fixture_path: String::new(),
        fixture_sha256: ctx.tooling[index].files.fixture.sha256.clone(),
        context_age_seconds: now.checked_sub(ctx.created_at),
        context_remaining_seconds: ctx.expires_at.checked_sub(now),
        samples: (0..2)
            .map(|index| {
                Ok(Sample {
                    index,
                    nonce: nonce(ctx, id, index)?,
                    passed: false,
                    handle: None,
                    group_submit_sha256: None,
                    receipt: None,
                })
            })
            .collect::<Result<_>>()?,
        before_stop: None,
        after_reopen: None,
        marker: None,
        initial_ready_ns: None,
        ordinary_stop_ns: None,
        restart_ready_ns: None,
        marker_unchanged: false,
        ordinary_stop_joined: false,
        ordinary_stop_engine_quiesced: None,
        restart_uptime_seconds: None,
        final_seq: None,
        engine_cleanup_complete: false,
        server_stop_complete: false,
        client_stop_complete: false,
        failure: None,
        complete: false,
    })
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
fn restart_uptime(launch: Instant, now: Instant) -> Result<u64> {
    let remaining = (launch + Duration::from_secs(1800))
        .checked_duration_since(now)
        .ok_or(Error::Timeout)?
        .as_secs();
    if remaining == 0 || remaining > 1800 {
        return Err(Error::Timeout);
    }
    Ok(remaining)
}
async fn interrupt(signal: &mut Signal) -> Result<()> {
    let pending =
        std::future::poll_fn(|cx| std::task::Poll::Ready(signal.poll_recv(cx).is_ready())).await;
    if pending {
        Err(Error::Invalid("installed_interrupted"))
    } else {
        Ok(())
    }
}
fn running(status: &wire::Status, handle: &CommandHandle) -> Result<bool> {
    if status.invocation != handle.invocation()
        || !status.authority_valid
        || status.cancel_requested
        || status.warning.is_some()
        || status.outcome.is_some()
        || matches!(
            status.state,
            wire::State::Exited | wire::State::UnknownLaunch | wire::State::Lost
        )
        || matches!(
            status.input.health,
            wire::InputHealth::Gap | wire::InputHealth::Uncertain
        )
        || status.input.length != 16384
        || status.input.accepted > 16384
        || status.input.delivered > status.input.accepted
        || [&status.stdout, &status.stderr].iter().any(|s| {
            s.closed || !s.gaps.is_empty() || s.produced > 65536 || s.retained > s.produced
        })
    {
        return Err(bad());
    }
    Ok(status.admission == wire::Admission::Engine
        && status.engine_known
        && status.state == wire::State::Running
        && status.input.accepted == 16384
        && status.input.delivered == 16384
        && status.input.eof_sent
        && status.input.health == wire::InputHealth::Closed
        && status.stdout.produced == 65536
        && status.stderr.produced == 65536)
}
struct Client<'a> {
    workspace: &'a Path,
    ctx: &'a Context,
    id: &'a IdentityReady,
    env: &'a Environment,
    executable: &'a Path,
    socket: &'a Path,
    deadline: Instant,
}
impl Client<'_> {
    async fn call(
        &self,
        args: &[OsString],
        input: Vec<u8>,
        deadline: Instant,
        signal: &mut Signal,
    ) -> Result<super::process::Output> {
        interrupt(signal).await?;
        benchmark::execute(
            self.workspace,
            args,
            self.env,
            input,
            deadline.min(self.deadline),
            async {
                signal.recv().await;
            },
        )
        .await
    }
    fn selected(&self, operation: &str, handle_file: &Path, seconds: u64) -> Vec<OsString> {
        let mut args = matrix::base(self.workspace, "client", self.socket, &[]);
        args.extend([
            "--no-autostart".into(),
            "--wait-seconds".into(),
            seconds.to_string().into(),
            operation.into(),
            "--handle-file".into(),
            handle_file.as_os_str().into(),
        ]);
        args
    }
    async fn submit(&self, sample: &mut Sample, signal: &mut Signal) -> Result<PathBuf> {
        let mut args = matrix::base(self.workspace, "client", self.socket, &self.ctx.relays);
        args.extend([
            "--no-autostart".into(),
            "--stdin".into(),
            "--detach".into(),
            "--wait-seconds".into(),
            "90".into(),
            nostr::key::PublicKey::from_hex(&self.id.server_account)
                .map_err(|_| bad())?
                .to_bech32()
                .map_err(|_| bad())?
                .into(),
            "--".into(),
            self.executable.as_os_str().into(),
        ]);
        args.extend(if sample.index == 0 {
            fixture::recovery_arguments(&sample.nonce)?
        } else {
            fixture::arguments(&sample.nonce, 1)?
        });
        let (input, _, _) = fixture::bytes(&sample.nonce, sample.index)?;
        let output = self
            .call(
                &args,
                input,
                Instant::now() + Duration::from_secs(90),
                signal,
            )
            .await?;
        if !output.status.success() || !output.stdout.is_empty() {
            return Err(bad());
        }
        let bytes = output
            .stderr
            .strip_prefix(b"marmish handle: ")
            .and_then(|b| b.strip_suffix(b"\n"))
            .ok_or_else(bad)?;
        let handle = CommandHandle::decode(bytes)?;
        if handle.profile() != "client" || hex::encode(handle.remote()) != self.id.server_account {
            return Err(bad());
        }
        sample.handle = Some(handle);
        let path = self
            .workspace
            .join(format!("recovery-handle-{}.json", sample.index));
        records::write_raw(&path, bytes)?;
        Ok(path)
    }
    async fn status(
        &self,
        handle: &CommandHandle,
        file: &Path,
        signal: &mut Signal,
    ) -> Result<wire::Status> {
        let deadline = self.deadline.min(Instant::now() + Duration::from_secs(90));
        for attempt in 0..30 {
            let output = self
                .call(
                    &self.selected("command-status", file, 90),
                    Vec::new(),
                    deadline,
                    signal,
                )
                .await?;
            if !output.status.success()
                || !output.stderr.is_empty()
                || !output.stdout.ends_with(b"\n")
            {
                return Err(bad());
            }
            let status: wire::Status = serde_json::from_slice(&output.stdout).map_err(|_| bad())?;
            let mut canonical = serde_json::to_vec(&status).map_err(|_| bad())?;
            canonical.push(b'\n');
            if canonical != output.stdout {
                return Err(bad());
            }
            if running(&status, handle)? {
                return Ok(status);
            }
            if attempt < 29 {
                tokio::select! { biased; _=signal.recv()=>return Err(Error::Invalid("installed_interrupted")),
                _=tokio::time::sleep_until((Instant::now()+Duration::from_secs(1)).min(deadline))=>{} }
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
        }
        Err(Error::Timeout)
    }
    async fn finish(
        &self,
        sample: &mut Sample,
        file: &Path,
        daemon: &mut Daemon,
        signal: &mut Signal,
    ) -> Result<()> {
        let (_, stdout, stderr) = fixture::bytes(&sample.nonce, sample.index)?;
        let output = self
            .call(
                &self.selected("wait", file, 300),
                Vec::new(),
                Instant::now() + Duration::from_secs(330),
                signal,
            )
            .await;
        // A known admission remains known even if waiting fails. Bind before propagating.
        sample.group_submit_sha256 = Some(
            cleanup::client_group_submit(
                &mut daemon.child,
                self.socket,
                sample.handle.as_ref().ok_or_else(bad)?,
                self.deadline,
            )
            .await?,
        );
        let output = output?;
        if output.status.code() != Some(7) || output.stdout != stdout || output.stderr != stderr {
            return Err(bad());
        }
        let receipt: serde_json::Value = records::read(
            &fixture::receipt_path(self.executable, &sample.nonce)?,
            65536,
        )?;
        let expected = expected_receipt(
            self.executable,
            sample,
            &self.ctx.tooling[ROLES
                .iter()
                .position(|r| *r == self.id.role)
                .ok_or_else(bad)?]
            .files
            .fixture
            .sha256,
        )?;
        if receipt != expected {
            return Err(bad());
        }
        sample.receipt = Some(receipt);
        sample.passed = true;
        Ok(())
    }
}
fn expected_receipt(executable: &Path, sample: &Sample, hash: &str) -> Result<serde_json::Value> {
    if sample.index == 0 {
        serde_json::to_value(fixture::expected_recovery_receipt(
            executable,
            &sample.nonce,
            hash,
        )?)
        .map_err(|_| bad())
    } else {
        serde_json::to_value(fixture::expected_receipt(
            executable,
            &sample.nonce,
            1,
            hash,
        )?)
        .map_err(|_| bad())
    }
}
fn cleanup_plan(unknown: bool, pending_binding: bool, failed: bool) -> (bool, bool) {
    let prepare = !unknown && !pending_binding;
    (prepare && failed, prepare)
}
#[derive(Default)]
struct Runtime {
    server: Option<Daemon>,
    client: Option<Daemon>,
    tree: Option<super::super::command_check::EngineTree>,
    client_profile: Option<cleanup::ProfileReceipt>,
    marker_file: Option<fs::File>,
    sockets: Option<PathBuf>,
    launch: Option<Instant>,
    expected: BTreeMap<(String, String), (String, String)>,
    unknown: bool,
    pending_binding: bool,
}
impl Runtime {
    fn bind(&mut self, sample: &Sample, id: &IdentityReady) -> Result<()> {
        let (Some(handle), Some(digest)) = (&sample.handle, &sample.group_submit_sha256) else {
            self.unknown = true;
            return Err(bad());
        };
        let key = (id.client_account.clone(), hex::encode(handle.invocation()));
        if self.expected.contains_key(&key) || self.expected.len() >= 2 {
            self.unknown = true;
            return Err(bad());
        }
        let session = hex::encode(handle.session());
        let changed = self
            .expected
            .values()
            .any(|(previous, _)| *previous != session);
        if changed {
            self.unknown = true;
            return Err(bad());
        }
        self.expected.insert(key, (session, digest.clone()));
        self.pending_binding = false;
        Ok(())
    }
    async fn close(
        &mut self,
        w: &Path,
        executable: &Path,
        nonce: &str,
        result: &mut RecoveryResult,
    ) {
        if result.marker.is_some() || fixture::recovery_marker(executable, nonce).is_ok() {
            let _ = fixture::release_recovery(executable, nonce);
        }
        let Some(launch) = self.launch else {
            return;
        };
        let cleanup_deadline = launch + Duration::from_secs(1710);
        let stop_deadline = launch + Duration::from_secs(1770);
        let (cancel, prepare) =
            cleanup_plan(self.unknown, self.pending_binding, result.failure.is_some());
        if prepare && let (Some(server), Some(tree)) = (self.server.as_mut(), self.tree.take()) {
            if cancel {
                let _ = cleanup::cancel_fixture(
                    &mut server.child,
                    &server.socket,
                    &self.expected,
                    2,
                    cleanup_deadline,
                )
                .await;
            }
            if let Ok(mut guard) = cleanup::prepare(
                &mut server.child,
                &w.join("profiles/server"),
                &server.socket,
                self.expected.clone(),
                cleanup_deadline,
            )
            .await
            {
                result.final_seq = Some(guard.final_seq());
                result.engine_cleanup_complete = tree.remove_installed(&mut guard).await.is_ok();
            }
        }
        let client = self.client.take();
        let server = self.server.take();
        let receipt = self.client_profile.take();
        let (server_ok, client_ok) = tokio::join!(
            async {
                match server {
                    Some(d) => d.stop(stop_deadline).await.is_ok(),
                    None => false,
                }
            },
            async {
                let Some(mut d) = client else {
                    return false;
                };
                let path = w.join("profiles/client");
                let proof = async {
                    receipt.as_ref().ok_or_else(bad)?.verify(&path)?;
                    cleanup::verify_no_engine(&mut d.child, &path, &d.socket, cleanup_deadline)
                        .await
                }
                .await;
                let stopped = d.stop_no_engine(stop_deadline).await;
                proof.is_ok()
                    && stopped.is_ok()
                    && receipt.as_ref().is_some_and(|r| r.verify(&path).is_ok())
                    && cleanup::require_no_engine(&path).is_ok()
            }
        );
        result.server_stop_complete = server_ok;
        result.client_stop_complete = client_ok;
    }
}
async fn work(
    ctx: &Context,
    w: &Path,
    id: &IdentityReady,
    result: &mut RecoveryResult,
    runtime: &mut Runtime,
    stage: &mut Failure,
    signal: &mut Signal,
) -> Result<()> {
    if result.context_age_seconds.is_none() {
        *stage = Failure::Clock;
        return Err(bad());
    }
    remaining(ctx, 2220)?;
    let env = Environment {
        home: w.join("home"),
        temporary: w.join("tmp"),
        utilities: w.join("utilities"),
    };
    let index = ROLES.iter().position(|r| *r == id.role).ok_or_else(bad)?;
    let executable = benchmark::copy_fixture(w, ctx, index)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(executable.parent().ok_or_else(bad)?.join("recovery"))?;
    result.fixture_path = executable.to_str().ok_or_else(bad)?.into();
    cleanup::require_no_engine(&w.join("profiles/client"))?;
    runtime.client_profile = Some(cleanup::ProfileReceipt::capture(
        &w.join("profiles/client"),
    )?);
    benchmark::configure(
        w,
        ctx,
        id,
        &executable,
        &env,
        3,
        Instant::now() + Duration::from_secs(150),
    )
    .await?;
    interrupt(signal).await?;
    remaining(ctx, 2070)?;
    cleanup::require_no_engine(&w.join("profiles/server"))?;
    cleanup::require_no_engine(&w.join("profiles/client"))?;
    runtime
        .client_profile
        .as_ref()
        .ok_or_else(bad)?
        .verify(&w.join("profiles/client"))?;
    let mut random = [0; 8];
    getrandom::fill(&mut random).map_err(|_| Error::Identity("random"))?;
    let sockets = std::env::temp_dir()
        .canonicalize()?
        .join(format!("mr-{}", hex::encode(random)));
    fs::DirBuilder::new().mode(0o700).create(&sockets)?;
    runtime.sockets = Some(sockets.clone());
    let launch = Instant::now();
    runtime.launch = Some(launch);
    let ready_deadline = launch + Duration::from_secs(150);
    let deadline = launch + Duration::from_secs(1350);
    *stage = Failure::Activation;
    runtime.server = Some(Daemon::start(
        w,
        "server",
        sockets.join("s"),
        &env,
        &ctx.relays,
        1800,
    )?);
    runtime.client = Some(Daemon::start(
        w,
        "client",
        sockets.join("c"),
        &env,
        &ctx.relays,
        1800,
    )?);
    let activation = async {
        matrix::ready(runtime.server.as_mut().ok_or_else(bad)?, ready_deadline).await?;
        runtime.tree = Some(super::super::command_check::EngineTree::capture_path(
            &w.join("profiles/server"),
        )?);
        matrix::ready(runtime.client.as_mut().ok_or_else(bad)?, ready_deadline).await?;
        runtime
            .client_profile
            .as_ref()
            .ok_or_else(bad)?
            .verify(&w.join("profiles/client"))?;
        let d = runtime.client.as_mut().ok_or_else(bad)?;
        cleanup::verify_no_engine(
            &mut d.child,
            &w.join("profiles/client"),
            &d.socket,
            ready_deadline,
        )
        .await
    };
    tokio::select! {biased;_=signal.recv()=>return Err(Error::Invalid("installed_interrupted")),r=activation=>r?}
    result.initial_ready_ns = Some(u64::try_from(launch.elapsed().as_nanos()).map_err(|_| bad())?);
    let client_socket = sockets.join("c");
    let client = Client {
        workspace: w,
        ctx,
        id,
        env: &env,
        executable: &executable,
        socket: &client_socket,
        deadline,
    };
    *stage = Failure::Submit;
    let file = match client.submit(&mut result.samples[0], signal).await {
        Ok(f) => f,
        Err(e) => {
            runtime.unknown = true;
            return Err(e);
        }
    };
    runtime.pending_binding = true;
    *stage = Failure::Running;
    let handle = result.samples[0].handle.as_ref().ok_or_else(bad)?;
    result.before_stop = Some(client.status(handle, &file, signal).await?);
    let marker = fixture::recovery_marker(&executable, &result.samples[0].nonce)?;
    runtime.marker_file = Some(fixture::hold_recovery_marker(
        &executable,
        &result.samples[0].nonce,
        marker,
    )?);
    result.marker = Some(marker);
    *stage = Failure::Stop;
    interrupt(signal).await?;
    let stop_started = Instant::now();
    let server = runtime.server.take().ok_or_else(bad)?;
    server
        .ordinary_stop(deadline.min(Instant::now() + Duration::from_secs(30)))
        .await?;
    result.ordinary_stop_ns =
        Some(u64::try_from(stop_started.elapsed().as_nanos()).map_err(|_| bad())?);
    result.ordinary_stop_joined = true;
    // ordinary_stop succeeds only after the actual joined report says false.
    result.ordinary_stop_engine_quiesced = Some(false);
    // No cancel, prepare, new tree capture or competing owner occurred in the stopped window.
    *stage = Failure::Reopen;
    interrupt(signal).await?;
    runtime
        .tree
        .as_ref()
        .ok_or_else(bad)?
        .verify_path(&w.join("profiles/server"))?;
    let uptime = restart_uptime(launch, Instant::now())?;
    result.restart_uptime_seconds = Some(uptime);
    let reopen_started = Instant::now();
    runtime.server = Some(Daemon::start(
        w,
        "server",
        sockets.join("s"),
        &env,
        &ctx.relays,
        uptime,
    )?);
    tokio::select! {biased;_=signal.recv()=>return Err(Error::Invalid("installed_interrupted")),
    r=matrix::ready(runtime.server.as_mut().ok_or_else(bad)?,deadline.min(reopen_started+Duration::from_secs(150)))=>r?}
    runtime
        .tree
        .as_ref()
        .ok_or_else(bad)?
        .verify_path(&w.join("profiles/server"))?;
    result.restart_ready_ns =
        Some(u64::try_from(reopen_started.elapsed().as_nanos()).map_err(|_| bad())?);
    result.after_reopen = Some(client.status(handle, &file, signal).await?);
    if fixture::recovery_marker(&executable, &result.samples[0].nonce)? != marker {
        return Err(bad());
    }
    result.marker_unchanged = true;
    fixture::release_recovery(&executable, &result.samples[0].nonce)?;
    *stage = Failure::Completion;
    let outcome = client
        .finish(
            &mut result.samples[0],
            &file,
            runtime.client.as_mut().ok_or_else(bad)?,
            signal,
        )
        .await;
    let binding = runtime.bind(&result.samples[0], id);
    outcome.and(binding)?;
    fixture::verify_recovery_marker(&executable, &result.samples[0].nonce, marker)?;
    *stage = Failure::Submit;
    let file = match client.submit(&mut result.samples[1], signal).await {
        Ok(f) => f,
        Err(e) => {
            runtime.unknown = true;
            return Err(e);
        }
    };
    runtime.pending_binding = true;
    *stage = Failure::Completion;
    let outcome = client
        .finish(
            &mut result.samples[1],
            &file,
            runtime.client.as_mut().ok_or_else(bad)?,
            signal,
        )
        .await;
    let binding = runtime.bind(&result.samples[1], id);
    outcome.and(binding)?;
    Ok(())
}
pub(super) async fn run(ctx: &Context, workspace: &Path, output: &Path) -> Result<()> {
    ctx.validate()?;
    ctx.require_cohort("recovery")?;
    let id = records::own_identity(workspace, ctx)?;
    crate::identity::check_dir(workspace)?;
    crate::identity::check_dir(output.parent().ok_or_else(bad)?)?;
    let mut result = new_result(ctx, &id, super::now()?)?;
    let mut runtime = Runtime::default();
    let mut stage = Failure::Preparation;
    let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let outcome = work(
        ctx,
        workspace,
        &id,
        &mut result,
        &mut runtime,
        &mut stage,
        &mut signal,
    )
    .await;
    if outcome.is_err() {
        result.failure = Some(
            if matches!(outcome, Err(Error::Invalid("installed_interrupted"))) {
                Failure::Interrupted
            } else {
                stage
            },
        );
    }
    if interrupt(&mut signal).await.is_err() {
        result.failure = Some(Failure::Interrupted);
    }
    let path = PathBuf::from(&result.fixture_path);
    let first_nonce = result.samples[0].nonce.clone();
    runtime
        .close(workspace, &path, &first_nonce, &mut result)
        .await;
    if interrupt(&mut signal).await.is_err() {
        result.failure = Some(Failure::Interrupted);
    }
    result.complete = outcome.is_ok()
        && result.failure.is_none()
        && result.engine_cleanup_complete
        && result.server_stop_complete
        && result.client_stop_complete;
    if !result.complete && result.failure.is_none() {
        result.failure = Some(Failure::Cleanup);
    }
    if let Some(sockets) = runtime.sockets {
        let _ = fs::remove_dir(sockets);
    }
    records::write(output, &result)?;
    if result.complete {
        validate(&result, ctx, &id)
    } else {
        Err(bad())
    }
}
pub(super) fn validate_record(r: &RecoveryResult, ctx: &Context, id: &IdentityReady) -> Result<()> {
    ctx.validate()?;
    ctx.require_cohort("recovery")?;
    id.validate(ctx, &id.role)?;
    let index = ROLES.iter().position(|v| *v == id.role).ok_or_else(bad)?;
    if r.schema != 1
        || r.kind != "recovery-result"
        || r.context_sha256 != ctx.digest()?
        || r.role != id.role
        || r.identity_ready_sha256 != records::digest(&records::canonical(id)?)
        || records::canonical(&r.installation)? != records::canonical(&id.installation)?
        || r.route != "direct"
        || r.relays != ctx.relays
        || r.fixture_sha256 != ctx.tooling[index].files.fixture.sha256
        || r.complete == r.failure.is_some()
        || r.samples.len() != 2
        || r.context_age_seconds
            .zip(r.context_remaining_seconds)
            .is_some_and(|(a, b)| a.checked_add(b) != ctx.expires_at.checked_sub(ctx.created_at))
        || r.context_age_seconds.is_none() && r.failure != Some(Failure::Clock)
        || r.ordinary_stop_joined != (r.ordinary_stop_engine_quiesced == Some(false))
        || r.ordinary_stop_engine_quiesced == Some(true)
        || (r.marker.is_some() || r.after_reopen.is_some() || r.ordinary_stop_joined)
            && r.before_stop.is_none()
        || r.engine_cleanup_complete && r.final_seq.is_none()
        || !r.fixture_path.is_empty()
            && (!Path::new(&r.fixture_path).is_absolute()
                || !Path::new(&r.fixture_path).ends_with("fixture/main")
                || r.fixture_path.contains(['*', '?', '[', ']']))
        || r.restart_uptime_seconds.is_some_and(|n| n == 0 || n > 1800)
        || r.initial_ready_ns.is_some_and(|n| n > 150_000_000_000)
        || r.ordinary_stop_ns.is_some_and(|n| n > 35_000_000_000)
        || r.restart_ready_ns.is_some_and(|n| n > 150_000_000_000)
        || r.final_seq == Some(0)
    {
        return Err(bad());
    }
    let mut first: Option<&CommandHandle> = None;
    let mut stopped = false;
    for (index, sample) in r.samples.iter().enumerate() {
        if sample.index != index as u8
            || sample.nonce != nonce(ctx, id, index as u8)?
            || stopped && (sample.passed || sample.handle.is_some())
        {
            return Err(bad());
        }
        if let Some(handle) = &sample.handle {
            if handle.profile() != "client" || hex::encode(handle.remote()) != id.server_account {
                return Err(bad());
            }
            if let Some(previous) = first
                && (previous.invocation() == handle.invocation()
                    || previous.session() != handle.session())
            {
                return Err(bad());
            }
            first = Some(handle);
        }
        if sample.group_submit_sha256.is_some() && sample.handle.is_none()
            || sample
                .handle
                .as_ref()
                .zip(sample.group_submit_sha256.as_ref())
                .is_some_and(|(h, d)| hex::encode(h.submit_sha256()) == *d)
        {
            return Err(bad());
        }
        if sample
            .group_submit_sha256
            .as_ref()
            .is_some_and(|s| !records::hexadecimal(s, 64))
        {
            return Err(bad());
        }
        if let Some(receipt) = &sample.receipt
            && receipt != &expected_receipt(Path::new(&r.fixture_path), sample, &r.fixture_sha256)?
        {
            return Err(bad());
        }
        if sample.passed {
            if sample.handle.is_none()
                || sample.group_submit_sha256.is_none()
                || sample.receipt.is_none()
            {
                return Err(bad());
            }
        } else {
            stopped = true;
        }
    }
    if let Some(before) = &r.before_stop {
        running(before, r.samples[0].handle.as_ref().ok_or_else(bad)?)?
            .then_some(())
            .ok_or_else(bad)?;
    }
    if let Some(after) = &r.after_reopen {
        running(after, r.samples[0].handle.as_ref().ok_or_else(bad)?)?
            .then_some(())
            .ok_or_else(bad)?;
    }
    Ok(())
}
pub(super) fn validate(r: &RecoveryResult, ctx: &Context, id: &IdentityReady) -> Result<()> {
    validate_record(r, ctx, id)?;
    if !r.complete
        || r.context_remaining_seconds.is_none_or(|n| n < 2220)
        || r.samples.iter().any(|s| !s.passed)
        || r.before_stop.is_none()
        || r.after_reopen.is_none()
        || r.marker.is_none()
        || !r.marker_unchanged
        || !r.ordinary_stop_joined
        || r.restart_uptime_seconds.is_none()
        || r.initial_ready_ns.is_none()
        || r.ordinary_stop_ns.is_none()
        || r.restart_ready_ns.is_none()
        || r.final_seq.is_none()
        || !r.engine_cleanup_complete
        || !r.server_stop_complete
        || !r.client_stop_complete
    {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn markdown_summary(r: &RecoveryResult) -> Result<String> {
    let mut s = format!(
        "### {} recovery\n\nRunning worker recovery qualified: {}. Completed commands: {}/2.\n\n| Command | Nonce | Exact bytes and receipt |\n|---|---|---|\n",
        r.role,
        r.complete,
        r.samples.iter().filter(|s| s.passed).count()
    );
    for sample in &r.samples {
        s.push_str(&format!(
            "| {} | `{}` | {} |\n",
            sample.index, sample.nonce, sample.passed
        ));
    }
    s.push_str(&format!("\nOriginal joined stop: {}; reopened same marker: {}; protected cleanup/server stop/client stop: {}/{}/{}.\n",r.ordinary_stop_joined,r.marker_unchanged,r.engine_cleanup_complete,r.server_stop_complete,r.client_stop_complete));
    if let Some(failure) = r.failure {
        s.push_str(&format!(
            "Failure: `{}`.\n",
            serde_json::to_string(&failure).map_err(|_| bad())?
        ));
    }
    for (label, value) in [
        ("before_stop", serde_json::to_value(&r.before_stop)),
        ("after_reopen", serde_json::to_value(&r.after_reopen)),
        ("marker", serde_json::to_value(r.marker)),
        (
            "ordinary_stop_engine_quiesced",
            serde_json::to_value(r.ordinary_stop_engine_quiesced),
        ),
    ] {
        s.push_str(&format!("{label}: `{}`.\n", value.map_err(|_| bad())?));
    }
    s.push_str("Native identical/conflicting-Submit protocol recovery remains an independent gate. No release qualification or throughput percentile is inferred.\n");
    Ok(s)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn failed_record(role: &str) -> (Context, IdentityReady, RecoveryResult) {
        let (mut ctx, mut id, _) = benchmark::tests::failed_record_for(role);
        ctx.cohort = "recovery".into();
        id.context_sha256 = ctx.digest().unwrap();
        let mut result = new_result(&ctx, &id, ctx.created_at).unwrap();
        result.failure = Some(Failure::Preparation);
        (ctx, id, result)
    }
    fn running_status(handle: &CommandHandle) -> wire::Status {
        wire::Status {
            invocation: handle.invocation(),
            admission: wire::Admission::Engine,
            authority_valid: true,
            cancel_requested: false,
            engine_known: true,
            input: wire::InputStatus {
                accepted: 16384,
                delivered: 16384,
                length: 16384,
                eof_sent: true,
                health: wire::InputHealth::Closed,
            },
            stdout: wire::OutputStatus {
                closed: false,
                gaps: vec![],
                produced: 65536,
                retained: 65536,
            },
            stderr: wire::OutputStatus {
                closed: false,
                gaps: vec![],
                produced: 65536,
                retained: 65536,
            },
            state: wire::State::Running,
            outcome: None,
            warning: None,
        }
    }
    fn complete() -> (Context, IdentityReady, RecoveryResult) {
        let (ctx, id, mut r) = failed_record(ROLES[0]);
        r.fixture_path = "/private/fixture/main".into();
        for sample in &mut r.samples {
            sample.handle = Some(CommandHandle::decode(&serde_json::to_vec(&serde_json::json!({
                "format":1,"profile":"client","remote":id.server_account,"session":"1".repeat(32),
                "invocation":format!("{:032x}",sample.index+1),"submit_sha256":"2".repeat(64)
            })).unwrap()).unwrap());
            sample.group_submit_sha256 = Some("3".repeat(64));
            sample.receipt = Some(
                expected_receipt(Path::new(&r.fixture_path), sample, &r.fixture_sha256).unwrap(),
            );
            sample.passed = true;
        }
        let status = running_status(r.samples[0].handle.as_ref().unwrap());
        r.before_stop = Some(status.clone());
        r.after_reopen = Some(status);
        r.marker = Some(fixture::MarkerIdentity {
            device: 1,
            inode: 2,
        });
        r.marker_unchanged = true;
        r.ordinary_stop_joined = true;
        r.ordinary_stop_engine_quiesced = Some(false);
        r.restart_uptime_seconds = Some(1700);
        r.initial_ready_ns = Some(10);
        r.ordinary_stop_ns = Some(10);
        r.restart_ready_ns = Some(10);
        r.final_seq = Some(1);
        r.engine_cleanup_complete = true;
        r.server_stop_complete = true;
        r.client_stop_complete = true;
        r.failure = None;
        r.complete = true;
        (ctx, id, r)
    }
    #[test]
    fn failed_records_preserve_two_nullable_slots_for_every_role() {
        for role in ROLES {
            let (ctx, id, r) = failed_record(role);
            validate_record(&r, &ctx, &id).unwrap();
            assert!(validate(&r, &ctx, &id).is_err());
            let value = serde_json::to_value(&r).unwrap();
            assert!(value["samples"][0]["handle"].is_null());
            for field in ["before_stop", "after_reopen", "marker", "final_seq"] {
                let mut missing = value.clone();
                missing.as_object_mut().unwrap().remove(field);
                assert!(serde_json::from_value::<RecoveryResult>(missing).is_err());
            }
        }
    }
    #[test]
    fn running_requires_exact_prefix_eof_authority_and_live_worker() {
        let (_, _, r) = complete();
        let handle = r.samples[0].handle.as_ref().unwrap();
        let s = running_status(handle);
        assert!(running(&s, handle).unwrap());
        let mut pending = s.clone();
        pending.stdout.produced = 65535;
        pending.stdout.retained = 65535;
        assert!(!running(&pending, handle).unwrap());
        for state in [wire::State::Accepted, wire::State::Launching] {
            let mut pending = s.clone();
            pending.state = state;
            assert!(!running(&pending, handle).unwrap());
        }
        for change in 0..12 {
            let mut changed = s.clone();
            match change {
                0 => changed.input.health = wire::InputHealth::Uncertain,
                1 => changed.stdout.produced = 65537,
                2 => changed.state = wire::State::Exited,
                3 => changed.authority_valid = false,
                4 => changed.cancel_requested = true,
                5 => changed.warning = Some(wire::Warning::InputFailed),
                6 => changed.state = wire::State::Lost,
                7 => changed.state = wire::State::UnknownLaunch,
                8 => changed.stdout.closed = true,
                9 => changed.input.accepted = 16385,
                10 => changed.input.delivered = 16385,
                _ => changed.stderr.gaps.push(wire::Gap {
                    start: 0,
                    end: Some(1),
                    reason: wire::GapReason::Limit,
                }),
            }
            assert!(running(&changed, handle).is_err());
        }
    }
    #[test]
    fn reopening_only_uses_original_launch_budget() {
        let launch = Instant::now();
        assert_eq!(
            restart_uptime(launch, launch + Duration::from_millis(100_001)).unwrap(),
            1699
        );
        assert!(restart_uptime(launch, launch + Duration::from_secs(1800)).is_err());
        assert!(restart_uptime(launch, launch - Duration::from_secs(1)).is_err());
    }
    #[test]
    fn successful_record_requires_all_independent_bindings_and_proofs() {
        let (ctx, id, r) = complete();
        validate(&r, &ctx, &id).unwrap();
        let value = serde_json::to_value(&r).unwrap();
        for change in 0..7 {
            let mut changed = value.clone();
            match change {
                0 => changed["samples"][0]["group_submit_sha256"] = serde_json::Value::Null,
                1 => changed["samples"][0]["receipt"]["stdin_sha256"] = "0".repeat(64).into(),
                2 => changed["marker_unchanged"] = false.into(),
                3 => changed["ordinary_stop_engine_quiesced"] = true.into(),
                4 => changed["restart_uptime_seconds"] = 1801.into(),
                5 => changed["after_reopen"]["stdout"]["produced"] = 65535.into(),
                _ => changed["client_stop_complete"] = false.into(),
            }
            let changed: RecoveryResult = serde_json::from_value(changed).unwrap();
            assert!(validate(&changed, &ctx, &id).is_err());
        }
    }
    #[test]
    fn records_reject_fabricated_proof_order_and_render_unknowns() {
        let (ctx, id, r) = complete();
        for change in 0..6 {
            let mut value = serde_json::to_value(&r).unwrap();
            match change {
                0 => value["before_stop"] = serde_json::Value::Null,
                1 => value["final_seq"] = serde_json::Value::Null,
                2 => value["ordinary_stop_engine_quiesced"] = true.into(),
                3 => value["fixture_path"] = "relative/fixture/main".into(),
                4 => value["fixture_path"] = "/private/fixture/*".into(),
                _ => value["samples"][0]["group_submit_sha256"] = "2".repeat(64).into(),
            }
            let changed: RecoveryResult = serde_json::from_value(value).unwrap();
            assert!(validate_record(&changed, &ctx, &id).is_err());
        }
        let (_, _, failed) = failed_record(ROLES[0]);
        let text = markdown_summary(&failed).unwrap();
        for label in [
            "before_stop",
            "after_reopen",
            "marker",
            "ordinary_stop_engine_quiesced",
        ] {
            assert!(text.contains(&format!("{label}: `null`")));
        }
    }
    #[test]
    fn cleanup_plan_gates_the_actual_cancel_and_prepare_decisions() {
        for (unknown, pending, failed, expected) in [
            (true, false, true, (false, false)),
            (false, true, true, (false, false)),
            (false, false, true, (true, true)),
            (false, false, false, (false, true)),
        ] {
            assert_eq!(cleanup_plan(unknown, pending, failed), expected);
        }
        let (ctx, id, r) = complete();
        for field in ["handle", "group_submit_sha256"] {
            let mut value = serde_json::to_value(&r.samples[0]).unwrap();
            value[field] = serde_json::Value::Null;
            let missing: Sample = serde_json::from_value(value).unwrap();
            let mut runtime = Runtime::default();
            assert!(runtime.bind(&missing, &id).is_err());
            assert!(runtime.unknown);
        }
        let mut runtime = Runtime::default();
        runtime.bind(&r.samples[0], &id).unwrap();
        assert!(runtime.bind(&r.samples[0], &id).is_err());
        assert!(runtime.unknown);
        let mut changed = serde_json::to_value(&r.samples[1]).unwrap();
        changed["handle"]["session"] = "4".repeat(32).into();
        let changed: Sample = serde_json::from_value(changed).unwrap();
        let mut runtime = Runtime::default();
        runtime.bind(&r.samples[0], &id).unwrap();
        assert!(runtime.bind(&changed, &id).is_err());
        assert!(runtime.unknown);
        assert_eq!(runtime.expected.len(), 1);
        let mut runtime = Runtime::default();
        for sample in &r.samples {
            runtime.bind(sample, &id).unwrap();
        }
        let mut third = serde_json::to_value(&r.samples[1]).unwrap();
        third["handle"]["invocation"] = "4".repeat(32).into();
        let third: Sample = serde_json::from_value(third).unwrap();
        assert!(runtime.bind(&third, &id).is_err());
        assert!(runtime.unknown);
        assert_eq!(runtime.expected.len(), 2);
        let mut failed = r;
        failed.complete = false;
        failed.failure = Some(Failure::Completion);
        failed.samples[0].passed = false;
        assert!(validate_record(&failed, &ctx, &id).is_err());
    }
}
