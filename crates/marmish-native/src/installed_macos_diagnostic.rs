//! Mixed-source, nonqualifying diagnostic. Never constructs a candidate Context.
use super::{
    autoexit, edge_rsync, matrix, prepare, process, records, streaming_cases, streaming_cleanup,
    streaming_fixture,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    sync::watch,
    time::Instant,
};
const CANDIDATE: &str = "571d98b77780d265a2fb29d84ae160fec72ba37b";
const RUN: &str = "42358105270";
const TARGET: &str = "aarch64-apple-darwin";
const RELAY: &str = "wss://relay.us.whitenoise.chat";
const CAP: usize = 1024 * 1024;
#[derive(clap::Args)]
pub(super) struct Options {
    #[arg(long)]
    pub assets: PathBuf,
    #[arg(long)]
    pub bootstrap: PathBuf,
    #[arg(long)]
    pub fixture: PathBuf,
    #[arg(long)]
    pub workspace: PathBuf,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long)]
    pub pins: PathBuf,
    #[arg(long)]
    pub concurrent: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FilePin {
    sha256: String,
    bytes: u64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactPin {
    id: u64,
    name: String,
    sha256: String,
    bytes: u64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Artifacts {
    product: ArtifactPin,
    tooling: ArtifactPin,
    bootstrap: ArtifactPin,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Pins {
    schema: u8,
    kind: String,
    qualification: bool,
    repository: String,
    candidate_run_id: String,
    candidate_attempt: String,
    candidate_commit: String,
    harness_commit: String,
    target: String,
    version: String,
    old_harness_sha256: String,
    artifacts: Artifacts,
    manifest: FilePin,
    tooling_record: FilePin,
    bootstrap: FilePin,
    fixture: FilePin,
}
fn bad() -> Error {
    Error::Invalid("macos_diagnostic")
}
fn cutoff(overall: Instant, seconds: u64) -> Instant {
    overall.min(Instant::now() + Duration::from_secs(seconds))
}
fn directory(path: &Path) -> Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)?;
    crate::identity::check_dir(path)
}
fn pinned(path: &Path, pin: &FilePin, limit: u64) -> Result<Vec<u8>> {
    let bytes = records::read_bytes(path, limit)?;
    if bytes.len() as u64 != pin.bytes || records::digest(&bytes) != pin.sha256 {
        return Err(bad());
    }
    Ok(bytes)
}
impl Pins {
    fn validate(&self) -> Result<()> {
        let compiled = crate::native_manifest::compiled_release_metadata();
        if self.schema != 1
            || self.kind != "macos-stream-diagnostic-pins"
            || self.qualification
            || self.repository != "Epiphytic/marmish"
            || self.candidate_run_id != RUN
            || self.candidate_attempt != "1"
            || self.candidate_commit != CANDIDATE
            || self.target != TARGET
            || self.harness_commit != compiled.commit
            || !records::hexadecimal(&self.harness_commit, 40)
            || !records::hexadecimal(&self.old_harness_sha256, 64)
            || self.version.is_empty()
            || self.version.len() > 64
        {
            return Err(bad());
        }
        let expected = [
            (
                &self.artifacts.product,
                13570516008,
                47988691,
                "v02-candidate-aarch64-apple-darwin-42358105270-1",
                "f6c27bfab5b3dd2370eb38fa93f99f1c48ae906a7f04c6f45a59bf2bc758f970",
            ),
            (
                &self.artifacts.tooling,
                13570605471,
                9460623,
                "v02-installed-tools-aarch64-apple-darwin-42358105270-1",
                "6cf4a04d8ee549bd86d9f7a256fefd7cfdcdb68eaccc189ba5c03b2c1315895c",
            ),
            (
                &self.artifacts.bootstrap,
                13570810425,
                3543,
                "v02-pinned-bootstrap-42358105270-1",
                "974f86f238a8798ad2e565f300930070073cc5097748df07c30a9626c871d2ba",
            ),
        ];
        for (a, id, bytes, name, hash) in expected {
            if a.id != id || a.bytes != bytes || a.name != name || a.sha256 != hash {
                return Err(bad());
            }
        }
        for p in [
            &self.manifest,
            &self.tooling_record,
            &self.bootstrap,
            &self.fixture,
        ] {
            if p.bytes == 0 || p.bytes > 268435456 || !records::hexadecimal(&p.sha256, 64) {
                return Err(bad());
            }
        }
        Ok(())
    }
}
#[derive(Default)]
struct Pipe {
    bytes: Vec<u8>,
    overflow: bool,
    io_error: bool,
}
async fn read_pipe(mut source: impl AsyncRead + Unpin, state: &mut Pipe) {
    let mut block = [0u8; 8192];
    loop {
        match source.read(&mut block).await {
            Ok(0) => break,
            Ok(n) => {
                let keep = n.min(CAP.saturating_sub(state.bytes.len()));
                state.bytes.extend_from_slice(&block[..keep]);
                state.overflow |= keep < n;
            }
            Err(_) => {
                state.io_error = true;
                break;
            }
        }
    }
}
#[derive(Default)]
struct Capture {
    status: Option<ExitStatus>,
    stdout: Pipe,
    stderr: Pipe,
    written: usize,
    epipe: bool,
    input_error: bool,
    timed_out: bool,
    interrupted: bool,
    cleanup: bool,
}
async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    if *receiver.borrow() {
        return;
    }
    while receiver.changed().await.is_ok() {
        if *receiver.borrow() {
            return;
        }
    }
    std::future::pending::<()>().await;
}
async fn capture(
    executable: &Path,
    args: &[OsString],
    workspace: &Path,
    env: &process::Environment,
    input: &[u8],
    deadline: Instant,
    mut signal: watch::Receiver<bool>,
) -> Result<Capture> {
    if *signal.borrow() || Instant::now() >= deadline {
        return Ok(Capture {
            interrupted: *signal.borrow(),
            timed_out: Instant::now() >= deadline,
            cleanup: true,
            ..Capture::default()
        });
    }
    let mut command = process::command(executable, args, workspace, env);
    command.stdin(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return Ok(Capture {
                input_error: true,
                cleanup: true,
                ..Capture::default()
            });
        }
    };
    let mut stdin = child.stdin.take().ok_or_else(bad)?;
    let out = child.stdout.take().ok_or_else(bad)?;
    let err = child.stderr.take().ok_or_else(bad)?;
    let mut result = Capture::default();
    let mut observation_error = false;
    {
        let feed = async {
            while result.written < input.len() {
                match stdin.write(&input[result.written..]).await {
                    Ok(0) => {
                        result.input_error = true;
                        break;
                    }
                    Ok(n) => result.written += n,
                    Err(e) => {
                        result.epipe = e.kind() == std::io::ErrorKind::BrokenPipe;
                        result.input_error = !result.epipe;
                        break;
                    }
                }
            }
            drop(stdin);
        };
        let output = read_pipe(out, &mut result.stdout);
        let error = read_pipe(err, &mut result.stderr);
        let pipes = async {
            tokio::join!(feed, output, error);
        };
        tokio::pin!(pipes);
        let mut pipes_done = false;
        let mut exit_seen = false;
        let mut drain_deadline = deadline;
        loop {
            if !exit_seen
                && match edge_rsync::leader_exited(&child) {
                    Ok(value) => value,
                    Err(_) => {
                        observation_error = true;
                        break;
                    }
                }
            {
                exit_seen = true;
                drain_deadline = cutoff(deadline, 2);
            }
            if exit_seen && pipes_done {
                break;
            }
            tokio::select! {
                biased;
                _ = cancelled(&mut signal) => { result.interrupted = true; break; }
                _ = tokio::time::sleep_until(drain_deadline) => { result.timed_out = true; break; }
                _ = &mut pipes, if !pipes_done => { pipes_done = true; }
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    }
    result.input_error |= observation_error;
    // All pipe futures are now dropped; the owned unreaped leader still fences PGID.
    match edge_rsync::reap_owned_group(&mut child).await {
        Ok(status) => {
            result.status = Some(status);
            result.cleanup = true;
        }
        Err(_) => result.cleanup = false,
    }
    Ok(result)
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    case: String,
    stage: String,
    failure: Option<String>,
    exit: Option<i32>,
    signal: Option<i32>,
    input_bytes: usize,
    input_sha256: String,
    stdin_written: usize,
    stdout_bytes: usize,
    stdout_sha256: String,
    stderr_bytes: usize,
    stderr_sha256: String,
    input_epipe: bool,
    capture_overflow: bool,
    capture_io: bool,
    timed_out: bool,
    interrupted: bool,
    owned_group_clean: bool,
    stream_present: bool,
    binding_equal: bool,
    client_terminal: Option<marmish_stream::wire::Terminal>,
    server_terminal: Option<marmish_stream::wire::Terminal>,
    receipt_present: bool,
    receipt_valid: bool,
    output_match: bool,
    offsets_available: bool,
    wall_ms: u64,
}
impl Observation {
    fn captured(case: &str, input: &[u8], c: &Capture, start: Instant) -> Self {
        Self {
            case: case.into(),
            stage: "stream_capture".into(),
            failure: None,
            exit: c.status.and_then(|s| s.code()),
            signal: c.status.and_then(|s| s.signal()),
            input_bytes: input.len(),
            input_sha256: records::digest(input),
            stdin_written: c.written,
            stdout_bytes: c.stdout.bytes.len(),
            stdout_sha256: records::digest(&c.stdout.bytes),
            stderr_bytes: c.stderr.bytes.len(),
            stderr_sha256: records::digest(&c.stderr.bytes),
            input_epipe: c.epipe,
            capture_overflow: c.stdout.overflow || c.stderr.overflow,
            capture_io: c.stdout.io_error || c.stderr.io_error || c.input_error,
            timed_out: c.timed_out,
            interrupted: c.interrupted,
            owned_group_clean: c.cleanup,
            stream_present: false,
            binding_equal: false,
            client_terminal: None,
            server_terminal: None,
            receipt_present: false,
            receipt_valid: false,
            output_match: false,
            offsets_available: false,
            wall_ms: start.elapsed().as_millis().min(u64::MAX as u128) as u64,
        }
    }
    fn fail(&mut self, category: &str) {
        if self.failure.is_none() {
            self.failure = Some(category.into());
        }
    }
}
struct State {
    client: Option<matrix::Daemon>,
    server: Option<matrix::Daemon>,
    environment: process::Environment,
    accounts: Vec<String>,
    fixture: PathBuf,
}
#[derive(Default, Serialize)]
struct Finalized {
    client_cancel: bool,
    server_cancel: bool,
    client_idle: bool,
    server_idle: bool,
    client_stop: bool,
    server_stop: bool,
}
#[derive(Serialize)]
struct Report<'a> {
    schema: u8,
    kind: &'static str,
    qualification: bool,
    same_host_only: bool,
    relay: &'static str,
    pins: Option<&'a Pins>,
    probe_sha256: String,
    stage: String,
    failure: Option<String>,
    observations: Vec<Observation>,
    cleanup: Finalized,
    passed: bool,
}
fn write_private(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
async fn invoke(
    state: &State,
    w: &Path,
    args: &[OsString],
    deadline: Instant,
    signal: watch::Receiver<bool>,
) -> Result<Capture> {
    capture(
        &w.join("bin/marmish"),
        args,
        w,
        &state.environment,
        &[],
        deadline,
        signal,
    )
    .await
}
fn base(w: &Path, name: &str) -> Vec<OsString> {
    vec![
        "--state-dir".into(),
        w.join("profiles").into_os_string(),
        "--identity".into(),
        name.into(),
        "--key-storage".into(),
        "file".into(),
        "--tor".into(),
        "off".into(),
        "--relay".into(),
        RELAY.into(),
    ]
}
fn success(c: &Capture) -> bool {
    c.status.is_some_and(|s| s.success())
        && c.cleanup
        && !c.timed_out
        && !c.interrupted
        && !c.stdout.overflow
        && !c.stderr.overflow
        && !c.stdout.io_error
        && !c.stderr.io_error
        && !c.input_error
}
#[derive(Clone, Copy)]
struct ProfileSpec<'a> {
    identity_name: &'a str,
    role: &'a str,
}
impl ProfileSpec<'_> {
    fn validate(self) -> Result<()> {
        crate::identity::validate_name(self.identity_name)?;
        if !matches!(self.role, "client" | "server") {
            return Err(bad());
        }
        Ok(())
    }
}
fn setup_args(w: &Path, spec: ProfileSpec<'_>, peers: Option<&[String]>) -> Result<Vec<OsString>> {
    spec.validate()?;
    let mut args = base(w, spec.identity_name);
    args.push("setup".into());
    match peers {
        None => args.extend(["--role".into(), spec.role.into(), "--no-grants".into()]),
        Some(peers) => {
            args.push("--update".into());
            for peer in peers {
                args.extend([
                    if spec.role == "server" {
                        "--grant"
                    } else {
                        "--trust"
                    }
                    .into(),
                    records::public_destination(peer)?.into(),
                ]);
            }
        }
    }
    Ok(args)
}
fn setup_valid_named(s: &prepare::Setup, spec: ProfileSpec<'_>, new: bool, granted: bool) -> bool {
    s.schema == 1
        && s.identity == spec.identity_name
        && s.backend == "file"
        && s.role == spec.role
        && s.inbound_enabled == (spec.role == "server" && granted)
        && s.config_changed
        && s.identity_created == new
        && s.owner_opened == new
        && s.engine_stop_complete
        && s.mdk_stop_complete
        && s.runtime_cleanup_complete
}
fn setup_valid(s: &prepare::Setup, name: &str, new: bool, granted: bool) -> bool {
    setup_valid_named(
        s,
        ProfileSpec {
            identity_name: name,
            role: name,
        },
        new,
        granted,
    )
}
async fn install_image(
    o: &Options,
    pins: &Pins,
    environment: &process::Environment,
    deadline: Instant,
    signal: watch::Receiver<bool>,
) -> Result<PathBuf> {
    let expected = crate::native_manifest::hex32(&pins.manifest.sha256)?;
    let manifest = crate::native_manifest::verify_directory(
        &o.assets,
        crate::native_manifest::AssetNaming::VersionedDownloads,
        expected,
    )?;
    if manifest.commit != CANDIDATE || manifest.target != TARGET || manifest.version != pins.version
    {
        return Err(bad());
    }
    pinned(&o.bootstrap, &pins.bootstrap, 131072)?;
    let fixture = pinned(&o.fixture, &pins.fixture, 268435456)?;
    let args = vec![
        o.bootstrap.clone().into_os_string(),
        "--asset-dir".into(),
        o.assets.clone().into_os_string(),
        "--prefix".into(),
        o.workspace.join("install").into_os_string(),
        "--bin-dir".into(),
        o.workspace.join("bin").into_os_string(),
        "--state-dir".into(),
        o.workspace.join("profiles").into_os_string(),
        "--key-storage".into(),
        "file".into(),
    ];
    let c = capture(
        Path::new("/bin/sh"),
        &args,
        &o.workspace,
        environment,
        &[],
        cutoff(deadline, 120),
        signal.clone(),
    )
    .await?;
    if c.interrupted {
        return Err(Error::Invalid("interrupted"));
    }
    if !success(&c) {
        return Err(Error::Invalid("install_failed"));
    }
    let executable = o.workspace.join("bin/marmish").canonicalize()?;
    let root = executable.parent().ok_or_else(bad)?;
    if !root.starts_with(o.workspace.join("install/versions"))
        || crate::native_manifest::verify_directory(
            root,
            crate::native_manifest::AssetNaming::InstalledFixedNames,
            expected,
        )? != manifest
    {
        return Err(bad());
    }
    directory(&o.workspace.join("fixture"))?;
    directory(&o.workspace.join("fixture/receipts"))?;
    let fixture_path = o.workspace.join("fixture/main");
    write_private(&fixture_path, &fixture, 0o700)?;
    fixture_path.canonicalize().map_err(Error::from)
}
async fn install_and_setup(
    o: &Options,
    pins: &Pins,
    state: &mut State,
    deadline: Instant,
    signal: watch::Receiver<bool>,
    stage: &mut String,
) -> Result<()> {
    state.fixture = install_image(o, pins, &state.environment, deadline, signal.clone()).await?;
    *stage = "setup".into();
    for name in ["client", "server"] {
        let args = setup_args(
            &o.workspace,
            ProfileSpec {
                identity_name: name,
                role: name,
            },
            None,
        )?;
        let c = invoke(
            state,
            &o.workspace,
            &args,
            cutoff(deadline, 120),
            signal.clone(),
        )
        .await?;
        if c.interrupted {
            return Err(Error::Invalid("interrupted"));
        }
        let setup: prepare::Setup = serde_json::from_slice(&c.stdout.bytes).map_err(|_| bad())?;
        if !success(&c) || !setup_valid(&setup, name, true, false) {
            return Err(Error::Invalid("setup_failed"));
        }
        state
            .accounts
            .push(hex::encode(crate::command_config::account(&setup.npub)?));
    }
    for (i, name) in ["client", "server"].into_iter().enumerate() {
        let peer = &state.accounts[1 - i];
        let args = setup_args(
            &o.workspace,
            ProfileSpec {
                identity_name: name,
                role: name,
            },
            Some(std::slice::from_ref(peer)),
        )?;
        let c = invoke(
            state,
            &o.workspace,
            &args,
            cutoff(deadline, 120),
            signal.clone(),
        )
        .await?;
        if c.interrupted {
            return Err(Error::Invalid("interrupted"));
        }
        let setup: prepare::Setup = serde_json::from_slice(&c.stdout.bytes).map_err(|_| bad())?;
        if !success(&c)
            || !setup_valid(&setup, name, false, name == "server")
            || hex::encode(crate::command_config::account(&setup.npub)?) != state.accounts[i]
        {
            return Err(Error::Invalid("setup_failed"));
        }
        let path = o.workspace.join("profiles").join(name).join("config.json");
        let config: serde_json::Value = records::read(&path, 1048576)?;
        let bytes = configure_value(config, name, peer, &o.workspace, &state.fixture)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    Ok(())
}
fn configure_value(
    config: serde_json::Value,
    name: &str,
    peer: &str,
    workspace: &Path,
    fixture: &Path,
) -> Result<Vec<u8>> {
    configure_peers(config, name, &[peer.to_owned()], workspace, fixture)
}
fn configure_peers(
    mut config: serde_json::Value,
    name: &str,
    peers: &[String],
    workspace: &Path,
    fixture: &Path,
) -> Result<Vec<u8>> {
    if peers.is_empty()
        || peers.windows(2).any(|w| w[0] >= w[1])
        || peers.iter().any(|p| !records::hexadecimal(p, 64))
    {
        return Err(bad());
    }
    if !matches!(name, "client" | "server") {
        return Err(bad());
    }

    let peers = serde_json::json!(peers);
    let empty = serde_json::json!([]);
    if config["role"] != name
        || config["relays"] != serde_json::json!([RELAY])
        || config["tor"] != "off"
        || config["aliases"] != serde_json::json!({})
        || config["command_policy"] != serde_json::json!({"version":1})
        || config["execution_clients"] != if name == "server" { &peers } else { &empty }.clone()
        || config["trusted_remotes"] != if name == "client" { &peers } else { &empty }.clone()
    {
        return Err(bad());
    }
    config["cwd"] = serde_json::json!(workspace.to_str().ok_or_else(bad)?);
    config["command_policy"]["defaults"]["allow_exec"] = if name == "server" {
        serde_json::json!([fixture.to_str().ok_or_else(bad)?])
    } else {
        empty
    };
    config["limits"] = serde_json::json!({"max_uptime":1200,"active_per_principal":4});
    let bytes = records::canonical(&config)?;
    crate::command_config::CommandConfig::from_bytes(&bytes)?;
    Ok(bytes)
}
fn nonce() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| bad())?;
    Ok(hex::encode(bytes))
}
fn bound(c: &streaming_cleanup::Entry, s: &streaming_cleanup::Entry, accounts: &[String]) -> bool {
    accounts.len() == 2
        && c.direction == "client"
        && s.direction == "server"
        && c.client == accounts[0]
        && s.client == accounts[0]
        && c.server == accounts[1]
        && s.server == accounts[1]
        && c.stream == s.stream
        && c.session == s.session
        && c.group_sha256 == s.group_sha256
        && c.open_digest.is_some()
        && c.open_digest == s.open_digest
}
fn choose<'a>(
    before: &[streaming_cleanup::Entry],
    client: &'a [streaming_cleanup::Entry],
    server: &'a [streaming_cleanup::Entry],
    accounts: &[String],
    id: Option<&str>,
) -> Result<(&'a streaming_cleanup::Entry, &'a streaming_cleanup::Entry)> {
    let matches = client
        .iter()
        .filter(|c| {
            id.map_or_else(
                || !before.iter().any(|b| b.stream == c.stream),
                |id| c.stream == id,
            )
        })
        .filter_map(|c| {
            server
                .iter()
                .find(|s| bound(c, s, accounts))
                .map(|s| (c, s))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [pair] => Ok(*pair),
        [] => Err(Error::Invalid("list_absent")),
        _ => Err(Error::Invalid("list_ambiguous")),
    }
}
fn check_capture(
    obs: &mut Observation,
    c: &Capture,
    nonce: &str,
    payload_stderr: &[u8],
) -> Result<()> {
    obs.output_match = c.stdout.bytes
        == streaming_fixture::bytes(nonce, 43, streaming_fixture::STDOUT_BYTES)?
        && payload_stderr == streaming_fixture::bytes(nonce, 149, streaming_fixture::STDERR_BYTES)?;
    if c.interrupted {
        obs.fail("interrupted");
    } else if c.timed_out {
        obs.fail("budget_exhausted");
    } else if !c.cleanup {
        obs.fail("cleanup_unconfirmed");
    } else if obs.capture_overflow {
        obs.fail("capture_overflow");
    } else if obs.capture_io {
        obs.fail("capture_io");
    } else if c.epipe {
        obs.fail("input_epipe");
    } else if obs.signal.is_some() {
        obs.fail("child_signal");
    } else if obs.exit != Some(7) {
        obs.fail("child_nonzero");
    } else if !obs.output_match {
        obs.fail("hash_mismatch");
    }
    Ok(())
}
fn check_receipt(obs: &mut Observation, state: &State, pins: &Pins, nonce: &str) -> Result<()> {
    check_receipt_named(obs, &state.fixture, pins, nonce, "macos-arm64")
}
fn check_receipt_named(
    obs: &mut Observation,
    fixture: &Path,
    pins: &Pins,
    nonce: &str,
    role: &str,
) -> Result<()> {
    let path = streaming_fixture::receipt_path(fixture, nonce)?;
    obs.receipt_present = path.try_exists()?;
    if obs.receipt_present {
        let actual: Result<streaming_fixture::Receipt> = records::read(&path, 16384);
        obs.receipt_valid = actual.is_ok_and(|a| {
            streaming_fixture::receipt(fixture, &pins.fixture.sha256, nonce, role)
                .is_ok_and(|e| a == e)
        });
    }
    if !obs.receipt_present {
        obs.fail("receipt_absent");
    } else if !obs.receipt_valid {
        obs.fail("receipt_malformed");
    }
    Ok(())
}
async fn direct(
    o: &Options,
    pins: &Pins,
    state: &State,
    deadline: Instant,
    signal: watch::Receiver<bool>,
) -> Result<Observation> {
    let n = nonce()?;
    let args = streaming_fixture::arguments(&n, "macos-arm64")?;
    let input = streaming_fixture::bytes(&n, 7, streaming_fixture::STDIN_BYTES)?;
    let start = Instant::now();
    let c = capture(
        &state.fixture,
        &args,
        &o.workspace,
        &state.environment,
        &input,
        cutoff(deadline, 15),
        signal,
    )
    .await?;
    let mut obs = Observation::captured("fixture_direct", &input, &c, start);
    obs.stage = "fixture_direct".into();
    check_capture(&mut obs, &c, &n, &c.stderr.bytes)?;
    check_receipt(&mut obs, state, pins, &n)?;
    Ok(obs)
}
struct StreamCase {
    nonce: String,
    case: String,
    input: Vec<u8>,
    args: Vec<OsString>,
}
fn stream_case(o: &Options, state: &State, case: &str, compress: bool) -> Result<StreamCase> {
    let nonce = nonce()?;
    let args = stream_args(
        &o.workspace,
        &state.client.as_ref().ok_or_else(bad)?.socket,
        &state.accounts[1],
        &nonce,
        compress,
    )?;
    Ok(StreamCase {
        input: streaming_fixture::bytes(&nonce, 7, streaming_fixture::STDIN_BYTES)?,
        nonce,
        case: case.into(),
        args,
    })
}
fn stream_args(
    workspace: &Path,
    socket: &Path,
    account: &str,
    nonce: &str,
    compress: bool,
) -> Result<Vec<OsString>> {
    stream_args_named(
        workspace,
        socket,
        "client",
        account,
        nonce,
        "macos-arm64",
        compress,
    )
}
fn stream_args_named(
    workspace: &Path,
    socket: &Path,
    identity: &str,
    account: &str,
    nonce: &str,
    role: &str,
    compress: bool,
) -> Result<Vec<OsString>> {
    ProfileSpec {
        identity_name: identity,
        role: "client",
    }
    .validate()?;
    let mut args = matrix::base(workspace, identity, socket, &[RELAY.into()]);
    args.extend([
        "--no-autostart".into(),
        "stream".into(),
        records::public_destination(account)?.into(),
    ]);
    if compress {
        args.push("--compress".into());
    }
    args.extend(["--".into(), "./fixture/main".into()]);
    args.extend(streaming_fixture::arguments(nonce, role)?);
    Ok(args)
}
fn cancel_args_named(
    w: &Path,
    name: &str,
    socket: &Path,
    entry: &streaming_cleanup::Entry,
) -> Result<Vec<OsString>> {
    let mut args = matrix::base(w, name, socket, &[RELAY.into()]);
    args.extend(streaming_cases::cancel_args(&entry.server, &entry.stream)?);
    Ok(args)
}
async fn entries(
    state: &mut State,
    deadline: Instant,
) -> Result<(Vec<streaming_cleanup::Entry>, Vec<streaming_cleanup::Entry>)> {
    let c = state.client.as_mut().ok_or_else(bad)?;
    let clients = streaming_cleanup::entries(&mut c.child, &c.socket, deadline).await?;
    let s = state.server.as_mut().ok_or_else(bad)?;
    let servers = streaming_cleanup::entries(&mut s.child, &s.socket, deadline).await?;
    Ok((clients, servers))
}
struct EvidenceWindow<'a> {
    before: &'a [streaming_cleanup::Entry],
    allow_fallback: bool,
    deadline: Instant,
}
fn inspect_capture(
    obs: &mut Observation,
    case: &StreamCase,
    capture: &Capture,
) -> Result<Option<String>> {
    let prefix = streaming_cases::stderr(&capture.stderr.bytes);
    obs.stream_present = prefix.is_ok();
    let (id, payload) = match &prefix {
        Ok((id, payload)) => (Some(id.as_str()), *payload),
        Err(_) => (None, capture.stderr.bytes.as_slice()),
    };
    check_capture(obs, capture, &case.nonce, payload)?;
    if id.is_none() {
        // Missing framing explains the payload mismatch; retain stronger capture failures.
        if obs.failure.as_deref() == Some("hash_mismatch") {
            obs.failure = None;
        }
        obs.fail(if capture.stderr.bytes.starts_with(b"marmish stream: ") {
            "stream_id_malformed"
        } else {
            "stream_id_absent"
        });
    }
    Ok(id.map(str::to_owned))
}
fn inspect_journals(
    obs: &mut Observation,
    before: &[streaming_cleanup::Entry],
    client: &[streaming_cleanup::Entry],
    server: &[streaming_cleanup::Entry],
    accounts: &[String],
    id: Option<&str>,
    allow_fallback: bool,
) {
    let binding = if id.is_none() && !allow_fallback {
        Err(Error::Invalid("list_ambiguous"))
    } else {
        choose(before, client, server, accounts, id)
    };
    match binding {
        Ok((c, s)) => {
            obs.binding_equal = true;
            obs.client_terminal = c.terminal;
            obs.server_terminal = s.terminal;
            if c.terminal != Some(marmish_stream::wire::Terminal::Exited { code: 7 })
                || s.terminal != c.terminal
                || c.active
                || s.active
                || c.cancel_requested
                || s.cancel_requested
            {
                obs.fail("terminal_mismatch");
            }
        }
        Err(Error::Invalid("list_ambiguous")) => obs.fail("list_ambiguous"),
        Err(_) => obs.fail("list_absent"),
    }
}
async fn inspect(
    pins: &Pins,
    state: &mut State,
    case: &StreamCase,
    capture: &Capture,
    start: Instant,
    window: EvidenceWindow<'_>,
) -> Result<Observation> {
    let mut obs = Observation::captured(&case.case, &case.input, capture, start);
    let prefix = inspect_capture(&mut obs, case, capture)?;
    let id = prefix.as_deref();
    if obs.failure.is_none() {
        obs.stage = "client_journal".into();
    }
    match entries(state, window.deadline).await {
        Ok((client, server)) => {
            inspect_journals(
                &mut obs,
                window.before,
                &client,
                &server,
                &state.accounts,
                id,
                window.allow_fallback,
            );
        }
        Err(_) => obs.fail("list_absent"),
    }
    if obs.failure.is_none() {
        obs.stage = "receipt".into();
    }
    check_receipt(&mut obs, state, pins, &case.nonce)?;
    Ok(obs)
}
async fn work(
    o: &Options,
    pins: &Pins,
    state: &mut State,
    deadline: Instant,
    signal: watch::Receiver<bool>,
    stage: &mut String,
    observations: &mut Vec<Observation>,
) -> Result<()> {
    *stage = "install".into();
    install_and_setup(o, pins, state, deadline, signal.clone(), stage).await?;
    *stage = "fixture_direct".into();
    observations.push(direct(o, pins, state, deadline, signal.clone()).await?);
    let cs = autoexit::socket(&o.workspace, "client", &state.environment)?;
    let ss = autoexit::socket(&o.workspace, "server", &state.environment)?;
    if cs == ss {
        return Err(bad());
    }
    let relays = vec![RELAY.into()];
    *stage = "server_ready".into();
    state.server = Some(matrix::Daemon::start(
        &o.workspace,
        "server",
        ss,
        &state.environment,
        &relays,
        1200,
    )?);
    {
        let ready = matrix::ready(
            state.server.as_mut().ok_or_else(bad)?,
            cutoff(deadline, 180),
        );
        tokio::pin!(ready);
        let mut receiver = signal.clone();
        tokio::select! {biased;_ = cancelled(&mut receiver)=>return Err(Error::Invalid("interrupted")),r=&mut ready=>r?}
    }
    *stage = "client_ready".into();
    state.client = Some(matrix::Daemon::start(
        &o.workspace,
        "client",
        cs,
        &state.environment,
        &relays,
        1200,
    )?);
    {
        let ready = matrix::ready(
            state.client.as_mut().ok_or_else(bad)?,
            cutoff(deadline, 180),
        );
        tokio::pin!(ready);
        let mut receiver = signal.clone();
        tokio::select! {biased;_ = cancelled(&mut receiver)=>return Err(Error::Invalid("interrupted")),r=&mut ready=>r?}
    }
    for (name, compress) in [("raw", false), ("zlib", true)] {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        *stage = "stream_capture".into();
        let before = entries(state, deadline).await?.0;
        let case = stream_case(o, state, name, compress)?;
        let start = Instant::now();
        let c = capture(
            &o.workspace.join("bin/marmish"),
            &case.args,
            &o.workspace,
            &state.environment,
            &case.input,
            cutoff(deadline, 180),
            signal.clone(),
        )
        .await?;
        observations.push(
            inspect(
                pins,
                state,
                &case,
                &c,
                start,
                EvidenceWindow {
                    before: &before,
                    allow_fallback: true,
                    deadline,
                },
            )
            .await?,
        );
        if *signal.borrow() {
            return Err(Error::Invalid("interrupted"));
        }
    }
    if o.concurrent && Instant::now() < deadline {
        let before = entries(state, deadline).await?.0;
        let cases = [
            stream_case(o, state, "concurrent-1", true)?,
            stream_case(o, state, "concurrent-2", true)?,
            stream_case(o, state, "concurrent-3", true)?,
        ];
        let end = cutoff(deadline, 240);
        let start = Instant::now();
        let exe = o.workspace.join("bin/marmish");
        let (a, b, c) = tokio::join!(
            capture(
                &exe,
                &cases[0].args,
                &o.workspace,
                &state.environment,
                &cases[0].input,
                end,
                signal.clone()
            ),
            capture(
                &exe,
                &cases[1].args,
                &o.workspace,
                &state.environment,
                &cases[1].input,
                end,
                signal.clone()
            ),
            capture(
                &exe,
                &cases[2].args,
                &o.workspace,
                &state.environment,
                &cases[2].input,
                end,
                signal.clone()
            )
        );
        for (case, c) in cases.iter().zip([a, b, c]) {
            observations.push(
                inspect(
                    pins,
                    state,
                    case,
                    &c?,
                    start,
                    EvidenceWindow {
                        before: &before,
                        allow_fallback: false,
                        deadline,
                    },
                )
                .await?,
            );
        }
    }
    Ok(())
}
async fn idle(daemon: &mut matrix::Daemon, deadline: Instant) -> bool {
    loop {
        if streaming_cleanup::status(&mut daemon.child, &daemon.socket, deadline)
            .await
            .is_ok_and(|s| s.validate_idle().is_ok())
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep_until(cutoff(deadline, 1)).await;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupStep {
    ClientCancel,
    ServerCancel,
    ClientIdle,
    ServerIdle,
    ClientStop,
    ServerStop,
}
trait CleanupActions {
    async fn perform(&mut self, step: CleanupStep) -> bool;
}
async fn finalize_actions(actions: &mut impl CleanupActions) -> Finalized {
    Finalized {
        client_cancel: actions.perform(CleanupStep::ClientCancel).await,
        server_cancel: actions.perform(CleanupStep::ServerCancel).await,
        client_idle: actions.perform(CleanupStep::ClientIdle).await,
        server_idle: actions.perform(CleanupStep::ServerIdle).await,
        client_stop: actions.perform(CleanupStep::ClientStop).await,
        server_stop: actions.perform(CleanupStep::ServerStop).await,
    }
}
struct DaemonCleanup<'a> {
    o: &'a Options,
    state: &'a mut State,
    deadline: Instant,
}
impl CleanupActions for DaemonCleanup<'_> {
    async fn perform(&mut self, step: CleanupStep) -> bool {
        let o = self.o;
        let state = &mut self.state;
        let deadline = self.deadline;
        let (_, quiet) = watch::channel(false);
        let mut result = Finalized::default();
        match step {
            CleanupStep::ClientCancel => {
                if let Some(client) = state.client.as_mut() {
                    let cancel_entries = streaming_cleanup::entries(
                        &mut client.child,
                        &client.socket,
                        cutoff(deadline, 15),
                    )
                    .await;
                    result.client_cancel = cancel_entries.is_ok();
                    if let Ok(entries) = cancel_entries {
                        for entry in entries.into_iter().filter(|e| {
                            e.direction == "client"
                                && e.open_digest.is_some()
                                && e.terminal.is_none()
                        }) {
                            match cancel_args_named(&o.workspace, "client", &client.socket, &entry)
                            {
                                Ok(args) => {
                                    let out = capture(
                                        &o.workspace.join("bin/marmish"),
                                        &args,
                                        &o.workspace,
                                        &state.environment,
                                        &[],
                                        cutoff(deadline, 45),
                                        quiet.clone(),
                                    )
                                    .await;
                                    result.client_cancel &= out.is_ok_and(|c| {
                                        success(&c)
                                            && c.stderr
                                                .bytes
                                                .windows(b"terminal state confirmed".len())
                                                .any(|b| b == b"terminal state confirmed")
                                    });
                                }
                                Err(_) => result.client_cancel = false,
                            }
                        }
                    }
                }
                result.client_cancel
            }
            CleanupStep::ServerCancel => {
                if let Some(server) = state.server.as_mut() {
                    result.server_cancel = streaming_cleanup::cancel_all(
                        &mut server.child,
                        &server.socket,
                        cutoff(deadline, 60),
                    )
                    .await
                    .is_ok();
                }
                result.server_cancel
            }
            CleanupStep::ClientIdle => {
                if let Some(client) = state.client.as_mut() {
                    result.client_idle = idle(client, cutoff(deadline, 15)).await;
                }
                result.client_idle
            }
            CleanupStep::ServerIdle => {
                if let Some(server) = state.server.as_mut() {
                    result.server_idle = idle(server, cutoff(deadline, 15)).await;
                }
                result.server_idle
            }
            CleanupStep::ClientStop => {
                if let Some(client) = state.client.take() {
                    result.client_stop = client
                        .stop_streaming(cutoff(deadline, 15), false)
                        .await
                        .is_ok();
                }
                result.client_stop
            }
            CleanupStep::ServerStop => {
                if let Some(server) = state.server.take() {
                    result.server_stop = server
                        .stop_streaming(cutoff(deadline, 15), true)
                        .await
                        .is_ok();
                }
                result.server_stop
            }
        }
    }
}
async fn finalize(o: &Options, state: &mut State, deadline: Instant) -> Finalized {
    finalize_actions(&mut DaemonCleanup { o, state, deadline }).await
}

fn finalized(c: &Finalized) -> bool {
    c.client_cancel
        && c.server_cancel
        && c.client_idle
        && c.server_idle
        && c.client_stop
        && c.server_stop
}
fn category(stage: &str, error: &Error) -> &'static str {
    match error {
        Error::Invalid("interrupted") => "interrupted",
        Error::Timeout => "budget_exhausted",
        Error::Invalid("install_failed") => "install_failed",
        Error::Invalid("setup_failed") => "setup_failed",
        _ => match stage {
            "pins" => "pins_invalid",
            "install" => "install_failed",
            "setup" => "setup_failed",
            "server_ready" | "client_ready" => "readiness_timeout",
            "fixture_direct" => "capture_io",
            _ => "capture_io",
        },
    }
}
fn publish(output: &Path, report: &Report<'_>) -> Result<()> {
    publish_report(
        PublicationContext {
            output,
            report_kind: report.kind,
        },
        &records::canonical(report)?,
        report.passed,
    )
}
struct PublicationContext<'a> {
    output: &'a Path,
    report_kind: &'a str,
}
fn publish_report(context: PublicationContext<'_>, report: &[u8], passed: bool) -> Result<()> {
    let output = context.output;
    if !matches!(
        context.report_kind,
        "macos-stream-diagnostic" | "macos-cold-multiprincipal-diagnostic"
    ) {
        return Err(bad());
    }
    let value: serde_json::Value = serde_json::from_slice(report).map_err(|_| bad())?;
    if value["kind"] != context.report_kind || value["passed"] != passed {
        return Err(bad());
    }
    let result = (|| {
        records::write_raw(&output.join("report.json"), report)?;
        if passed {
            let bytes = records::read_bytes(&output.join("report.json"), 1048576)?;
            records::write(
                &output.join("complete.json"),
                &serde_json::json!({"schema":1,"kind":"macos-diagnostic-complete","report_sha256":records::digest(&bytes),"passed":true}),
            )?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = records::write(
            &output.join("failure.json"),
            &serde_json::json!({"schema":1,"kind":"macos-diagnostic-failure","failure":"report_failed"}),
        );
    }
    result
}
fn finish_report(
    report: &mut Report<'_>,
    result: &Result<()>,
    stage: &str,
    concurrent: bool,
    interrupted: bool,
) {
    report.failure = result.as_ref().err().map(|e| category(stage, e).to_owned());
    report.stage = stage.into();
    report.passed = result.is_ok()
        && !interrupted
        && finalized(&report.cleanup)
        && report.observations.len() == if concurrent { 6 } else { 3 }
        && report.observations.iter().all(|v| v.failure.is_none());
    if interrupted {
        report.failure = Some("interrupted".into());
    }
    if !report.passed && report.failure.is_none() {
        report.failure = Some(
            if !finalized(&report.cleanup) {
                "cleanup_unconfirmed"
            } else {
                "hash_mismatch"
            }
            .into(),
        );
    }
}
fn signal_watcher() -> Result<(watch::Receiver<bool>, tokio::task::JoinHandle<()>)> {
    // Install signal handlers before child or relay work. A second signal never aborts finalization.
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let (send, signal) = watch::channel(false);
    let watcher = tokio::spawn(async move {
        loop {
            tokio::select! {_ = interrupt.recv()=>{},_ = terminate.recv()=>{}}
            let _ = send.send(true);
        }
    });
    Ok((signal, watcher))
}
pub(super) async fn run(o: Options) -> Result<()> {
    let (signal, watcher) = signal_watcher()?;
    let result = run_inner(&o, signal).await;
    watcher.abort();
    let _ = watcher.await;
    result
}
fn preflight_paths(o: &Options) -> Result<()> {
    for path in [
        &o.assets,
        &o.bootstrap,
        &o.fixture,
        &o.workspace,
        &o.output,
        &o.pins,
    ] {
        if !path.is_absolute() {
            return Err(bad());
        }
    }
    if o.workspace == o.output
        || o.output.starts_with(&o.workspace)
        || o.workspace.starts_with(&o.output)
    {
        return Err(bad());
    }
    for path in [&o.workspace, &o.output] {
        let parent = path.parent().ok_or_else(bad)?;
        crate::identity::check_dir(parent)?;
        if parent.canonicalize()? != parent {
            return Err(bad());
        }
    }
    Ok(())
}
fn prepare_environment(o: &Options) -> Result<process::Environment> {
    directory(&o.workspace)?;
    let environment = process::Environment {
        home: o.workspace.join("home"),
        temporary: o.workspace.join("tmp"),
        utilities: o.workspace.join("utilities"),
    };
    directory(&environment.home)?;
    directory(&environment.temporary)?;
    process::utilities(&environment.utilities)?;
    Ok(environment)
}
async fn run_inner(o: &Options, signal: watch::Receiver<bool>) -> Result<()> {
    preflight_paths(o)?;
    directory(&o.output)?;
    let mut stage = "pins".to_string();
    let mut observations = Vec::new();
    let pins = records::read::<Pins>(&o.pins, 16384).and_then(|p| {
        p.validate()?;
        Ok(p)
    });
    let mut report = Report {
        schema: 1,
        kind: "macos-stream-diagnostic",
        qualification: false,
        same_host_only: true,
        relay: RELAY,
        pins: pins.as_ref().ok(),
        probe_sha256: records::digest(&records::read_bytes(&std::env::current_exe()?, 268435456)?),
        stage: stage.clone(),
        failure: None,
        observations: Vec::new(),
        cleanup: Finalized::default(),
        passed: false,
    };
    let result = async {
        let pins = pins.as_ref().map_err(|_| bad())?;
        if crate::native_manifest::compiled_release_metadata().target != TARGET
            || unsafe { libc::geteuid() } == 0
        {
            return Err(bad());
        }
        let environment = prepare_environment(o)?;
        let mut state = State {
            client: None,
            server: None,
            environment,
            accounts: Vec::new(),
            fixture: PathBuf::new(),
        };
        let work_end = Instant::now() + Duration::from_secs(900);
        let result = work(
            o,
            pins,
            &mut state,
            work_end,
            signal.clone(),
            &mut stage,
            &mut observations,
        )
        .await;
        report.cleanup = finalize(o, &mut state, Instant::now() + Duration::from_secs(180)).await;
        let removal = if finalized(&report.cleanup) {
            fs::remove_dir_all(&o.workspace).map_err(Error::from)
        } else {
            Ok(())
        };
        result.and(removal)
    }
    .await;
    report.observations = observations;
    finish_report(&mut report, &result, &stage, o.concurrent, *signal.borrow());
    publish(&o.output, &report)?;
    if report.passed { Ok(()) } else { Err(bad()) }
}
#[cfg(test)]
#[path = "installed_macos_diagnostic_tests.rs"]
mod tests;

#[path = "installed_macos_cold_multiprincipal.rs"]
mod cold_multiprincipal;
pub(super) use cold_multiprincipal::{Options as ColdOptions, run as run_cold};
