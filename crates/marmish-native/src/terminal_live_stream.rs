//! One attached room owns one ordered stream and one physical sink.
use super::{
    GuardResult, Reason, guard, io,
    selection::{AttachmentGuard, ServerAttachment},
};
use crate::ipc::terminal_handshake::{
    Attached,
    terminal_socket_session::{DaemonSocket, ServerRead},
    terminal_socket_state::InputEffect,
};
use crate::mdk_adapter::terminal_live_enrollment::AttachedRoom;
use crate::terminal_frames::{
    Data, Direction, Size,
    delivery::{Attachment, Effect, Fault, Offer, Receipt},
    encode_app,
};
use crate::terminal_ipc_wire::{Close, Role};
use crate::terminal_output::Filter;
use crate::terminal_output_batches::Batches;
use crate::{Error, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const HEALTH: Duration = Duration::from_millis(10);
const CLOSE_SEND_BUDGET: Duration = Duration::from_secs(1);
const CHANNEL: Error = Error::Transport("terminal_attachment");
const STOP: Error = Error::Invalid("terminal_attachment_stopped");

// Emit only finite, internal categories. Neither room identifiers nor terminal bytes
// are accepted by this boundary.
fn trace<T>(stage: &'static str, result: Result<T>) -> Result<T> {
    if let Err(error) = &result {
        let category = match error {
            Error::Invalid(category) | Error::Transport(category) => category,
            Error::Timeout => "timeout",
            _ => "operation_failed",
        };
        crate::diagnostics::event(stage, category);
    }
    result
}

fn sample() -> Result<guard::Sample> {
    guard::sample().ok_or(Error::Invalid("terminal_clock"))
}

fn delivery<T>(result: std::result::Result<T, Fault>) -> Result<T> {
    result.map_err(|fault| match fault {
        Fault::Expired | Fault::Lease => Error::Timeout,
        _ => CHANNEL,
    })
}

fn stopped(stop: &AtomicBool) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        Err(STOP)
    } else {
        Ok(())
    }
}

async fn next_turn(progress: bool) {
    if progress {
        tokio::task::yield_now().await;
    } else {
        tokio::time::sleep(HEALTH).await;
    }
}

async fn egress(
    room: &mut AttachedRoom,
    lane: &mut Attachment,
    outgoing: Direction,
    stop: &AtomicBool,
) -> Result<bool> {
    stopped(stop)?;
    let clock = sample()?;
    let now = clock.continuous;
    let wall = clock.wall.as_secs();
    let author = room.binding().local_terminal;
    if let Some(control) = delivery(lane.control(now))? {
        let bytes = encode_app(
            &author,
            wall,
            room.frame_binding(),
            outgoing,
            &control.into(),
        )
        .ok_or(CHANNEL)?;
        room.send_frame(bytes).await?;
        lane.control_begun(control, sample()?.continuous);
        return Ok(true);
    }
    let Some((sequence, bytes)) = delivery(lane.begin_handoff(&author, wall, now))? else {
        return Ok(false);
    };
    room.send_frame(bytes.to_vec()).await?;
    delivery(lane.handed_off(sequence, sample()?.continuous))?;
    Ok(true)
}

enum LocalInput {
    Bytes(usize),
    Resize,
}

#[derive(Default)]
struct LocalClaims {
    input: Option<(u64, LocalInput)>,
    display: Option<(u64, u64)>,
    display_id: u64,
}

impl LocalClaims {
    fn input(
        &mut self,
        socket: &DaemonSocket<Attached>,
        lane: &mut Attachment,
        now: Duration,
    ) -> Result<bool> {
        if self.input.is_some() {
            return Ok(false);
        }
        let Some(input) = socket.next_input() else {
            return Ok(false);
        };
        let (data, claim) = match input {
            InputEffect::Bytes(bytes) => {
                (Data::Input(bytes.to_vec()), LocalInput::Bytes(bytes.len()))
            }
            InputEffect::Resize((rows, cols)) => {
                (Data::Resize(Size { rows, cols }), LocalInput::Resize)
            }
        };
        match lane.enqueue(data, now) {
            Ok(sequence) => {
                self.input = Some((sequence, claim));
                Ok(true)
            }
            Err(Fault::Window) => Ok(false),
            Err(fault) => delivery(Err(fault)),
        }
    }

    fn acknowledge(&mut self, socket: &mut DaemonSocket<Attached>, sequence: u64) -> Result<()> {
        if self.input.as_ref().is_some_and(|(id, _)| *id <= sequence) {
            match self.input.take().ok_or(CHANNEL)?.1 {
                LocalInput::Bytes(bytes) => socket.input_written(bytes)?,
                LocalInput::Resize => socket.resize_applied()?,
            }
        }
        Ok(())
    }

    fn display(
        &mut self,
        socket: &mut DaemonSocket<Attached>,
        lane: &mut Attachment,
        continuous: Duration,
        absolute: Instant,
    ) -> Result<bool> {
        let Some((Offer::Bytes(network, bytes), deadline)) =
            delivery(lane.begin_timed_sink(continuous))?
        else {
            return Ok(false);
        };
        let now = Instant::now();
        let deadline = now
            .checked_add(deadline.saturating_sub(continuous))
            .ok_or(Error::Timeout)?
            .min(absolute);
        let local = match self.display {
            None => {
                Filter::validate_batch(bytes)?;
                self.display_id = self.display_id.checked_add(1).ok_or(CHANNEL)?;
                self.display = Some((self.display_id, network));
                self.display_id
            }
            Some((local, current)) if network == current => local,
            Some(_) => return Err(CHANNEL),
        };
        let Some(committed) = socket.offer(local, bytes, now, deadline)? else {
            return Ok(false);
        };
        if committed != bytes.len() {
            return Err(CHANNEL);
        }
        delivery(lane.sink_progress(network, Effect::Wrote(committed), sample()?.continuous))?;
        self.display = None;
        Ok(true)
    }
}

pub(crate) async fn client(
    mut room: AttachedRoom,
    attached: Attached,
    stop: &AtomicBool,
) -> Result<()> {
    let binding = room.frame_binding();
    let role = match binding.role {
        crate::terminal_frames::Role::Writer => Role::Writer,
        crate::terminal_frames::Role::Observer => Role::Observer,
    };
    let mut socket = DaemonSocket::new(attached, role);
    let mut lane =
        Attachment::new(binding, Direction::FromServer, sample()?.continuous).ok_or(CHANNEL)?;
    let limit = room.ceiling().deadline()?.into_std();
    let mut claims = LocalClaims::default();
    let result = async {
        loop {
            stopped(stop)?;
            trace("terminal_client_liveness", room.check_live().await)?;
            let mut progress =
                match trace("terminal_client_local_read", socket.read(Instant::now()))? {
                    ServerRead::Detached => return Ok(Close::Ended),
                    ServerRead::Received => true,
                    ServerRead::Pending => false,
                };
            if let Some((incoming, _)) =
                trace("terminal_client_receive", room.try_receive_frame().await)?
            {
                let receipt = trace(
                    "terminal_client_admit",
                    delivery(lane.admit(incoming, sample()?.continuous)),
                )?;
                match receipt {
                    Receipt::Ack => claims.acknowledge(&mut socket, lane.acknowledged())?,
                    Receipt::Closed => return Ok(Close::Ended),
                    _ => {}
                }
                progress = true;
            }
            stopped(stop)?;
            let now = sample()?.continuous;
            progress |= trace(
                "terminal_client_input",
                claims.input(&socket, &mut lane, now),
            )?;
            progress |= trace(
                "terminal_client_display",
                claims.display(&mut socket, &mut lane, now, limit),
            )?;
            let writable = socket.wants_write()?;
            if writable {
                socket.flush(Instant::now(), limit)?;
            }
            progress |= trace(
                "terminal_client_send",
                egress(&mut room, &mut lane, Direction::ToServer, stop).await,
            )?;
            next_turn(progress).await;
        }
    }
    .await;
    let close = match &result {
        Ok(reason) => *reason,
        Err(Error::Timeout) => Close::Idle,
        Err(_) => Close::Transport,
    };
    if matches!(result, Ok(Close::Ended)) {
        lane.close();
        let _ = tokio::time::timeout(
            CLOSE_SEND_BUDGET,
            egress(&mut room, &mut lane, Direction::ToServer, stop),
        )
        .await;
    }
    socket.begin_close(close);
    let local_deadline = Instant::now() + HEALTH;
    while Instant::now() < local_deadline && socket.wants_write().unwrap_or(false) {
        if socket.flush(Instant::now(), local_deadline).is_err() {
            break;
        }
        tokio::task::yield_now().await;
    }
    let cleanup = room.close().await;
    result.map(|_| ()).and(cleanup)
}

#[derive(Default)]
struct NativeOutput {
    batches: Batches,
    raw: Option<(Vec<u8>, usize)>,
    ready: Option<Vec<u8>>,
    previous: Option<Duration>,
}

impl NativeOutput {
    fn blocked(&self) -> bool {
        self.raw.is_some() || self.ready.is_some()
    }

    fn drained(&self) -> bool {
        !self.blocked() && !self.batches.has_pending()
    }

    fn turn(&mut self, lane: &mut Attachment, now: Duration) -> Result<bool> {
        if let Some(previous) = self.previous.replace(now) {
            self.batches.advance(now.saturating_sub(previous))?;
        }
        if self.ready.is_none() {
            if let Some((bytes, offset)) = self.raw.as_mut() {
                *offset += self.batches.push(&bytes[*offset..], now)?;
                if *offset == bytes.len() {
                    self.raw = None;
                }
            }
            self.ready = self.batches.take(now);
        }
        let Some(bytes) = self.ready.take() else {
            return Ok(false);
        };
        match lane.enqueue(Data::Output(bytes.clone()), now) {
            Ok(_) => Ok(true),
            Err(Fault::Window) => {
                self.ready = Some(bytes);
                Ok(false)
            }
            Err(fault) => delivery(Err(fault)),
        }
    }
}

#[derive(Default)]
struct NativeSink {
    waiting: Option<io::Operation>,
    submitted: Option<(u64, Option<usize>)>,
}

impl NativeSink {
    fn submit(
        &mut self,
        guard: &mut AttachmentGuard,
        lane: &mut Attachment,
        now: Duration,
    ) -> Result<bool> {
        if self.submitted.is_some() {
            return Ok(false);
        }
        if self.waiting.is_none() {
            let Some((offer, deadline)) = delivery(lane.begin_timed_sink(now))? else {
                return Ok(false);
            };
            let (id, operation) = match offer {
                Offer::Bytes(id, bytes) => (id, io::Input::Bytes(bytes.to_vec())),
                Offer::Resize(id, size) => (
                    id,
                    io::Input::Resize {
                        rows: size.rows,
                        cols: size.cols,
                    },
                ),
            };
            self.waiting = Some(io::Operation::new(id, deadline, operation).ok_or(CHANNEL)?);
        }
        let operation = self.waiting.take().ok_or(CHANNEL)?;
        let bytes = match &operation.input {
            io::Input::Bytes(bytes) => Some(bytes.len()),
            io::Input::Resize { .. } => None,
        };
        let sequence = operation.sequence;
        match guard.admitted().submit(operation) {
            Ok(()) => {
                self.submitted = Some((sequence, bytes));
                Ok(true)
            }
            Err(io::PushError::Contended(operation)) => {
                self.waiting = Some(operation);
                Ok(false)
            }
            // Native retirement can win between the liveness check and submit.
            // Drain the final output on the next turn without acknowledging this input.
            Err(io::PushError::Closed(Reason::ShellExit)) => Ok(false),
            Err(_) => Err(CHANNEL),
        }
    }

    fn complete(&mut self, completion: io::Completion, lane: &mut Attachment) -> Result<()> {
        let expected = self.submitted.take().ok_or(CHANNEL)?;
        let (sequence, effect) = match completion {
            io::Completion::Written(id, size) if expected == (id, Some(size)) => {
                (id, Effect::Wrote(size))
            }
            io::Completion::Resized(id) if expected == (id, None) => (id, Effect::Resized),
            _ => return Err(CHANNEL),
        };
        delivery(lane.sink_progress(sequence, effect, sample()?.continuous))?;
        Ok(())
    }
}

pub(crate) async fn server(mut attached: ServerAttachment, stop: &AtomicBool) -> Result<()> {
    let mut lane = Attachment::new(
        attached.room.frame_binding(),
        Direction::ToServer,
        sample()?.continuous,
    )
    .ok_or(CHANNEL)?;
    let mut output = NativeOutput::default();
    let mut sink = NativeSink::default();
    let mut renewal = None;
    let mut shell_exited = false;
    let result = async {
        loop {
            stopped(stop)?;
            let allowed = trace("terminal_server_liveness", attached.check_live().await)?;
            let shell_retired = matches!(allowed, GuardResult::Retired(Reason::ShellExit));
            if !shell_retired && !attachment_ready(allowed)? {
                next_turn(false).await;
                continue;
            }
            if shell_retired {
                renewal = None;
            } else if let Some(received) = renewal {
                if !attachment_ready(attached.guard.renew(received))? {
                    next_turn(false).await;
                    continue;
                }
                renewal = None;
            }
            let mut progress = false;
            if let Some((incoming, received)) = trace(
                "terminal_server_receive",
                attached.room.try_receive_frame().await,
            )? {
                match trace(
                    "terminal_server_admit",
                    delivery(lane.admit(incoming, sample()?.continuous)),
                )? {
                    Receipt::Closed => return Ok(false),
                    Receipt::Ordered | Receipt::Ack | Receipt::Pulse => {
                        let renewed = attached.guard.renew(received);
                        if !matches!(renewed, GuardResult::Retired(Reason::ShellExit))
                            && !attachment_ready(renewed)?
                        {
                            renewal = Some(received);
                            next_turn(false).await;
                            continue;
                        }
                    }
                    Receipt::Ignored => {}
                }
                progress = true;
            }
            stopped(stop)?;
            if !shell_exited && let Some(event) = attached.guard.admitted().event(!output.blocked())
            {
                match event {
                    io::Event::Complete(completion) => sink.complete(completion, &mut lane)?,
                    io::Event::Output(bytes) => output.raw = Some((bytes, 0)),
                    io::Event::Closed(Reason::ShellExit) => shell_exited = true,
                    io::Event::Closed(_) => return Err(CHANNEL),
                }
                progress = true;
            }
            let now = sample()?.continuous;
            if !shell_retired && !shell_exited {
                progress |= trace(
                    "terminal_server_write",
                    sink.submit(&mut attached.guard, &mut lane, now),
                )?;
            }
            progress |= trace("terminal_server_read", output.turn(&mut lane, now))?;
            progress |= trace(
                "terminal_server_send",
                egress(&mut attached.room, &mut lane, Direction::FromServer, stop).await,
            )?;
            if shell_exited && output.drained() && lane.output_drained() {
                return Ok(true);
            }
            next_turn(progress).await;
        }
    }
    .await;
    if matches!(result, Ok(true)) || attached.guard.replaced() {
        lane.close();
        let _ = tokio::time::timeout(
            CLOSE_SEND_BUDGET,
            egress(&mut attached.room, &mut lane, Direction::FromServer, stop),
        )
        .await;
    }
    let cleanup = attached.close().await;
    result.map(|_| ()).and(cleanup)
}

fn attachment_ready<T>(state: GuardResult<T>) -> Result<bool> {
    match state {
        GuardResult::Applied(_) => Ok(true),
        GuardResult::Withheld => Ok(false),
        GuardResult::Retired(Reason::Deadline) => Err(Error::Timeout),
        GuardResult::Retired(_) => Err(CHANNEL),
    }
}
