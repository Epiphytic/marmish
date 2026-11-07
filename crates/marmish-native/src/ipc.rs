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

#[derive(Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    Stream {
        request: crate::stream_service::StreamLocalRequest,
    },
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
    CommandEvidence {
        after: Option<u64>,
        limit: u32,
    },
    CommandCleanupStatus {},
    CommandLocalCancel {
        cursor: u64,
    },
    CommandPrepareCleanup {},
    CommandPrepare {
        remote: String,
        relays: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fresh: Option<String>,
    },
    CommandSubmit {
        remote: String,
        relays: Vec<String>,
        session: String,
        request_id: String,
        submit: crate::command_wire::Submit,
    },
    CommandStatus {
        handle: crate::command_client::CommandHandle,
        request_id: String,
    },
    CommandCancel {
        handle: crate::command_client::CommandHandle,
        request_id: String,
    },
    CommandPoll {
        handle: crate::command_client::CommandHandle,
        request_id: String,
        stdout_cursor: u64,
        stderr_cursor: u64,
        max_bytes: u32,
    },
    CommandResponse {
        handle: crate::command_client::CommandHandle,
        request_id: String,
    },
    CommandCursors {
        handle: crate::command_client::CommandHandle,
    },
    CommandUnconsumed {
        handle: crate::command_client::CommandHandle,
        limit: usize,
    },
    CommandInvocations {
        after: Option<u64>,
        limit: usize,
    },
    CommandConsume {
        handle: crate::command_client::CommandHandle,
        request_id: String,
        response_sha256: String,
        expected_cursors: Option<crate::command_local::Cursors>,
    },
}

impl Request {
    pub(crate) fn is_command(&self) -> bool {
        !matches!(
            self,
            Self::Status {}
                | Self::OpenConversation { .. }
                | Self::Send { .. }
                | Self::Received { .. }
        )
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub ok: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) command: Option<crate::command_local::LocalResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stream: Option<crate::stream_service::StreamLocalResult>,
}

impl Response {
    pub fn success(value: impl Serialize) -> Self {
        match serde_json::to_value(value) {
            Ok(value) => Self {
                ok: true,
                result: Some(value),
                error: None,
                command: None,
                stream: None,
            },
            Err(_) => Self::failure("response_encoding"),
        }
    }

    pub fn failure(category: &str) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(category.to_owned()),
            command: None,
            stream: None,
        }
    }
    pub(crate) fn command(result: crate::command_local::LocalResult) -> Self {
        Self {
            ok: true,
            result: None,
            error: None,
            command: Some(result),
            stream: None,
        }
    }
    pub(crate) fn stream(result: crate::stream_service::StreamLocalResult) -> Self {
        Self {
            ok: true,
            result: None,
            error: None,
            command: None,
            stream: Some(result),
        }
    }
}

pub(crate) struct Pending {
    pub request: Request,
    pub respond: oneshot::Sender<Response>,
    pub deadline: tokio::time::Instant,
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
    verify_peer_uid(stream.peer_cred()?.uid())
}

fn verify_peer_uid(uid: u32) -> Result<()> {
    if uid != unsafe { libc::geteuid() } {
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

pub(crate) async fn request(path: &Path, request: &Request) -> Result<Response> {
    tokio::time::timeout(Duration::from_secs(180), async {
        let mut stream = UnixStream::connect(path).await?;
        verify_peer(&stream)?;
        write_frame(&mut stream, request).await?;
        read_frame(&mut stream).await
    })
    .await
    .map_err(|_| Error::Timeout)?
}

/// One absolute budget for connect, request IO and the actor's response. Failure
/// after sending Submit is ambiguous; its provisional handle remains recoverable.
pub(crate) async fn command_request(
    path: &Path,
    request: &Request,
) -> Result<crate::command_local::LocalResult> {
    if !request.is_command() {
        return Err(Error::Invalid("command_local_operation"));
    }
    tokio::time::timeout(IO_TIMEOUT, async {
        let mut stream = UnixStream::connect(path).await?;
        verify_peer(&stream)?;
        write_frame(&mut stream, request).await?;
        let response: Response = read_frame(&mut stream).await?;
        if let Some(result) = response.command {
            if !response.ok
                || response.result.is_some()
                || response.error.is_some()
                || response.stream.is_some()
            {
                return Err(Error::Invalid("command_local_response"));
            }
            return Ok(result);
        }
        if response.ok || response.result.is_some() || response.stream.is_some() {
            return Err(Error::Invalid("command_local_response"));
        }
        match response.error.as_deref() {
            Some("busy") => Ok(crate::command_local::LocalResult::LocalBusy),
            Some("operation_uncertain") => Err(Error::Timeout),
            _ => Err(Error::Invalid("command_local_response")),
        }
    })
    .await
    .map_err(|_| Error::Timeout)?
}

/// A bounded same-UID stream operation. Pending remote work never holds IPC open.
pub(crate) async fn stream_request(
    path: &Path,
    request: crate::stream_service::StreamLocalRequest,
) -> Result<crate::stream_service::StreamLocalResult> {
    tokio::time::timeout(IO_TIMEOUT, async {
        let mut stream = UnixStream::connect(path).await?;
        verify_peer(&stream)?;
        write_frame(&mut stream, &Request::Stream { request }).await?;
        let response: Response = read_frame(&mut stream).await?;
        if response.result.is_some() || response.command.is_some() {
            return Err(Error::Invalid("stream_local_response"));
        }
        if !response.ok && response.stream.is_none() {
            return stream_failure(response.error.as_deref().unwrap_or_default());
        }
        if !response.ok || response.error.is_some() {
            return Err(Error::Invalid("stream_local_response"));
        }
        let result = response
            .stream
            .ok_or(Error::Invalid("stream_local_response"))?;
        if let crate::stream_service::StreamLocalResult::Failure { category } = &result {
            return stream_failure(category);
        }
        Ok(result)
    })
    .await
    .map_err(|_| Error::Timeout)?
}

fn stream_failure(category: &str) -> Result<crate::stream_service::StreamLocalResult> {
    if category.is_empty()
        || category.len() > 80
        || !category
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(Error::Invalid("stream_local_response"));
    }
    use std::io::Write as _;
    let _ = writeln!(
        std::io::stderr().lock(),
        "marmish stream: local error {category}"
    );
    Err(Error::Invalid("stream_local_failure"))
}

/// The caller owns the listener task and cancels/joins it on shutdown. At most
/// 64 clients may hold connections; a full owner queue returns busy immediately.
pub(crate) async fn serve(
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
                    let exchange_deadline = tokio::time::Instant::now() + IO_TIMEOUT;
                    let result = tokio::time::timeout(IO_TIMEOUT, read_frame::<Request>(&mut stream)).await;
                    let command = matches!(&result,Ok(Ok(request)) if request.is_command());
                    let response = match result {
                        Ok(Ok(Request::Status {})) => status.borrow().clone(),
                        Ok(Ok(request)) => {
                            let command = request.is_command();
                            let deadline = if command {exchange_deadline - Duration::from_millis(100)} else {tokio::time::Instant::now() + Duration::from_secs(150)};
                            let (respond, receiver) = oneshot::channel();
                            if queue.try_send(Pending { request, respond, deadline }).is_err() {
                                Response::failure("busy")
                            } else {
                                match tokio::time::timeout_at(deadline, receiver).await {
                                    Ok(Ok(response)) => response,
                                    _ => Response::failure("operation_uncertain"),
                                }
                            }
                        }
                        _ => Response::failure("invalid_request"),
                    };
                    let write_deadline = if command {exchange_deadline} else {tokio::time::Instant::now() + IO_TIMEOUT};
                    let _ = tokio::time::timeout_at(write_deadline, write_frame(&mut stream, &response)).await;
                });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_uid_predicate_rejects_foreign_uid() {
        // Predicate proof only: no privileged foreign-UID process is spawned.
        let uid = unsafe { libc::geteuid() };
        assert!(verify_peer_uid(uid).is_ok());
        assert!(matches!(
            verify_peer_uid(uid.wrapping_add(1)),
            Err(Error::Invalid("ipc_peer_uid"))
        ));
    }

    #[test]
    fn command_requests_are_closed_and_reject_duplicate_fields() {
        for body in [
            r#"{"operation":"command_prepare","remote":"00","remote":"11","relays":[]}"#,
            r#"{"operation":"command_prepare","remote":"00","relays":[],"extra":0}"#,
            r#"{"operation":"command_invocations","after":true,"limit":1}"#,
            r#"{"operation":"command_invocations","after":null,"limit":-1}"#,
        ] {
            assert!(serde_json::from_str::<Request>(body).is_err(), "{body}");
        }
    }

    #[test]
    fn preparation_fresh_is_optional_closed_and_roundtrips() {
        let old = r#"{"operation":"command_prepare","remote":"00","relays":[]}"#;
        let parsed: Request = serde_json::from_str(old).unwrap();
        assert!(matches!(
            parsed,
            Request::CommandPrepare { fresh: None, .. }
        ));
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            serde_json::from_str::<serde_json::Value>(old).unwrap()
        );
        let fresh = Request::CommandPrepare {
            remote: "00".into(),
            relays: vec![],
            fresh: Some("11".into()),
        };
        assert!(matches!(
            serde_json::from_slice::<Request>(&serde_json::to_vec(&fresh).unwrap()).unwrap(),
            Request::CommandPrepare { fresh: Some(_), .. }
        ));
        for body in [
            r#"{"operation":"command_prepare","remote":"00","relays":[],"fresh":true}"#,
            r#"{"operation":"command_prepare","remote":"00","relays":[],"fresh":"a","fresh":"b"}"#,
        ] {
            assert!(serde_json::from_str::<Request>(body).is_err());
        }
    }
    #[tokio::test(start_paused = true)]
    async fn command_exchange_has_one_absolute_deadline() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.path().join("s");
        let listener = bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let started = tokio::time::Instant::now();
        assert!(matches!(
            command_request(
                &socket,
                &Request::CommandInvocations {
                    after: None,
                    limit: 1
                }
            )
            .await,
            Err(Error::Timeout)
        ));
        assert_eq!(tokio::time::Instant::now() - started, IO_TIMEOUT);
        server.abort();
        let _ = server.await;
    }

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
    async fn stream_errors_preserve_categories_and_reject_mixed_or_untrusted_diagnostics() {
        use crate::stream_service::{StreamLocalRequest, StreamLocalResult};
        let denied = Response::stream(StreamLocalResult::Failure {
            category: "stream_admin_not_found".into(),
        });
        let untrusted = Response::stream(StreamLocalResult::Failure {
            category: "error\nextra diagnostic".into(),
        });
        let mut mixed = denied.clone();
        mixed.result = Some(serde_json::json!({}));
        for (response, expected) in [
            (denied, "stream_local_failure"),
            (Response::failure("busy"), "stream_local_failure"),
            (untrusted, "stream_local_response"),
            (mixed, "stream_local_response"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            let socket = directory.path().join("s");
            let listener = bind(&socket).unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                assert!(matches!(
                    read_frame::<Request>(&mut socket).await.unwrap(),
                    Request::Stream { .. }
                ));
                write_frame(&mut socket, &response).await.unwrap();
            });
            assert!(matches!(
                stream_request(&socket, StreamLocalRequest::List { remote: None }).await,
                Err(Error::Invalid(category)) if category == expected
            ));
            server.await.unwrap();
        }
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
                deadline: tokio::time::Instant::now() + IO_TIMEOUT,
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
