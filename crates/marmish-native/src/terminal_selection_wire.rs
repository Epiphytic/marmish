//! Typed selection records inside the authenticated volatile Terminal room.
use crate::terminal_bootstrap::enrollment::fresh;
use crate::terminal_frames::{Direction, Size};
use cgka_traits::app_event::MarmotAppEvent;
use std::num::NonZeroU64;

const KIND: u64 = 24868;
const MAX_EVENT: usize = 768;
const HEADER: usize = 50;
const MAX_RECORD: usize = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Session {
    pub(super) transcript: [u8; 32],
    pub(super) transaction: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Default,
    New,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    pub(super) claim: [u8; 16],
    pub(super) generation: NonZeroU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    Capacity,
    Permission,
    Unavailable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Record {
    Request { choice: Choice, size: Size },
    Accepted { binding: Binding, slot: [u8; 16] },
    Busy,
    Refused(Refusal),
    Confirm(Binding),
}

impl Record {
    fn direction(self) -> Direction {
        match self {
            Self::Request { .. } | Self::Confirm(_) => Direction::ToServer,
            Self::Accepted { .. } | Self::Busy | Self::Refused(_) => Direction::FromServer,
        }
    }

    fn write(self, bytes: &mut Vec<u8>) -> Option<()> {
        match self {
            Self::Request { choice, size } if size.valid() => {
                let choice = match choice {
                    Choice::Default => 1,
                    Choice::New => 2,
                };
                bytes.extend([1, choice]);
                bytes.extend(size.rows.to_be_bytes());
                bytes.extend(size.cols.to_be_bytes());
            }
            Self::Accepted { binding, slot } if slot != [0; 16] => {
                bytes.push(2);
                binding.write(bytes)?;
                bytes.extend(slot);
            }
            Self::Busy => bytes.push(3),
            Self::Refused(reason) => {
                let reason = match reason {
                    Refusal::Capacity => 1,
                    Refusal::Permission => 2,
                    Refusal::Unavailable => 3,
                };
                bytes.extend([4, reason]);
            }
            Self::Confirm(binding) => {
                bytes.push(5);
                binding.write(bytes)?;
            }
            _ => return None,
        }
        Some(())
    }
}

impl Binding {
    fn write(self, bytes: &mut Vec<u8>) -> Option<()> {
        if self.claim == [0; 16] {
            return None;
        }
        bytes.extend(self.claim);
        bytes.extend(self.generation.get().to_be_bytes());
        Some(())
    }

    fn read(bytes: &[u8]) -> Option<Self> {
        let (claim, generation) = bytes.split_at_checked(16)?;
        let claim = nonzero(claim.try_into().ok()?)?;
        let generation = NonZeroU64::new(u64::from_be_bytes(generation.try_into().ok()?))?;
        Some(Self { claim, generation })
    }
}

pub(super) fn encode(
    author: &[u8; 32],
    now: u64,
    session: Session,
    record: Record,
) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(MAX_RECORD);
    bytes.push(1);
    bytes.extend(nonzero(session.transcript)?);
    bytes.extend(nonzero(session.transaction)?);
    record.write(&mut bytes)?;
    let event = MarmotAppEvent::new(hex::encode(author), now, KIND, vec![], hex::encode(bytes));
    let encoded = event.encode().ok()?;
    (encoded.len() <= MAX_EVENT).then_some(encoded)
}

pub(super) fn decode(
    bytes: &[u8],
    author: &[u8; 32],
    now: u64,
    transcript: [u8; 32],
    transaction: Option<[u8; 16]>,
    direction: Direction,
) -> Option<(Session, Record)> {
    if bytes.len() > MAX_EVENT {
        return None;
    }
    let event = MarmotAppEvent::decode(bytes).ok()?;
    event.validate_sender(&hex::encode(author)).ok()?;
    if event.kind != KIND || !event.tags.is_empty() || !fresh(event.created_at, now) {
        return None;
    }
    let raw = canonical_record(&event.content)?;
    let session = Session {
        transcript: nonzero(raw[1..33].try_into().ok()?)?,
        transaction: nonzero(raw[33..49].try_into().ok()?)?,
    };
    if session.transcript != transcript || transaction.is_some_and(|id| id != session.transaction) {
        return None;
    }
    let record = read_record(&raw)?;
    (record.direction() == direction).then_some((session, record))
}

fn canonical_record(content: &str) -> Option<Vec<u8>> {
    let length = content.len();
    if !(2 * HEADER..=2 * MAX_RECORD).contains(&length)
        || !length.is_multiple_of(2)
        || !content
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut prefix = [0; HEADER];
    hex::decode_to_slice(&content[..2 * HEADER], &mut prefix).ok()?;
    let tail = match prefix[49] {
        1 => 5,
        2 => 40,
        3 => 0,
        4 => 1,
        5 => 24,
        _ => return None,
    };
    if prefix[0] != 1 || length != 2 * (HEADER + tail) {
        return None;
    }
    hex::decode(content).ok()
}

fn read_record(raw: &[u8]) -> Option<Record> {
    let payload = &raw[HEADER..];
    match raw[49] {
        1 => {
            let choice = match payload[0] {
                1 => Choice::Default,
                2 => Choice::New,
                _ => return None,
            };
            let size = Size {
                rows: u16::from_be_bytes(payload[1..3].try_into().ok()?),
                cols: u16::from_be_bytes(payload[3..5].try_into().ok()?),
            };
            size.valid().then_some(Record::Request { choice, size })
        }
        2 => Some(Record::Accepted {
            binding: Binding::read(&payload[..24])?,
            slot: nonzero(payload[24..].try_into().ok()?)?,
        }),
        3 => Some(Record::Busy),
        4 => Some(Record::Refused(match payload[0] {
            1 => Refusal::Capacity,
            2 => Refusal::Permission,
            3 => Refusal::Unavailable,
            _ => return None,
        })),
        5 => Some(Record::Confirm(Binding::read(payload)?)),
        _ => None,
    }
}

fn nonzero<const N: usize>(value: [u8; N]) -> Option<[u8; N]> {
    (value != [0; N]).then_some(value)
}

#[cfg(test)]
#[path = "terminal_selection_wire_tests.rs"]
mod tests;
