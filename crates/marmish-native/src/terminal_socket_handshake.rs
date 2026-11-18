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
pub(super) struct Intake {
    send: mpsc::Sender<PendingStart>,
    pending: Arc<Semaphore>,
    attached: Arc<Semaphore>,
}

impl Intake {
    pub(super) fn new() -> (Self, mpsc::Receiver<PendingStart>) {
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

pub(super) struct PendingStart {
    pub(super) request: Start,
    stream: UnixStream,
    deadline: Instant,
    attached: Arc<Semaphore>,
    _pending: OwnedSemaphorePermit,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    ok: bool,
    terminal_stream: Role,
}

pub(super) struct Attached {
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
        refuse(stream, deadline, "terminal_busy").await;
        return;
    };
    drop(classify);
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
            mpsc::error::TrySendError::Full(_) => "terminal_busy",
        };
        let PendingStart {
            stream, _pending, ..
        } = error.into_inner();
        drop(_pending);
        refuse(stream, deadline, reason).await;
    }
}

impl PendingStart {
    /// Race this against enrollment. No valid client bytes precede Ready.
    pub(super) async fn cancelled(&self) -> Error {
        let watch = async {
            loop {
                if let Err(error) = self.stream.readable().await {
                    return error.into();
                }
                match self.stream.try_read(&mut [0u8; 1]) {
                    Ok(0) => return Error::Invalid("terminal_start_closed"),
                    Ok(_) => return Error::Invalid("terminal_start_invalid"),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(error) => return error.into(),
                }
            }
        };
        timeout_at(self.deadline, watch)
            .await
            .unwrap_or(Error::Timeout)
    }

    pub(super) async fn admit(self) -> Result<Attached> {
        let Self {
            request,
            mut stream,
            deadline,
            attached,
            _pending,
        } = self;
        let result = if request.attach != Attach::Default {
            Err(Error::Invalid("terminal_mode_unavailable"))
        } else {
            attached
                .try_acquire_owned()
                .map_err(|_| Error::Invalid("terminal_busy"))
        };
        let permit = match result {
            Ok(permit) => permit,
            Err(Error::Invalid(reason)) => {
                drop(_pending);
                refuse(stream, deadline, reason).await;
                return Err(Error::Invalid(reason));
            }
            Err(error) => return Err(error),
        };
        drop(_pending);
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

pub(super) async fn start(path: &Path, request: Start, deadline: Instant) -> Result<OwnedStream> {
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
        match (
            response.ok,
            response.error.as_deref(),
            response.terminal_stream,
        ) {
            (false, Some("terminal_start_invalid"), None) => {
                Err(Error::Invalid("terminal_start_invalid"))
            }
            (false, Some("terminal_busy"), None) => Err(Error::Invalid("terminal_busy")),
            (false, Some("terminal_unavailable"), None) => {
                Err(Error::Invalid("terminal_unavailable"))
            }
            (false, Some("terminal_mode_unavailable"), None) => {
                Err(Error::Invalid("terminal_mode_unavailable"))
            }
            _ => Err(Error::Invalid("terminal_start_response")),
        }
    })
    .await
    .map_err(|_| Error::Timeout)?
}

#[path = "terminal_socket_handshake_tests.rs"]
mod tests;

#[path = "terminal_socket_state.rs"]
mod terminal_socket_state;

#[path = "terminal_socket_io.rs"]
mod terminal_socket_io;

#[path = "terminal_socket_session.rs"]
mod terminal_socket_session;
