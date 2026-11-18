//! Local Terminal stream records, following the initial JSON IPC reply.
use std::num::NonZeroU64;

const VERSION: u8 = 1;
pub(crate) const MAX_DATA: usize = 4096;
const MAX_BODY: usize = 2 + 8 + MAX_DATA;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    ToDaemon,
    ToClient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    Writer,
    Observer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Close {
    Ended = 1,
    Idle,
    Replaced,
    Authority,
    Transport,
    Budget,
}

impl Close {
    fn decode(code: u8) -> Result<Self, WireError> {
        Ok(match code {
            1 => Self::Ended,
            2 => Self::Idle,
            3 => Self::Replaced,
            4 => Self::Authority,
            5 => Self::Transport,
            6 => Self::Budget,
            _ => return Err(WireError::Invalid),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Record<'a> {
    Input(&'a [u8]),
    Ack(NonZeroU64),
    Resize { rows: u16, columns: u16 },
    Detach,
    Output { id: NonZeroU64, bytes: &'a [u8] },
    Credit(NonZeroU64),
    Close(Close),
    ResizeApplied,
}

impl Record<'_> {
    fn allowed(self, direction: Direction, role: Role) -> bool {
        match (direction, self) {
            (Direction::ToDaemon, Self::Input(_) | Self::Resize { .. })
            | (Direction::ToClient, Self::Credit(_) | Self::ResizeApplied) => role == Role::Writer,
            (Direction::ToDaemon, Self::Ack(_) | Self::Detach)
            | (Direction::ToClient, Self::Output { .. } | Self::Close(_)) => true,
            _ => false,
        }
    }

    fn ends_direction(self) -> bool {
        matches!(self, Self::Close(_) | Self::Detach)
    }

    fn body_spec(self) -> Result<(u8, usize), WireError> {
        Ok(match self {
            Self::Input(bytes) if (1..=MAX_DATA).contains(&bytes.len()) => (1, 2 + bytes.len()),
            Self::Ack(_) => (2, 10),
            Self::Resize { rows, columns }
                if (2..=250).contains(&rows) && (2..=500).contains(&columns) =>
            {
                (3, 6)
            }
            Self::Detach => (4, 2),
            Self::Output { bytes, .. } if (1..=MAX_DATA).contains(&bytes.len()) => {
                (5, 10 + bytes.len())
            }
            Self::Credit(_) => (6, 10),
            Self::Close(_) => (7, 3),
            Self::ResizeApplied => (8, 2),
            _ => return Err(WireError::Invalid),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WireError {
    Invalid,
    AfterEnd,
    Poisoned,
    Truncated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Eof {
    Boundary,
    Ended,
}

pub(crate) fn encode(
    direction: Direction,
    role: Role,
    record: Record<'_>,
) -> Result<Vec<u8>, WireError> {
    let (opcode, length) = record.body_spec()?;
    if !record.allowed(direction, role) {
        return Err(WireError::Invalid);
    }
    let mut out = Vec::with_capacity(length + 2);
    out.extend_from_slice(&(length as u16).to_be_bytes());
    out.extend_from_slice(&[VERSION, opcode]);
    match record {
        Record::Input(bytes) => out.extend_from_slice(bytes),
        Record::Ack(id) | Record::Credit(id) => out.extend_from_slice(&id.get().to_be_bytes()),
        Record::Resize { rows, columns } => {
            out.extend_from_slice(&rows.to_be_bytes());
            out.extend_from_slice(&columns.to_be_bytes());
        }
        Record::Detach | Record::ResizeApplied => {}
        Record::Output { id, bytes } => {
            out.extend_from_slice(&id.get().to_be_bytes());
            out.extend_from_slice(bytes);
        }
        Record::Close(close) => out.push(close as u8),
    }
    Ok(out)
}

fn id(bytes: [u8; 8]) -> Result<NonZeroU64, WireError> {
    NonZeroU64::new(u64::from_be_bytes(bytes)).ok_or(WireError::Invalid)
}

fn decode(body: &[u8], direction: Direction, role: Role) -> Result<Record<'_>, WireError> {
    let record = match body {
        [VERSION, 1, bytes @ ..] => Record::Input(bytes),
        [VERSION, 2, bytes @ ..] if bytes.len() == 8 => {
            Record::Ack(id(bytes.try_into().map_err(|_| WireError::Invalid)?)?)
        }
        [VERSION, 3, r0, r1, c0, c1] => Record::Resize {
            rows: u16::from_be_bytes([*r0, *r1]),
            columns: u16::from_be_bytes([*c0, *c1]),
        },
        [VERSION, 4] => Record::Detach,
        [VERSION, 5, i0, i1, i2, i3, i4, i5, i6, i7, bytes @ ..] => Record::Output {
            id: id([*i0, *i1, *i2, *i3, *i4, *i5, *i6, *i7])?,
            bytes,
        },
        [VERSION, 6, bytes @ ..] if bytes.len() == 8 => {
            Record::Credit(id(bytes.try_into().map_err(|_| WireError::Invalid)?)?)
        }
        [VERSION, 7, code] => Record::Close(Close::decode(*code)?),
        [VERSION, 8] => Record::ResizeApplied,
        _ => return Err(WireError::Invalid),
    };
    let (_, exact_length) = record.body_spec()?;
    if exact_length != body.len() || !record.allowed(direction, role) {
        return Err(WireError::Invalid);
    }
    Ok(record)
}

#[derive(Clone, Copy)]
enum Status {
    Reading,
    Ended,
    Eof,
    Failed,
}

pub(crate) struct Decoder {
    direction: Direction,
    role: Role,
    status: Status,
    bytes: [u8; MAX_BODY + 2],
    used: usize,
}

impl Decoder {
    pub(crate) fn new(direction: Direction, role: Role) -> Self {
        Self {
            direction,
            role,
            status: Status::Reading,
            bytes: [0; MAX_BODY + 2],
            used: 0,
        }
    }

    /// Return at most one record and the prefix it consumed. When a record is
    /// returned, the owner retains any suffix for its next call.
    pub(crate) fn push(&mut self, input: &[u8]) -> Result<(usize, Option<Record<'_>>), WireError> {
        match self.status {
            Status::Reading => {}
            Status::Ended | Status::Eof if input.is_empty() => return Ok((0, None)),
            Status::Ended | Status::Eof => {
                self.status = Status::Failed;
                return Err(WireError::AfterEnd);
            }
            Status::Failed => return Err(WireError::Poisoned),
        }
        let header = if self.used < 2 {
            let count = (2 - self.used).min(input.len());
            self.bytes[self.used..self.used + count].copy_from_slice(&input[..count]);
            self.used += count;
            if self.used < 2 {
                return Ok((count, None));
            }
            count
        } else {
            0
        };
        let size = u16::from_be_bytes([self.bytes[0], self.bytes[1]]) as usize;
        if !(2..=MAX_BODY).contains(&size) {
            self.status = Status::Failed;
            return Err(WireError::Invalid);
        }
        let end = size + 2;
        let count = (end - self.used).min(input.len() - header);
        self.bytes[self.used..self.used + count].copy_from_slice(&input[header..header + count]);
        self.used += count;
        if self.used < end {
            return Ok((header + count, None));
        }
        self.used = 0;
        match decode(&self.bytes[2..end], self.direction, self.role) {
            Ok(record) => {
                if record.ends_direction() {
                    self.status = Status::Ended;
                }
                Ok((header + count, Some(record)))
            }
            Err(error) => {
                self.status = Status::Failed;
                Err(error)
            }
        }
    }

    pub(crate) fn end_of_input(&mut self) -> Result<Eof, WireError> {
        match self.status {
            Status::Ended => Ok(Eof::Ended),
            Status::Failed => Err(WireError::Poisoned),
            Status::Eof => Ok(Eof::Boundary),
            Status::Reading if self.used == 0 => {
                self.status = Status::Eof;
                Ok(Eof::Boundary)
            }
            Status::Reading => {
                self.status = Status::Failed;
                Err(WireError::Truncated)
            }
        }
    }
}

#[cfg(test)]
#[path = "terminal_ipc_wire_tests.rs"]
mod tests;
