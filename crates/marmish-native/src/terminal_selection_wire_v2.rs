//! Exact selection records inside a freshly enrolled Terminal room.

use crate::terminal_bootstrap::enrollment::fresh;
use crate::terminal_frames::{Direction, Size};
use crate::terminal_ipc_wire::Role;
use cgka_traits::app_event::MarmotAppEvent;
use std::num::NonZeroU64;

const KIND: u64 = 24868;
const VERSION: u8 = 2;
const MAX_EVENT: usize = 768;
const HEADER: usize = 50;
const MAX_RECORD: usize = 112;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Session {
    pub(crate) transcript: [u8; 32],
    pub(crate) transaction: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Regular,
    New,
    Observe,
    Takeover,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ticket {
    nonce: [u8; 16],
    pub(crate) slot: [u8; 16],
    pub(crate) claim: [u8; 16],
    pub(crate) generation: NonZeroU64,
}

impl Ticket {
    pub(crate) fn new(
        nonce: [u8; 16],
        slot: [u8; 16],
        claim: [u8; 16],
        generation: NonZeroU64,
    ) -> Option<Self> {
        Some(Self {
            nonce: nonzero(nonce)?,
            slot: nonzero(slot)?,
            claim: nonzero(claim)?,
            generation,
        })
    }

    pub(crate) fn bytes(self) -> [u8; 56] {
        let mut bytes = [0; 56];
        bytes[..16].copy_from_slice(&self.nonce);
        bytes[16..32].copy_from_slice(&self.slot);
        bytes[32..48].copy_from_slice(&self.claim);
        bytes[48..].copy_from_slice(&self.generation.get().to_be_bytes());
        bytes
    }

    pub(crate) fn read(bytes: &[u8]) -> Option<Self> {
        let (nonce, tail) = bytes.split_at_checked(16)?;
        let (slot, tail) = tail.split_at_checked(16)?;
        let (claim, generation) = tail.split_at_checked(16)?;
        Self::new(
            nonce.try_into().ok()?,
            slot.try_into().ok()?,
            claim.try_into().ok()?,
            NonZeroU64::new(u64::from_be_bytes(generation.try_into().ok()?))?,
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Default,
    Named([u8; 16]),
    Offer(Ticket),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) choice: Choice,
    pub(crate) target: Target,
    pub(crate) size: Size,
}

impl Request {
    fn valid(self) -> bool {
        self.size.valid()
            && match (self.choice, self.target) {
                (Choice::New, Target::Named(_)) => false,
                (_, Target::Named(slot)) => slot != [0; 16],
                (Choice::Regular, Target::Offer(_)) => false,
                _ => true,
            }
    }

    pub(crate) fn accepts(self, binding: Binding, slot: [u8; 16]) -> bool {
        if !self.valid() || slot == [0; 16] || binding.claim == [0; 16] {
            return false;
        }
        let expected_role = match self.choice {
            Choice::Observe => Role::Observer,
            Choice::Takeover if !matches!(self.target, Target::Offer(_)) => return false,
            Choice::Regular | Choice::New | Choice::Takeover => Role::Writer,
        };
        binding.role == expected_role
            && match self.target {
                Target::Default => true,
                Target::Named(target) => slot == target,
                Target::Offer(ticket) => {
                    binding.claim != ticket.claim
                        && if self.choice == Choice::New {
                            slot != ticket.slot
                        } else {
                            slot == ticket.slot
                        }
                }
            }
    }

    pub(crate) fn accepts_busy(self, ticket: Ticket) -> bool {
        self.valid()
            && matches!(self.choice, Choice::Regular | Choice::Takeover)
            && match self.target {
                Target::Default => true,
                Target::Named(target) => ticket.slot == target,
                Target::Offer(_) => false,
            }
    }

    fn write(self, bytes: &mut Vec<u8>) -> Option<()> {
        if !self.valid() {
            return None;
        }
        let choice = match self.choice {
            Choice::Regular => 1,
            Choice::New => 2,
            Choice::Observe => 3,
            Choice::Takeover => 4,
        };
        let target = match self.target {
            Target::Default => 1,
            Target::Named(_) => 2,
            Target::Offer(_) => 3,
        };
        bytes.extend([1, choice, target]);
        bytes.extend(self.size.rows.to_be_bytes());
        bytes.extend(self.size.cols.to_be_bytes());
        match self.target {
            Target::Default => (),
            Target::Named(slot) => bytes.extend(slot),
            Target::Offer(ticket) => bytes.extend(ticket.bytes()),
        }
        Some(())
    }

    fn read(bytes: &[u8]) -> Option<Self> {
        let [
            choice,
            target,
            rows_a,
            rows_b,
            cols_a,
            cols_b,
            ref extra @ ..,
        ] = *bytes
        else {
            return None;
        };
        let choice = match choice {
            1 => Choice::Regular,
            2 => Choice::New,
            3 => Choice::Observe,
            4 => Choice::Takeover,
            _ => return None,
        };
        let target = match target {
            1 if extra.is_empty() => Target::Default,
            2 => Target::Named(extra.try_into().ok()?),
            3 => Target::Offer(Ticket::read(extra)?),
            _ => return None,
        };
        let request = Self {
            choice,
            target,
            size: Size {
                rows: u16::from_be_bytes([rows_a, rows_b]),
                cols: u16::from_be_bytes([cols_a, cols_b]),
            },
        };
        request.valid().then_some(request)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) role: Role,
    pub(crate) claim: [u8; 16],
    pub(crate) generation: NonZeroU64,
}

impl Binding {
    fn write(self, bytes: &mut Vec<u8>) -> Option<()> {
        bytes.push(match self.role {
            Role::Writer => 1,
            Role::Observer => 2,
        });
        bytes.extend(nonzero(self.claim)?);
        bytes.extend(self.generation.get().to_be_bytes());
        Some(())
    }

    fn read(bytes: &[u8]) -> Option<Self> {
        let (role, tail) = bytes.split_first()?;
        let (claim, generation) = tail.split_at_checked(16)?;
        Some(Self {
            role: match role {
                1 => Role::Writer,
                2 => Role::Observer,
                _ => return None,
            },
            claim: nonzero(claim.try_into().ok()?)?,
            generation: NonZeroU64::new(u64::from_be_bytes(generation.try_into().ok()?))?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Refusal {
    Capacity = 1,
    Permission,
    Unavailable,
    WriterMissing,
    StaleOffer,
    SlotMissing,
    ReplacementPending,
}

impl Refusal {
    fn read(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::Capacity,
            2 => Self::Permission,
            3 => Self::Unavailable,
            4 => Self::WriterMissing,
            5 => Self::StaleOffer,
            6 => Self::SlotMissing,
            7 => Self::ReplacementPending,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Record {
    Request(Request),
    Accepted { binding: Binding, slot: [u8; 16] },
    Busy(Ticket),
    Refused(Refusal),
    Confirm(Binding),
}

impl Record {
    fn direction(self) -> Direction {
        match self {
            Self::Request(_) | Self::Confirm(_) => Direction::ToServer,
            Self::Accepted { .. } | Self::Busy(_) | Self::Refused(_) => Direction::FromServer,
        }
    }

    fn write(self, bytes: &mut Vec<u8>) -> Option<()> {
        match self {
            Self::Request(request) => request.write(bytes)?,
            Self::Accepted { binding, slot } => {
                bytes.push(2);
                binding.write(bytes)?;
                bytes.extend(nonzero(slot)?);
            }
            Self::Busy(ticket) => {
                bytes.push(3);
                bytes.extend(ticket.bytes());
            }
            Self::Refused(reason) => bytes.extend([4, reason as u8]),
            Self::Confirm(binding) => {
                bytes.push(5);
                binding.write(bytes)?;
            }
        }
        Some(())
    }

    fn read(data: &[u8]) -> Option<Self> {
        let (opcode, tail) = data.split_first()?;
        Some(match opcode {
            1 => Self::Request(Request::read(tail)?),
            2 => {
                let (binding, slot) = tail.split_at_checked(25)?;
                Self::Accepted {
                    binding: Binding::read(binding)?,
                    slot: nonzero(slot.try_into().ok()?)?,
                }
            }
            3 => Self::Busy(Ticket::read(tail)?),
            4 => {
                let [code] = *tail else { return None };
                Self::Refused(Refusal::read(code)?)
            }
            5 => Self::Confirm(Binding::read(tail)?),
            _ => return None,
        })
    }
}

pub(crate) fn encode(
    author: &[u8; 32],
    now: u64,
    session: Session,
    record: Record,
) -> Option<Vec<u8>> {
    let mut raw = Vec::with_capacity(MAX_RECORD);
    raw.push(VERSION);
    raw.extend(nonzero(session.transcript)?);
    raw.extend(nonzero(session.transaction)?);
    record.write(&mut raw)?;
    let event = MarmotAppEvent::new(hex::encode(author), now, KIND, vec![], hex::encode(raw));
    let bytes = event.encode().ok()?;
    (bytes.len() <= MAX_EVENT).then_some(bytes)
}

pub(crate) fn decode(
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
    let content = event.content.as_bytes();
    if !(2 * HEADER..=2 * MAX_RECORD).contains(&content.len())
        || !content.len().is_multiple_of(2)
        || !content
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return None;
    }
    let raw = hex::decode(content).ok()?;
    if raw[0] != VERSION {
        return None;
    }
    let session = Session {
        transcript: nonzero(raw[1..33].try_into().ok()?)?,
        transaction: nonzero(raw[33..49].try_into().ok()?)?,
    };
    if session.transcript != transcript || transaction.is_some_and(|id| id != session.transaction) {
        return None;
    }
    let record = Record::read(&raw[49..])?;
    (record.direction() == direction).then_some((session, record))
}

fn nonzero<const N: usize>(value: [u8; N]) -> Option<[u8; N]> {
    (value != [0; N]).then_some(value)
}

#[cfg(test)]
#[path = "terminal_selection_wire_v2_tests.rs"]
mod tests;
