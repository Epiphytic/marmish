//! Nonblocking, bounded record IO for an owned Terminal socket.

use crate::terminal_ipc_wire::{Decoder, Direction, Eof, MAX_DATA, Record, Role, encode};
use crate::{Error, Result};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

const FRAME: usize = MAX_DATA + 12;
const STALL: Duration = Duration::from_secs(5);
const IO: Error = Error::Invalid("terminal_local_channel");
const BOUNDARY: Error = Error::Invalid("terminal_local_record");

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReadEvent {
    Pending,
    Record,
    Eof(Eof),
}

pub(super) struct Reader {
    decoder: Decoder,
    suffix: [u8; FRAME],
    next: usize,
    used: usize,
    deadline: Option<Instant>,
    rejected: bool,
}

impl Reader {
    pub(super) fn new(direction: Direction, role: Role) -> Self {
        Self {
            decoder: Decoder::new(direction, role),
            suffix: [0; FRAME],
            next: 0,
            used: 0,
            deadline: None,
            rejected: false,
        }
    }

    pub(super) fn has_buffered(&self) -> bool {
        self.next < self.used
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(super) fn read(
        &mut self,
        stream: &mut impl Read,
        now: Instant,
        accept: impl FnOnce(Record<'_>) -> Result<()>,
    ) -> Result<ReadEvent> {
        if self.rejected {
            return Err(IO);
        }
        if self.deadline.is_some_and(|end| now >= end) {
            return Err(Error::Timeout);
        }
        if !self.has_buffered() {
            match stream.read(&mut self.suffix) {
                Ok(0) => {
                    return self
                        .decoder
                        .end_of_input()
                        .map(ReadEvent::Eof)
                        .map_err(|_| BOUNDARY);
                }
                Ok(n) => {
                    self.next = 0;
                    self.used = n;
                }
                Err(error) if retryable(&error) => return Ok(ReadEvent::Pending),
                Err(_) => return Err(IO),
            }
        }
        let (consumed, record) = self
            .decoder
            .push(&self.suffix[self.next..self.used])
            .map_err(|_| BOUNDARY)?;
        self.next += consumed;
        if let Some(record) = record {
            self.deadline = None;
            if let Err(error) = accept(record) {
                self.rejected = true;
                return Err(error);
            }
            return Ok(ReadEvent::Record);
        }
        self.deadline.get_or_insert(now + STALL);
        Ok(ReadEvent::Pending)
    }
}

struct Frame<M> {
    bytes: Vec<u8>,
    offset: usize,
    deadline: Instant,
    completion: M,
    last: bool,
}

pub(super) struct Writer<M> {
    direction: Direction,
    role: Role,
    current: Option<Frame<M>>,
    ended: bool,
}

impl<M> Writer<M> {
    pub(super) fn new(direction: Direction, role: Role) -> Self {
        Self {
            direction,
            role,
            current: None,
            ended: false,
        }
    }

    pub(super) fn has_frame(&self) -> bool {
        self.current.is_some()
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.current.as_ref().map(|frame| frame.deadline)
    }

    pub(super) fn queue(
        &mut self,
        record: Record<'_>,
        completion: M,
        now: Instant,
        limit: Instant,
    ) -> Result<()> {
        if self.ended || self.has_frame() {
            return Err(BOUNDARY);
        }
        if now >= limit {
            return Err(Error::Timeout);
        }
        self.current = Some(Frame {
            bytes: encode(self.direction, self.role, record).map_err(|_| BOUNDARY)?,
            offset: 0,
            deadline: (now + STALL).min(limit),
            completion,
            last: matches!(record, Record::Close(_) | Record::Detach),
        });
        Ok(())
    }

    /// Drop a selected frame only if it has emitted no bytes.
    pub(super) fn discard_unwritten(&mut self, discard: impl FnOnce(&M) -> bool) {
        if self
            .current
            .as_ref()
            .is_some_and(|frame| frame.offset == 0 && discard(&frame.completion))
        {
            self.current = None;
        }
    }

    /// One write attempt after readiness or on initial queueing.
    pub(super) fn flush(&mut self, stream: &mut impl Write, now: Instant) -> Result<Option<M>> {
        let Some(frame) = self.current.as_mut() else {
            return Ok(None);
        };
        if now >= frame.deadline {
            return Err(Error::Timeout);
        }
        match stream.write(&frame.bytes[frame.offset..]) {
            Ok(0) => return Err(IO),
            Ok(n) => frame.offset += n,
            Err(error) if retryable(&error) => return Ok(None),
            Err(_) => return Err(IO),
        }
        if frame.offset < frame.bytes.len() {
            return Ok(None);
        }
        let frame = self.current.take().expect("completed current frame");
        self.ended = frame.last;
        Ok(Some(frame.completion))
    }
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

#[path = "terminal_socket_io_tests.rs"]
mod tests;
