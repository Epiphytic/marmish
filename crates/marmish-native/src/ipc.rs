//! Bounded same-user IPC. Network operations remain in the single MDK owner.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::Path,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{mpsc, oneshot, watch},
};

const FRAME_LIMIT: usize = 65_536;
const IO_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Status {},
    OpenConversation {
        peer: String,
    },
    Send {
        session: String,
        id: String,
        data: String,
    },
    Received {
        after: Option<u64>,
        limit: usize,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub ok: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl Response {
    pub fn success(value: impl Serialize) -> Self {
        match serde_json::to_value(value) {
            Ok(value) => Self {
                ok: true,
                result: Some(value),
                error: None,
            },
            Err(_) => Self::failure("response_encoding"),
        }
    }

    pub fn failure(category: &str) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(category.to_owned()),
        }
    }
}

pub struct Pending {
    pub request: Request,
    pub respond: oneshot::Sender<Response>,
}

/// Binds only inside a private directory owned by this user. The profile lease
/// must already be held by the caller before removing a stale socket.
pub fn bind(path: &Path) -> Result<UnixListener> {
    let parent = path.parent().ok_or(Error::Invalid("socket_parent"))?;
    let metadata = std::fs::symlink_metadata(parent)?;
    let uid = unsafe { libc::geteuid() };
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(Error::Invalid("socket_directory_permissions"));
    }
    if path.as_os_str().as_encoded_bytes().len() >= 104 {
        return Err(Error::Invalid("socket_path_length"));
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() && meta.uid() == uid => {
            // A held lease establishes sole ownership of this profile path.
            std::fs::remove_file(path)?;
        }
        Ok(_) => return Err(Error::Invalid("unsafe_socket_path")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn verify_peer(stream: &UnixStream) -> Result<()> {
    let credentials = stream.peer_cred()?;
    if credentials.uid() != unsafe { libc::geteuid() } {
        return Err(Error::Invalid("ipc_peer_uid"));
    }
    Ok(())
}

async fn read_frame<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> Result<T> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > FRAME_LIMIT {
        return Err(Error::Invalid("ipc_frame_size"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("ipc_frame_json"))
}

async fn write_frame(stream: &mut UnixStream, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Invalid("ipc_frame_json"))?;
    if bytes.is_empty() || bytes.len() > FRAME_LIMIT {
        return Err(Error::Invalid("ipc_frame_size"));
    }
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    stream.shutdown().await?;
    Ok(())
}

pub async fn request(path: &Path, request: &Request) -> Result<Response> {
    tokio::time::timeout(Duration::from_secs(180), async {
        let mut stream = UnixStream::connect(path).await?;
        verify_peer(&stream)?;
        write_frame(&mut stream, request).await?;
        read_frame(&mut stream).await
    })
    .await
    .map_err(|_| Error::Timeout)?
}

/// The caller owns the listener task and cancels/joins it on shutdown. At most
/// 64 clients may hold connections; a full owner queue returns busy immediately.
pub async fn serve(
    listener: UnixListener,
    queue: mpsc::Sender<Pending>,
    status: watch::Receiver<Response>,
) -> Result<()> {
    let clients = std::sync::Arc::new(tokio::sync::Semaphore::new(64));
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(_) => {
                        crate::diagnostics::event("ipc_accept", "retry_pending");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                if verify_peer(&stream).is_err() { continue; }
                let Ok(permit) = clients.clone().try_acquire_owned() else { continue };
                let queue = queue.clone();
                let status = status.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let result = tokio::time::timeout(IO_TIMEOUT, read_frame::<Request>(&mut stream)).await;
                    let response = match result {
                        Ok(Ok(Request::Status {})) => status.borrow().clone(),
                        Ok(Ok(request)) => {
                            let (respond, receiver) = oneshot::channel();
                            if queue.try_send(Pending { request, respond }).is_err() {
                                Response::failure("busy")
                            } else {
                                match tokio::time::timeout(Duration::from_secs(150), receiver).await {
                                    Ok(Ok(response)) => response,
                                    _ => Response::failure("operation_uncertain"),
                                }
                            }
                        }
                        _ => Response::failure("invalid_request"),
                    };
                    let _ = tokio::time::timeout(IO_TIMEOUT, write_frame(&mut stream, &response)).await;
                });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversize_frame_is_rejected_before_body_allocation() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender.write_u32(FRAME_LIMIT as u32 + 1).await.unwrap();
        assert!(matches!(
            read_frame::<Request>(&mut receiver).await,
            Err(Error::Invalid("ipc_frame_size"))
        ));
    }

    #[tokio::test]
    async fn rejects_unknown_request_fields_and_authenticates_peer() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        verify_peer(&receiver).unwrap();
        let body = br#"{"operation":"status","extra":true}"#;
        sender.write_u32(body.len() as u32).await.unwrap();
        sender.write_all(body).await.unwrap();
        assert!(read_frame::<Request>(&mut receiver).await.is_err());
    }

    #[tokio::test]
    async fn status_remains_available_with_saturated_owner_queue() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.path().join("s");
        let listener = bind(&socket).unwrap();
        let (queue, _owner) = mpsc::channel(1);
        let (respond, _response) = oneshot::channel();
        queue
            .try_send(Pending {
                request: Request::Received {
                    after: None,
                    limit: 1,
                },
                respond,
            })
            .ok()
            .unwrap();
        let (_publish, snapshots) =
            watch::channel(Response::success(serde_json::json!({"state":"bootstrap"})));
        let server = tokio::spawn(serve(listener, queue, snapshots));
        let status = request(&socket, &Request::Status {}).await.unwrap();
        assert!(status.ok);
        assert_eq!(status.result.unwrap()["state"], "bootstrap");
        let busy = request(
            &socket,
            &Request::OpenConversation {
                peer: "ab".repeat(32),
            },
        )
        .await
        .unwrap();
        assert!(!busy.ok);
        assert_eq!(busy.error.as_deref(), Some("busy"));
        server.abort();
        let _ = server.await;
    }
}
