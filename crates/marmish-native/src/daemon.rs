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
}

pub async fn open(config: Config) -> Result<(ProfileLease, MdkOwner)> {
    let (mut profile, material, policy, peers, retention) =
        tokio::task::spawn_blocking(move || {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&config.state_dir)?;
            let mut profile = ProfileLease::acquire(&config.state_dir, &config.identity)?;
            let material = profile.prepare(
                config.backend,
                &config.helper,
                std::time::Instant::now() + Duration::from_secs(30),
            )?;
            Ok::<_, Error>((
                profile,
                material,
                config.relay_policy,
                config.allowed_peers,
                config.retention,
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
    let mut owner = MdkOwner::open(&profile, &material, connector, peers)?;
    owner.set_retention(retention)?;
    profile.mark_ready(owner.opened_binding())?;
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

async fn dispatch(owner: &mut MdkOwner, request: Request) -> Result<Response> {
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
    }
}

pub async fn run(config: Config) -> Result<()> {
    let (profile, mut owner) = open(config).await?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let socket = profile.socket_path();
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
        }
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        let mut degraded = false;
        loop {
            let mut snapshot = serde_json::to_value(owner.status()?)
                .map_err(|_| Error::Invalid("status_encoding"))?;
            snapshot["state"] = serde_json::Value::String(
                if degraded { "transport_degraded" } else { "active" }.into(),
            );
            status.send_replace(Response::success(snapshot));
            tokio::select! {
                _ = &mut shutdown_signal => break,
                pending = receive.recv() => {
                    let Some(pending) = pending else { break };
                    let response = tokio::select! {
                        _ = &mut shutdown_signal => break,
                        response = dispatch(&mut owner, pending.request) => response.unwrap_or_else(failure),
                    };
                    let _ = pending.respond.send(response);
                }
                _ = interval.tick() => {
                    if server.is_finished() { return Err(Error::Invalid("ipc_listener_stopped")); }
                    let outcome = tokio::select! {
                        _ = &mut shutdown_signal => break,
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
                }
            }
        }
        Ok::<_, Error>(())
    }.await;
    server.abort();
    let _ = server.await;
    let shutdown = owner
        .shutdown(Instant::now() + Duration::from_secs(30))
        .await;
    let cleanup = std::fs::remove_file(&socket);
    result?;
    shutdown?;
    cleanup?;
    drop(profile);
    Ok(())
}
