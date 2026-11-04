//! Installed three-by-three execution. Local children, never artifact PIDs, own authority.
use super::{
    cleanup, fixture,
    process::{self, Environment},
    records::{self, Context, IdentityReady, ROLES, TARGETS},
};
use crate::{Error, Result};
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
use tokio::{process::Child, task::JoinHandle, time::Instant};
fn bad() -> Error {
    Error::Invalid("installed_matrix")
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct Fixture {
    pub path: String,
    pub sha256: String,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct ServerReady {
    schema: u8,
    kind: String,
    context_sha256: String,
    role: String,
    identity_ready_sha256: BTreeMap<String, String>,
    server_account: String,
    ready_at: u64,
    expires_at: u64,
    fixture: Fixture,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Handle {
    format: u8,
    profile: String,
    remote: String,
    session: String,
    invocation: String,
    submit_sha256: String,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct Pair {
    client: String,
    server: String,
    nonce: String,
    vector: u8,
    handle: Handle,
    group_submit_sha256: String,
    fixture: Fixture,
    argv_sha256: String,
    stdin_bytes: usize,
    stdin_sha256: String,
    stdout_bytes: usize,
    stdout_sha256: String,
    stderr_bytes: usize,
    stderr_sha256: String,
    exit: u8,
    wall_ns: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClientResult {
    schema: u8,
    kind: String,
    context_sha256: String,
    role: String,
    identity_ready_sha256: BTreeMap<String, String>,
    server_ready_sha256: BTreeMap<String, String>,
    pairs: Vec<Pair>,
    complete: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServerResult {
    schema: u8,
    kind: String,
    context_sha256: String,
    role: String,
    identity_ready_sha256: BTreeMap<String, String>,
    server_ready_sha256: String,
    receipts: BTreeMap<String, fixture::Receipt>,
    admissions: u8,
    engine_cleanup_complete: bool,
    server_stop_complete: bool,
    client_stop_complete: bool,
    complete: bool,
}
pub(super) struct Run {
    pub context: PathBuf,
    pub workspace: PathBuf,
    pub identities: PathBuf,
    pub inbox: PathBuf,
    pub output: PathBuf,
}
pub(super) struct Daemon {
    pub(super) child: Child,
    out: JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
    err: JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
    pub(super) socket: PathBuf,
}
impl Daemon {
    pub(super) fn start(
        workspace: &Path,
        name: &str,
        socket: PathBuf,
        env: &Environment,
        relays: &[String],
        uptime: u64,
    ) -> Result<Self> {
        let args = base(workspace, name, &socket, relays)
            .into_iter()
            .chain([
                OsString::from("--foreground"),
                OsString::from("--max-uptime"),
                uptime.to_string().into(),
                "daemon".into(),
            ])
            .collect::<Vec<_>>();
        let mut child =
            process::command(&workspace.join("bin/marmishd"), &args, workspace, env).spawn()?;
        let out = tokio::spawn(process::drain(child.stdout.take().ok_or_else(bad)?, 65536));
        let err = tokio::spawn(process::drain(child.stderr.take().ok_or_else(bad)?, 65536));
        Ok(Self {
            child,
            out,
            err,
            socket,
        })
    }
    pub(super) async fn stop(self, deadline: Instant) -> Result<()> {
        self.stop_inner(deadline, true).await
    }
    pub(super) async fn stop_no_engine(self, deadline: Instant) -> Result<()> {
        self.stop_inner(deadline, false).await
    }
    /// Joined ordinary stop while running work survives; this grants no cleanup authority.
    pub(super) async fn ordinary_stop(self, deadline: Instant) -> Result<()> {
        self.stop_inner(deadline, false).await
    }
    async fn stop_inner(self, deadline: Instant, engine_quiesced: bool) -> Result<()> {
        let Self {
            mut child,
            out,
            err,
            ..
        } = self;
        if let Err(error) = cleanup::stop(&mut child, deadline).await {
            out.abort();
            err.abort();
            let _ = out.await;
            let _ = err.await;
            return Err(error);
        }
        let read = tokio::time::timeout_at(deadline, async {
            Ok::<_, Error>((
                out.await.map_err(|_| bad())??,
                err.await.map_err(|_| bad())??,
            ))
        })
        .await
        .map_err(|_| Error::Timeout)??;
        if read.0.1 || read.1.1 {
            return Err(bad());
        }
        if engine_quiesced {
            cleanup::stop_report(&read.0.0)
        } else {
            cleanup::no_engine_stop_report(&read.0.0)
        }
    }
}
pub(super) fn base(
    workspace: &Path,
    name: &str,
    socket: &Path,
    relays: &[String],
) -> Vec<OsString> {
    let mut a = vec![
        "--state-dir".into(),
        workspace.join("profiles").into_os_string(),
        "--identity".into(),
        name.into(),
        "--key-storage".into(),
        "file".into(),
        "--tor".into(),
        "off".into(),
        "--socket".into(),
        socket.as_os_str().into(),
    ];
    for r in relays {
        a.extend(["--relay".into(), r.into()]);
    }
    a
}
fn pins(ids: &BTreeMap<String, IdentityReady>) -> Result<BTreeMap<String, String>> {
    ids.iter()
        .map(|(r, i)| Ok((r.clone(), records::digest(&records::canonical(i)?))))
        .collect()
}
fn nonce(
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    client: &str,
    server: &str,
) -> Result<String> {
    Ok(records::digest(&records::canonical(&(
        "installed-matrix-pair/1",
        ctx.digest()?,
        client,
        server,
        &ids[client].nonce,
        &ids[server].nonce,
    ))?))
}
fn configuration_peers(name: &str, ids: &BTreeMap<String, IdentityReady>) -> Vec<String> {
    ids.values()
        .map(|i| {
            if name == "server" {
                i.client_account.clone()
            } else {
                i.server_account.clone()
            }
        })
        .collect()
}
fn setup_args(
    workspace: &Path,
    name: &str,
    ctx: &Context,
    peers: &[String],
) -> Result<Vec<OsString>> {
    let mut args = base(
        workspace,
        name,
        &workspace.join("setup-unused.sock"),
        &ctx.relays,
    );
    args.extend(["setup".into(), "--update".into()]);
    for peer in peers {
        let account = nostr::key::PublicKey::from_hex(peer).map_err(|_| bad())?;
        args.extend([
            if name == "server" {
                "--grant"
            } else {
                "--trust"
            }
            .into(),
            account.to_bech32().map_err(|_| bad())?.into(),
        ]);
    }
    Ok(args)
}
fn configure_value(
    mut value: serde_json::Value,
    name: &str,
    ctx: &Context,
    peers: &[String],
    fixture: &Path,
) -> Result<serde_json::Value> {
    let server = name == "server";
    let mut ordered = peers.to_vec();
    ordered.sort();
    if ordered.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(bad());
    }
    let desired = serde_json::json!(ordered);
    let empty = serde_json::json!([]);
    if value["role"] != name
        || value["relays"] != serde_json::json!(ctx.relays)
        || value["tor"] != "off"
        || value["command_policy"] != serde_json::json!({"version":1})
        || value["execution_clients"] != if server { &desired } else { &empty }.clone()
        || value["trusted_remotes"] != if server { &empty } else { &desired }.clone()
    {
        return Err(bad());
    }
    // Only fixture policy, cwd and limits lack setup flags. Keep every setup-owned field.
    value["command_policy"]["defaults"]["allow_exec"] = serde_json::json!(if server {
        vec![fixture.to_str().ok_or_else(bad)?.to_owned()]
    } else {
        vec![]
    });
    value["cwd"] = serde_json::json!(fixture.parent().and_then(Path::to_str));
    value["limits"] = if server {
        serde_json::json!({"max_commands":4,"max_uptime":1800})
    } else {
        serde_json::json!({"max_uptime":1800})
    };
    crate::command_config::CommandConfig::from_bytes(&records::canonical(&value)?)?;
    Ok(value)
}
async fn configure(
    workspace: &Path,
    name: &str,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    fixture: &Path,
    env: &Environment,
    deadline: Instant,
) -> Result<()> {
    let peers = configuration_peers(name, ids);
    let output = process::run(
        &workspace.join("bin/marmish"),
        &setup_args(workspace, name, ctx, &peers)?,
        workspace,
        env,
        deadline,
        65536,
    )
    .await?;
    let setup: super::prepare::Setup = serde_json::from_slice(&output.stdout).map_err(|_| bad())?;
    let own = &ids[ROLES[TARGETS
        .iter()
        .position(|t| *t == crate::native_manifest::compiled_release_metadata().target)
        .ok_or_else(bad)?]];
    let account = if name == "server" {
        &own.server_account
    } else {
        &own.client_account
    };
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
        || hex::encode(crate::command_config::account(&setup.npub)?) != *account
    {
        return Err(bad());
    }
    let path = workspace.join("profiles").join(name).join("config.json");
    let old = records::read_bytes(&path, 1048576)?;
    crate::command_config::CommandConfig::from_bytes(&old)?;
    let value = configure_value(
        serde_json::from_slice(&old).map_err(|_| bad())?,
        name,
        ctx,
        &peers,
        fixture,
    )?;
    let b = records::canonical(&value)?;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)?;
    f.write_all(&b)?;
    f.sync_all()?;
    if records::read_bytes(&path, 1048576)? != b {
        return Err(bad());
    }
    Ok(())
}
fn budgets(remaining: u64) -> Result<(u64, u64)> {
    let uptime = 1800.min(remaining.checked_sub(60).ok_or_else(bad)?);
    let work = uptime
        .checked_sub(360)
        .filter(|v| *v >= 600)
        .ok_or_else(bad)?;
    Ok((uptime, work))
}
pub(super) async fn ready(d: &mut Daemon, deadline: Instant) -> Result<()> {
    loop {
        let value = cleanup::request::<serde_json::Value>(
            &mut d.child,
            &d.socket,
            &crate::ipc::Request::Status {},
            deadline,
        )
        .await;
        if let Ok(v) = value
            && v.get("state").and_then(serde_json::Value::as_str) == Some("active")
        {
            return Ok(());
        }
        if d.child.try_wait()?.is_some() {
            return Err(bad());
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn validate_ready(
    r: &ServerReady,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    role: &str,
) -> Result<()> {
    if r.schema != 1
        || r.kind != "server-ready"
        || r.context_sha256 != ctx.digest()?
        || r.role != role
        || r.identity_ready_sha256 != pins(ids)?
        || r.server_account != ids[role].server_account
        || r.expires_at != ctx.expires_at
        || r.ready_at.saturating_add(120) < ctx.created_at
        || r.ready_at > ctx.expires_at
        || !Path::new(&r.fixture.path).is_absolute()
        || r.fixture.sha256
            != ctx.tooling[ROLES.iter().position(|v| *v == role).ok_or_else(bad)?]
                .files
                .fixture
                .sha256
    {
        return Err(bad());
    }
    Ok(())
}
async fn wait_servers(
    o: &Run,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    deadline: Instant,
) -> Result<BTreeMap<String, ServerReady>> {
    loop {
        let mut all = BTreeMap::new();
        for r in ROLES {
            let p = o.inbox.join(format!("{r}-server-ready.json"));
            if p.try_exists()? {
                let v: ServerReady = records::read(&p, 65536)?;
                validate_ready(&v, ctx, ids, r)?;
                all.insert(r.into(), v);
            }
        }
        if all.len() == 3 {
            return Ok(all);
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
fn fixture_handle(stderr: &[u8], remote: &str) -> Result<(Handle, Vec<u8>)> {
    let rest = stderr.strip_prefix(b"marmish handle: ").ok_or_else(bad)?;
    let line = rest.iter().position(|b| *b == b'\n').ok_or_else(bad)?;
    let h: Handle = serde_json::from_slice(&rest[..line]).map_err(|_| bad())?;
    if !rest[line + 1..].is_empty()
        || line > 1024
        || h.format != 1
        || h.profile != "client"
        || h.remote != remote
        || !records::hexadecimal(&h.session, 32)
        || !records::hexadecimal(&h.invocation, 32)
        || !records::hexadecimal(&h.submit_sha256, 64)
    {
        return Err(bad());
    }
    Ok((h, rest[..line].to_vec()))
}
struct ClientEndpoint<'a> {
    daemon: &'a mut Child,
    socket: &'a Path,
    environment: &'a Environment,
}
struct FixtureCommand {
    handle: Handle,
    encoded: Vec<u8>,
    output: Result<process::Output>,
    elapsed: Duration,
}
fn verify_wait(output: &process::Output, stdout: &[u8], stderr: &[u8]) -> Result<()> {
    if output.status.code() != Some(7) || output.stdout != stdout || output.stderr != stderr {
        return Err(bad());
    }
    Ok(())
}
// Keep both actual CLI children under one deadline and measure neither receipt nor cleanup.
async fn fixture_command(
    workspace: &Path,
    local: (&Path, &Environment),
    submit_args: &[OsString],
    input: &[u8],
    remote: &str,
    deadline: Instant,
) -> Result<FixtureCommand> {
    let began = Instant::now();
    let sample_deadline = deadline.min(began + Duration::from_secs(330));
    let detached = process::run_input(
        &workspace.join("bin/marmish"),
        submit_args,
        workspace,
        local.1,
        sample_deadline,
        65536,
        Some(input),
    )
    .await?;
    if !detached.status.success() || !detached.stdout.is_empty() {
        return Err(bad());
    }
    let (handle, encoded) = fixture_handle(&detached.stderr, remote)?;
    let mut wait_args = base(workspace, "client", local.0, &[]);
    wait_args.extend([
        "--no-autostart".into(),
        "--wait-seconds".into(),
        "300".into(),
        "wait".into(),
        "--handle".into(),
        String::from_utf8(encoded.clone())
            .map_err(|_| bad())?
            .into(),
    ]);
    let output = process::run(
        &workspace.join("bin/marmish"),
        &wait_args,
        workspace,
        local.1,
        sample_deadline,
        524288,
    )
    .await;
    Ok(FixtureCommand {
        handle,
        encoded,
        output,
        elapsed: began.elapsed(),
    })
}
async fn clients(
    o: &Run,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    role: &str,
    servers: &BTreeMap<String, ServerReady>,
    local: ClientEndpoint<'_>,
    deadline: Instant,
) -> Result<ClientResult> {
    let mut result = ClientResult {
        schema: 1,
        kind: "client-result".into(),
        context_sha256: ctx.digest()?,
        role: role.into(),
        identity_ready_sha256: pins(ids)?,
        server_ready_sha256: servers
            .iter()
            .map(|(r, s)| Ok((r.clone(), records::digest(&records::canonical(s)?))))
            .collect::<Result<_>>()?,
        pairs: Vec::new(),
        complete: false,
    };
    for (i, server) in ROLES.into_iter().enumerate() {
        let ready = &servers[server];
        let nonce = nonce(ctx, ids, role, server)?;
        let vector = i as u8;
        let (input, out, err) = fixture::bytes(&nonce, vector)?;
        let mut args = base(&o.workspace, "client", local.socket, &ctx.relays);
        args.extend([
            "--no-autostart".into(),
            "--stdin".into(),
            "--detach".into(),
            "--wait-seconds".into(),
            "300".into(),
            nostr::key::PublicKey::from_hex(&ids[server].server_account)
                .map_err(|_| bad())?
                .to_bech32()
                .map_err(|_| bad())?
                .into(),
            "--".into(),
            ready.fixture.path.clone().into(),
        ]);
        let fixture_args = fixture::arguments(&nonce, vector)?;
        args.extend(fixture_args.clone());
        let command = fixture_command(
            &o.workspace,
            (local.socket, local.environment),
            &args,
            &input,
            &ids[server].server_account,
            deadline,
        )
        .await?;
        // Query the independent cached admission digest after timing, even if wait failed.
        let command_handle = crate::command_client::CommandHandle::decode(&command.encoded)?;
        let group_submit_sha256 =
            cleanup::client_group_submit(local.daemon, local.socket, &command_handle, deadline)
                .await?;
        let output = command.output?;
        verify_wait(&output, &out, &err)?;
        result.pairs.push(Pair {
            client: role.into(),
            server: server.into(),
            nonce,
            vector,
            handle: command.handle,
            group_submit_sha256,
            fixture: ready.fixture.clone(),
            argv_sha256: fixture::argv_digest(Path::new(&ready.fixture.path), &fixture_args)?,
            stdin_bytes: input.len(),
            stdin_sha256: records::digest(&input),
            stdout_bytes: out.len(),
            stdout_sha256: records::digest(&out),
            stderr_bytes: err.len(),
            stderr_sha256: records::digest(&err),
            exit: 7,
            wall_ns: u64::try_from(command.elapsed.as_nanos()).map_err(|_| bad())?,
        });
    }
    result.complete = true;
    Ok(result)
}
fn validate_client(
    c: &ClientResult,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    servers: &BTreeMap<String, ServerReady>,
    role: &str,
) -> Result<()> {
    let server_pins = servers
        .iter()
        .map(|(r, s)| Ok((r.clone(), records::digest(&records::canonical(s)?))))
        .collect::<Result<BTreeMap<_, _>>>()?;
    if c.schema != 1
        || c.kind != "client-result"
        || c.context_sha256 != ctx.digest()?
        || c.role != role
        || c.identity_ready_sha256 != pins(ids)?
        || c.server_ready_sha256 != server_pins
        || !c.complete
        || c.pairs.len() != 3
    {
        return Err(bad());
    }
    for (i, p) in c.pairs.iter().enumerate() {
        let server = ROLES[i];
        let f = &servers[server].fixture;
        let expected = fixture::expected_receipt(
            Path::new(&f.path),
            &nonce(ctx, ids, role, server)?,
            i as u8,
            &f.sha256,
        )?;
        if p.client != role
            || p.server != server
            || p.nonce != expected.nonce
            || p.vector != i as u8
            || p.fixture.path != f.path
            || p.fixture.sha256 != f.sha256
            || p.argv_sha256 != expected.argv_sha256
            || p.stdin_bytes != expected.stdin_bytes
            || p.stdin_sha256 != expected.stdin_sha256
            || p.stdout_bytes != expected.stdout_bytes
            || p.stdout_sha256 != expected.stdout_sha256
            || p.stderr_bytes != expected.stderr_bytes
            || p.stderr_sha256 != expected.stderr_sha256
            || p.exit != 7
            || p.wall_ns == 0
            || p.wall_ns > 330_000_000_000
            || p.handle.remote != ids[server].server_account
            || p.handle.profile != "client"
            || p.handle.format != 1
            || !records::hexadecimal(&p.handle.invocation, 32)
            || !records::hexadecimal(&p.handle.session, 32)
            || !records::hexadecimal(&p.group_submit_sha256, 64)
            || p.group_submit_sha256 == p.handle.submit_sha256
            || !records::hexadecimal(&p.handle.submit_sha256, 64)
        {
            return Err(bad());
        }
    }
    Ok(())
}
async fn wait_clients(
    o: &Run,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    servers: &BTreeMap<String, ServerReady>,
    deadline: Instant,
) -> Result<BTreeMap<String, ClientResult>> {
    loop {
        let mut all = BTreeMap::new();
        for role in ROLES {
            let path = o.inbox.join(format!("{role}-client-result.json"));
            if path.try_exists()? {
                let c: ClientResult = records::read(&path, 1048576)?;
                validate_client(&c, ctx, ids, servers, role)?;
                all.insert(role.into(), c);
            }
        }
        if all.len() == 3 {
            return Ok(all);
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
struct Runtime {
    server: Option<Daemon>,
    client: Option<Daemon>,
    tree: Option<super::super::command_check::EngineTree>,
    client_receipt: cleanup::ProfileReceipt,
}
impl Runtime {
    async fn close(
        &mut self,
        w: &Path,
        expected: BTreeMap<(String, String), (String, String)>,
        cancel: bool,
        deadline: Instant,
    ) -> (bool, bool, bool) {
        if cancel && let Some(server) = self.server.as_mut() {
            let _ =
                cleanup::cancel_fixture(&mut server.child, &server.socket, &expected, 3, deadline)
                    .await;
        }
        let engine = if let (Some(server), Some(tree)) = (self.server.as_mut(), self.tree.take()) {
            match cleanup::prepare(
                &mut server.child,
                &w.join("profiles/server"),
                &server.socket,
                expected,
                deadline,
            )
            .await
            {
                Ok(mut guard) => tree.remove_installed(&mut guard).await.is_ok(),
                Err(_) => false,
            }
        } else {
            false
        };
        let client_profile = w.join("profiles/client");
        let receipt_valid = self.client_receipt.verify(&client_profile).is_ok();
        let client_proof = if receipt_valid && let Some(d) = self.client.as_mut() {
            cleanup::verify_no_engine(&mut d.child, &client_profile, &d.socket, deadline)
                .await
                .is_ok()
        } else {
            false
        };
        let client = match self.client.take() {
            Some(d) => {
                let stopped = d.stop_no_engine(deadline).await.is_ok();
                client_proof
                    && stopped
                    && self.client_receipt.verify(&client_profile).is_ok()
                    && cleanup::require_no_engine(&client_profile).is_ok()
            }
            None => false,
        };
        let server = match self.server.take() {
            Some(d) => d.stop(deadline).await.is_ok(),
            None => false,
        };
        (engine, server, client)
    }
}
pub(super) async fn run(o: Run) -> Result<()> {
    let ctx: Context = records::read(&o.context, 65536)?;
    ctx.validate()?;
    ctx.require_cohort("matrix")?;
    let ids = records::identities(&o.identities, &ctx)?;
    let i = TARGETS
        .iter()
        .position(|t| *t == crate::native_manifest::compiled_release_metadata().target)
        .ok_or_else(bad)?;
    let role = ROLES[i];
    let own = &ids[role];
    let now = super::now()?;
    let remaining = ctx.expires_at.checked_sub(now).ok_or_else(bad)?;
    let preparation_deadline = Instant::now() + Duration::from_secs(remaining.min(150));
    crate::identity::check_dir(&o.workspace)?;
    crate::identity::check_dir(&o.output)?;
    let environment = Environment {
        home: o.workspace.join("home"),
        temporary: o.workspace.join("tmp"),
        utilities: o.workspace.join("utilities"),
    };
    let exe = records::verify_tools(&ctx, i)?;
    let tool = &ctx.tooling[i];
    let source = exe
        .parent()
        .ok_or_else(bad)?
        .join("marmish-installed-fixture");
    let b = records::read_bytes(&source, 268435456)?;
    let dir = o.workspace.join("fixture");
    fs::DirBuilder::new().mode(0o700).create(&dir)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(dir.join("receipts"))?;
    let executable = dir.join("main");
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&executable)?;
        f.write_all(&b)?;
        f.sync_all()?;
    }
    let executable = executable.canonicalize()?;
    if records::digest(&records::read_bytes(&executable, 268435456)?) != tool.files.fixture.sha256 {
        return Err(bad());
    }
    let fixture = Fixture {
        path: executable.to_str().ok_or_else(bad)?.into(),
        sha256: tool.files.fixture.sha256.clone(),
    };
    for n in ["client", "server"] {
        configure(
            &o.workspace,
            n,
            &ctx,
            &ids,
            &executable,
            &environment,
            preparation_deadline,
        )
        .await?;
    }
    match fs::symlink_metadata(o.workspace.join("profiles/server/commands")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(bad()),
    }
    cleanup::require_no_engine(&o.workspace.join("profiles/client"))?;
    let client_receipt = cleanup::ProfileReceipt::capture(&o.workspace.join("profiles/client"))?;
    let mut random = [0; 8];
    getrandom::fill(&mut random).map_err(|_| Error::Identity("random"))?;
    let sockets = std::env::temp_dir()
        .canonicalize()?
        .join(format!("mi-{}", hex::encode(random)));
    fs::DirBuilder::new().mode(0o700).create(&sockets)?;
    let (uptime, work_seconds) =
        budgets(ctx.expires_at.checked_sub(super::now()?).ok_or_else(bad)?)?;
    // Both cutoffs share the original server launch origin; reserve cleanup before its uptime.
    let launch = Instant::now();
    let deadline = launch + Duration::from_secs(work_seconds);
    let cleanup_deadline = launch + Duration::from_secs(uptime - 30);
    let mut runtime = Runtime {
        server: None,
        client: None,
        tree: None,
        client_receipt,
    };
    let mut receipts = BTreeMap::new();
    let mut expected = BTreeMap::new();
    let mut server_ready = None;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let work = async {
        runtime.server = Some(Daemon::start(
            &o.workspace,
            "server",
            sockets.join("s"),
            &environment,
            &ctx.relays,
            uptime,
        )?);
        ready(runtime.server.as_mut().ok_or_else(bad)?, deadline).await?;
        runtime.tree = Some(super::super::command_check::EngineTree::capture_path(
            &o.workspace.join("profiles/server"),
        )?);
        runtime
            .client_receipt
            .verify(&o.workspace.join("profiles/client"))?;
        runtime.client = Some(Daemon::start(
            &o.workspace,
            "client",
            sockets.join("c"),
            &environment,
            &ctx.relays,
            uptime,
        )?);
        ready(runtime.client.as_mut().ok_or_else(bad)?, deadline).await?;
        {
            runtime
                .client_receipt
                .verify(&o.workspace.join("profiles/client"))?;
            let client = runtime.client.as_mut().ok_or_else(bad)?;
            cleanup::verify_no_engine(
                &mut client.child,
                &o.workspace.join("profiles/client"),
                &client.socket,
                deadline,
            )
            .await?;
        }
        let sr = ServerReady {
            schema: 1,
            kind: "server-ready".into(),
            context_sha256: ctx.digest()?,
            role: role.into(),
            identity_ready_sha256: pins(&ids)?,
            server_account: own.server_account.clone(),
            ready_at: super::now()?,
            expires_at: ctx.expires_at,
            fixture: fixture.clone(),
        };
        records::write(&o.output.join(format!("{role}-server-ready.json")), &sr)?;
        server_ready = Some(records::digest(&records::canonical(&sr)?));
        let servers = wait_servers(&o, &ctx, &ids, deadline).await?;
        let c = clients(
            &o,
            &ctx,
            &ids,
            role,
            &servers,
            ClientEndpoint {
                daemon: &mut runtime.client.as_mut().ok_or_else(bad)?.child,
                socket: &sockets.join("c"),
                environment: &environment,
            },
            deadline,
        )
        .await?;
        records::write(&o.output.join(format!("{role}-client-result.json")), &c)?;
        let all = wait_clients(&o, &ctx, &ids, &servers, deadline).await?;
        for client in ROLES {
            let p = all[client]
                .pairs
                .iter()
                .find(|p| p.server == role)
                .ok_or_else(bad)?;
            let receipt: fixture::Receipt =
                records::read(&fixture::receipt_path(&executable, &p.nonce)?, 65536)?;
            let expected_receipt =
                fixture::expected_receipt(&executable, &p.nonce, p.vector, &fixture.sha256)?;
            if receipt != expected_receipt {
                return Err(bad());
            }
            receipts.insert(client.into(), receipt);
            expected.insert(
                (
                    ids[client].client_account.clone(),
                    p.handle.invocation.clone(),
                ),
                (p.handle.session.clone(), p.group_submit_sha256.clone()),
            );
        }
        Ok::<_, Error>(())
    };
    let result = tokio::select! {r=tokio::time::timeout_at(deadline,work)=>r.map_err(|_|Error::Timeout).and_then(|r|r),_=sigterm.recv()=>Err(Error::Invalid("installed_interrupted"))};
    let (engine, server, client) = runtime
        .close(&o.workspace, expected, result.is_err(), cleanup_deadline)
        .await;
    let client_path = o.output.join(format!("{role}-client-result.json"));
    if !client_path.try_exists()? {
        let failed = ClientResult {
            schema: 1,
            kind: "client-result".into(),
            context_sha256: ctx.digest()?,
            role: role.into(),
            identity_ready_sha256: pins(&ids)?,
            server_ready_sha256: BTreeMap::new(),
            pairs: Vec::new(),
            complete: false,
        };
        records::write(&client_path, &failed)?;
    }
    let complete = result.is_ok() && engine && server && client && receipts.len() == 3;
    let report = ServerResult {
        schema: 1,
        kind: "server-result".into(),
        context_sha256: ctx.digest()?,
        role: role.into(),
        identity_ready_sha256: pins(&ids)?,
        server_ready_sha256: server_ready.unwrap_or_default(),
        admissions: receipts.len() as u8,
        receipts,
        engine_cleanup_complete: engine,
        server_stop_complete: server,
        client_stop_complete: client,
        complete,
    };
    records::write(
        &o.output.join(format!("{role}-server-result.json")),
        &report,
    )?;
    if server && client {
        let _ = fs::remove_dir(&sockets);
    }
    if complete { Ok(()) } else { Err(bad()) }
}

/// Recompute every cell from independently delivered client and server records.
pub(super) fn rollup(
    context: &Path,
    directory: &Path,
    matrix_result: &str,
    output: &Path,
) -> Result<()> {
    let ctx: Context = records::read(context, 65536)?;
    ctx.validate()?;
    ctx.require_cohort("matrix")?;
    let ids = records::matrix_identities(directory, &ctx)?;
    let mut servers = BTreeMap::new();
    for role in ROLES {
        let s: ServerReady =
            records::read(&directory.join(format!("{role}-server-ready.json")), 65536)?;
        validate_ready(&s, &ctx, &ids, role)?;
        servers.insert(role.to_owned(), s);
    }
    let mut clients = BTreeMap::new();
    for role in ROLES {
        let c: ClientResult = records::read(
            &directory.join(format!("{role}-client-result.json")),
            1048576,
        )?;
        validate_client(&c, &ctx, &ids, &servers, role)?;
        clients.insert(role.to_owned(), c);
    }
    let mut cells = Vec::new();
    for server in ROLES {
        let s: ServerResult = records::read(
            &directory.join(format!("{server}-server-result.json")),
            1048576,
        )?;
        if s.schema != 1
            || s.kind != "server-result"
            || s.role != server
            || s.context_sha256 != ctx.digest()?
            || s.identity_ready_sha256 != pins(&ids)?
            || s.server_ready_sha256 != records::digest(&records::canonical(&servers[server])?)
            || s.admissions != 3
            || s.receipts.len() != 3
            || !s.complete
            || !s.engine_cleanup_complete
            || !s.server_stop_complete
            || !s.client_stop_complete
        {
            return Err(bad());
        }
        for client in ROLES {
            let p = clients[client]
                .pairs
                .iter()
                .find(|p| p.server == server)
                .ok_or_else(bad)?;
            let receipt = s.receipts.get(client).ok_or_else(bad)?;
            let expected = fixture::expected_receipt(
                Path::new(&p.fixture.path),
                &p.nonce,
                p.vector,
                &p.fixture.sha256,
            )?;
            if receipt != &expected {
                return Err(bad());
            }
            cells.push(serde_json::json!({"client":client,"server":server,"passed":true,"command_wall_ns":p.wall_ns,"stdout_bytes":p.stdout_bytes,"stderr_bytes":p.stderr_bytes,"payload_bytes_per_second":((p.stdout_bytes+p.stderr_bytes) as f64)*1e9/(p.wall_ns as f64),"fixture_sha256":p.fixture.sha256}));
        }
    }
    if matrix_result != "success" {
        return Err(Error::Invalid("installed_matrix_job"));
    }
    let result = serde_json::json!({"schema":1,"scope":"installed-matrix","artifact_prefix":ctx.prefix(),"context_sha256":ctx.digest()?,"candidate_run_id":ctx.candidate_run_id,"candidate_run_attempt":ctx.candidate_run_attempt,"source_commit":ctx.source_commit,"matrix_job_result":matrix_result,"cells":cells,"passed_cells":9,"expected_cells":9,"sample_count":9,"samples_per_directed_pair":1,"metric_definitions":{"command_wall_ns":"Local monotonic detached-submit plus handle-wait lifetime including both CLI startups, encrypted request, execution, output delivery and Finish; excludes receipt validation and protected cleanup; not network RTT","payload_bytes_per_second":"Validated stdout plus stderr bytes divided by command wall time; excludes stdin"},"pending_gates":["auto-exit","benchmark","recovery","policy","keychain","asset-publication","HTTPS"]});
    records::write(&output.join("matrix-rollup.json"), &result)?;
    let mut markdown = String::from(
        "# Installed native matrix\n\nScope: installed-matrix. Nine directed pairs, one sample per pair; no percentile estimates.\n\nCommand wall time spans detached submit plus handle wait, including both CLI startups, execution, encrypted output delivery and Finish, excluding independent receipt validation and protected cleanup; it is not network RTT. Speed is validated stdout plus stderr bytes divided by wall time, excluding stdin.\n\n| Client | Server | Samples | Wall (ms) | Output (bytes) | Speed (bytes/s) |\n| --- | --- | ---: | ---: | ---: | ---: |\n",
    );
    for cell in result["cells"].as_array().ok_or_else(bad)? {
        use std::fmt::Write;
        writeln!(
            markdown,
            "| {} | {} | 1 | {:.3} | {} | {:.3} |",
            cell["client"].as_str().ok_or_else(bad)?,
            cell["server"].as_str().ok_or_else(bad)?,
            cell["command_wall_ns"].as_u64().ok_or_else(bad)? as f64 / 1e6,
            cell["stdout_bytes"].as_u64().ok_or_else(bad)?
                + cell["stderr_bytes"].as_u64().ok_or_else(bad)?,
            cell["payload_bytes_per_second"].as_f64().ok_or_else(bad)?
        )
        .map_err(|_| bad())?;
    }
    markdown.push_str("\nPending gates: auto-exit, benchmark, recovery, policy, keychain, asset-publication, HTTPS. This matrix alone does not qualify a release.\n");
    records::write_raw(&output.join("matrix-rollup.md"), markdown.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_handle() -> Handle {
        Handle {
            format: 1,
            profile: "client".into(),
            remote: "a".repeat(64),
            session: "b".repeat(32),
            invocation: "c".repeat(32),
            submit_sha256: "d".repeat(64),
        }
    }

    #[test]
    fn detached_handle_requires_one_complete_line_and_preserves_encoding() {
        let handle = test_handle();
        let encoded = serde_json::to_vec(&handle).unwrap();
        let mut line = b"marmish handle: ".to_vec();
        line.extend_from_slice(&encoded);
        line.push(b'\n');
        let (_, observed) = fixture_handle(&line, &handle.remote).unwrap();
        assert_eq!(observed, encoded);
        for extra in [b"diagnostic\n".as_slice(), &line, b"\0"] {
            let mut bad_line = line.clone();
            bad_line.extend_from_slice(extra);
            assert!(fixture_handle(&bad_line, &handle.remote).is_err());
        }
        assert!(fixture_handle(&line, &"f".repeat(64)).is_err());
        assert!(fixture_handle(&line[..line.len() - 1], &handle.remote).is_err());
        let duplicate = String::from_utf8(line.clone())
            .unwrap()
            .replace("\"format\":1", "\"format\":1,\"format\":1");
        assert!(fixture_handle(duplicate.as_bytes(), &handle.remote).is_err());
        let mut unknown = serde_json::to_value(&handle).unwrap();
        unknown["unexpected"] = true.into();
        assert!(serde_json::from_value::<Handle>(unknown).is_err());
    }

    #[tokio::test]
    async fn owned_submit_once_then_wait_keeps_binary_stderr_and_original_deadline() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let w = root.path();
        for name in ["bin", "home", "tmp", "utilities"] {
            fs::create_dir(w.join(name)).unwrap();
        }
        let env = Environment {
            home: w.join("home"),
            temporary: w.join("tmp"),
            utilities: w.join("utilities"),
        };
        let socket = w.join("unused.sock");
        let local = (&socket as &Path, &env);
        let handle = test_handle();
        let encoded = serde_json::to_string(&handle).unwrap();
        let stdout = b"raw\0stdout\xff\n";
        let stderr = b"marmish handle: \0raw remote stderr\xff\n";
        fs::write(w.join("stdout"), stdout).unwrap();
        fs::write(w.join("stderr"), stderr).unwrap();
        let script = format!(
            r#"#!/bin/sh
mode=submit
handle=
for arg do
    if [ "$arg" = wait ]; then mode=wait; fi
    if [ "$handle" = next ]; then handle=$arg; fi
    if [ "$arg" = --handle ]; then handle=next; fi
done
if [ "$mode" = submit ]; then
    [ ! -e submit-count ] || exit 90
    : > submit-count
    /bin/cat > input
    /bin/sleep 0.03
    printf '%s\n' 'marmish handle: {encoded}' >&2
else
    case " $* " in *" --relay "*) exit 91;; esac
    [ "$handle" = '{encoded}' ] || exit 92
    [ ! -e wait-count ] || exit 93
    : > wait-count
    /bin/sleep 0.03
    /bin/cat stdout
    /bin/cat stderr >&2
    exit 7
fi
"#
        );
        let exe = w.join("bin/marmish");
        fs::write(&exe, script).unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
        let input = b"binary\0stdin\xff";
        let args = [OsString::from("--detach")];
        let observed = fixture_command(
            w,
            local,
            &args,
            input,
            &handle.remote,
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
        let output = observed.output.unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, stdout);
        assert_eq!(output.stderr, stderr);
        assert_eq!(fs::read(w.join("input")).unwrap(), input);
        assert!(observed.elapsed >= Duration::from_millis(50));
        assert!(w.join("submit-count").exists() && w.join("wait-count").exists());
        // Remove prior sentinels so an accidental spawn cannot satisfy this assertion.
        fs::remove_file(w.join("submit-count")).unwrap();
        fs::remove_file(w.join("wait-count")).unwrap();
        assert!(
            fixture_command(w, local, &args, input, &handle.remote, Instant::now(),)
                .await
                .is_err()
        );
        assert!(!w.join("submit-count").exists());
        assert!(!w.join("wait-count").exists());
    }

    #[tokio::test]
    async fn failed_wait_retains_handle_and_second_child_cannot_renew_cutoff() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let w = root.path();
        fs::create_dir(w.join("bin")).unwrap();
        let env = Environment {
            home: w.into(),
            temporary: w.into(),
            utilities: w.into(),
        };
        let socket = w.join("unused.sock");
        let handle = test_handle();
        let encoded = serde_json::to_vec(&handle).unwrap();
        let line = String::from_utf8(encoded.clone()).unwrap();
        let exe = w.join("bin/marmish");
        for (detach_code, wait_code, pause) in [(0, 255, "0"), (1, 7, "0"), (0, 7, "1.2")] {
            let script = format!(
                r#"#!/bin/sh
mode=submit
for arg do [ "$arg" != wait ] || mode=wait; done
if [ "$mode" = submit ]; then
    /bin/cat >/dev/null
    /bin/sleep {pause}
    printf '%s\n' 'marmish handle: {line}' >&2
    exit {detach_code}
fi
/bin/sleep {pause}
exit {wait_code}
"#
            );
            fs::write(&exe, script).unwrap();
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
            let started = Instant::now();
            let result = fixture_command(
                w,
                (&socket, &env),
                &["--detach".into()],
                b"input",
                &handle.remote,
                started + Duration::from_millis(if pause == "0" { 5000 } else { 2000 }),
            )
            .await;
            if detach_code != 0 {
                assert!(result.is_err());
            } else {
                let result = result.unwrap();
                assert_eq!(result.encoded, encoded);
                if pause == "0" {
                    assert_eq!(result.output.unwrap().status.code(), Some(255));
                } else {
                    assert!(matches!(result.output, Err(Error::Timeout)));
                    assert!(started.elapsed() < Duration::from_secs(3));
                }
            }
        }
    }

    #[test]
    fn wait_verification_rejects_wrong_exit_and_any_byte_or_diagnostic() {
        use std::os::unix::process::ExitStatusExt;
        let good = || process::Output {
            status: std::process::ExitStatus::from_raw(7 << 8),
            stdout: vec![0, 255],
            stderr: vec![255, 0],
        };
        verify_wait(&good(), &[0, 255], &[255, 0]).unwrap();
        for code in [0, 6, 254] {
            let mut output = good();
            output.status = std::process::ExitStatus::from_raw(code << 8);
            assert!(verify_wait(&output, &[0, 255], &[255, 0]).is_err());
        }
        let mut output = good();
        output.stdout[1] ^= 1;
        assert!(verify_wait(&output, &[0, 255], &[255, 0]).is_err());
        let mut output = good();
        output.stderr[0] ^= 1;
        assert!(verify_wait(&output, &[0, 255], &[255, 0]).is_err());
        for diagnostic in [b"delivery_pending\n".as_slice(), b"marmish handle: {}\n"] {
            let mut output = good();
            output.stderr.splice(0..0, diagnostic.iter().copied());
            assert!(verify_wait(&output, &[0, 255], &[255, 0]).is_err());
        }
    }

    #[tokio::test]
    async fn malformed_or_uncertain_detach_never_starts_wait_child() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let w = root.path();
        fs::create_dir(w.join("bin")).unwrap();
        let env = Environment {
            home: w.into(),
            temporary: w.into(),
            utilities: w.into(),
        };
        let socket = w.join("unused.sock");
        let handle = test_handle();
        let valid = format!(
            "marmish handle: {}\n",
            serde_json::to_string(&handle).unwrap()
        );
        let mut wrong = test_handle();
        wrong.remote = "f".repeat(64);
        let changed = format!(
            "marmish handle: {}\n",
            serde_json::to_string(&wrong).unwrap()
        );
        for (code, stdout, stderr, pause) in [
            (1, "", valid.clone(), "0"),
            (254, "", valid.clone(), "0"),
            (0, "unexpected", valid.clone(), "0"),
            (0, "", String::new(), "0"),
            (0, "", "marmish handle: {}\n".into(), "0"),
            (0, "", valid.repeat(2), "0"),
            (0, "", format!("{valid}diagnostic\n"), "0"),
            (0, "", changed, "0"),
            (0, "", valid.clone(), "0.3"),
        ] {
            let script = format!(
                r#"#!/bin/sh
mode=submit
for arg do [ "$arg" != wait ] || mode=wait; done
if [ "$mode" = wait ]; then : > waited; exit 7; fi
/bin/cat >/dev/null
/bin/sleep {pause}
printf '%s' '{stdout}'
printf '%s' '{stderr}' >&2
exit {code}
"#
            );
            let exe = w.join("bin/marmish");
            fs::write(&exe, script).unwrap();
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
            let deadline =
                Instant::now() + Duration::from_millis(if pause == "0" { 5000 } else { 80 });
            assert!(
                fixture_command(
                    w,
                    (&socket, &env),
                    &["--detach".into()],
                    b"input",
                    &handle.remote,
                    deadline,
                )
                .await
                .is_err()
            );
            assert!(!w.join("waited").exists());
        }
    }

    #[test]
    fn work_and_cleanup_are_inside_original_server_uptime() {
        for remaining in [1020, 1500, 2220] {
            let (uptime, work) = budgets(remaining).unwrap();
            let launch = Instant::now();
            assert_eq!(uptime - work, 360);
            assert!(launch + Duration::from_secs(work) < launch + Duration::from_secs(uptime - 30));
            assert!(uptime < remaining && uptime <= 1800);
        }
        assert!(budgets(1019).is_err());
    }

    #[tokio::test]
    async fn actual_setup_updates_grants_and_trust_then_preserves_them() {
        use crate::native_setup::{SetupOptions, SetupRole};
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(std::env::var_os("HOME").unwrap())
            .unwrap();
        let ctx = records::tests::ctx();
        let mut peers: Vec<_> = (0..3)
            .map(|_| hex::encode(nostr::key::Keys::generate().public_key().to_bytes()))
            .collect();
        peers.sort();
        peers.reverse(); // Setup canonicalizes peers; deliberately exercise noncanonical input.
        for (name, role) in [("client", SetupRole::Client), ("server", SetupRole::Server)] {
            let options = |update| SetupOptions {
                state_dir: root.path().join("profiles"),
                identity: name.into(),
                backend: crate::identity::BackendChoice::File,
                helper: PathBuf::new(),
                interactive: false,
                role: Some(role),
                relays: Some(ctx.relays.clone()),
                tor: Some("off".into()),
                grants: vec![],
                revoke: vec![],
                trust: vec![],
                untrust: vec![],
                update,
                no_grants: !update,
                debug: false,
            };
            crate::native_setup::run(options(false)).await.unwrap();
            let argv = setup_args(root.path(), name, &ctx, &peers).unwrap();
            let mut update = options(true);
            let flag = if name == "server" {
                "--grant"
            } else {
                "--trust"
            };
            let values: Vec<_> = argv
                .windows(2)
                .filter(|p| p[0] == flag)
                .map(|p| p[1].to_str().unwrap().to_owned())
                .collect();
            assert_eq!(values.len(), 3);
            assert!(argv.iter().any(|v| v == "--update"));
            if name == "server" {
                update.grants = values;
            } else {
                update.trust = values;
            }
            let report = crate::native_setup::run(update).await.unwrap();
            assert!(report.config_changed && !report.identity_created && !report.owner_opened);
            assert_eq!(report.inbound_enabled, name == "server");
            let path = root.path().join("profiles").join(name).join("config.json");
            let before: serde_json::Value = records::read(&path, 1048576).unwrap();
            let after = configure_value(
                before.clone(),
                name,
                &ctx,
                &peers,
                Path::new("/private/fixture/main"),
            )
            .unwrap();
            let mut effective = crate::command_config::CommandConfig::from_bytes(
                &records::canonical(&after).unwrap(),
            )
            .unwrap();
            effective
                .apply_limit_overrides(crate::command_limits::LimitOverrides::default())
                .unwrap();
            assert_eq!(
                effective
                    .limits()
                    .max_commands
                    .map(std::num::NonZeroU64::get),
                if name == "server" { Some(4) } else { None }
            );
            assert_eq!(
                effective.limits().max_uptime,
                Some(Duration::from_secs(1800))
            );
            for field in [
                "role",
                "relays",
                "tor",
                "execution_clients",
                "trusted_remotes",
                "aliases",
                "version",
            ] {
                assert_eq!(before[field], after[field]);
            }
            let peer_field = if name == "server" {
                "execution_clients"
            } else {
                "trusted_remotes"
            };
            let canonical: Vec<String> =
                serde_json::from_value(before[peer_field].clone()).unwrap();
            assert_eq!(canonical, peers.iter().rev().cloned().collect::<Vec<_>>());
            let mut extra = before.clone();
            extra[peer_field]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(hex::encode(
                    nostr::key::Keys::generate().public_key().to_bytes()
                )));
            assert!(
                configure_value(
                    extra,
                    name,
                    &ctx,
                    &peers,
                    Path::new("/private/fixture/main")
                )
                .is_err()
            );
            let mut unexpected_policy = before.clone();
            unexpected_policy["command_policy"]["defaults"] =
                serde_json::json!({"deny_exec":["**"]});
            assert!(
                configure_value(
                    unexpected_policy,
                    name,
                    &ctx,
                    &peers,
                    Path::new("/private/fixture/main")
                )
                .is_err()
            );
            let mut wrong = before;
            wrong[if name == "server" {
                "execution_clients"
            } else {
                "trusted_remotes"
            }] = serde_json::json!([]);
            assert!(
                configure_value(
                    wrong,
                    name,
                    &ctx,
                    &peers,
                    Path::new("/private/fixture/main")
                )
                .is_err()
            );
        }
    }
    /// The supervisor downloads all twelve immutable records into one flat directory.
    #[test]
    fn flat_twelve_record_rollup_and_red_membership_or_results() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let input = d.path().join("records");
        let output = d.path().join("output");
        fs::create_dir(&input).unwrap();
        fs::create_dir(&output).unwrap();
        for p in [&input, &output] {
            fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let ctx = records::tests::ctx();
        let context = d.path().join("context.json");
        records::write(&context, &ctx).unwrap();
        let mut ids = BTreeMap::new();
        for (i, role) in ROLES.into_iter().enumerate() {
            let id = IdentityReady {
                schema: 1,
                kind: "identity-ready".into(),
                context_sha256: ctx.digest().unwrap(),
                role: role.into(),
                nonce: format!("{:064x}", i + 1),
                client_account: format!("{:064x}", i * 2 + 1),
                server_account: format!("{:064x}", i * 2 + 2),
                identity_ready_at: ctx.created_at,
                installation: records::Installation {
                    version: ctx.version.clone(),
                    target: TARGETS[i].into(),
                    source_commit: ctx.source_commit.clone(),
                    manifest_sha256: ctx.manifests[role].clone(),
                    bootstrap_sha256: ctx.bootstrap_sha256.clone(),
                    files: [
                        "THIRD-PARTY-LICENSES",
                        "marmish",
                        "marmish-worker",
                        "marmishd",
                    ]
                    .into_iter()
                    .map(|n| {
                        (
                            n.into(),
                            records::FilePin {
                                sha256: "f".repeat(64),
                                bytes: 1,
                            },
                        )
                    })
                    .collect(),
                },
            };
            records::write(&input.join(format!("{role}-identity-ready.json")), &id).unwrap();
            ids.insert(role.into(), id);
        }
        // The exchange directory contract remains exactly three files.
        assert!(records::identities(&input, &ctx).is_ok());
        let mut servers: BTreeMap<String, ServerReady> = BTreeMap::new();
        for role in ROLES {
            let r = ServerReady {
                schema: 1,
                kind: "server-ready".into(),
                context_sha256: ctx.digest().unwrap(),
                role: role.into(),
                identity_ready_sha256: pins(&ids).unwrap(),
                server_account: ids[role].server_account.clone(),
                ready_at: ctx.created_at,
                expires_at: ctx.expires_at,
                fixture: Fixture {
                    path: format!("/private/tmp/{role}/main"),
                    sha256: ctx.tooling[ROLES.iter().position(|v| *v == role).unwrap()]
                        .files
                        .fixture
                        .sha256
                        .clone(),
                },
            };
            records::write(&input.join(format!("{role}-server-ready.json")), &r).unwrap();
            servers.insert(role.into(), r);
        }
        let mut clients: BTreeMap<String, ClientResult> = BTreeMap::new();
        for client in ROLES {
            let pairs = ROLES
                .into_iter()
                .enumerate()
                .map(|(i, server)| {
                    let f = servers[server].fixture.clone();
                    let n = nonce(&ctx, &ids, client, server).unwrap();
                    let e = fixture::expected_receipt(Path::new(&f.path), &n, i as u8, &f.sha256)
                        .unwrap();
                    Pair {
                        client: client.into(),
                        server: server.into(),
                        nonce: n,
                        vector: i as u8,
                        handle: Handle {
                            format: 1,
                            profile: "client".into(),
                            remote: ids[server].server_account.clone(),
                            session: "a".repeat(32),
                            invocation: format!("{:032x}", i + 1),
                            submit_sha256: "b".repeat(64),
                        },
                        group_submit_sha256: "e".repeat(64),
                        fixture: f,
                        argv_sha256: e.argv_sha256,
                        stdin_bytes: e.stdin_bytes,
                        stdin_sha256: e.stdin_sha256,
                        stdout_bytes: e.stdout_bytes,
                        stdout_sha256: e.stdout_sha256,
                        stderr_bytes: e.stderr_bytes,
                        stderr_sha256: e.stderr_sha256,
                        exit: 7,
                        wall_ns: 1_000_000_000,
                    }
                })
                .collect();
            let c = ClientResult {
                schema: 1,
                kind: "client-result".into(),
                context_sha256: ctx.digest().unwrap(),
                role: client.into(),
                identity_ready_sha256: pins(&ids).unwrap(),
                server_ready_sha256: servers
                    .iter()
                    .map(|(r, s)| (r.clone(), records::digest(&records::canonical(s).unwrap())))
                    .collect(),
                pairs,
                complete: true,
            };
            records::write(&input.join(format!("{client}-client-result.json")), &c).unwrap();
            clients.insert(client.into(), c);
        }
        for server in ROLES {
            let receipts = ROLES
                .into_iter()
                .map(|c| {
                    let p = clients[c]
                        .pairs
                        .iter()
                        .find(|p| p.server == server)
                        .unwrap();
                    (
                        c.into(),
                        fixture::expected_receipt(
                            Path::new(&p.fixture.path),
                            &p.nonce,
                            p.vector,
                            &p.fixture.sha256,
                        )
                        .unwrap(),
                    )
                })
                .collect();
            records::write(
                &input.join(format!("{server}-server-result.json")),
                &ServerResult {
                    schema: 1,
                    kind: "server-result".into(),
                    context_sha256: ctx.digest().unwrap(),
                    role: server.into(),
                    identity_ready_sha256: pins(&ids).unwrap(),
                    server_ready_sha256: records::digest(
                        &records::canonical(&servers[server]).unwrap(),
                    ),
                    receipts,
                    admissions: 3,
                    engine_cleanup_complete: true,
                    server_stop_complete: true,
                    client_stop_complete: true,
                    complete: true,
                },
            )
            .unwrap();
        }
        assert!(records::identities(&input, &ctx).is_err());
        let red = |job_result| {
            let fresh = tempfile::Builder::new()
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap();
            assert!(rollup(&context, &input, job_result, fresh.path()).is_err());
        };
        rollup(&context, &input, "success", &output).unwrap();
        let result: serde_json::Value =
            records::read(&output.join("matrix-rollup.json"), 65536).unwrap();
        assert_eq!(result["sample_count"], 9);
        assert_eq!(result["cells"].as_array().unwrap().len(), 9);
        assert!(
            String::from_utf8(
                records::read_bytes(&output.join("matrix-rollup.md"), 65536).unwrap()
            )
            .unwrap()
            .contains("no percentile estimates")
        );
        red("failure");
        let client_path = input.join("linux-amd64-client-result.json");
        let original = records::read_bytes(&client_path, 1048576).unwrap();
        let request_digest = "b".repeat(64);
        for wrong in [None, Some("invalid"), Some(request_digest.as_str())] {
            let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
            if let Some(wrong) = wrong {
                value["pairs"][0]["group_submit_sha256"] = wrong.into();
            } else {
                value["pairs"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("group_submit_sha256");
            }
            fs::remove_file(&client_path).unwrap();
            records::write(&client_path, &value).unwrap();
            red("success");
        }
        fs::remove_file(&client_path).unwrap();
        records::write_raw(&client_path, &original).unwrap();
        let fresh = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        rollup(&context, &input, "success", fresh.path()).unwrap();
        let mut unpinned = servers["linux-amd64"].clone();
        unpinned.fixture.sha256 = "0".repeat(64);
        assert!(validate_ready(&unpinned, &ctx, &ids, "linux-amd64").is_err());
        records::write(&input.join("unexpected.json"), &true).unwrap();
        red("success");
        fs::remove_file(input.join("unexpected.json")).unwrap();
        let missing = input.join("linux-amd64-server-result.json");
        let saved = records::read_bytes(&missing, 1048576).unwrap();
        fs::remove_file(&missing).unwrap();
        red("success");
        records::write_raw(&missing, &saved).unwrap();
        let path = input.join("linux-arm64-server-result.json");
        let mut failed: ServerResult = records::read(&path, 1048576).unwrap();
        failed.complete = false;
        fs::remove_file(&path).unwrap();
        records::write(&path, &failed).unwrap();
        red("success");
    }
    #[test]
    fn nonce_receipt_binds_executable_arguments_and_streams() {
        let n = "12".repeat(32);
        let f = Path::new("/private/tmp/f/main");
        let a = fixture::expected_receipt(f, &n, 0, &"a".repeat(64)).unwrap();
        let b = fixture::expected_receipt(f, &n, 1, &"a".repeat(64)).unwrap();
        let c =
            fixture::expected_receipt(Path::new("/private/tmp/other/main"), &n, 0, &"a".repeat(64))
                .unwrap();
        assert_ne!(a.argv_sha256, b.argv_sha256);
        assert_ne!(a.argv_sha256, c.argv_sha256);
        assert_ne!(a.stdout_sha256, b.stdout_sha256);
        assert_ne!(a.stdout_sha256, a.stderr_sha256);
        assert_eq!(a.exit, 7);
    }
}
