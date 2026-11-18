//! Local socket ownership for one admitted writer or observer.

use super::terminal_socket_io::{ReadEvent, Reader, Writer};
use super::terminal_socket_state::{
    DaemonInput, DaemonOutput, ForegroundResize, INPUT_WINDOW, InputEffect, OutputOffer,
};
use crate::terminal_ipc_wire::{Close, Direction, Eof, MAX_DATA, Record, Role};
use crate::{Error, Result};
use std::net::Shutdown;
use std::num::NonZeroU64;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const IO: Error = Error::Invalid("terminal_local_channel");
const STALL: Duration = Duration::from_secs(5);

enum ClientSent {
    Input,
    Ack,
    Resize,
    Detach,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum FrontendOutput {
    Pending,
    Bytes(usize),
    Closed(Close),
}

pub(super) struct ForegroundSocket {
    stream: UnixStream,
    reader: Reader,
    writer: Writer<ClientSent>,
    role: Role,
    resize: ForegroundResize,
    credit: u64,
    input_sent: u64,
    output_received: u64,
    output_at: Option<Instant>,
    ack: Option<NonZeroU64>,
    ack_at: Option<Instant>,
    close: Option<Close>,
    eof: bool,
    detach: bool,
    detached: bool,
    close_at: Option<Instant>,
    detach_at: Option<Instant>,
    input_at: Option<Instant>,
    resize_at: Option<Instant>,
}

impl ForegroundSocket {
    pub(super) fn new(stream: UnixStream, role: Role) -> Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            reader: Reader::new(Direction::ToClient, role),
            writer: Writer::new(Direction::ToDaemon, role),
            role,
            resize: ForegroundResize::default(),
            credit: 0,
            input_sent: 0,
            output_received: 0,
            output_at: None,
            ack: None,
            ack_at: None,
            close: None,
            eof: false,
            detach: false,
            detached: false,
            close_at: None,
            detach_at: None,
            input_at: None,
            resize_at: None,
        })
    }

    pub(super) fn fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    pub(super) fn peer_close(&self) -> Option<Close> {
        self.close
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.reader
            .deadline()
            .into_iter()
            .chain(self.writer.deadline())
            .chain(self.output_at)
            .chain(self.close_at)
            .chain(self.ack_at)
            .chain(self.detach_at)
            .chain(self.input_at)
            .chain(self.resize_at)
            .min()
    }

    pub(super) fn buffered(&self) -> bool {
        self.reader.has_buffered() || self.eof
    }

    pub(super) fn wants_read(&self) -> bool {
        self.output_at.is_none() && (!self.eof || self.ack_at.is_none())
    }

    pub(super) fn submit_resize(&mut self, size: (u16, u16)) -> Result<()> {
        if self.role == Role::Observer {
            return Err(IO);
        }
        if self.close.is_none() && !self.detach {
            self.resize.request(size);
        }
        Ok(())
    }

    pub(super) fn display_committed(&mut self) -> Result<()> {
        if self.detach {
            return Ok(());
        }
        if self.ack_at.is_some() {
            return Err(IO);
        }
        self.ack_at = Some(self.output_at.take().ok_or(IO)?);
        self.ack = Some(NonZeroU64::new(self.output_received).ok_or(IO)?);
        Ok(())
    }

    pub(super) fn wants_write(&self, has_input: bool) -> bool {
        if self.detached {
            return false;
        }
        self.writer.has_frame()
            || self.ack.is_some()
            || self.detach
            || self.close.is_none()
                && (self.resize.is_ready()
                    || self.role == Role::Writer && has_input && self.input_sent < self.credit)
    }

    pub(super) fn read(
        &mut self,
        output: &mut [u8; MAX_DATA],
        now: Instant,
    ) -> Result<FrontendOutput> {
        if !self.wants_read() {
            return Err(IO);
        }
        self.check_deadline(now)?;
        if self.eof {
            return self.close.map(FrontendOutput::Closed).ok_or(IO);
        }
        let mut result = FrontendOutput::Pending;
        let received = self.reader.read(&mut self.stream, now, |record| {
            match record {
                Record::Output { id, bytes } => {
                    if self.ack_at.is_some()
                        || self.output_received.checked_add(1) != Some(id.get())
                    {
                        return Err(IO);
                    }
                    output[..bytes.len()].copy_from_slice(bytes);
                    self.output_received = id.get();
                    self.output_at = Some(now + STALL);
                    result = FrontendOutput::Bytes(bytes.len());
                }
                Record::Credit(credit) => {
                    if credit.get() <= self.credit
                        || credit.get().saturating_sub(self.input_sent) > INPUT_WINDOW
                    {
                        return Err(IO);
                    }
                    self.credit = credit.get();
                    self.input_at = None;
                }
                Record::ResizeApplied => {
                    self.resize.applied()?;
                    self.resize_at = None;
                }
                Record::Close(reason) => {
                    self.close = Some(reason);
                    self.resize_at = None;
                    self.resize = ForegroundResize::default();
                    self.close_at = Some(now + STALL);
                    self.input_at = None;
                }
                _ => return Err(IO),
            }
            Ok(())
        })?;
        match received {
            ReadEvent::Eof(Eof::Ended) => {
                let reason = self.close.ok_or(IO)?;
                self.eof = true;
                if self.ack_at.is_some() {
                    Ok(FrontendOutput::Pending)
                } else {
                    Ok(FrontendOutput::Closed(reason))
                }
            }
            ReadEvent::Eof(Eof::Boundary) => Err(IO),
            _ => Ok(result),
        }
    }

    /// Returns exactly the bytes staged into the one immutable frame.
    pub(super) fn write(&mut self, input: &[u8], now: Instant) -> Result<usize> {
        self.check_deadline(now)?;
        let mut accepted = 0;
        if !self.writer.has_frame() {
            let limit = now + STALL;
            if let Some(ack) = self.ack.take() {
                self.writer.queue(
                    Record::Ack(ack),
                    ClientSent::Ack,
                    now,
                    self.ack_at.ok_or(IO)?,
                )?;
            } else if self.detach && !self.detached {
                self.writer.queue(
                    Record::Detach,
                    ClientSent::Detach,
                    now,
                    self.detach_at.ok_or(IO)?,
                )?;
            } else if self.close.is_none() {
                if let Some((rows, columns)) = self.resize.take_ready() {
                    self.resize_at = Some(self.input_at.take().unwrap_or(limit));
                    self.writer.queue(
                        Record::Resize { rows, columns },
                        ClientSent::Resize,
                        now,
                        self.resize_at.ok_or(IO)?,
                    )?;
                } else if self.role == Role::Writer && !input.is_empty() {
                    accepted =
                        self.credit
                            .saturating_sub(self.input_sent)
                            .min(input.len().min(MAX_DATA) as u64) as usize;
                    if accepted != 0 {
                        self.writer.queue(
                            Record::Input(&input[..accepted]),
                            ClientSent::Input,
                            now,
                            limit,
                        )?;
                        self.input_sent += accepted as u64;
                        self.input_at = None;
                    }
                }
            }
        }
        match self.writer.flush(&mut self.stream, now)? {
            Some(ClientSent::Ack) => self.ack_at = None,
            Some(ClientSent::Detach) => {
                self.detached = true;
                self.detach_at = None;
                self.stream.shutdown(Shutdown::Write)?;
            }
            _ => {}
        }
        Ok(accepted)
    }

    pub(super) fn begin_detach(&mut self, now: Instant) {
        self.detach = true;
        self.detach_at.get_or_insert(now + STALL);
        self.input_at = None;
        self.output_at = None;
        self.resize_at = None;
    }

    pub(super) fn detach_complete(&self) -> bool {
        self.detached
    }

    pub(super) fn wait_for_input_progress(&mut self, has_input: bool, now: Instant) {
        if (has_input || self.resize.is_ready())
            && self.role == Role::Writer
            && self.close.is_none()
            && !self.detach
        {
            self.input_at.get_or_insert(now + STALL);
        }
    }

    pub(super) fn check_deadline(&self, now: Instant) -> Result<()> {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            return Err(Error::Timeout);
        }
        Ok(())
    }
}

enum ServerSent {
    Credit(NonZeroU64),
    Resize,
    Output(NonZeroU64),
    Close,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ServerRead {
    Pending,
    Received,
    Detached,
}

pub(crate) struct DaemonSocket<S> {
    stream: S,
    reader: Reader,
    writer: Writer<ServerSent>,
    input: Option<DaemonInput>,
    output: DaemonOutput,
    output_at: Option<Instant>,
    close: Option<Close>,
    closed: bool,
    detached: bool,
}

impl<S: ReadWrite> DaemonSocket<S> {
    pub(crate) fn new(stream: S, role: Role) -> Self {
        Self {
            stream,
            reader: Reader::new(Direction::ToDaemon, role),
            writer: Writer::new(Direction::ToClient, role),
            input: (role == Role::Writer).then(DaemonInput::new),
            output: DaemonOutput::default(),
            output_at: None,
            close: None,
            closed: false,
            detached: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    pub(crate) fn read(&mut self, now: Instant) -> Result<ServerRead> {
        self.check_deadline(now)?;
        let mut detach = false;
        let event = self
            .reader
            .read(&mut self.stream, now, |record| match record {
                Record::Input(bytes) if !self.closed => {
                    self.input.as_mut().ok_or(IO)?.receive(bytes)
                }
                Record::Ack(id) => self.output.ack(id),
                Record::Resize { rows, columns } if !self.closed => {
                    self.input.as_mut().ok_or(IO)?.queue_resize((rows, columns))
                }
                Record::Detach => {
                    detach = true;
                    Ok(())
                }
                _ => Err(IO),
            })?;
        match event {
            ReadEvent::Pending => Ok(ServerRead::Pending),
            ReadEvent::Record if !detach => Ok(ServerRead::Received),
            ReadEvent::Record | ReadEvent::Eof(Eof::Ended) => {
                self.detached = true;
                self.output = DaemonOutput::default();
                self.output_at = None;
                self.writer
                    .discard_unwritten(|sent| matches!(sent, ServerSent::Output(_)));
                Ok(ServerRead::Detached)
            }
            ReadEvent::Eof(Eof::Boundary) => Err(IO),
        }
    }

    pub(crate) fn next_input(&self) -> Option<InputEffect<'_>> {
        if self.close.is_some() || self.closed {
            return None;
        }
        self.input.as_ref()?.next()
    }

    pub(crate) fn input_written(&mut self, bytes: usize) -> Result<()> {
        self.input.as_mut().ok_or(IO)?.wrote(bytes)
    }

    pub(crate) fn resize_applied(&mut self) -> Result<()> {
        self.input.as_mut().ok_or(IO)?.resize_applied()
    }

    pub(crate) fn begin_close(&mut self, reason: Close) {
        self.close.get_or_insert(reason);
        self.output_at = None;
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        let receive = self
            .close
            .is_none()
            .then(|| self.reader.deadline())
            .flatten();
        receive
            .into_iter()
            .chain(self.writer.deadline())
            .chain(self.output_at)
            .min()
    }

    #[cfg(test)]
    pub(crate) fn buffered(&self) -> bool {
        self.reader.has_buffered()
    }

    fn check_deadline(&self, now: Instant) -> Result<()> {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            return Err(Error::Timeout);
        }
        Ok(())
    }

    pub(crate) fn wants_write(&self) -> Result<bool> {
        if self.closed {
            return Ok(false);
        }
        if self.writer.has_frame() || self.close.is_some() {
            return Ok(true);
        }
        match self.input.as_ref() {
            Some(input) => Ok(input.resize_receipt_ready() || input.credit()?.is_some()),
            None => Ok(false),
        }
    }

    /// Offer the same native head until it is committed, leaving other control
    /// records ahead of a fresh output frame.
    pub(crate) fn offer(
        &mut self,
        sequence: u64,
        bytes: &[u8],
        now: Instant,
        native_deadline: Instant,
    ) -> Result<Option<usize>> {
        if self.close.is_some() || self.closed {
            return Err(IO);
        }
        if self.detached {
            return Ok(None);
        }
        let deadline = self.output_at.unwrap_or(now + STALL).min(native_deadline);
        self.output_at = Some(deadline);
        if now >= deadline {
            return Err(Error::Timeout);
        }
        if !self.output.has_head() && self.wants_write()? {
            return Ok(None);
        }
        match self.output.offer(sequence, bytes.len())? {
            OutputOffer::Encode(id) => {
                self.writer.queue(
                    Record::Output { id, bytes },
                    ServerSent::Output(id),
                    now,
                    deadline,
                )?;
                Ok(None)
            }
            OutputOffer::Waiting => Ok(None),
            OutputOffer::Committed(bytes) => {
                self.output_at = None;
                Ok(Some(bytes))
            }
        }
    }

    pub(crate) fn flush(&mut self, now: Instant, limit: Instant) -> Result<()> {
        self.check_deadline(now)?;
        if !self.writer.has_frame() && !self.closed {
            if let Some(reason) = self.close {
                self.writer
                    .queue(Record::Close(reason), ServerSent::Close, now, limit)?;
            } else if let Some(input) = &self.input {
                if input.resize_receipt_ready() {
                    self.writer
                        .queue(Record::ResizeApplied, ServerSent::Resize, now, limit)?;
                } else if let Some(credit) = input.credit()? {
                    self.writer.queue(
                        Record::Credit(credit),
                        ServerSent::Credit(credit),
                        now,
                        limit,
                    )?;
                }
            }
        }
        match self.writer.flush(&mut self.stream, now)? {
            Some(ServerSent::Credit(credit)) => {
                self.input.as_mut().ok_or(IO)?.credit_sent(credit)?
            }
            Some(ServerSent::Resize) => self.input.as_mut().ok_or(IO)?.resize_receipt_sent()?,
            Some(ServerSent::Output(id)) if !self.detached => self.output.fully_written(id)?,
            Some(ServerSent::Output(_)) | None => {}
            Some(ServerSent::Close) => {
                self.closed = true;
                self.stream.finish_write()?;
            }
        }
        Ok(())
    }
}

pub(crate) trait ReadWrite: std::io::Read + std::io::Write + AsRawFd {
    fn finish_write(&mut self) -> Result<()>;
}

impl ReadWrite for UnixStream {
    fn finish_write(&mut self) -> Result<()> {
        self.shutdown(Shutdown::Write).map_err(Error::from)
    }
}

#[cfg(test)]
#[path = "terminal_socket_session_tests.rs"]
mod tests;
