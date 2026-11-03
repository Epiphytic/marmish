//! One serialized MDK owner per identity. Status is a watch snapshot, so relay
//! bootstrap and publication never block local status requests.
use crate::{
    Error, Result,
    identity::{BackendChoice, ProfileLease},
    ipc::{self, Pending, Request, Response},
    mdk_adapter::MdkOwner,
    transport::{ArtiDirs, RelayConnector, RelayPolicy},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::os::unix::fs::DirBuilderExt;
use std::{path::PathBuf, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};

pub struct Config {
    pub state_dir: PathBuf,
    pub identity: String,
    pub backend: BackendChoice,
    pub helper: PathBuf,
    pub relay_policy: RelayPolicy,
    pub allowed_peers: Vec<String>,
    pub retention: Duration,
    pub max_commands: Option<std::num::NonZeroU64>,
    pub max_uptime: Option<Duration>,
}

pub async fn open(config: Config) -> Result<(ProfileLease, MdkOwner)> {
    let origin = crate::command_limits::ServiceLifetime::start(None)?;
    let (mut profile, material, policy, peers, retention, commands) =
        tokio::task::spawn_blocking(move || {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&config.state_dir)?;
            let mut profile = ProfileLease::acquire(&config.state_dir, &config.identity)?;
            let mut commands = match std::fs::symlink_metadata(profile.root().join("config.json")) {
                Ok(_) => Some(crate::command_config::CommandConfig::load(profile.root())?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            if let Some(commands) = &mut commands {
                commands.apply_limit_overrides(crate::command_limits::LimitOverrides {
                    max_commands: config.max_commands,
                    max_uptime: config.max_uptime,
                })?;
            } else if config.max_commands.is_some() || config.max_uptime.is_some() {
                return Err(Error::Invalid("commands_disabled"));
            }
            let mut peers = config.allowed_peers;
            if let Some(commands) = &commands {
                if commands.relay_policy().urls() != config.relay_policy.urls()
                    || commands.relay_policy().mode() != config.relay_policy.mode()
                {
                    return Err(Error::Invalid("command_runtime_binding"));
                }
                peers.extend(
                    commands
                        .trusted_remotes()
                        .iter()
                        .chain(commands.execution_clients())
                        .map(hex::encode),
                );
                peers.sort();
                peers.dedup();
            }
            let material = profile.prepare(
                config.backend,
                &config.helper,
                std::time::Instant::now() + Duration::from_secs(30),
            )?;
            Ok::<_, Error>((
                profile,
                material,
                config.relay_policy,
                peers,
                config.retention,
                commands,
            ))
        })
        .await
        .map_err(|_| Error::Identity("initialization_worker"))??;
    let connector = RelayConnector::new(
        policy,
        ArtiDirs {
            state: profile.arti_state().to_owned(),
            cache: profile.arti_cache().to_owned(),
        },
    )?;
    let lifetime = std::sync::Arc::new(
        origin.constrain(commands.as_ref().and_then(|c| c.limits().max_uptime))?,
    );
    if lifetime.expired()? {
        return Err(Error::Timeout);
    }
    crate::native_preflight::check(&profile, &material)?;
    let mut owner = MdkOwner::open(&profile, &material, connector, peers)?;
    owner.set_retention(retention)?;
    profile.mark_ready(owner.opened_binding())?;
    if let Some(commands) = commands {
        let worker = crate::native_layout::resolve(&std::env::current_exe()?)?.worker;
        owner.enable_commands(&profile, &material, commands, worker)?;
        owner.command_set_lifetime(lifetime);
    }
    Ok((profile, owner))
}

fn failure(error: Error) -> Response {
    Response::failure(match error {
        Error::Io(_) => "local_io",
        Error::Invalid(_) => "invalid_input",
        Error::Identity(_) => "identity",
        Error::Storage(_) => "storage",
        Error::Transport(_) => "transport",
        Error::Mdk(_) => "marmot",
        Error::Timeout => "deadline",
    })
}

async fn dispatch(
    owner: &mut MdkOwner,
    profile: &str,
    rooms: &mut crate::command_local::RoomIntents,
    request: Request,
) -> Result<Response> {
    if let Some(response) = owner.command_acceptance(&request)? {
        return Ok(response);
    }
    if request.is_command() {
        let result = owner
            .command_local(profile, &request)
            .unwrap_or_else(crate::command_local::failure);
        let result = match (&request, result) {
            (
                Request::CommandPrepare { remote, .. },
                crate::command_local::LocalResult::LocalBusy,
            ) => {
                let bytes = hex::decode(remote).map_err(|_| Error::Invalid("command_remote"))?;
                rooms.enqueue(
                    bytes
                        .try_into()
                        .map_err(|_| Error::Invalid("command_remote"))?,
                    Instant::now(),
                )
            }
            (_, result) => result,
        };
        return Ok(Response::command(result));
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    match request {
        Request::Status {} => Ok(Response::success(owner.status()?)),
        Request::OpenConversation { peer } => Ok(Response::success(
            owner.open_conversation(&peer, deadline).await?,
        )),
        Request::Send { session, id, data } => {
            if data.len() > 21_848 {
                return Err(Error::Invalid("payload_size"));
            }
            let bytes = STANDARD
                .decode(&data)
                .map_err(|_| Error::Invalid("payload_base64"))?;
            if STANDARD.encode(&bytes) != data {
                return Err(Error::Invalid("payload_base64"));
            }
            Ok(Response::success(
                owner.send(&session, &id, &bytes, deadline).await?,
            ))
        }
        Request::Received { after, limit } => {
            if limit == 0 || limit > 2 {
                return Err(Error::Invalid("received_limit"));
            }
            let messages = owner.received_page(after, limit)?;
            let public = messages.into_iter().map(|message| serde_json::json!({
                "cursor":message.cursor,"session":message.session,"application_id":message.application_id,
                "sender":message.sender,"data":STANDARD.encode(message.data),"invalidated":message.invalidated
            })).collect::<Vec<_>>();
            Ok(Response::success(public))
        }
        _ => Err(Error::Invalid("command_local_operation")),
    }
}

pub async fn run(config: Config) -> Result<()> {
    let (profile, mut owner) = open(config).await?;
    let result = serve_owner(&profile, &mut owner).await;
    let stopped = stop(profile, owner).await;
    result.and(stopped)
}

pub(crate) async fn stop(profile: ProfileLease, owner: MdkOwner) -> Result<()> {
    stop_reporting(profile, owner, true).await
}

pub(crate) async fn stop_silent(profile: ProfileLease, owner: MdkOwner) -> Result<()> {
    stop_reporting(profile, owner, false).await
}

async fn stop_reporting(profile: ProfileLease, owner: MdkOwner, emit: bool) -> Result<()> {
    use crate::command_lifecycle::{OwnedIdentity, stop_owned};
    let observed = stop_owned(
        OwnedIdentity { owner, profile },
        Instant::now() + Duration::from_secs(30),
    )
    .await;
    let report = match observed.pending {
        Some(pending) => {
            crate::diagnostics::event("daemon_shutdown", "engine_join_pending");
            pending.finish().await
        }
        None => observed.report,
    };
    use std::io::Write;
    let encoded = serde_json::to_string(&report).map_err(|_| Error::Invalid("shutdown_report"))?;
    if emit {
        writeln!(std::io::stdout().lock(), "{encoded}")?;
    }
    if !report.engine_stop_complete || !report.mdk_stop_complete || !report.runtime_cleanup_complete
    {
        return Err(Error::Identity("owned_stop_incomplete"));
    }
    Ok(())
}

async fn serve_owner(profile: &ProfileLease, owner: &mut MdkOwner) -> Result<()> {
    let lifetime = owner.command_lifetime();
    let expired = async move {
        loop {
            if lifetime
                .as_ref()
                .is_some_and(|l| l.expired().unwrap_or(true))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::pin!(expired);
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let socket = profile.socket_path();
    let profile_name = profile
        .root()
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(Error::Invalid("identity_name"))?;
    let mut room_intents = crate::command_local::RoomIntents::default();
    let listener = ipc::bind(&socket)?;
    let (queue, mut receive) = mpsc::channel::<Pending>(64);
    let (status, snapshots) =
        watch::channel(Response::success(serde_json::json!({"state":"activating"})));
    let server = tokio::spawn(ipc::serve(listener, queue, snapshots));
    let shutdown_signal = async {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    };
    tokio::pin!(shutdown_signal);
    let result = async {
        tokio::select! {
            activated = owner.activate(Instant::now() + Duration::from_secs(150)) => activated?,
            _ = &mut shutdown_signal => return Ok(()),
            _ = &mut expired => return Ok(()),
        }
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        let mut degraded = false;
        loop {
            if owner.command_count_exit_ready()? {break;}
            let mut snapshot = serde_json::to_value(owner.status()?)
                .map_err(|_| Error::Invalid("status_encoding"))?;
            snapshot["state"] = serde_json::Value::String(
                if degraded { "transport_degraded" } else { "active" }.into(),
            );
            status.send_replace(Response::success(snapshot));
            tokio::select! {
                _ = &mut shutdown_signal => break,
                _ = &mut expired => break,
                pending = receive.recv() => {
                    let Some(pending) = pending else { break };
                    if pending.request.is_command() && (pending.deadline<=Instant::now() || pending.respond.is_closed()) {continue;}
                    let response = tokio::select! {
                        _ = &mut shutdown_signal => break,
                        _ = &mut expired => break,
                        response = dispatch(owner, profile_name, &mut room_intents, pending.request) => response.unwrap_or_else(failure),
                    };
                    let _ = pending.respond.send(response);
                }
                _ = interval.tick() => {
                    if server.is_finished() { return Err(Error::Invalid("ipc_listener_stopped")); }
                    let outcome = tokio::select! {
                        _ = &mut shutdown_signal => break,
                        _ = &mut expired => break,
                        result = owner.tick(Instant::now() + Duration::from_secs(15)) => result,
                    };
                    match outcome {
                        Ok(()) => degraded = false,
                        Err(Error::Transport(_) | Error::Timeout) => {
                            degraded = true;
                            crate::diagnostics::event("daemon_transport", "retry_pending");
                        }
                        Err(error) => return Err(error),
                    }
                    if let Some(remote)=room_intents.next(Instant::now()) {
                        let deadline=Instant::now()+Duration::from_secs(15);
                        let result=tokio::select! {
                            _ = &mut shutdown_signal => break,
                            _ = &mut expired => break,
                            result=owner.command_open_room_turn(remote,deadline)=>result,
                        };
                        if let Err(error)=result {
                            let category=match error {
                                Error::Timeout=>"command_room_discovery_timeout",
                                Error::Invalid("command_unauthorized")=>"command_room_unauthorized",
                                Error::Invalid("command_room_capacity")=>"command_room_capacity",
                                Error::Invalid("clock_behind_floor")=>"command_room_clock",
                                Error::Storage(_)=>"command_room_cleanup",
                                _=>"command_room_creation",
                            };
                            room_intents.failed(remote,Instant::now(),category);
                        }
                    }
                }
            }
        }
        Ok::<_, Error>(())
    }.await;
    server.abort();
    let _ = server.await;
    result
}
