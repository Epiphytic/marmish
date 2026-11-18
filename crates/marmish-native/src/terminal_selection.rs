//! One volatile writer selection transaction, backed by the sole native PTY owner.
use super::selection_wire::{self as wire, Binding, Choice, Record, Refusal, Session};
use super::{
    AdmittedTerminal, CAPACITY, GuardResult, LiveControl, QueueError, Reason, SlotId, Source,
    SourceId, StartChoice, StartOutcome, VerifiedStart, random,
};
use crate::mdk_adapter::{
    CommandDirection,
    terminal_live_enrollment::{AttachedRoom, EnrollmentCeiling, ReadyEnrollment},
};
use crate::terminal_frames::{Direction, Size};
use crate::{Error, Result};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio::time::timeout_at;

const INVALID: Error = Error::Invalid("terminal_selection_record");
const SOURCE: Error = Error::Invalid("terminal_selection_source");
const CONFIRM: Duration = Duration::from_secs(5);
const ATTACHMENT: Duration = Duration::from_secs(8 * 60 * 60);

#[derive(Default)]
pub(crate) struct SourceTokens(u64);

impl SourceTokens {
    fn next(&mut self) -> Result<NonZeroU64> {
        self.0 = self.0.checked_add(1).ok_or(SOURCE)?;
        NonZeroU64::new(self.0).ok_or(SOURCE)
    }
}

struct SourceScope(Arc<Source>);

impl Drop for SourceScope {
    fn drop(&mut self) {
        self.0.retire(Reason::OwnerLost);
    }
}

/// This cannot be constructed by enrollment or by the client.
pub(crate) struct WriterGuard {
    source: SourceScope,
    admitted: AdmittedTerminal,
}

impl WriterGuard {
    pub(crate) fn matches_binding(&self, attachment: [u8; 16], generation: NonZeroU64) -> bool {
        self.admitted.claim.random == attachment
            && self.admitted.claim.generation == generation.get()
            && matches!(self.source.0.check(), GuardResult::Applied(()))
    }

    fn binding(&self) -> Result<Binding> {
        Ok(Binding {
            claim: self.admitted.claim.random,
            generation: NonZeroU64::new(self.admitted.claim.generation).ok_or(SOURCE)?,
        })
    }

    pub(super) fn renew(&self, at: Duration) -> GuardResult<bool> {
        self.admitted.renew(self.source.0.id, at)
    }

    pub(super) fn admitted(&mut self) -> &mut AdmittedTerminal {
        &mut self.admitted
    }
}

pub(crate) enum ServerOutcome {
    Attached(ServerAttachment),
    Busy,
    Refused(#[cfg(test)] Refusal),
}

pub(crate) struct ServerAttachment {
    pub(super) writer: WriterGuard,
    pub(super) room: AttachedRoom,
}

impl ServerAttachment {
    pub(super) async fn check_live(&self) -> Result<GuardResult<()>> {
        if let Err(error) = self.room.check_live().await {
            self.writer.source.0.retire(Reason::Authority);
            return Err(error);
        }
        Ok(self.writer.source.0.check())
    }

    pub(crate) async fn close(self) -> Result<()> {
        let Self { writer, room } = self;
        drop(writer);
        room.close().await
    }
}

pub(crate) enum ClientOutcome {
    Attached {
        room: AttachedRoom,
        #[cfg_attr(
            not(test),
            allow(dead_code, reason = "named reattachment starts with Observe/Takeover")
        )]
        slot: SlotId,
    },
    Busy,
    Refused(Refusal),
}

pub(crate) async fn accept(
    mut room: ReadyEnrollment,
    control: &LiveControl,
    tokens: &mut SourceTokens,
    granted: &impl Fn() -> bool,
) -> Result<ServerOutcome> {
    if room.binding().direction != CommandDirection::Server {
        return Err(SOURCE);
    }
    let admission = room.admission_ceiling();
    let (session, Record::Request { choice, size }) =
        receive(&mut room, None, Direction::ToServer, admission, granted).await?
    else {
        return Err(INVALID);
    };
    let ceiling = EnrollmentCeiling::start(ATTACHMENT)?;
    let source = SourceScope(Source::with_ceiling(
        SourceId {
            leaf: room.binding().peer,
            epoch: room.binding().native.epoch.0,
            subscription: tokens.next()?,
        },
        room.fence(),
        Some(ceiling.continuous()),
    ));
    room.recheck(granted).await?;
    admission.check()?;
    let start = VerifiedStart {
        account: room.binding().peer,
        selection: match choice {
            Choice::Default => StartChoice::Default,
            Choice::New => StartChoice::New,
        },
        rows: size.rows,
        cols: size.cols,
        source: Arc::clone(&source.0),
        deadline: admission.continuous(),
    };
    let outcome = start_and_wait(&room, control, start, admission).await?;
    let writer = match outcome {
        StartOutcome::Attached(admitted) => WriterGuard { source, admitted },
        StartOutcome::Busy { .. } => {
            require_clear(&mut room, admission, granted).await?;
            send(&mut room, session, Record::Busy, admission, granted).await?;
            room.close().await?;
            return Ok(ServerOutcome::Busy);
        }
        StartOutcome::Refused(error) => {
            require_clear(&mut room, admission, granted).await?;
            let reason = refusal(error);
            send(
                &mut room,
                session,
                Record::Refused(reason),
                admission,
                granted,
            )
            .await?;
            room.close().await?;
            return Ok(ServerOutcome::Refused(
                #[cfg(test)]
                reason,
            ));
        }
    };

    require_clear(&mut room, admission, granted).await?;
    let binding = writer.binding()?;
    if !writer.matches_binding(binding.claim, binding.generation) {
        return Err(SOURCE);
    }
    let confirm = EnrollmentCeiling::start(CONFIRM)?.clamp(admission);
    let accepted = Record::Accepted {
        binding,
        slot: writer.admitted.slot.0,
    };
    send(&mut room, session, accepted, confirm, granted).await?;
    let (_, received) = receive(
        &mut room,
        Some(session.transaction),
        Direction::ToServer,
        confirm,
        granted,
    )
    .await?;
    if received != Record::Confirm(binding) {
        return Err(INVALID);
    }
    let (room, writer) = timeout_at(
        confirm.deadline()?,
        room.handoff_server(binding.claim, binding.generation, ceiling, granted, writer),
    )
    .await
    .map_err(|_| Error::Timeout)??;
    confirm.check()?;
    Ok(ServerOutcome::Attached(ServerAttachment { writer, room }))
}

async fn start_and_wait(
    room: &ReadyEnrollment,
    control: &LiveControl,
    start: VerifiedStart,
    ceiling: EnrollmentCeiling,
) -> Result<StartOutcome> {
    let (reply, result) = oneshot::channel();
    if let Err(error) = control.submit_start(start, reply) {
        return Ok(StartOutcome::Refused(match error {
            QueueError::Full => CAPACITY,
            QueueError::Stopped => SOURCE,
        }));
    }
    let authority = room.authority();
    timeout_at(ceiling.deadline()?, async {
        tokio::select! {
            biased;
            retired = authority.wait_for_retirement() => retired,
            started = result => started.map_err(|_| SOURCE),
        }
    })
    .await
    .map_err(|_| Error::Timeout)?
}

pub(crate) async fn request(
    mut room: ReadyEnrollment,
    choice: Choice,
    size: Size,
    granted: &impl Fn() -> bool,
) -> Result<ClientOutcome> {
    if room.binding().direction != CommandDirection::Client {
        return Err(SOURCE);
    }
    let session = Session {
        transcript: room.binding().transcript,
        transaction: random()?,
    };
    let admission = room.admission_ceiling();
    send(
        &mut room,
        session,
        Record::Request { choice, size },
        admission,
        granted,
    )
    .await?;
    let (_, outcome) = receive(
        &mut room,
        Some(session.transaction),
        Direction::FromServer,
        admission,
        granted,
    )
    .await?;
    match outcome {
        Record::Accepted { binding, slot } => {
            send(
                &mut room,
                session,
                Record::Confirm(binding),
                admission,
                granted,
            )
            .await?;
            let ceiling = EnrollmentCeiling::start(ATTACHMENT)?;
            let room = timeout_at(
                admission.deadline()?,
                room.handoff_client(binding.claim, binding.generation, ceiling, granted),
            )
            .await
            .map_err(|_| Error::Timeout)??;
            admission.check()?;
            Ok(ClientOutcome::Attached {
                room,
                slot: SlotId(slot),
            })
        }
        Record::Busy => {
            room.close().await?;
            Ok(ClientOutcome::Busy)
        }
        Record::Refused(reason) => {
            room.close().await?;
            Ok(ClientOutcome::Refused(reason))
        }
        _ => Err(INVALID),
    }
}

async fn require_clear(
    room: &mut ReadyEnrollment,
    ceiling: EnrollmentCeiling,
    granted: &impl Fn() -> bool,
) -> Result<()> {
    let unexpected = timeout_at(ceiling.deadline()?, room.try_receive_selection(granted))
        .await
        .map_err(|_| Error::Timeout)??;
    if unexpected.is_some() {
        room.retire();
        return Err(INVALID);
    }
    ceiling.check()
}

fn refusal(error: Error) -> Refusal {
    if matches!(error, Error::Invalid("terminal_capacity")) {
        Refusal::Capacity
    } else if matches!(error, Error::Invalid("terminal_grant_denied")) {
        Refusal::Permission
    } else {
        Refusal::Unavailable
    }
}

async fn send(
    room: &mut ReadyEnrollment,
    session: Session,
    record: Record,
    ceiling: EnrollmentCeiling,
    granted: &impl Fn() -> bool,
) -> Result<()> {
    let event =
        wire::encode(&room.binding().local_terminal, now()?, session, record).ok_or(INVALID)?;
    timeout_at(ceiling.deadline()?, room.send_selection(event, granted))
        .await
        .map_err(|_| Error::Timeout)??;
    ceiling.check()
}

async fn receive(
    room: &mut ReadyEnrollment,
    transaction: Option<[u8; 16]>,
    direction: Direction,
    ceiling: EnrollmentCeiling,
    granted: &impl Fn() -> bool,
) -> Result<(Session, Record)> {
    let app = timeout_at(ceiling.deadline()?, room.receive_selection(granted))
        .await
        .map_err(|_| Error::Timeout)??;
    ceiling.check()?;
    let binding = room.binding();
    wire::decode(
        &app.bytes,
        &binding.peer_terminal,
        now()?,
        binding.transcript,
        transaction,
        direction,
    )
    .ok_or(INVALID)
}

fn now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| INVALID)?
        .as_secs())
}

#[cfg(test)]
#[path = "terminal_selection_native_proof.rs"]
pub(crate) mod proof;
