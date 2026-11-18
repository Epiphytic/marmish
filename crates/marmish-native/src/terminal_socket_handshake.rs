//! Same-UID ownership transfer for the local Terminal stream fixture.

use super::{Request, Response, read_frame, verify_peer, write_frame, write_frame_body};
use crate::terminal_ipc_wire::Role;
use crate::transport::{RelayPolicy, RouteMode, TransportLimits};
use crate::{Error, Result};
use nostr::key::PublicKey;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    net::Shutdown,
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixStream as OwnedStream,
    },
    path::Path,
    sync::Arc,
};
use tokio::{
    net::UnixStream,
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    time::{Instant, timeout_at},
};

const PENDING: usize = 4;
const ATTACHED: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Attach {
    Default,
    New,
    Observe,
    Takeover,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    pub(crate) remote: String,
    pub(crate) session: String,
    pub(crate) relays: Vec<String>,
    pub(crate) rows: u16,
    pub(crate) columns: u16,
    pub(crate) attach: Attach,
}

impl Start {
    fn validate(&self) -> Result<()> {
        let canonical = |value: &str, len| {
            value.len() == len
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if !canonical(&self.remote, 64)
            || !canonical(&self.session, 32)
            || !(2..=250).contains(&self.rows)
            || !(2..=500).contains(&self.columns)
        {
            return Err(Error::Invalid("terminal_start_invalid"));
        }
        let key = PublicKey::from_hex(&self.remote)
            .map_err(|_| Error::Invalid("terminal_start_invalid"))?;
        key.xonly()
            .map_err(|_| Error::Invalid("terminal_start_invalid"))?;
        RelayPolicy::parse(&self.relays, RouteMode::Auto, TransportLimits::default())
            .map_err(|_| Error::Invalid("terminal_start_invalid"))?;
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct Intake {
    send: mpsc::Sender<PendingStart>,
    pending: Arc<Semaphore>,
    attached: Arc<Semaphore>,
}

impl Intake {
    pub(crate) fn new() -> (Self, mpsc::Receiver<PendingStart>) {
        let (send, receive) = mpsc::channel(PENDING);
        (
            Self {
                send,
                pending: Arc::new(Semaphore::new(PENDING)),
                attached: Arc::new(Semaphore::new(ATTACHED)),
            },
            receive,
        )
    }
}

pub(crate) struct PendingStart {
    pub(crate) request: Start,
    stream: OwnedStream,
    pub(crate) deadline: Instant,
    attached: Arc<Semaphore>,
    _pending: OwnedSemaphorePermit,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    ok: bool,
    terminal_stream: Role,
}

pub(crate) struct Attached {
    stream: OwnedStream,
    _permit: OwnedSemaphorePermit,
}

impl Read for Attached {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.stream.read(bytes)
    }
}

impl Write for Attached {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl AsRawFd for Attached {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

impl terminal_socket_session::ReadWrite for Attached {
    fn finish_write(&mut self) -> Result<()> {
        self.stream.shutdown(Shutdown::Write)?;
        Ok(())
    }
}

async fn refuse(mut stream: UnixStream, deadline: Instant, category: &'static str) {
    let _ = timeout_at(
        deadline,
        write_frame(&mut stream, &Response::failure(category)),
    )
    .await;
}

pub(super) async fn transfer(
    route: Option<Intake>,
    request: Start,
    stream: UnixStream,
    classify: OwnedSemaphorePermit,
    deadline: Instant,
) {
    let Some(route) = route else {
        drop(classify);
        refuse(stream, deadline, "terminal_unavailable").await;
        return;
    };
    if request.validate().is_err() {
        drop(classify);
        refuse(stream, deadline, "terminal_start_invalid").await;
        return;
    }
    let Ok(pending) = route.pending.try_acquire_owned() else {
        drop(classify);
        refuse(stream, deadline, "terminal_capacity").await;
        return;
    };
    drop(classify);
    let Ok(stream) = stream.into_std() else {
        return;
    };
    let candidate = PendingStart {
        request,
        stream,
        deadline,
        attached: route.attached,
        _pending: pending,
    };
    if let Err(error) = route.send.try_send(candidate) {
        let reason = match error {
            mpsc::error::TrySendError::Closed(_) => "terminal_unavailable",
            mpsc::error::TrySendError::Full(_) => "terminal_capacity",
        };
        let PendingStart {
            stream, _pending, ..
        } = error.into_inner();
        drop(_pending);
        if let Ok(stream) = UnixStream::from_std(stream) {
            refuse(stream, deadline, reason).await;
        }
    }
}

pub(crate) struct LocalStart {
    pub(crate) request: Start,
    stream: UnixStream,
    pub(crate) deadline: Instant,
    attached: Arc<Semaphore>,
    pending: OwnedSemaphorePermit,
}

impl PendingStart {
    pub(crate) fn register(self) -> Result<LocalStart> {
        Ok(LocalStart {
            request: self.request,
            stream: UnixStream::from_std(self.stream)?,
            deadline: self.deadline,
            attached: self.attached,
            pending: self._pending,
        })
    }

    #[cfg(test)]
    async fn admit(self) -> Result<Attached> {
        self.register()?.admit().await
    }
}

async fn cancellation(stream: &UnixStream, deadline: Instant) -> Error {
    let watch = async {
        loop {
            if let Err(error) = stream.readable().await {
                return error.into();
            }
            match stream.try_read(&mut [0u8; 1]) {
                Ok(0) => return Error::Invalid("terminal_start_closed"),
                Ok(_) => return Error::Invalid("terminal_start_invalid"),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(error) => return error.into(),
            }
        }
    };
    timeout_at(deadline, watch).await.unwrap_or(Error::Timeout)
}

impl LocalStart {
    /// Race this against admission with no local protocol bytes before Ready.
    pub(crate) async fn cancelled(&self) -> Error {
        cancellation(&self.stream, self.deadline).await
    }

    pub(crate) async fn refuse(self, category: &'static str) {
        let Self {
            stream,
            deadline,
            pending,
            ..
        } = self;
        drop(pending);
        refuse(stream, deadline, category).await;
    }

    pub(crate) fn reserve(&self) -> Result<OwnedSemaphorePermit> {
        if !matches!(self.request.attach, Attach::Default | Attach::New) {
            return Err(Error::Invalid("terminal_mode_unavailable"));
        }
        self.attached
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Invalid("terminal_capacity"))
    }

    #[cfg(test)]
    async fn admit(self) -> Result<Attached> {
        match self.reserve() {
            Ok(permit) => self.admit_reserved(permit).await,
            Err(error @ Error::Invalid(reason)) => {
                self.refuse(reason).await;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn admit_reserved(self, permit: OwnedSemaphorePermit) -> Result<Attached> {
        let Self {
            mut stream,
            deadline,
            pending,
            ..
        } = self;
        drop(pending);
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let response = Ready {
            ok: true,
            terminal_stream: Role::Writer,
        };
        timeout_at(deadline, write_frame_body(&mut stream, &response))
            .await
            .map_err(|_| Error::Timeout)??;
        let stream = stream.into_std()?;
        Ok(Attached {
            stream,
            _permit: permit,
        })
    }
}

pub(crate) async fn start(path: &Path, request: Start, deadline: Instant) -> Result<OwnedStream> {
    request.validate()?;
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    timeout_at(deadline, async {
        let mut stream = UnixStream::connect(path).await?;
        verify_peer(&stream)?;
        write_frame_body(&mut stream, &Request::TerminalStart { request }).await?;
        let reply: Box<serde_json::value::RawValue> = read_frame(&mut stream).await?;
        if let Ok(Ready {
            ok: true,
            terminal_stream: Role::Writer,
        }) = serde_json::from_str(reply.get())
        {
            return Ok(stream.into_std()?);
        }
        let response: Response = serde_json::from_str(reply.get())
            .map_err(|_| Error::Invalid("terminal_start_response"))?;
        if response.result.is_some() || response.command.is_some() || response.stream.is_some() {
            return Err(Error::Invalid("terminal_start_response"));
        }
        if response.ok || response.terminal_stream.is_some() {
            return Err(Error::Invalid("terminal_start_response"));
        }
        Err(Error::Invalid(match response.error.as_deref() {
            Some("invalid_request") => "terminal_daemon_upgrade_required",
            Some("terminal_busy") => "terminal_busy",
            Some("terminal_bootstrap_no_challenge") => "terminal_bootstrap_no_challenge",
            Some("terminal_attach_deadline") => "terminal_attach_deadline",
            Some("terminal_capacity") => "terminal_capacity",
            Some("terminal_command_binding") => "terminal_command_binding",
            Some("terminal_grant_denied") => "terminal_grant_denied",
            Some("terminal_mode_unavailable") => "terminal_mode_unavailable",
            Some("terminal_owner_unavailable") => "terminal_owner_unavailable",
            Some("terminal_start_invalid") => "terminal_start_invalid",
            Some("terminal_transport_unavailable") => "terminal_transport_unavailable",
            Some("terminal_unavailable") => "terminal_unavailable",
            _ => "terminal_start_response",
        }))
    })
    .await
    .map_err(|_| Error::Timeout)?
}

#[cfg(test)]
#[path = "terminal_socket_handshake_tests.rs"]
mod tests;

#[path = "terminal_cli.rs"]
mod terminal_cli;
pub(crate) use terminal_cli::{Prepared, block_on_worker, run as run_foreground};

#[path = "terminal_socket_state.rs"]
pub(crate) mod terminal_socket_state;

#[path = "terminal_socket_io.rs"]
mod terminal_socket_io;

#[path = "terminal_socket_session.rs"]
pub(crate) mod terminal_socket_session;
