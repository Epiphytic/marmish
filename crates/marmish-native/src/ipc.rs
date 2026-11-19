//! Bounded same-user IPC. Network operations remain in the single MDK owner.
use crate::client_inventory::{
    ClientInventoryReply, INVENTORY_VERSION, InventoryError, InventoryRequest, InventoryResult,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::Path,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{mpsc, oneshot, watch},
};

const FRAME_LIMIT: usize = 65_536;
const IO_TIMEOUT: Duration = Duration::from_secs(15);

#[path = "terminal_socket_handshake.rs"]
pub(crate) mod terminal_handshake;

#[cfg(test)]
#[path = "peer_pending_ipc.rs"]
pub(crate) mod peer;

#[cfg(any(test, marmish_peer_receiver_candidate))]
#[path = "peer_ipc_selection.rs"]
mod peer_selection;

#[cfg(marmish_peer_receiver_candidate)]
#[path = "peer_read_ipc.rs"]
pub(crate) mod peer_read;

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "peer_read_listener.rs"]
pub(crate) mod peer_read_listener;

#[derive(Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    #[cfg(test)]
    Peer {
        request: peer::WireRequest,
    },
    #[cfg(marmish_peer_receiver_candidate)]
    PeerRead {
        request: peer_read::Request,
    },
    TerminalStart {
        request: terminal_handshake::Start,
    },
    ClientInventory {
        request: InventoryRequest,
    },
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
        #[cfg(marmish_peer_receiver_candidate)]
        if matches!(self, Self::PeerRead { .. }) {
            return false;
        }
        #[cfg(test)]
        if matches!(self, Self::Peer { .. }) {
            return false;
        }
        if matches!(self, Self::TerminalStart { .. }) {
            return false;
        }
        !matches!(
            self,
            Self::Status {}
                | Self::ClientInventory { .. }
                | Self::OpenConversation { .. }
                | Self::Send { .. }
                | Self::Received { .. }
        )
    }
    pub(crate) fn uses_short_exchange(&self) -> bool {
        #[cfg(marmish_peer_receiver_candidate)]
        if matches!(self, Self::PeerRead { .. }) {
            return true;
        }
        #[cfg(test)]
        if matches!(self, Self::Peer { .. }) {
            return true;
        }
        self.is_command() || matches!(self, Self::ClientInventory { .. })
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) terminal_stream: Option<crate::terminal_ipc_wire::Role>,
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
                terminal_stream: None,
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
            terminal_stream: None,
        }
    }
    pub(crate) fn command(result: crate::command_local::LocalResult) -> Self {
        Self {
            ok: true,
            result: None,
            error: None,
            command: Some(result),
            stream: None,
            terminal_stream: None,
        }
    }
    pub(crate) fn stream(result: crate::stream_service::StreamLocalResult) -> Self {
        Self {
            ok: true,
            result: None,
            error: None,
            command: None,
            stream: Some(result),
            terminal_stream: None,
        }
    }

    pub(crate) fn inventory(
        result: std::result::Result<ClientInventoryReply, InventoryError>,
    ) -> Self {
        let reply = match result {
            Ok(reply) => reply,
            Err(error) => return Self::failure(error.category()),
        };
        let response = Self::success(reply);
        if !response.ok {
            return Self::failure(InventoryError::ResponseEncoding.category());
        }
        match serde_json::to_vec(&response) {
            Ok(bytes) if bytes.len() <= FRAME_LIMIT => response,
            Ok(_) => Self::failure(InventoryError::ResponseTooLarge.category()),
            Err(_) => Self::failure(InventoryError::ResponseEncoding.category()),
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

fn verify_peer(stream: &UnixStream) -> Result<u32> {
    let uid = stream.peer_cred()?.uid();
    verify_peer_uid(uid)?;
    Ok(uid)
}

fn verify_peer_uid(uid: u32) -> Result<()> {
    if uid != unsafe { libc::geteuid() } {
        return Err(Error::Invalid("ipc_peer_uid"));
    }
    Ok(())
}

async fn read_frame<T: for<'de> Deserialize<'de>>(
    stream: &mut (impl AsyncRead + Unpin),
) -> Result<T> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > FRAME_LIMIT {
        return Err(Error::Invalid("ipc_frame_size"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("ipc_frame_json"))
}

async fn write_frame_body(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &impl Serialize,
) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Invalid("ipc_frame_json"))?;
    if bytes.is_empty() || bytes.len() > FRAME_LIMIT {
        return Err(Error::Invalid("ipc_frame_size"));
    }
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    Ok(())
}

async fn write_frame(stream: &mut (impl AsyncWrite + Unpin), value: &impl Serialize) -> Result<()> {
    write_frame_body(stream, value).await?;
    stream.shutdown().await?;
    Ok(())
}

#[derive(Clone, Default)]
struct SocketFence {
    #[cfg(all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    fatal: Option<crate::dispatcher_fatal::Fatal>,
}

impl SocketFence {
    fn tripped(&self) -> bool {
        #[cfg(all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(fatal) = &self.fatal {
            return fatal.is_tripped();
        }
        false
    }

    #[cfg(all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    async fn tripped_wait(&self) {
        if let Some(fatal) = &self.fatal {
            fatal.tripped().await;
        } else {
            std::future::pending().await
        }
    }

    async fn read(&self, stream: &mut UnixStream) -> Result<Request> {
        #[cfg(all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(fatal) = &self.fatal {
            return read_frame(&mut crate::dispatcher_fatal::FencedStream::new(
                stream, fatal,
            ))
            .await;
        }
        read_frame(stream).await
    }

    async fn write(&self, stream: &mut UnixStream, response: &Response) -> Result<()> {
        #[cfg(all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(fatal) = &self.fatal {
            return write_frame(
                &mut crate::dispatcher_fatal::FencedStream::new(stream, fatal),
                response,
            )
            .await;
        }
        write_frame(stream, response).await
    }
}

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
#[derive(Default)]
pub(crate) struct CandidateListener {
    fatal: Option<crate::dispatcher_fatal::Fatal>,
    registration: CandidateRegistration,
}

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
#[derive(Default)]
enum CandidateRegistration {
    #[default]
    None,
    View(peer_read_listener::Offer),
    Passive(PassiveRegistration),
}

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(crate) struct PassiveRegistration {
    pub(crate) path: std::path::PathBuf,
    pub(crate) registration: crate::peer_candidate_startup::ListenerRegistration,
}

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
impl PassiveRegistration {
    fn register(self, listener: &UnixListener) -> Option<Self> {
        let bound = listener.local_addr().ok()?;
        if bound.as_pathname() != Some(self.path.as_path()) {
            return None;
        }
        self.registration.registered();
        Some(self)
    }
}

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
impl CandidateListener {
    #[cfg(test)]
    pub(crate) fn view_reader_for_test(&self) -> Option<peer_read::pending_view::Reader> {
        match &self.registration {
            CandidateRegistration::View(offer) => Some(offer.reader_for_test()),
            _ => None,
        }
    }

    pub(crate) fn registered(
        fatal: crate::dispatcher_fatal::Fatal,
        offer: Option<peer_read_listener::Offer>,
    ) -> Self {
        Self {
            fatal: Some(fatal),
            registration: offer.map_or(CandidateRegistration::None, CandidateRegistration::View),
        }
    }

    pub(crate) fn passive(
        fatal: crate::dispatcher_fatal::Fatal,
        registration: PassiveRegistration,
    ) -> Self {
        Self {
            fatal: Some(fatal),
            registration: CandidateRegistration::Passive(registration),
        }
    }
}

async fn exchange(path: &Path, request: &Request, timeout: Duration) -> Result<Response> {
    tokio::time::timeout(timeout, async {
        let mut stream = UnixStream::connect(path).await?;
        verify_peer(&stream)?;
        write_frame(&mut stream, request).await?;
        read_frame(&mut stream).await
    })
    .await
    .map_err(|_| Error::Timeout)?
}

#[cfg(all(
    test,
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(crate) async fn send_fault_fixture_status(stream: &mut UnixStream) -> Result<()> {
    verify_peer(stream)?;
    write_frame(stream, &Request::Status {}).await
}

#[cfg(all(
    test,
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(crate) async fn read_fault_fixture_status(stream: &mut UnixStream) -> Result<Response> {
    read_frame(stream).await
}

pub(crate) async fn request(path: &Path, request: &Request) -> Result<Response> {
    exchange(path, request, Duration::from_secs(180)).await
}

/// Read-only local query with one deadline and an exclusive typed reply.
pub(crate) async fn client_inventory_request(
    path: &Path,
    request: InventoryRequest,
) -> Result<ClientInventoryReply> {
    let listing = matches!(request, InventoryRequest::List { .. });
    let response = exchange(path, &Request::ClientInventory { request }, IO_TIMEOUT).await?;
    #[cfg(test)]
    if response.terminal_stream.is_some() {
        return Err(Error::Invalid("inventory_response"));
    }
    if response.command.is_some() || response.stream.is_some() {
        return Err(Error::Invalid("inventory_response"));
    }
    match (response.ok, response.result, response.error.as_deref()) {
        (true, Some(result), None) => {
            let reply: ClientInventoryReply =
                serde_json::from_value(result).map_err(|_| Error::Invalid("inventory_response"))?;
            if reply.version != INVENTORY_VERSION
                || listing != matches!(reply.result, InventoryResult::List { .. })
            {
                return Err(Error::Invalid("inventory_response"));
            }
            Ok(reply)
        }
        (false, None, Some("busy")) => Err(Error::Invalid("inventory_busy")),
        (false, None, Some("operation_uncertain")) => Err(Error::Timeout),
        (false, None, Some(category)) => Err(Error::Invalid(
            InventoryError::from_category(category)
                .map_or("inventory_response", |error| error.category()),
        )),
        _ => Err(Error::Invalid("inventory_response")),
    }
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
        #[cfg(test)]
        if response.terminal_stream.is_some() {
            return Err(Error::Invalid("command_local_response"));
        }
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
        #[cfg(test)]
        if response.terminal_stream.is_some() {
            return Err(Error::Invalid("stream_local_response"));
        }
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
#[cfg(test)]
pub(crate) async fn serve(
    listener: UnixListener,
    queue: mpsc::Sender<Pending>,
    status: watch::Receiver<Response>,
) -> Result<()> {
    serve_inner(listener, queue, status, None).await
}

pub(crate) async fn serve_inner(
    listener: UnixListener,
    queue: mpsc::Sender<Pending>,
    status: watch::Receiver<Response>,
    terminal: Option<terminal_handshake::Intake>,
) -> Result<()> {
    serve_with_listener(
        listener,
        queue,
        status,
        terminal,
        #[cfg(test)]
        None,
        #[cfg(any(
            test,
            all(
                marmish_peer_receiver_candidate,
                target_os = "linux",
                target_arch = "x86_64"
            )
        ))]
        None,
        #[cfg(test)]
        None,
        #[cfg(all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        CandidateListener::default(),
    )
    .await
}

#[cfg(test)]
pub(crate) async fn serve_with_peer(
    listener: UnixListener,
    queue: mpsc::Sender<Pending>,
    status: watch::Receiver<Response>,
    terminal: Option<terminal_handshake::Intake>,
    peer: Option<peer::Listener>,
    drain: Option<oneshot::Receiver<()>>,
) -> Result<()> {
    serve_with_listener(
        listener,
        queue,
        status,
        terminal,
        peer,
        drain,
        None,
        #[cfg(all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        CandidateListener::default(),
    )
    .await
}

#[cfg_attr(
    all(
        test,
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    ),
    allow(
        clippy::too_many_arguments,
        reason = "candidate reads coexist with independent test fixtures"
    )
)]
pub(crate) async fn serve_with_listener(
    listener: UnixListener,
    queue: mpsc::Sender<Pending>,
    status: watch::Receiver<Response>,
    terminal: Option<terminal_handshake::Intake>,
    #[cfg(test)] peer: Option<peer::Listener>,
    #[cfg(any(
        test,
        all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )
    ))]
    drain: Option<oneshot::Receiver<()>>,
    #[cfg(test)] mut candidate: Option<crate::daemon::candidate_lifecycle::Listener>,
    #[cfg(all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    registered: CandidateListener,
) -> Result<()> {
    #[cfg(test)]
    let peer = peer.map(|peer| peer.register(&listener)).transpose()?;
    #[cfg(test)]
    if let Some(registration) = candidate.as_mut() {
        if !registration.register(&listener) {
            candidate = None;
        } else if registration.exits() {
            return Err(Error::Invalid("candidate_fixture_listener_exit"));
        }
    }
    let fence = SocketFence {
        #[cfg(all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        fatal: registered.fatal,
    };
    if fence.tripped() {
        return Ok(());
    }
    #[cfg(all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    let (read_offer, passive) = match registered.registration {
        CandidateRegistration::None => (None, None),
        CandidateRegistration::View(offer) => (offer.register(&listener), None),
        CandidateRegistration::Passive(passive) => (None, passive.register(&listener)),
    };
    let receive_drain = async {
        #[cfg(any(
            test,
            all(
                marmish_peer_receiver_candidate,
                target_os = "linux",
                target_arch = "x86_64"
            )
        ))]
        if let Some(drain) = drain {
            let _ = drain.await;
            return;
        }
        std::future::pending::<()>().await;
    };
    let drain = async {
        #[cfg(all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        tokio::select! {
            _ = receive_drain => {},
            _ = fence.tripped_wait() => {},
        }
        #[cfg(not(all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )))]
        receive_drain.await;
    };
    tokio::pin!(drain);
    let clients = std::sync::Arc::new(tokio::sync::Semaphore::new(64));
    let mut tasks = tokio::task::JoinSet::new();
    let outcome = loop {
        if fence.tripped() {
            break Ok(());
        }
        tokio::select! {
            _ = &mut drain => break Ok(()),
            _ = tasks.join_next(), if !tasks.is_empty() => {}
            accepted = async {
                #[cfg(test)]
                if candidate.as_mut().is_some_and(|listener| listener.transient_accept_error()) {
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                listener.accept().await
            } => {
                let (mut stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(_) => {
                        #[cfg(test)]
                        if peer.is_some() {
                            break Err(Error::Invalid("peer_listener_accept"));
                        }
                        crate::diagnostics::event("ipc_accept", "retry_pending");
                        tokio::select! {
                            _ = &mut drain => break Ok(()),
                            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                        }
                        continue;
                    }
                };
                let accepted_at = tokio::time::Instant::now();
                let Ok(_uid) = verify_peer(&stream) else { continue };
                let Ok(permit) = clients.clone().try_acquire_owned() else { continue };
                let queue = queue.clone();
                let status = status.clone();
                let terminal = terminal.clone();
                let fence = fence.clone();
                #[cfg(test)]
                let intake = peer.as_ref().map(|(_, intake)| intake.clone());
                #[cfg(all(marmish_peer_receiver_candidate, target_os = "linux", target_arch = "x86_64"))]
                let read_intake = read_offer.as_ref().map(peer_read_listener::Offer::intake);
                tasks.spawn(async move {
                    let _permit = permit;
                    let exchange_deadline = accepted_at + IO_TIMEOUT;
                    let result = tokio::time::timeout_at(exchange_deadline, fence.read(&mut stream)).await;
                    if fence.tripped() {
                        return;
                    }
                    if let Ok(Ok(Request::TerminalStart { request })) = result {
                        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
                        terminal_handshake::transfer(terminal, request, stream, _permit, deadline, &fence).await;
                        return;
                    }
                    #[cfg(test)]
                    if let Ok(Ok(Request::Peer { request })) = result {
                        let response = peer::handle(
                            intake.as_ref(),
                            request,
                            _uid,
                            exchange_deadline - Duration::from_millis(100),
                        ).await;
                        if tokio::time::Instant::now() < exchange_deadline {
                            let _ = tokio::time::timeout_at(
                                exchange_deadline,
                                fence.write(&mut stream, &response),
                            ).await;
                        }
                        return;
                    }
                    #[cfg(all(marmish_peer_receiver_candidate, target_os = "linux", target_arch = "x86_64"))]
                    if let (Some(intake), Ok(Ok(Request::PeerRead { request }))) = (&read_intake, &result) {
                        intake.write(*request, &mut stream, exchange_deadline, &fence).await;
                        return;
                    }
                    let short_exchange = matches!(&result,Ok(Ok(request)) if request.uses_short_exchange());
                    let response = match result {
                        #[cfg(marmish_peer_receiver_candidate)]
                        Ok(Ok(Request::PeerRead { .. })) => Response::failure(peer_read::UNAVAILABLE),
                        Ok(Ok(Request::Status {})) => {
                            let response = status.borrow().clone();
                            #[cfg(all(test, marmish_peer_receiver_candidate, target_os = "linux", target_arch = "x86_64"))]
                            crate::daemon::registered::registered_loss::before_status_write(
                                &response, fence.fatal.as_ref(), exchange_deadline,
                            ).await;
                            response
                        }
                        Ok(Ok(request)) => {
                            let deadline = if short_exchange {exchange_deadline - Duration::from_millis(100)} else {tokio::time::Instant::now() + Duration::from_secs(150)};
                            let (respond, receiver) = oneshot::channel();
                            if fence.tripped() {
                                return;
                            }
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
                    let write_deadline = if short_exchange {exchange_deadline} else {tokio::time::Instant::now() + IO_TIMEOUT};
                    let _ = tokio::time::timeout_at(write_deadline, fence.write(&mut stream, &response)).await;
                });
            }
        }
    };
    #[cfg(test)]
    drop(peer);
    #[cfg(test)]
    if let Some(candidate) = candidate {
        candidate.before_drain();
    }
    #[cfg(all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    drop((read_offer, passive));
    drop(listener);
    tasks.shutdown().await;
    outcome
}

#[cfg(all(
    test,
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "ipc_fatal_tests.rs"]
mod fatal_listener_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(marmish_peer_receiver_candidate))]
    #[tokio::test]
    async fn stock_socket_rejects_candidate_peer_read() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("s");
        let listener = bind(&path).unwrap();
        let (queue, mut incoming) = mpsc::channel(1);
        let (_status, status) =
            watch::channel(Response::success(serde_json::json!({"state":"active"})));
        let task = tokio::spawn(serve(listener, queue, status));
        let mut stream = UnixStream::connect(&path).await.unwrap();
        write_frame(
            &mut stream,
            &serde_json::json!({"operation":"peer_read","request":{"kind":"list"}}),
        )
        .await
        .unwrap();
        let response: Response = tokio::time::timeout(IO_TIMEOUT, read_frame(&mut stream))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.error.as_deref(), Some("invalid_request"));
        assert!(matches!(
            incoming.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert!(
            exchange(&path, &Request::Status {}, IO_TIMEOUT)
                .await
                .unwrap()
                .ok
        );
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

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
