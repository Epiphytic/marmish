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

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct Offer([u8; 56]);

impl Offer {
    pub(crate) fn from_bytes(bytes: [u8; 56]) -> Result<Self> {
        if bytes[..48].chunks_exact(16).any(|id| id == [0; 16]) || bytes[48..] == [0; 8] {
            return Err(Error::Invalid("terminal_offer_invalid"));
        }
        Ok(Self(bytes))
    }

    pub(crate) fn bytes(self) -> [u8; 56] {
        self.0
    }

    fn slot(self) -> [u8; 16] {
        self.0[16..32].try_into().expect("fixed offer slot")
    }

    fn parse(raw: &str) -> Result<Self> {
        let bytes = parse_hex(raw).ok_or(Error::Invalid("terminal_offer_invalid"))?;
        Self::from_bytes(bytes)
    }
}

impl std::fmt::Debug for Offer {
    fn fmt(&self, format: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format.write_str("Offer(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Target {
    Default,
    Named([u8; 16]),
    Offer(Offer),
}

fn canonical(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn parse_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    if !canonical(value, N * 2) || hex::decode_to_slice(value, &mut bytes).is_err() {
        return None;
    }
    Some(bytes)
}

pub(crate) fn parse_slot(value: &str) -> Result<[u8; 16]> {
    parse_hex(value)
        .filter(|slot| *slot != [0; 16])
        .ok_or(Error::Invalid("terminal_slot_invalid"))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    pub(crate) remote: String,
    pub(crate) session: String,
    pub(crate) relays: Vec<String>,
    pub(crate) rows: u16,
    pub(crate) columns: u16,
    pub(crate) attach: Attach,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    pub(crate) session_slot: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    pub(crate) offer: Option<String>,
}

fn present_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

impl Start {
    fn validate(&self) -> Result<()> {
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
        self.target()?;
        Ok(())
    }

    pub(crate) fn target(&self) -> Result<Target> {
        const INVALID: Error = Error::Invalid("terminal_start_invalid");
        match (
            self.attach,
            self.session_slot.as_deref(),
            self.offer.as_deref(),
        ) {
            (_, None, None) => Ok(Target::Default),
            (Attach::Default | Attach::Observe | Attach::Takeover, Some(slot), None) => {
                parse_slot(slot).map(Target::Named).map_err(|_| INVALID)
            }
            (Attach::New | Attach::Observe | Attach::Takeover, None, Some(offer)) => {
                Offer::parse(offer).map(Target::Offer).map_err(|_| INVALID)
            }
            _ => Err(INVALID),
        }
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
    slot: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Busy {
    ok: bool,
    terminal_busy: BusyOffer,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BusyOffer {
    offer: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReady {
    ok: bool,
    #[serde(rename = "terminal_stream")]
    _terminal_stream: Role,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    ok: bool,
    error: String,
    #[serde(rename = "result")]
    _result: Option<()>,
}

pub(crate) enum StartOutcome {
    Ready {
        stream: OwnedStream,
        role: Role,
        slot: [u8; 16],
    },
    Busy(Offer),
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

    pub(crate) async fn busy(self, offer: Offer) -> Result<()> {
        let Self {
            mut stream,
            deadline,
            pending,
            ..
        } = self;
        drop(pending);
        let response = Busy {
            ok: false,
            terminal_busy: BusyOffer {
                offer: hex::encode(offer.bytes()),
            },
        };
        timeout_at(deadline, write_frame_body(&mut stream, &response))
            .await
            .map_err(|_| Error::Timeout)?
    }

    pub(crate) fn reserve(&self) -> Result<OwnedSemaphorePermit> {
        self.attached
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Invalid("terminal_capacity"))
    }

    #[cfg(test)]
    async fn admit(self) -> Result<Attached> {
        match self.reserve() {
            Ok(permit) => self.admit_reserved(permit, Role::Writer, [1; 16]).await,
            Err(error @ Error::Invalid(reason)) => {
                self.refuse(reason).await;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn admit_reserved(
        self,
        permit: OwnedSemaphorePermit,
        role: Role,
        slot: [u8; 16],
    ) -> Result<Attached> {
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
        if slot == [0; 16] {
            return Err(Error::Invalid("terminal_slot_invalid"));
        }
        let response = Ready {
            ok: true,
            terminal_stream: role,
            slot: hex::encode(slot),
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

async fn exchange(
    path: &Path,
    request: Start,
    deadline: Instant,
    before_write: impl FnOnce(),
) -> Result<StartOutcome> {
    request.validate()?;
    let attach = request.attach;
    let target = request.target()?;
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    timeout_at(deadline, async {
        let mut stream = UnixStream::connect(path).await?;
        verify_peer(&stream)?;
        before_write();
        write_frame_body(&mut stream, &Request::TerminalStart { request }).await?;
        let reply: Box<serde_json::value::RawValue> = read_frame(&mut stream).await?;
        decode_reply(reply.get(), attach, target, stream)
    })
    .await
    .map_err(|_| Error::Timeout)?
}

#[cfg(test)]
pub(crate) async fn start(path: &Path, request: Start, deadline: Instant) -> Result<StartOutcome> {
    exchange(path, request, deadline, || {}).await
}

fn decode_reply(
    body: &str,
    attach: Attach,
    target: Target,
    stream: UnixStream,
) -> Result<StartOutcome> {
    const INVALID: Error = Error::Invalid("terminal_start_response");
    if let Ok(Ready {
        ok: true,
        terminal_stream: role,
        slot,
    }) = serde_json::from_str(body)
    {
        let slot = parse_slot(&slot).map_err(|_| INVALID)?;
        let expected = if attach == Attach::Observe {
            Role::Observer
        } else {
            Role::Writer
        };
        let exact_slot = match target {
            Target::Default => true,
            Target::Named(named) => slot == named,
            Target::Offer(offer) => (slot == offer.slot()) != (attach == Attach::New),
        };
        if role != expected
            || !exact_slot
            || (attach == Attach::Takeover && !matches!(target, Target::Offer(_)))
        {
            return Err(INVALID);
        }
        return Ok(StartOutcome::Ready {
            stream: stream.into_std()?,
            role,
            slot,
        });
    }
    if let Ok(Busy {
        ok: false,
        terminal_busy: BusyOffer { offer },
    }) = serde_json::from_str(body)
    {
        if !matches!(attach, Attach::Default | Attach::Takeover)
            || matches!(target, Target::Offer(_))
        {
            return Err(INVALID);
        }
        let offer = Offer::parse(&offer).map_err(|_| INVALID)?;
        if let Target::Named(named) = target
            && named != offer.slot()
        {
            return Err(INVALID);
        }
        return Ok(StartOutcome::Busy(offer));
    }
    if let Ok(LegacyReady {
        ok: true,
        _terminal_stream: Role::Writer,
    }) = serde_json::from_str(body)
    {
        return Err(Error::Invalid("terminal_daemon_upgrade_required"));
    }
    let Failure {
        ok: false, error, ..
    } = serde_json::from_str(body).map_err(|_| INVALID)?
    else {
        return Err(INVALID);
    };
    Err(Error::Invalid(match error.as_str() {
        "invalid_request" | "terminal_mode_unavailable" => "terminal_daemon_upgrade_required",
        "terminal_attach_deadline" => "terminal_attach_deadline",
        "terminal_bootstrap_no_challenge" => "terminal_bootstrap_no_challenge",
        "terminal_busy" => "terminal_busy",
        "terminal_capacity" => "terminal_capacity",
        "terminal_command_binding" => "terminal_command_binding",
        "terminal_grant_denied" => "terminal_grant_denied",
        "terminal_native_unavailable" => "terminal_native_unavailable",
        "terminal_offer_stale" => "terminal_offer_stale",
        "terminal_owner_unavailable" => "terminal_owner_unavailable",
        "terminal_replacement_pending" => "terminal_replacement_pending",
        "terminal_slot_missing" => "terminal_slot_missing",
        "terminal_start_invalid" => "terminal_start_invalid",
        "terminal_takeover_uncertain" => "terminal_takeover_uncertain",
        "terminal_transport_unavailable" => "terminal_transport_unavailable",
        "terminal_unavailable" => "terminal_unavailable",
        "terminal_writer_missing" => "terminal_writer_missing",
        _ => return Err(INVALID),
    }))
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
