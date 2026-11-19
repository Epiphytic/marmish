//! One serialized MDK owner per identity. Status is a watch snapshot, so relay
//! bootstrap and publication never block local status requests.
use crate::{
    Error, Result,
    identity::{BackendChoice, ProfileLease},
    ipc::{self, Pending, Request, Response},
    mdk_adapter::MdkOwner,
    terminal_owner,
    transport::{ArtiDirs, RelayConnector, RelayPolicy},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::os::unix::fs::DirBuilderExt;
use std::{path::PathBuf, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};

#[cfg(test)]
#[path = "peer_candidate_lifecycle_tests.rs"]
pub(crate) mod candidate_lifecycle;

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
    let opened = open_inner(
        config,
        false,
        #[cfg(test)]
        None,
    )
    .await?;
    Ok((opened.profile, opened.owner))
}

struct Opened {
    profile: ProfileLease,
    owner: MdkOwner,
    terminal: Option<terminal_owner::Prepared>,
    #[cfg(test)]
    peer_identity: Option<(
        crate::identity::IdentityMaterial,
        crate::command_config::Role,
    )>,
}

async fn open_inner(
    config: Config,
    terminal: bool,
    #[cfg(test)] fixture: Option<FixtureTransport<'_>>,
) -> Result<Opened> {
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
        policy.clone(),
        ArtiDirs {
            state: profile.arti_state().to_owned(),
            cache: profile.arti_cache().to_owned(),
        },
    )?;
    #[cfg(test)]
    let connector = match fixture
        .as_ref()
        .and_then(|fixture| fixture.connector.cloned())
    {
        Some(selected)
            if selected.policy().urls() == policy.urls()
                && selected.policy().mode() == policy.mode() =>
        {
            selected
        }
        Some(_) => return Err(Error::Invalid("peer_fixture_ordinary_connector")),
        None => connector,
    };
    let lifetime = std::sync::Arc::new(
        origin.constrain(commands.as_ref().and_then(|c| c.limits().max_uptime))?,
    );
    if lifetime.expired()? {
        return Err(Error::Timeout);
    }
    crate::native_preflight::check(&profile, &material)?;
    let mut owner = MdkOwner::open(&profile, &material, connector, peers)?;
    owner.set_retention(retention)?;
    owner.reconcile_client_inventory(commands.as_ref())?;
    profile.mark_ready(owner.opened_binding())?;
    let terminal = match commands.as_ref().filter(|_| terminal) {
        Some(config) => Some(terminal_owner::Prepared::open(
            &profile,
            &material,
            Some(config.clone()),
            policy,
        )?),
        None => None,
    };
    #[cfg(test)]
    let peer_role = commands.as_ref().map(|config| config.role());
    if let Some(commands) = commands {
        let executable = std::env::current_exe()?;
        let executable = executable.as_path();
        #[cfg(test)]
        let executable = fixture.map_or(executable, |fixture| fixture.layout);
        let worker = crate::native_layout::resolve(executable)?.worker;
        owner.enable_commands(&profile, &material, commands, worker)?;
        owner.command_set_lifetime(lifetime);
    }
    Ok(Opened {
        profile,
        owner,
        terminal,
        #[cfg(test)]
        peer_identity: peer_role.map(|role| (material, role)),
    })
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
        Error::DaemonStartup(_) => "daemon_startup",
        Error::Copy(code) => crate::error::copy_category(&code),
        Error::Stream(code) => crate::error::stream_category(&code),
    })
}

async fn dispatch(
    owner: &mut MdkOwner,
    profile: &str,
    rooms: &mut crate::command_local::RoomIntents,
    request: Request,
) -> Result<Response> {
    #[cfg(marmish_peer_receiver_candidate)]
    if matches!(request, Request::PeerRead { .. }) {
        return Err(Error::Invalid("peer_local_operation"));
    }
    #[cfg(test)]
    if matches!(request, Request::Peer { .. }) {
        return Err(Error::Invalid("peer_local_operation"));
    }
    if let Request::ClientInventory { request } = request {
        return Ok(Response::inventory(owner.client_inventory(request)));
    }
    if let Request::Stream { request } = request {
        let result = owner.stream_request_local(request).unwrap_or_else(|error| {
            crate::stream_service::StreamLocalResult::Failure {
                category: crate::command_local::failure_category(error).into(),
            }
        });
        return Ok(Response::stream(result));
    }
    if let Some(response) = owner.command_acceptance(&request)? {
        return Ok(response);
    }
    if request.is_command() {
        let result = owner
            .command_local(profile, &request)
            .unwrap_or_else(crate::command_local::failure);
        let result = match (&request, result) {
            (
                Request::CommandPrepare { remote, fresh, .. },
                crate::command_local::LocalResult::LocalBusy,
            ) => owner.command_enqueue_preparation(
                crate::command_local::RoomKey::parse(remote, fresh.as_deref())?,
                rooms,
                Instant::now(),
            )?,
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
    run_with_startup(config, None).await
}

pub(crate) async fn run_with_startup(
    config: Config,
    startup: Option<crate::daemon_launch::StartupReporter>,
) -> Result<()> {
    run_inner(
        config,
        startup,
        #[cfg(test)]
        None,
        #[cfg(test)]
        None,
    )
    .await
}

#[cfg(test)]
pub(crate) struct PeerFixture {
    pub(crate) bootstrap: Option<crate::peer_invitation::daemon_peer::Bootstrap>,
    pub(crate) connector: Option<RelayConnector>,
    pub(crate) layout: PathBuf,
    pub(crate) stop: tokio::sync::oneshot::Receiver<()>,
}

#[cfg(test)]
struct FixtureTransport<'a> {
    connector: Option<&'a RelayConnector>,
    layout: &'a std::path::Path,
}

#[cfg(test)]
struct ServeFixture<'a> {
    peer_listener: Option<ipc::peer::Listener>,
    peer_control: Option<&'a ipc::peer::Control>,
    stop: Option<tokio::sync::oneshot::Receiver<()>>,
    candidate_stop: Option<tokio::sync::oneshot::Receiver<()>>,
    candidate: Option<&'a mut candidate_lifecycle::Runtime>,
}

#[cfg(test)]
pub(crate) async fn run_peer_fixture(
    config: Config,
    startup: Option<crate::daemon_launch::StartupReporter>,
    fixture: PeerFixture,
) -> Result<()> {
    run_inner(config, startup, Some(fixture), None).await
}

async fn run_inner(
    config: Config,
    startup: Option<crate::daemon_launch::StartupReporter>,
    #[cfg(test)] fixture: Option<PeerFixture>,
    #[cfg(test)] candidate: Option<candidate_lifecycle::Fixture>,
) -> Result<()> {
    use std::sync::Arc;
    #[cfg(test)]
    let transport = fixture
        .as_ref()
        .map(|fixture| FixtureTransport {
            connector: fixture.connector.as_ref(),
            layout: &fixture.layout,
        })
        .or_else(|| candidate.as_ref().map(|fixture| fixture.transport()));
    let opened = open_inner(
        config,
        true,
        #[cfg(test)]
        transport,
    )
    .await?;
    let Opened {
        profile,
        mut owner,
        terminal,
        #[cfg(test)]
        peer_identity,
    } = opened;
    let profile = Arc::new(profile);
    #[cfg(test)]
    let (mut candidate, candidate_stop) = match candidate {
        Some(fixture) => {
            let (runtime, stop) = fixture.start(Arc::clone(&profile));
            (Some(runtime), Some(stop))
        }
        None => (None, None),
    };
    #[cfg(test)]
    let (fixture_stop, bootstrap) = match fixture {
        Some(fixture) => (Some(fixture.stop), fixture.bootstrap),
        None => (None, None),
    };
    #[cfg(test)]
    let (peer_worker, peer_listener, peer_control) = match bootstrap.zip(peer_identity) {
        Some((bootstrap, (material, role))) if bootstrap.eligible(role) => {
            let (worker, listener, control) = ipc::peer::pair(&profile);
            match crate::peer_invitation::daemon_peer::Runtime::start(
                Arc::clone(&profile),
                material,
                role,
                bootstrap,
                worker,
            ) {
                Ok(runtime) => (Some(runtime), Some(listener), Some(control)),
                Err(_) => {
                    control.stop();
                    crate::diagnostics::event("peer_owner", "startup_unavailable");
                    (None, None, None)
                }
            }
        }
        _ => (None, None, None),
    };
    let (runtime, control, setup) = match terminal
        .map(|terminal| terminal_owner::Runtime::start(terminal, profile.clone()))
        .transpose()
    {
        Ok(Some((runtime, control))) => (Some(runtime), Some(control), Ok(())),
        Ok(None) => (None, None, Ok(())),
        Err(error) => (None, None, Err(error)),
    };
    let result = match setup {
        Ok(()) => {
            serve_owner(
                &profile,
                &mut owner,
                startup,
                control,
                #[cfg(test)]
                ServeFixture {
                    peer_listener,
                    peer_control: peer_control.as_ref(),
                    stop: fixture_stop,
                    candidate_stop,
                    candidate: candidate.as_mut(),
                },
            )
            .await
        }
        Err(error) => Err(error),
    };
    #[cfg(test)]
    if let Some(control) = &peer_control {
        control.stop();
    }
    #[cfg(test)]
    if let Some(candidate) = &candidate {
        candidate.stop();
    }
    let joined = match runtime {
        Some(runtime) => runtime.join().await,
        None => Ok(()),
    };
    #[cfg(test)]
    let joined = joined.and(match peer_worker {
        Some(runtime) => runtime.join().await,
        None => Ok(()),
    });
    #[cfg(test)]
    let joined = joined.and(match candidate {
        Some(runtime) => runtime.join().await,
        None => Ok(()),
    });
    // A fallback reaper can still hold the exact lease after the owning
    // workers join. Durable stop waits until that last owner releases it.
    let mut profile = profile;
    let mut incomplete = false;
    let began = crate::command_limits::continuous_time().ok();
    let profile = loop {
        match Arc::try_unwrap(profile) {
            Ok(profile) => break profile,
            Err(leased) => profile = leased,
        }
        let elapsed = began
            .zip(crate::command_limits::continuous_time().ok())
            .and_then(|(began, now)| now.checked_sub(began));
        if !incomplete && elapsed.is_none_or(|elapsed| elapsed >= Duration::from_secs(15)) {
            incomplete = true;
            crate::diagnostics::event("terminal_owner", "owned_stop_incomplete");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    #[cfg(test)]
    let incomplete = incomplete || matches!(result, Err(Error::Identity("owned_stop_incomplete")));
    let stopped = stop_reporting(profile, owner, joined.is_ok() && !incomplete).await;
    result.and(joined).and(if incomplete {
        Err(Error::Identity("owned_stop_incomplete"))
    } else {
        stopped
    })
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
    if !report.engine_stop_complete
        || !report.stream_stop_complete
        || !report.mdk_stop_complete
        || !report.runtime_cleanup_complete
    {
        return Err(Error::Identity("owned_stop_incomplete"));
    }
    Ok(())
}

async fn serve_owner(
    profile: &ProfileLease,
    owner: &mut MdkOwner,
    mut startup: Option<crate::daemon_launch::StartupReporter>,
    mut terminal: Option<terminal_owner::Control>,
    #[cfg(test)] fixture: ServeFixture<'_>,
) -> Result<()> {
    #[cfg(test)]
    let ServeFixture {
        peer_listener,
        peer_control,
        stop: fixture_stop,
        candidate_stop,
        candidate,
    } = fixture;
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
    let terminal_intake = terminal.as_ref().map(|terminal| terminal.intake.clone());
    #[cfg(test)]
    let mut candidate = candidate;
    #[cfg(test)]
    let candidate_listener = candidate
        .as_deref_mut()
        .map(|candidate| candidate.listener(socket));
    #[cfg(test)]
    let candidate_mode = candidate.is_some();
    #[cfg(test)]
    let mut ordinary_ready = false;
    #[cfg(test)]
    let (drain, drain_request) = match candidate_mode || fixture_stop.is_some() {
        true => {
            let (drain, request) = tokio::sync::oneshot::channel();
            (Some(drain), Some(request))
        }
        false => (None, None),
    };
    let mut server = tokio::spawn(async move {
        #[cfg(test)]
        if drain_request.is_some() {
            return ipc::serve_with_listener(
                listener,
                queue,
                snapshots,
                terminal_intake,
                peer_listener,
                drain_request,
                candidate_listener,
                #[cfg(all(
                    marmish_peer_receiver_candidate,
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                ipc::CandidateListener::default(),
            )
            .await;
        }
        ipc::serve_inner(listener, queue, snapshots, terminal_intake).await
    });
    #[cfg(test)]
    let mut listener_joined = false;
    let shutdown_signal = async {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
            _ = async {
                #[cfg(test)]
                if let Some(stop) = fixture_stop.or(candidate_stop) {
                    let _ = stop.await;
                    return;
                }
                std::future::pending::<()>().await;
            } => {},
        }
    };
    tokio::pin!(shutdown_signal);
    #[cfg(test)]
    let track_listener = drain.is_some();
    #[cfg(not(test))]
    let track_listener = false;
    let result = async {
        tokio::select! {
            stopped = &mut server, if track_listener => {
                #[cfg(test)]
                { listener_joined = true; }
                return listener_stop(stopped);
            }
            activated = owner.activate(Instant::now() + Duration::from_secs(150)) => activated?,
            _ = &mut shutdown_signal => return Ok(()),
            _ = &mut expired => return Ok(()),
        }
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        let mut degraded = false;
        let mut last_retry_notice = None;
        loop {
            if owner.command_count_exit_ready()? {break;}
            if let Some(terminal) = terminal.as_mut() {
                terminal.refresh(owner);
            }
            let mut snapshot = serde_json::to_value(owner.status()?)
                .map_err(|_| Error::Invalid("status_encoding"))?;
            snapshot["state"] = serde_json::Value::String(
                if degraded { "transport_degraded" } else { "active" }.into(),
            );
            status.send_replace(Response::success(snapshot));
            if let Some(reporter) = startup.take() {
                reporter.ready().await?;
            }
            #[cfg(test)]
            if let Some(candidate) = candidate.as_deref_mut()
                && !degraded && !ordinary_ready {
                tokio::select! {
                    biased;
                    stopped = &mut server => {
                        listener_joined = true;
                        return listener_stop(stopped);
                    }
                    _ = &mut shutdown_signal => break,
                    _ = &mut expired => break,
                    _ = std::future::ready(()) => {}
                }
                if owner.command_count_exit_ready()? { break; }
                candidate.ordinary_ready();
                ordinary_ready = true;
            }
            tokio::select! {
                stopped = &mut server, if track_listener => {
                    #[cfg(test)]
                    { listener_joined = true; }
                    return listener_stop(stopped);
                }
                _ = &mut shutdown_signal => break,
                _ = &mut expired => break,
                Some(request) = async {
                    match terminal.as_mut() {
                        Some(terminal) => terminal.requests.recv().await,
                        None => std::future::pending().await,
                    }
                } => request.complete(owner),
                pending = receive.recv() => {
                    let Some(pending) = pending else {
                        #[cfg(test)]
                        if candidate_mode {
                            listener_joined = true;
                            return listener_stop((&mut server).await);
                        }
                        break;
                    };
                    if pending.request.uses_short_exchange() && (pending.deadline<=Instant::now() || pending.respond.is_closed()) {continue;}
                    let response = tokio::select! {
                        _ = &mut shutdown_signal => break,
                        _ = &mut expired => break,
                        response = dispatch(owner, profile_name, &mut room_intents, pending.request) => response.unwrap_or_else(failure),
                    };
                    let _ = pending.respond.send(response);
                }
                _ = interval.tick() => {
                    if server.is_finished() {
                        #[cfg(test)]
                        if candidate_mode {
                            listener_joined = true;
                            return listener_stop((&mut server).await);
                        }
                        return Err(Error::Invalid("ipc_listener_stopped"));
                    }
                    let outcome = tokio::select! {
                        _ = &mut shutdown_signal => break,
                        _ = &mut expired => break,
                        result = owner.tick(Instant::now() + Duration::from_secs(15)) => result,
                    };
                    match outcome {
                        Ok(()) => degraded = false,
                        Err(Error::Transport(_) | Error::Timeout) => {
                            degraded = true;
                            let now = Instant::now();
                            if last_retry_notice.is_none_or(|last| now.duration_since(last) >= Duration::from_secs(30)) {
                                crate::diagnostics::event("daemon_transport", "retry_pending");
                                last_retry_notice = Some(now);
                            }
                        }
                        Err(error) => return Err(error),
                    }
                    if let Some(remote)=room_intents.next(Instant::now()) {
                        let deadline=Instant::now()+Duration::from_secs(crate::command_local::ROOM_TURN_SECONDS);
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
                                Error::Invalid("command_preparation_capacity")=>"command_preparation_capacity",
                                Error::Invalid("command_room_uncertain")=>"command_room_uncertain",
                                Error::Invalid("command_room_start_expired")=>"command_room_start_expired",
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
    #[cfg(test)]
    if let Some(candidate) = candidate {
        candidate.stop();
    }
    #[cfg(test)]
    if let Some(drain) = drain {
        if let Some(control) = peer_control {
            control.stop();
        }
        let _ = drain.send(());
        if listener_joined {
            return result;
        }
        let drained = server
            .await
            .map_err(|_| Error::Identity("owned_stop_incomplete"));
        return if candidate_mode {
            result.and(drained.flatten())
        } else {
            result.and(drained?)
        };
    }
    server.abort();
    let _ = server.await;
    result
}

fn listener_stop(outcome: std::result::Result<Result<()>, tokio::task::JoinError>) -> Result<()> {
    match outcome {
        Ok(Ok(())) => Err(Error::Invalid("ipc_listener_stopped")),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(Error::Identity("owned_stop_incomplete")),
    }
}
