//! Explicit live command acceptance. Never invoked by ordinary offline tests.
//! This child module intentionally uses the real identity owner and journal.
use super::command_adapter::CommandProbeExpected;
use super::*;
use crate::command_engine::{CleanupState, Readiness};
use crate::command_lifecycle::{self, OwnedIdentity, PendingStop};
use crate::command_wire::{self as wire, RequestOp, ResponseResult};
use crate::daemon::{self, Config};
use crate::identity::{BackendChoice, ProfileLease};
use crate::journal::{ClientDelivery, CommandRoom, Owner, PreparedCommandPublication};
use crate::transport::{RelayPolicy, RouteMode, TransportLimits};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::{future::Future, pin::Pin};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ServerLife {
    Running,
    Stopping,
    Stopped,
    Opening,
    Reopened,
}
type OpeningServer = Pin<Box<dyn Future<Output = Result<(ProfileLease, MdkOwner)>>>>;
struct Pair {
    client: Option<MdkOwner>,
    server: Option<MdkOwner>,
    client_profile: Option<ProfileLease>,
    server_profile: Option<ProfileLease>,
    root: Option<tempfile::TempDir>,
    engine_tree: Option<EngineTree>,
    invocations: Vec<wire::Id>,
    server_life: ServerLife,
    server_stop: Option<PendingStop>,
    server_open: Option<OpeningServer>,
    relays: Vec<String>,
    mode: RouteMode,
}
impl Drop for Pair {
    fn drop(&mut self) {
        // An unwinding/implicit drop is degraded, never deletion authority.
        // Preserve allocation and leases rather than unlinking beneath a live
        // engine or detaching it while releasing its profile lock.
        if let Some(root) = self.root.take() {
            let _ = root.keep();
        }
        std::mem::forget(self.client.take());
        std::mem::forget(self.server.take());
        std::mem::forget(self.client_profile.take());
        std::mem::forget(self.server_profile.take());
        std::mem::forget(self.server_stop.take());
        std::mem::forget(self.server_open.take());
    }
}

fn fresh_id() -> Result<wire::Id> {
    fixed_id(&foundation::fresh_id(now()?)?)
}
fn fixed_id(value: &str) -> Result<wire::Id> {
    hex::decode(value)
        .map_err(|_| Error::Invalid("command_check_id"))?
        .try_into()
        .map_err(|_| Error::Invalid("command_check_id"))
}
fn private_root() -> Result<tempfile::TempDir> {
    // Refuse a missing, redirected or writable HOME rather than falling back to
    // /tmp: the actual engine checks every ancestor of its private root.
    let home = PathBuf::from(std::env::var_os("HOME").ok_or(Error::Invalid("command_check_home"))?);
    let metadata = std::fs::symlink_metadata(&home)?;
    if !home.is_absolute()
        || !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o022 != 0
    {
        return Err(Error::Invalid("command_check_home"));
    }
    Ok(tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .prefix(".marmish-command-check-")
        .tempdir_in(home)?)
}
fn write_config(
    profile: &ProfileLease,
    peer: wire::Account,
    relays: &[String],
    mode: RouteMode,
    role: &str,
    cwd: &Path,
) -> Result<()> {
    let tor = match mode {
        RouteMode::Off => "off",
        RouteMode::Auto => "auto",
        RouteMode::Always => "always",
    };
    let peer = hex::encode(peer);
    let config = json!({"version":1,"role":role,"relays":relays,"tor":tor,
        "trusted_remotes":if role == "client" {vec![peer.clone()]} else {vec![]},
        "execution_clients":if role == "server" {vec![peer]} else {vec![]},
        "command_policy":{"version":1,"defaults":{"allow_exec":["/**"]}},
        "cwd":cwd,"limits":{},"aliases":{}});
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(profile.root().join("config.json"))?;
    file.write_all(
        &serde_json::to_vec(&config).map_err(|_| Error::Invalid("command_check_config"))?,
    )?;
    file.sync_all()?;
    Ok(())
}
fn config(root: &Path, identity: &str, relays: &[String], mode: RouteMode) -> Result<Config> {
    Ok(Config {
        max_commands: None,
        max_uptime: None,
        state_dir: root.to_owned(),
        identity: identity.into(),
        backend: BackendChoice::File,
        helper: std::env::current_exe()?,
        relay_policy: RelayPolicy::parse(relays, mode, TransportLimits::default())?,
        allowed_peers: vec![],
        retention: Duration::from_secs(86400),
    })
}
impl Pair {
    async fn open(relays: &[String], mode: RouteMode) -> Result<Self> {
        let root = private_root()?;
        let path = root.path().to_owned();
        let setup = path.clone();
        let relays_owned = relays.to_vec();
        tokio::task::spawn_blocking(move || {
            let helper = std::env::current_exe()?;
            let mut client = ProfileLease::acquire(&setup, "client")?;
            let mut server = ProfileLease::acquire(&setup, "server")?;
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            let a = client.prepare(BackendChoice::File, &helper, deadline)?;
            let b = server.prepare(BackendChoice::File, &helper, deadline)?;
            write_config(
                &client,
                b.public_key().to_bytes(),
                &relays_owned,
                mode,
                "client",
                &setup,
            )?;
            write_config(
                &server,
                a.public_key().to_bytes(),
                &relays_owned,
                mode,
                "server",
                &setup,
            )?;
            Ok::<_, Error>(())
        })
        .await
        .map_err(|_| Error::Identity("command_check_prepare"))??;
        // Construct the pair before opening either engine so an opening failure
        // still retains every successfully opened owner for explicit cleanup.
        let mut pair = Self {
            root: Some(root),
            engine_tree: None,
            invocations: Vec::new(),
            server_life: ServerLife::Running,
            server_stop: None,
            server_open: None,
            relays: relays.to_vec(),
            mode,
            client_profile: None,
            server_profile: None,
            client: None,
            server: None,
        };
        let opened = daemon::open(config(&path, "client", relays, mode)?).await;
        let (profile, owner) = opened?;
        pair.client_profile = Some(profile);
        pair.client = Some(owner);
        match daemon::open(config(&path, "server", relays, mode)?).await {
            Ok((profile, owner)) => {
                pair.server_profile = Some(profile);
                pair.server = Some(owner);
                Ok(pair)
            }
            Err(error) => {
                // Even a partial pair follows the same joined lifetime rule.
                // Preserve its private allocation on any failed stop outcome.
                if let (Some(owner), Some(profile)) =
                    (pair.client.take(), pair.client_profile.take())
                {
                    let stopped = command_lifecycle::stop_owned(
                        OwnedIdentity { owner, profile },
                        Instant::now() + Duration::from_secs(30),
                    )
                    .await;
                    let report = match stopped.pending {
                        Some(pending) => pending.finish().await,
                        None => stopped.report,
                    };
                    if !report.engine_stop_complete
                        || !report.mdk_stop_complete
                        || !report.runtime_cleanup_complete
                    {
                        std::mem::forget(pair);
                    }
                }
                Err(error)
            }
        }
    }
    fn owners(&mut self) -> Result<(&mut MdkOwner, &mut MdkOwner)> {
        match (&mut self.client, &mut self.server) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => Err(Error::Storage("command_check_owner")),
        }
    }
    async fn turn(&mut self, deadline: Instant) -> Result<()> {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let (a, b) = self.owners()?;
        let limit = deadline.min(Instant::now() + Duration::from_secs(15));
        let (a, b) = tokio::join!(a.tick(limit), b.tick(limit));
        for result in [a, b] {
            match result {
                Ok(()) | Err(Error::Timeout) => {}
                Err(Error::Transport(
                    "relay_reconnect"
                    | "transport_recovery_pending"
                    | "recovery_subscription"
                    | "subscription_sync"
                    | "notification_loss"
                    | "group_state_unavailable",
                )) => {}
                Err(error) => return Err(error),
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(())
    }
}

fn response_owner(owner: &mut MdkOwner, room: &CommandRoom) -> Result<Owner> {
    Ok(Owner::from_context(
        &owner.command_context(&GroupId::new(room.group.clone()))?,
    ))
}

fn read_response(
    journal: &mut Journal,
    owner: &Owner,
    id: wire::Id,
) -> Result<Option<wire::Response>> {
    journal
        .command_client_response(owner, id)?
        .map(|(response, digest)| {
            if <[u8; 32]>::from(Sha256::digest(wire::encode_response(&response)?)) != digest {
                return Err(Error::Invalid("command_check_response_digest"));
            }
            Ok(response)
        })
        .transpose()
}
fn consume_response(
    pair: &mut Pair,
    room: &CommandRoom,
    response: &wire::Response,
    expected: Option<(u64, u64)>,
) -> Result<()> {
    let client = pair
        .client
        .as_mut()
        .ok_or(Error::Storage("command_check_owner"))?;
    let owner = response_owner(client, room)?;
    let digest = Sha256::digest(wire::encode_response(response)?).into();
    let expected = expected.map(|(stdout, stderr)| ClientDelivery { stdout, stderr });
    let consumed =
        client
            .journal
            .command_consume_response(&owner, response.id, digest, expected)?;
    // Idempotent consumption must return the current durable cursor pair.
    if client
        .journal
        .command_consume_response(&owner, response.id, digest, expected)?
        != consumed
    {
        return Err(Error::Invalid("command_check_consume"));
    }
    Ok(())
}

async fn prepare_engine_tree(pair: &mut Pair, deadline: Instant) -> Result<()> {
    loop {
        let readiness = pair
            .server
            .as_mut()
            .and_then(|owner| owner.commands.as_mut())
            .and_then(|service| service.engine_mut())
            .ok_or(Error::Invalid("command_check_engine"))?
            .readiness();
        match readiness {
            Readiness::Ready => {
                pair.engine_tree = Some(EngineTree::capture(
                    pair.server_profile
                        .as_ref()
                        .ok_or(Error::Storage("command_check_profile"))?,
                )?);
                return Ok(());
            }
            Readiness::Failed => return Err(Error::Storage("command_check_engine")),
            Readiness::Opening => {
                if Instant::now() >= deadline {
                    return Err(Error::Timeout);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
}

async fn remove_engine_tree(pair: &mut Pair, cancel: bool, deadline: Instant) -> Result<()> {
    let owner = pair
        .server
        .as_mut()
        .ok_or(Error::Storage("command_check_owner"))?;
    let mut canceled = !cancel || pair.invocations.is_empty();
    let fence = loop {
        owner.command_poll_cleanup()?;
        if !canceled {
            match owner.command_cancel_fixture(&pair.invocations) {
                Ok(()) => canceled = true,
                Err(Error::Storage("command_cleanup_busy")) => {}
                Err(error) => return Err(error),
            }
        }
        if canceled {
            match owner.command_begin_cleanup() {
                Ok(fence) => break fence,
                Err(Error::Storage("command_cleanup_busy")) => {}
                Err(error) => return Err(error),
            }
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    loop {
        let (state, observed) = owner.command_poll_cleanup()?;
        match state {
            CleanupState::Quiesced if observed == Some(fence) => break,
            CleanupState::Failed => return Err(Error::Storage("command_check_quiescence")),
            _ => {
                if Instant::now() >= deadline {
                    return Err(Error::Timeout);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
    // No actor tick or engine stop can interleave: the same owned guard and
    // profile remain held throughout these synchronous descriptor operations.
    pair.engine_tree
        .take()
        .ok_or(Error::Storage("command_check_tree"))?
        .remove()?;
    if owner.command_poll_cleanup()? != (CleanupState::Quiesced, Some(fence)) {
        return Err(Error::Storage("command_check_quiescence"));
    }
    Ok(())
}

async fn stop_identity(
    owner: Option<MdkOwner>,
    profile: Option<ProfileLease>,
    deadline: Instant,
) -> command_lifecycle::LifecycleStopReport {
    match (owner, profile) {
        (Some(owner), Some(profile)) => {
            let stopped =
                command_lifecycle::stop_owned(OwnedIdentity { owner, profile }, deadline).await;
            match stopped.pending {
                Some(pending) => pending.finish().await,
                None => stopped.report,
            }
        }
        // Any incomplete ownership tuple is a degraded stop, never deletion
        // authority. Keep its resources rather than implicitly unlocking it.
        (owner, profile) => {
            std::mem::forget(owner);
            std::mem::forget(profile);
            command_lifecycle::LifecycleStopReport::default()
        }
    }
}

async fn request(
    pair: &mut Pair,
    room: &CommandRoom,
    op: RequestOp,
    deadline: Instant,
) -> Result<wire::Response> {
    request_id(pair, room, fresh_id()?, op, deadline).await
}
async fn request_id(
    pair: &mut Pair,
    room: &CommandRoom,
    id: wire::Id,
    op: RequestOp,
    deadline: Instant,
) -> Result<wire::Response> {
    loop {
        match pair
            .client
            .as_mut()
            .ok_or(Error::Storage("command_check_owner"))?
            .command_request(room.session, id, op.clone())
        {
            Ok(_) => break,
            // Consuming a response does not retire its native obligations.
            // Keep the same request identity and let both owners reconcile
            // before retrying local admission of the next Poll.
            Err(Error::Invalid("command_poll_busy")) => pair.turn(deadline).await?,
            Err(error) => return Err(error),
        }
    }
    loop {
        pair.turn(deadline).await?;
        let client = pair
            .client
            .as_mut()
            .ok_or(Error::Storage("command_check_owner"))?;
        let owner = response_owner(client, room)?;
        if let Some(response) = read_response(&mut client.journal, &owner, id)? {
            return Ok(response);
        }
    }
}
fn sample(name: &str, start: Instant, bytes: &[u8]) -> Value {
    let elapsed = start.elapsed();
    json!({"stage":name,"milliseconds":elapsed.as_millis(),"bytes":bytes.len(),
        "sha256":hex::encode(Sha256::digest(bytes)),
        "bytes_per_second":if bytes.is_empty(){None}else{Some((bytes.len() as f64/elapsed.as_secs_f64().max(0.000001)) as u64)}})
}
struct CommandCase<'a> {
    name: &'a str,
    argv: Vec<Vec<u8>>,
    stdin: Vec<u8>,
    expected_stdout: &'a [u8],
    expected_stderr: &'a [u8],
    exit: u8,
}
async fn command(
    pair: &mut Pair,
    room: &CommandRoom,
    case: CommandCase<'_>,
    deadline: Instant,
) -> Result<Value> {
    let CommandCase {
        name,
        argv,
        stdin,
        expected_stdout,
        expected_stderr,
        exit,
    } = case;
    let start = Instant::now();
    let invocation = fresh_id()?;
    pair.invocations.push(invocation);
    let response = request(
        pair,
        room,
        RequestOp::Submit(wire::Submit {
            argv,
            invocation,
            stdin,
        }),
        deadline,
    )
    .await?;
    if !matches!(response.result, Ok(ResponseResult::Submit { .. })) {
        return Err(Error::Invalid("command_check_submit"));
    }
    consume_response(pair, room, &response, None)?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    loop {
        let client = pair
            .client
            .as_mut()
            .ok_or(Error::Storage("command_check_owner"))?;
        let owner = response_owner(client, room)?;
        let cursors = client.journal.command_client_cursors(&owner, invocation)?;
        if cursors.stdout != stdout.len() as u64 || cursors.stderr != stderr.len() as u64 {
            return Err(Error::Invalid("command_check_cursor"));
        }
        let response = request(
            pair,
            room,
            RequestOp::Poll {
                invocation,
                stdout_cursor: cursors.stdout,
                stderr_cursor: cursors.stderr,
                max_bytes: 16384,
            },
            deadline,
        )
        .await?;
        let Ok(ResponseResult::Poll {
            status,
            stdout: out,
            stderr: err,
        }) = &response.result
        else {
            return Err(Error::Invalid("command_check_poll"));
        };
        if out.next_cursor != stdout.len() as u64 + out.bytes.len() as u64
            || err.next_cursor != stderr.len() as u64 + err.bytes.len() as u64
            || !status.stdout.gaps.is_empty()
            || !status.stderr.gaps.is_empty()
            || stdout.len() + out.bytes.len() > 32768
            || stderr.len() + err.bytes.len() > 32768
        {
            return Err(Error::Invalid("command_check_output"));
        }
        if expected_stdout.get(stdout.len()..stdout.len() + out.bytes.len())
            != Some(out.bytes.as_slice())
            || expected_stderr.get(stderr.len()..stderr.len() + err.bytes.len())
                != Some(err.bytes.as_slice())
        {
            return Err(Error::Invalid("command_check_bytes"));
        }
        let previous = (stdout.len() as u64, stderr.len() as u64);
        stdout.extend_from_slice(&out.bytes);
        stderr.extend_from_slice(&err.bytes);
        let complete = status.state == wire::State::Exited
            && status.stdout.closed
            && status.stderr.closed
            && stdout.len() as u64 == status.stdout.produced
            && stderr.len() as u64 == status.stderr.produced;
        if complete
            && (stdout != expected_stdout
                || stderr != expected_stderr
                || !matches!(status.outcome,Some(wire::Outcome {kind:wire::OutcomeKind::Code,value:Some(value)}) if value==exit))
        {
            return Err(Error::Invalid("command_check_result"));
        }
        // Validate terminal outcome as well as literal chunks before recording
        // local delivery; a failed validation leaves this response unconsumed.
        consume_response(pair, room, &response, Some(previous))?;
        if complete {
            let mut result = sample(name, start, &stdout);
            result["stderr_bytes"] = json!(stderr.len());
            result["stderr_sha256"] = json!(hex::encode(Sha256::digest(&stderr)));
            result["exit_code"] = json!(exit);
            return Ok(result);
        }
    }
}
fn stage_deadline(deadline: Instant, seconds: u64) -> Instant {
    deadline.min(Instant::now() + Duration::from_secs(seconds))
}
struct RecoveryFiles {
    parent: std::fs::File,
    parent_id: (u64, u64),
    marker: PathBuf,
    gate: PathBuf,
    marker_id: Option<(u64, u64)>,
}
impl RecoveryFiles {
    fn new(root: &Path) -> Result<Self> {
        let parent = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(root)?;
        let stat = fd_stat(&parent)?;
        if stat.st_uid != unsafe { libc::geteuid() } || stat.st_mode & 0o077 != 0 {
            return Err(Error::Invalid("command_check_marker"));
        }
        let marker = root.join("recovery-marker");
        let gate = root.join("recovery-release");
        for name in [&marker, &gate] {
            match std::fs::symlink_metadata(name) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(Error::Invalid("command_check_marker")),
            }
        }
        Ok(Self {
            parent,
            parent_id: inode(&stat),
            marker,
            gate,
            marker_id: None,
        })
    }
    fn verify_parent(&self) -> Result<()> {
        let path = self
            .marker
            .parent()
            .ok_or(Error::Invalid("command_check_marker"))?;
        let current = std::fs::symlink_metadata(path)?;
        if !current.is_dir()
            || (current.dev(), current.ino()) != self.parent_id
            || inode(&fd_stat(&self.parent)?) != self.parent_id
        {
            return Err(Error::Invalid("command_check_marker"));
        }
        Ok(())
    }
    fn verify(&mut self) -> Result<()> {
        use std::io::Read;
        self.verify_parent()?;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.marker)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
        {
            return Err(Error::Invalid("command_check_marker"));
        }
        let identity = (metadata.dev(), metadata.ino());
        if self.marker_id.is_some_and(|expected| expected != identity) {
            return Err(Error::Invalid("command_check_marker"));
        }
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(5)
            .read_to_end(&mut bytes)?;
        if bytes != b"run\n" {
            return Err(Error::Invalid("command_check_marker"));
        }
        self.marker_id = Some(identity);
        Ok(())
    }
    fn release(&self) -> Result<()> {
        self.verify_parent()?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.gate)?;
        file.sync_all()?;
        self.parent.sync_all()?;
        Ok(())
    }
}
async fn recovery_poll(
    pair: &mut Pair,
    room: &CommandRoom,
    invocation: wire::Id,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    deadline: Instant,
) -> Result<wire::Status> {
    let previous = (stdout.len() as u64, stderr.len() as u64);
    let response = request(
        pair,
        room,
        RequestOp::Poll {
            invocation,
            stdout_cursor: previous.0,
            stderr_cursor: previous.1,
            max_bytes: 16384,
        },
        deadline,
    )
    .await?;
    let Ok(ResponseResult::Poll {
        status,
        stdout: out,
        stderr: err,
    }) = &response.result
    else {
        return Err(Error::Invalid("command_recovery_poll"));
    };
    let expected_out = b"before-restartafter-restart";
    let expected_err = b"before-stderrafter-stderr";
    if out.next_cursor != previous.0 + out.bytes.len() as u64
        || err.next_cursor != previous.1 + err.bytes.len() as u64
        || !status.stdout.gaps.is_empty()
        || !status.stderr.gaps.is_empty()
        || status.warning.is_some()
        || expected_out.get(stdout.len()..stdout.len() + out.bytes.len())
            != Some(out.bytes.as_slice())
        || expected_err.get(stderr.len()..stderr.len() + err.bytes.len())
            != Some(err.bytes.as_slice())
    {
        return Err(Error::Invalid("command_recovery_bytes"));
    }
    let status = status.clone();
    stdout.extend_from_slice(&out.bytes);
    stderr.extend_from_slice(&err.bytes);
    consume_response(pair, room, &response, Some(previous))?;
    Ok(status)
}
fn running_eof(status: &wire::Status) -> bool {
    status.state == wire::State::Running
        && status.input.eof_sent
        && status.input.length == 0
        && status.input.accepted == 0
        && status.input.delivered == 0
        && !matches!(
            status.input.health,
            wire::InputHealth::Gap | wire::InputHealth::Uncertain
        )
        && status.warning.is_none()
        && status.authority_valid
        && status.engine_known
}
async fn restart_server(pair: &mut Pair, deadline: Instant, report: &mut Value) -> Result<()> {
    report["failed_stage"] = json!("restart_stop");
    let began = Instant::now();
    pair.server_life = ServerLife::Stopping;
    let owned = OwnedIdentity {
        owner: pair
            .server
            .take()
            .ok_or(Error::Storage("command_check_owner"))?,
        profile: pair
            .server_profile
            .take()
            .ok_or(Error::Storage("command_check_profile"))?,
    };
    let stopped = command_lifecycle::stop_owned(owned, stage_deadline(deadline, 30)).await;
    report["restart_stop_ms"] = json!(began.elapsed().as_millis());
    if let Some(pending) = stopped.pending {
        pair.server_stop = Some(pending);
        return Err(Error::Timeout);
    }
    if !stopped.report.engine_stop_complete
        || !stopped.report.mdk_stop_complete
        || !stopped.report.runtime_cleanup_complete
    {
        return Err(Error::Storage("command_recovery_stop"));
    }
    pair.server_life = ServerLife::Stopped;
    let cfg = config(
        pair.root
            .as_ref()
            .ok_or(Error::Storage("command_check_root"))?
            .path(),
        "server",
        &pair.relays,
        pair.mode,
    )?;
    pair.server_open = Some(Box::pin(daemon::open(cfg)));
    pair.server_life = ServerLife::Opening;
    report["failed_stage"] = json!("restart_open");
    let began = Instant::now();
    let opened = tokio::time::timeout_at(
        stage_deadline(deadline, 30),
        pair.server_open
            .as_mut()
            .ok_or(Error::Storage("command_recovery_open"))?,
    )
    .await;
    report["restart_open_ms"] = json!(began.elapsed().as_millis());
    match opened {
        Ok(result) => {
            pair.server_open = None;
            let (profile, owner) = result?;
            pair.server_profile = Some(profile);
            pair.server = Some(owner);
            pair.server_life = ServerLife::Reopened;
        }
        Err(_) => return Err(Error::Timeout),
    }
    pair.engine_tree
        .as_ref()
        .ok_or(Error::Storage("command_check_tree"))?
        .verify(
            pair.server_profile
                .as_ref()
                .ok_or(Error::Storage("command_check_profile"))?,
        )?;
    report["failed_stage"] = json!("restart_activation");
    let began = Instant::now();
    let activation_result = async {
        let activation = stage_deadline(deadline, 150);
        pair.server
            .as_mut()
            .ok_or(Error::Storage("command_check_owner"))?
            .activate(activation)
            .await?;
        while pair
            .server
            .as_ref()
            .ok_or(Error::Storage("command_check_owner"))?
            .recovery
        {
            pair.turn(activation).await?;
        }
        pair.engine_tree
            .as_ref()
            .ok_or(Error::Storage("command_check_tree"))?
            .verify(
                pair.server_profile
                    .as_ref()
                    .ok_or(Error::Storage("command_check_profile"))?,
            )
    }
    .await;
    report["restart_activation_ms"] = json!(began.elapsed().as_millis());
    activation_result?;
    Ok(())
}
async fn raw_submit_probe(
    pair: &mut Pair,
    room: &CommandRoom,
    submit: wire::Submit,
    expected: CommandProbeExpected,
    deadline: Instant,
) -> Result<PreparedCommandPublication> {
    let publication = {
        let client = pair
            .client
            .as_mut()
            .ok_or(Error::Storage("command_check_owner"))?;
        let owner = response_owner(client, room)?;
        let request = wire::Request {
            binding: owner.binding().clone(),
            id: fresh_id()?,
            op: RequestOp::Submit(submit),
        };
        let bytes = wire::encode_request(&request)?;
        let publication = prepare_command_publication(
            &room.group,
            request.binding.clone(),
            request.id,
            wire::request_digest(&bytes),
            bytes,
            owner.binding().client,
        )?;
        client.arm_command_probe(GroupId::new(room.group.clone()), request, expected)?;
        publication
    };
    let result = async {
        {
            let client = pair
                .client
                .as_mut()
                .ok_or(Error::Storage("command_check_owner"))?;
            let effects = tokio::time::timeout_at(
                deadline,
                client.runtime.send(SendIntent::AppMessage {
                    group_id: GroupId::new(room.group.clone()),
                    payload: publication.encoded_app_event.clone(),
                    expected_epoch: None,
                }),
            )
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Mdk("command_recovery_send"))?;
            client.apply_effects(effects, deadline).await?;
        }
        loop {
            pair.turn(deadline).await?;
            if let Some(valid) = pair
                .client
                .as_mut()
                .ok_or(Error::Storage("command_check_owner"))?
                .take_command_probe()
            {
                return if valid {
                    Ok(())
                } else {
                    Err(Error::Invalid("command_recovery_probe"))
                };
            }
        }
    }
    .await;
    pair.client
        .as_mut()
        .ok_or(Error::Storage("command_check_owner"))?
        .clear_command_probe();
    result?;
    Ok(publication)
}
fn native_probe_retained(
    pair: &Pair,
    room: &CommandRoom,
    p: &PreparedCommandPublication,
) -> Result<(bool, bool, bool)> {
    let client = pair
        .client
        .as_ref()
        .ok_or(Error::Storage("command_check_owner"))?;
    let group = GroupId::new(room.group.clone());
    let fanouts = client
        .storage
        .list_outbound_fanouts()
        .map_err(|_| Error::Storage("command_recovery_native"))?;
    let queued = client
        .storage
        .list_queued_outbound_intents(&group)
        .map_err(|_| Error::Storage("command_recovery_native"))?;
    let retained_fanout = fanouts.iter().any(|f| {
        f.group_id() == Some(&group)
            && f.application_message()
                .is_some_and(|m| m.app_event_id == p.key.app_event_id)
    });
    let retained_queue = queued.iter().any(
        |q| matches!(&q.intent,SendIntent::AppMessage{payload,..}if payload==&p.encoded_app_event),
    );
    let unknown = (retained_fanout || retained_queue)
        && !client
            .journal
            .publication_owned(&p.key.group, &p.key.app_event_id)?
        && !client.journal.command_publication_owned(&p.key)?;
    Ok((retained_fanout, retained_queue, unknown))
}
fn recovery_record_unchanged(
    pair: &mut Pair,
    room: &CommandRoom,
    submit: &wire::Submit,
) -> Result<()> {
    let server = pair
        .server
        .as_mut()
        .ok_or(Error::Storage("command_check_owner"))?;
    let owner = response_owner(server, room)?;
    let key = crate::journal::InvocationKey {
        server: owner.binding().server,
        principal: owner.binding().client,
        invocation: submit.invocation,
    };
    let record = server
        .journal
        .command_get(&key)?
        .ok_or(Error::Invalid("command_recovery_record"))?;
    if record.digest != wire::submit_digest(&room.group, owner.binding(), submit)?
        || record.submit.as_ref() != Some(submit)
        || server.journal.command_server_records(None, 2)?.len() != 1
    {
        return Err(Error::Invalid("command_recovery_record"));
    }
    Ok(())
}
fn recovery_report() -> Value {
    json!({"stage":"running_recovery", "milliseconds":null,"bytes":0,"bytes_per_second":null,
      "sha256":null,"stderr_bytes":0,"exit_code":null,"failed_stage":"prepare",
      "restart_while_running":false,"local_identical_attach":false,"identical_submit_existing":false,
      "local_conflict_refused":false,"remote_conflict_refused":false,"execution_count":null,
      "recovered_output_verified":false,"probe_native_retained":false,"purpose_ownership_unknown":false,
      "unowned_probe_fanouts":null,"unowned_probe_queued":null,
      "restart_stop_ms":null,"restart_open_ms":null,"restart_activation_ms":null})
}
fn recovered_prefix(status: &wire::Status, stdout: &[u8], stderr: &[u8]) -> bool {
    running_eof(status)
        && stdout == b"before-restart"
        && stderr == b"before-stderr"
        && status.stdout.produced == stdout.len() as u64
        && status.stderr.produced == stderr.len() as u64
        && status.stdout.gaps.is_empty()
        && status.stderr.gaps.is_empty()
        && !status.stdout.closed
        && !status.stderr.closed
}
async fn recovery_command(
    pair: &mut Pair,
    room: &CommandRoom,
    deadline: Instant,
    report: &mut Value,
) -> Result<()> {
    let started = Instant::now();
    let root = pair
        .root
        .as_ref()
        .ok_or(Error::Storage("command_check_root"))?
        .path()
        .to_owned();
    let mut files = RecoveryFiles::new(&root)?;
    let invocation = fresh_id()?;
    pair.invocations.push(invocation);
    let script=b"umask 077\nprintf '%s\\n' run >> \"$1\" || exit 123\nprintf '%s' before-restart\nprintf '%s' before-stderr >&2\nn=0\nwhile [ ! -f \"$2\" ] && [ \"$n\" -lt 360 ]; do\n sleep 1\n n=$((n + 1))\ndone\n[ -f \"$2\" ] || exit 124\nprintf '%s' after-restart\nprintf '%s' after-stderr >&2\nexit 7\n";
    let submit = wire::Submit {
        invocation,
        stdin: vec![],
        argv: vec![
            std::fs::canonicalize("/bin/sh")?
                .into_os_string()
                .into_encoded_bytes(),
            b"-c".to_vec(),
            script.to_vec(),
            b"marmish-recovery".to_vec(),
            files.marker.clone().into_os_string().into_encoded_bytes(),
            files.gate.clone().into_os_string().into_encoded_bytes(),
        ],
    };
    let original_id = fresh_id()?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result=async {
        report["failed_stage"]=json!("submit");
        let accepted=request_id(pair,room,original_id,RequestOp::Submit(submit.clone()),stage_deadline(deadline,20)).await?;
        if !matches!(accepted.result,Ok(ResponseResult::Submit{disposition:wire::Disposition::Accepted,..})) {return Err(Error::Invalid("command_recovery_submit"));}
        consume_response(pair,room,&accepted,None)?;
        report["failed_stage"]=json!("running_prefix");
        let initial=stage_deadline(deadline,20);
        loop {
            let status=recovery_poll(pair,room,invocation,&mut stdout,&mut stderr,initial).await?;
            if running_eof(&status) && stdout==b"before-restart" && stderr==b"before-stderr" {break;}
        }
        files.verify()?;
        restart_server(pair,deadline,report).await?;
        report["restart_while_running"]=json!(true);
        report["failed_stage"]=json!("recovered_status");
        let status_response=request(pair,room,RequestOp::Status{invocation},stage_deadline(deadline,20)).await?;
        if !matches!(&status_response.result,Ok(ResponseResult::Status(status))if recovered_prefix(status,&stdout,&stderr)) {
            return Err(Error::Invalid("command_recovery_status"));
        }
        consume_response(pair,room,&status_response,None)?;files.verify()?;
        report["recovered_output_verified"]=json!(true);
        report["failed_stage"]=json!("identical_retry");
        let retry_deadline=stage_deadline(deadline,20);
        let client=pair.client.as_mut().ok_or(Error::Storage("command_check_owner"))?;
        let repeated=client.command_request(room.session,original_id,RequestOp::Submit(submit.clone()))?;
        let owner=response_owner(client,room)?;
        if repeated.digest!=wire::submit_digest(&room.group,owner.binding(),&submit)?
            || repeated.submit.as_ref()!=Some(&submit) {
            return Err(Error::Invalid("command_recovery_retry"));
        }
        let original_request=wire::Request{binding:owner.binding().clone(),id:original_id,op:RequestOp::Submit(submit.clone())};
        let pending=client.journal.command_find_request(&owner,&original_request)?
            .ok_or(Error::Invalid("command_recovery_retry"))?;
        if pending.digest!=wire::request_digest(&wire::encode_request(&original_request)?) {
            return Err(Error::Invalid("command_recovery_retry"));
        }
        report["local_identical_attach"]=json!(true);
        let identical=raw_submit_probe(pair,room,submit.clone(),CommandProbeExpected::Existing,
            retry_deadline.min(stage_deadline(deadline,15))).await?;
        report["identical_submit_existing"]=json!(true);
        report["failed_stage"]=json!("local_conflict");
        let mut changed=submit.clone();changed.argv.push(b"changed".to_vec());
        let local=pair.client.as_mut().ok_or(Error::Storage("command_check_owner"))?
            .command_request(room.session,fresh_id()?,RequestOp::Submit(changed.clone()));
        if !matches!(local,Err(Error::Invalid("command_invocation_conflict"))) {
            return Err(Error::Invalid("command_recovery_local_conflict"));
        }
        report["local_conflict_refused"]=json!(true);
        report["failed_stage"]=json!("remote_conflict");
        let conflict=raw_submit_probe(pair,room,changed,CommandProbeExpected::InvocationConflict,
            stage_deadline(deadline,15)).await?;
        report["remote_conflict_refused"]=json!(true);
        report["failed_stage"]=json!("terminal_output");
        files.verify()?;recovery_record_unchanged(pair,room,&submit)?;files.release()?;
        loop {
            let status=recovery_poll(pair,room,invocation,&mut stdout,&mut stderr,deadline).await?;
            if status.state==wire::State::Exited && status.stdout.closed && status.stderr.closed {
                if stdout!=b"before-restartafter-restart" || stderr!=b"before-stderrafter-stderr"
                    || status.stdout.produced!=stdout.len() as u64 || status.stderr.produced!=stderr.len() as u64
                    || !matches!(status.outcome,Some(wire::Outcome{kind:wire::OutcomeKind::Code,value:Some(7)})) {
                    return Err(Error::Invalid("command_recovery_result"));
                }
                break;
            }
        }
        files.verify()?;recovery_record_unchanged(pair,room,&submit)?;
        report["execution_count"]=json!(1);
        report["exit_code"]=json!(7);
        report["failed_stage"]=json!("native_obligations");
        let identical_retained=native_probe_retained(pair,room,&identical)?;
        let conflict_retained=native_probe_retained(pair,room,&conflict)?;
        if !(identical_retained.0||identical_retained.1) || !(conflict_retained.0||conflict_retained.1) {
            return Err(Error::Invalid("command_recovery_obligation"));
        }
        report["probe_native_retained"]=json!(true);
        report["unowned_probe_fanouts"]=json!([identical_retained.0,conflict_retained.0]);
        report["unowned_probe_queued"]=json!([identical_retained.1,conflict_retained.1]);
        let unknown=identical_retained.2&&conflict_retained.2;
        report["purpose_ownership_unknown"]=json!(unknown);
        if !unknown {return Err(Error::Invalid("command_recovery_ownership"));}
        report["failed_stage"]=Value::Null;
        Ok(())
    }.await;
    for (key, value) in sample("running_recovery", started, &stdout)
        .as_object()
        .ok_or(Error::Invalid("command_check_report"))?
    {
        report[key] = value.clone();
    }
    report["stderr_bytes"] = json!(stderr.len());
    if result.is_err()
        && std::fs::symlink_metadata(&files.gate)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        let _ = files.release();
    }
    result
}

async fn exercise(pair: &mut Pair, deadline: Instant, samples: &mut Vec<Value>) -> Result<()> {
    prepare_engine_tree(pair, deadline).await?;
    let (a, b) = pair.owners()?;
    let (a_ready, b_ready) = tokio::join!(a.activate(deadline), b.activate(deadline));
    a_ready?;
    b_ready?;
    let peer: wire::Account = hex::decode(
        &pair
            .server
            .as_ref()
            .ok_or(Error::Storage("command_check_owner"))?
            .identity,
    )
    .map_err(|_| Error::Identity("command_check_account"))?
    .try_into()
    .map_err(|_| Error::Identity("command_check_account"))?;
    let room = pair
        .client
        .as_mut()
        .ok_or(Error::Storage("command_check_owner"))?
        .open_command_room(peer, false, deadline)
        .await?;
    while pair
        .server
        .as_ref()
        .ok_or(Error::Storage("command_check_owner"))?
        .journal
        .command_room(&room.group)?
        .is_none()
    {
        pair.turn(deadline).await?;
    }
    let mut recovery = recovery_report();
    let result = recovery_command(pair, &room, deadline, &mut recovery).await;
    samples.push(recovery);
    result?;
    let printf = std::fs::canonicalize("/usr/bin/printf")?
        .into_os_string()
        .into_encoded_bytes();
    let literal = b"spaces 'double\\\"' $HOME $(false); * ? [x] \\ newline\n\xff".to_vec();
    samples.push(
        command(
            pair,
            &room,
            CommandCase {
                name: "literal_argv",
                argv: vec![printf, b"%s".to_vec(), literal.clone()],
                stdin: vec![],
                expected_stdout: &literal,
                expected_stderr: b"",
                exit: 0,
            },
            deadline,
        )
        .await?,
    );
    let binary: Vec<u8> = (0..16384).map(|n| (n % 256) as u8).collect();
    samples.push(
        command(
            pair,
            &room,
            CommandCase {
                name: "binary_stdin",
                argv: vec![
                    std::fs::canonicalize("/bin/cat")?
                        .into_os_string()
                        .into_encoded_bytes(),
                ],
                stdin: binary.clone(),
                expected_stdout: &binary,
                expected_stderr: b"",
                exit: 0,
            },
            deadline,
        )
        .await?,
    );
    samples.push(
        command(
            pair,
            &room,
            CommandCase {
                name: "stderr_exit",
                argv: vec![
                    std::fs::canonicalize("/bin/sh")?
                        .into_os_string()
                        .into_encoded_bytes(),
                    b"-c".to_vec(),
                    b"printf '%s' stderr-check >&2; exit 7".to_vec(),
                ],
                stdin: vec![],
                expected_stdout: b"",
                expected_stderr: b"stderr-check",
                exit: 7,
            },
            deadline,
        )
        .await?,
    );
    Ok(())
}

/// Finite report only: no keys, relay URLs, payload bytes or private paths.
/// Failed cleanup retains the owner/profile/root together instead of detaching
/// an engine thread and deleting the lock or state underneath it.
pub(crate) async fn run(relays: Vec<String>, mode: RouteMode) -> Result<Value> {
    RelayPolicy::parse(&relays, mode, TransportLimits::default())?;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(600);
    let mut pair = Pair::open(&relays, mode).await?;
    let mut samples = Vec::new();
    let result = exercise(&mut pair, deadline - Duration::from_secs(30), &mut samples).await;
    // An expired reopen/stop observer stays red, but its exact owned operation
    // must finish before any final stop or private-allocation deletion.
    if let Some(opening) = pair.server_open.take() {
        if let Ok((profile, owner)) = opening.await {
            pair.server_profile = Some(profile);
            pair.server = Some(owner);
            pair.server_life = ServerLife::Reopened;
        } else {
            pair.server_life = ServerLife::Stopped;
        }
    }
    if let Some(stopping) = pair.server_stop.take() {
        let _ = stopping.finish().await;
        pair.server_life = ServerLife::Stopped;
    }
    let cleanup = remove_engine_tree(&mut pair, result.is_err(), deadline).await;
    let engine_quiesced = cleanup.is_ok();
    let finite_detail = |result: &Result<()>| match result {
        Err(
            Error::Invalid(detail)
            | Error::Identity(detail)
            | Error::Storage(detail)
            | Error::Transport(detail)
            | Error::Mdk(detail),
        ) => Some(*detail),
        Err(Error::Timeout) => Some("timeout"),
        Err(Error::Io(_)) => Some("local_io"),
        Ok(()) => None,
    };
    let detail = finite_detail(&result);
    let cleanup_detail = finite_detail(&cleanup);
    let client_stop = stop_identity(pair.client.take(), pair.client_profile.take(), deadline);
    let server_stop = stop_identity(pair.server.take(), pair.server_profile.take(), deadline);
    let (client_stop, server_stop) = tokio::join!(client_stop, server_stop);
    let engine_stop = client_stop.engine_stop_complete && server_stop.engine_stop_complete;
    let mdk_stop = client_stop.mdk_stop_complete && server_stop.mdk_stop_complete;
    let runtime_cleanup =
        client_stop.runtime_cleanup_complete && server_stop.runtime_cleanup_complete;
    let mut state_cleanup = false;
    if engine_quiesced && engine_stop && mdk_stop && runtime_cleanup {
        state_cleanup = pair.root.take().is_some_and(|root| root.close().is_ok());
    }
    let category = match result {
        Ok(()) => None,
        Err(Error::Timeout) => Some("timeout"),
        Err(Error::Invalid(_)) => Some("validation"),
        Err(Error::Identity(_)) => Some("identity"),
        Err(Error::Storage(_)) => Some("storage"),
        Err(Error::Transport(_)) => Some("transport"),
        Err(Error::Mdk(_)) => Some("marmot"),
        Err(Error::Io(_)) => Some("local_io"),
    };
    Ok(
        json!({"schema":1,"ok":category.is_none()&&state_cleanup,"category":category,
        "detail":detail,"cleanup_detail":cleanup_detail,
        "milliseconds":started.elapsed().as_millis(),"samples":samples,
        "engine_quiesced":engine_quiesced,"engine_stop_complete":engine_stop,
        "mdk_stop_complete":mdk_stop,"runtime_cleanup":runtime_cleanup,
        "state_cleanup":state_cleanup,"state_retained":!state_cleanup,
        "restart_while_running":samples.iter().any(|v|v["restart_while_running"]==true),
        "replay_conflict":samples.iter().any(|v|v["remote_conflict_refused"]==true)}),
    )
}

// A fixture allocation receipt, captured before commands execute. Every descent
// and unlink is relative to held descriptors; paths never come from the wire.
pub(super) struct EngineTree {
    parent: std::fs::File,
    directory: std::fs::File,
    parent_id: (u64, u64),
    root_id: (u64, u64),
    name: std::ffi::CString,
}
struct TreeEntry {
    name: std::ffi::CString,
    identity: (u64, u64),
    directory: Option<std::fs::File>,
    children: Vec<TreeEntry>,
}
#[allow(clippy::unnecessary_cast)] // dev_t/ino_t differ between Linux and macOS.
fn inode(stat: &libc::stat) -> (u64, u64) {
    (stat.st_dev as u64, stat.st_ino as u64)
}
fn fd_stat(file: &std::fs::File) -> Result<libc::stat> {
    use std::os::fd::AsRawFd;
    let mut stat = std::mem::MaybeUninit::uninit();
    if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { stat.assume_init() })
}
fn entry_stat(parent: &std::fs::File, name: &std::ffi::CStr) -> Result<libc::stat> {
    use std::os::fd::AsRawFd;
    let mut stat = std::mem::MaybeUninit::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { stat.assume_init() })
}
fn safe_entry(stat: &libc::stat, directory: bool) -> Result<()> {
    if stat.st_uid != unsafe { libc::geteuid() }
        || stat.st_mode & libc::S_IFMT
            != if directory {
                libc::S_IFDIR
            } else {
                libc::S_IFREG
            }
        || stat.st_mode & 0o7777 != if directory { 0o700 } else { 0o600 }
        || (!directory && stat.st_nlink != 1)
    {
        return Err(Error::Invalid("command_check_unsafe_tree"));
    }
    Ok(())
}
fn open_directory(parent: &std::fs::File, name: &std::ffi::CStr) -> Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    safe_entry(&fd_stat(&file)?, true)?;
    Ok(file)
}
#[cfg(target_os = "linux")]
unsafe fn errno_pointer() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}
#[cfg(target_os = "macos")]
unsafe fn errno_pointer() -> *mut libc::c_int {
    unsafe { libc::__error() }
}
struct DirectoryStream(*mut libc::DIR);
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        unsafe { libc::closedir(self.0) };
    }
}
fn entry_names(directory: &std::fs::File) -> Result<Vec<std::ffi::CString>> {
    use std::os::fd::AsRawFd;
    // A fresh open-file description avoids sharing directory offsets between
    // proof scans. fdopendir owns only this new descriptor.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let stream = unsafe { libc::fdopendir(descriptor) };
    if stream.is_null() {
        unsafe { libc::close(descriptor) };
        return Err(std::io::Error::last_os_error().into());
    }
    let stream = DirectoryStream(stream);
    let mut names = Vec::new();
    loop {
        unsafe {
            *errno_pointer() = 0;
        }
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            if unsafe { *errno_pointer() } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name == c"." || name == c".." {
            continue;
        }
        if names.len() >= 4096 {
            return Err(Error::Invalid("command_check_tree_bound"));
        }
        names.push(name.to_owned());
    }
    names.sort();
    Ok(names)
}
fn snapshot_tree(
    directory: &std::fs::File,
    depth: usize,
    count: &mut usize,
) -> Result<Vec<TreeEntry>> {
    if depth > 8 {
        return Err(Error::Invalid("command_check_tree_bound"));
    }
    let names = entry_names(directory)?;
    if depth >= 8 && !names.is_empty() {
        return Err(Error::Invalid("command_check_tree_bound"));
    }
    let mut entries = Vec::new();
    for name in names {
        *count += 1;
        if *count > 4096 {
            return Err(Error::Invalid("command_check_tree_bound"));
        }
        let stat = entry_stat(directory, &name)?;
        let is_directory = stat.st_mode & libc::S_IFMT == libc::S_IFDIR;
        safe_entry(&stat, is_directory)?;
        let child = if is_directory {
            Some(open_directory(directory, &name)?)
        } else {
            None
        };
        if let Some(child) = &child
            && inode(&fd_stat(child)?) != inode(&stat)
        {
            return Err(Error::Invalid("command_check_tree_changed"));
        }
        let children = match &child {
            Some(child) => snapshot_tree(child, depth + 1, count)?,
            None => vec![],
        };
        entries.push(TreeEntry {
            name,
            identity: inode(&stat),
            directory: child,
            children,
        });
    }
    Ok(entries)
}
fn unlink_tree(parent: &std::fs::File, entries: Vec<TreeEntry>) -> Result<()> {
    use std::os::fd::AsRawFd;
    for entry in entries {
        let stat = entry_stat(parent, &entry.name)?;
        safe_entry(&stat, entry.directory.is_some())?;
        if inode(&stat) != entry.identity {
            return Err(Error::Invalid("command_check_tree_changed"));
        }
        let flags = if let Some(directory) = entry.directory {
            if inode(&fd_stat(&directory)?) != entry.identity {
                return Err(Error::Invalid("command_check_tree_changed"));
            }
            unlink_tree(&directory, entry.children)?;
            if inode(&entry_stat(parent, &entry.name)?) != entry.identity {
                return Err(Error::Invalid("command_check_tree_changed"));
            }
            libc::AT_REMOVEDIR
        } else {
            0
        };
        if unsafe { libc::unlinkat(parent.as_raw_fd(), entry.name.as_ptr(), flags) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}
impl EngineTree {
    fn capture(profile: &ProfileLease) -> Result<Self> {
        Self::capture_path(profile.root())
    }
    pub(super) fn capture_path(root: &Path) -> Result<Self> {
        let parent = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)?;
        safe_entry(&fd_stat(&parent)?, true)?;
        let name = c"commands".to_owned();
        let directory = open_directory(&parent, &name)?;
        let root_id = inode(&fd_stat(&directory)?);
        if inode(&entry_stat(&parent, &name)?) != root_id {
            return Err(Error::Invalid("command_check_tree_changed"));
        }
        Ok(Self {
            parent_id: inode(&fd_stat(&parent)?),
            parent,
            directory,
            root_id,
            name,
        })
    }
    fn verify(&self, profile: &ProfileLease) -> Result<()> {
        // Fresh nofollow opens are comparison only, never a replacement receipt.
        self.verify_path(profile.root())
    }
    pub(super) fn verify_path(&self, root: &Path) -> Result<()> {
        let fresh = Self::capture_path(root)?;
        if fresh.parent_id != self.parent_id || fresh.root_id != self.root_id {
            return Err(Error::Invalid("command_check_tree_changed"));
        }
        Ok(())
    }
    // Caller must hold this instance's actual CleanupGuard, profile lease and
    // unchanged final fence throughout the synchronous protected removal.
    fn remove(self) -> Result<()> {
        self.remove_inner()
    }
    #[cfg(feature = "installed-checks")]
    pub(super) async fn remove_installed(
        self,
        guard: &mut super::installed_check::OwnedCleanupGuard<'_>,
    ) -> Result<()> {
        guard.revalidate().await?;
        self.verify_path(guard.profile())?;
        self.remove_inner()?;
        guard.after_unlink().await
    }
    fn remove_inner(self) -> Result<()> {
        use std::os::fd::AsRawFd;
        if inode(&fd_stat(&self.parent)?) != self.parent_id
            || inode(&fd_stat(&self.directory)?) != self.root_id
            || inode(&entry_stat(&self.parent, &self.name)?) != self.root_id
        {
            return Err(Error::Invalid("command_check_tree_changed"));
        }
        let entries = snapshot_tree(&self.directory, 0, &mut 1)?;
        // Prevalidation completed before the first unlink. Any later race is
        // still a red partial cleanup, never permission for fallback rm -rf.
        unlink_tree(&self.directory, entries)?;
        if inode(&entry_stat(&self.parent, &self.name)?) != self.root_id
            || inode(&fd_stat(&self.parent)?) != self.parent_id
        {
            return Err(Error::Invalid("command_check_tree_changed"));
        }
        if unsafe {
            libc::unlinkat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::AT_REMOVEDIR,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
    fn fixture() -> (tempfile::TempDir, ProfileLease) {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = ProfileLease::acquire(root.path(), "account").unwrap();
        std::fs::create_dir(profile.root().join("commands")).unwrap();
        std::fs::set_permissions(
            profile.root().join("commands"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        (root, profile)
    }
    fn file(path: &Path) {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
    }
    #[test]
    fn guarded_tree_receipt_removes_only_registered_owned_subtree() {
        let (_root, profile) = fixture();
        let engine = profile.root().join("commands");
        file(&engine.join("state"));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(engine.join("job"))
            .unwrap();
        file(&engine.join("job/output"));
        let receipt = EngineTree::capture(&profile).unwrap();
        receipt.remove().unwrap();
        assert!(!engine.exists());
        assert!(profile.root().join("owner.lock").exists());
    }
    #[test]
    fn unsafe_entry_fails_before_first_unlink_and_never_follows_link() {
        for hardlink in [false, true] {
            let (_root, profile) = fixture();
            let engine = profile.root().join("commands");
            let outside = profile.root().join("outside");
            file(&outside);
            file(&engine.join("a-before-link"));
            if hardlink {
                std::fs::hard_link(&outside, engine.join("z-link")).unwrap();
            } else {
                symlink(&outside, engine.join("z-link")).unwrap();
            }
            assert!(EngineTree::capture(&profile).unwrap().remove().is_err());
            assert!(outside.exists());
            assert!(engine.join("a-before-link").exists());
        }
    }
    #[test]
    fn replaced_root_receipt_is_rejected_without_unlink() {
        let (_root, profile) = fixture();
        let engine = profile.root().join("commands");
        file(&engine.join("original"));
        let receipt = EngineTree::capture(&profile).unwrap();
        std::fs::rename(&engine, profile.root().join("moved")).unwrap();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&engine)
            .unwrap();
        file(&engine.join("replacement"));
        assert!(receipt.remove().is_err());
        assert!(engine.join("replacement").exists());
        assert!(profile.root().join("moved/original").exists());
    }
    #[test]
    fn excessive_depth_preserves_prevalidated_tree() {
        let (_root, profile) = fixture();
        let engine = profile.root().join("commands");
        file(&engine.join("a-first"));
        let mut nested = engine.clone();
        for _ in 0..9 {
            nested = nested.join("d");
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&nested)
                .unwrap();
        }
        assert!(EngineTree::capture(&profile).unwrap().remove().is_err());
        assert!(engine.join("a-first").exists());
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, symlink};
    #[test]
    fn recovered_prefix_requires_exact_produced_counts_and_gap_free_streams() {
        let out = b"before-restart";
        let err = b"before-stderr";
        let mut status = wire::Status {
            admission: wire::Admission::Engine,
            authority_valid: true,
            cancel_requested: false,
            engine_known: true,
            invocation: [1; 16],
            outcome: None,
            state: wire::State::Running,
            warning: None,
            input: wire::InputStatus {
                accepted: 0,
                delivered: 0,
                eof_sent: true,
                health: wire::InputHealth::Closed,
                length: 0,
            },
            stdout: wire::OutputStatus {
                closed: false,
                gaps: vec![],
                produced: out.len() as u64,
                retained: out.len() as u64,
            },
            stderr: wire::OutputStatus {
                closed: false,
                gaps: vec![],
                produced: err.len() as u64,
                retained: err.len() as u64,
            },
        };
        assert!(recovered_prefix(&status, out, err));
        status.stdout.produced += 1;
        assert!(!recovered_prefix(&status, out, err));
        status.stdout.produced -= 1;
        status.stderr.gaps.push(wire::Gap {
            start: 0,
            end: Some(1),
            reason: wire::GapReason::Expired,
        });
        assert!(!recovered_prefix(&status, out, err));
        status.stderr.gaps.clear();
        status.stderr.closed = true;
        assert!(!recovered_prefix(&status, out, err));
        status.stderr.closed = false;
        assert!(!recovered_prefix(&status, b"wrong", err));
        assert!(!recovered_prefix(&status, out, b"wrong"));
        let report = recovery_report();
        for field in [
            "restart_while_running",
            "recovered_output_verified",
            "identical_submit_existing",
            "probe_native_retained",
            "purpose_ownership_unknown",
        ] {
            assert_eq!(report[field], false);
        }
        for field in [
            "execution_count",
            "restart_stop_ms",
            "restart_open_ms",
            "restart_activation_ms",
        ] {
            assert!(report[field].is_null());
        }
    }
    fn root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        root
    }
    fn marker(path: &Path, bytes: &[u8]) {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(bytes).unwrap();
    }
    #[test]
    fn one_execution_marker_and_release_gate_are_bounded_and_exclusive() {
        let root = root();
        let mut receipt = RecoveryFiles::new(root.path()).unwrap();
        assert!(receipt.verify().is_err());
        marker(&receipt.marker, b"run\n");
        receipt.verify().unwrap();
        receipt.verify().unwrap();
        receipt.release().unwrap();
        assert!(receipt.release().is_err());
        assert_eq!(
            std::fs::metadata(&receipt.gate).unwrap().mode() & 0o777,
            0o600
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(&receipt.marker)
            .unwrap()
            .write_all(b"run\n")
            .unwrap();
        assert!(receipt.verify().is_err());
    }
    #[test]
    fn execution_marker_rejects_links_and_same_content_replacement() {
        let root = root();
        let mut receipt = RecoveryFiles::new(root.path()).unwrap();
        marker(&receipt.marker, b"run\n");
        receipt.verify().unwrap();
        std::fs::rename(&receipt.marker, root.path().join("original")).unwrap();
        marker(&receipt.marker, b"run\n");
        assert!(receipt.verify().is_err());
        std::fs::remove_file(&receipt.marker).unwrap();
        symlink(root.path().join("original"), &receipt.marker).unwrap();
        assert!(receipt.verify().is_err());
        std::fs::remove_file(&receipt.marker).unwrap();
        std::fs::hard_link(root.path().join("original"), &receipt.marker).unwrap();
        assert!(receipt.verify().is_err());
    }
    #[test]
    fn original_engine_receipt_must_survive_reopen_without_recapture() {
        let root = root();
        let profile = ProfileLease::acquire(root.path(), "account").unwrap();
        let commands = profile.root().join("commands");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&commands)
            .unwrap();
        let receipt = EngineTree::capture(&profile).unwrap();
        receipt.verify(&profile).unwrap();
        let other = self::root();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(other.path().join("commands"))
            .unwrap();
        assert!(receipt.verify_path(other.path()).is_err());
        std::fs::rename(&commands, profile.root().join("original")).unwrap();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&commands)
            .unwrap();
        assert!(receipt.verify(&profile).is_err());
        assert!(profile.root().join("original").exists());
    }
}
