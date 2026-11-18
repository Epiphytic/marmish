//! Ordering and accounting shared by the Terminal socket fixture and controllers.

use crate::terminal_ipc_wire::MAX_DATA;
use crate::{Error, Result};
use std::collections::VecDeque;
use std::num::NonZeroU64;

pub(super) const INPUT_WINDOW: u64 = 16 * 1024;
const INPUT: Error = Error::Invalid("terminal_socket_input");
const OUTPUT: Error = Error::Invalid("terminal_socket_ack");

type Geometry = (u16, u16);

enum Resize {
    Pending { at: u64, size: Geometry },
    Applied,
}

pub(crate) enum InputEffect<'a> {
    Bytes(&'a [u8]),
    Resize(Geometry),
}

pub(super) struct DaemonInput {
    bytes: VecDeque<u8>,
    accepted: u64,
    drained: u64,
    credit_sent: u64,
    resize: Option<Resize>,
}

impl DaemonInput {
    pub(super) fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(INPUT_WINDOW as usize),
            accepted: 0,
            drained: 0,
            credit_sent: 0,
            resize: None,
        }
    }

    pub(super) fn credit(&self) -> Result<Option<NonZeroU64>> {
        let next = self.drained.checked_add(INPUT_WINDOW).ok_or(INPUT)?;
        Ok((next > self.credit_sent).then(|| NonZeroU64::new(next).expect("positive window")))
    }

    pub(super) fn credit_sent(&mut self, credit: NonZeroU64) -> Result<()> {
        let credit = credit.get();
        let ceiling = self.drained.checked_add(INPUT_WINDOW).ok_or(INPUT)?;
        if credit <= self.credit_sent || credit > ceiling {
            return Err(INPUT);
        }
        self.credit_sent = credit;
        Ok(())
    }

    pub(super) fn receive(&mut self, bytes: &[u8]) -> Result<()> {
        let accepted = self.accepted.checked_add(bytes.len() as u64).ok_or(INPUT)?;
        if bytes.is_empty() || bytes.len() > MAX_DATA || accepted > self.credit_sent {
            return Err(INPUT);
        }
        self.bytes.extend(bytes);
        self.accepted = accepted;
        Ok(())
    }

    pub(super) fn queue_resize(&mut self, size: Geometry) -> Result<()> {
        if self.resize.is_some() {
            return Err(INPUT);
        }
        self.resize = Some(Resize::Pending {
            at: self.accepted,
            size,
        });
        Ok(())
    }

    pub(super) fn next(&self) -> Option<InputEffect<'_>> {
        let byte_limit = match self.resize {
            Some(Resize::Pending { at, size }) => {
                if self.drained == at {
                    return Some(InputEffect::Resize(size));
                }
                (at - self.drained) as usize
            }
            _ => MAX_DATA,
        };
        let (first, second) = self.bytes.as_slices();
        let head = if first.is_empty() { second } else { first };
        let count = head.len().min(byte_limit).min(MAX_DATA);
        (count > 0).then(|| InputEffect::Bytes(&head[..count]))
    }

    pub(super) fn wrote(&mut self, count: usize) -> Result<()> {
        if !matches!(self.next(), Some(InputEffect::Bytes(bytes)) if (1..=bytes.len()).contains(&count))
        {
            return Err(INPUT);
        }
        self.bytes.drain(..count);
        self.drained += count as u64;
        Ok(())
    }

    pub(super) fn resize_applied(&mut self) -> Result<()> {
        if !matches!(self.next(), Some(InputEffect::Resize(_))) {
            return Err(INPUT);
        }
        self.resize = Some(Resize::Applied);
        Ok(())
    }

    pub(super) fn resize_receipt_ready(&self) -> bool {
        matches!(self.resize, Some(Resize::Applied))
    }

    pub(super) fn resize_receipt_sent(&mut self) -> Result<()> {
        if !self.resize_receipt_ready() {
            return Err(INPUT);
        }
        self.resize = None;
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct ForegroundResize {
    latest: Option<Geometry>,
    outstanding: bool,
}

impl ForegroundResize {
    pub(super) fn request(&mut self, size: Geometry) {
        self.latest = Some(size);
    }

    pub(super) fn is_ready(&self) -> bool {
        !self.outstanding && self.latest.is_some()
    }

    pub(super) fn take_ready(&mut self) -> Option<Geometry> {
        if self.outstanding {
            return None;
        }
        let size = self.latest.take()?;
        self.outstanding = true;
        Some(size)
    }

    pub(super) fn applied(&mut self) -> Result<()> {
        if !std::mem::take(&mut self.outstanding) {
            return Err(INPUT);
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct DaemonOutput {
    committed: u64,
    last_native: u64,
    head: Option<OutputHead>,
}

struct OutputHead {
    id: NonZeroU64,
    sequence: u64,
    length: usize,
    written: bool,
    acknowledged: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum OutputOffer {
    Encode(NonZeroU64),
    Waiting,
    Committed(usize),
}

impl DaemonOutput {
    pub(super) fn has_head(&self) -> bool {
        self.head.is_some()
    }

    pub(super) fn offer(&mut self, sequence: u64, length: usize) -> Result<OutputOffer> {
        if let Some(head) = &self.head {
            if (sequence, length) != (head.sequence, head.length) {
                return Err(OUTPUT);
            }
            if !head.acknowledged {
                return Ok(OutputOffer::Waiting);
            }
            self.committed = head.id.get();
            self.last_native = head.sequence;
            self.head = None;
            return Ok(OutputOffer::Committed(length));
        }
        if sequence <= self.last_native || !(1..=MAX_DATA).contains(&length) {
            return Err(OUTPUT);
        }
        let id = self
            .committed
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(OUTPUT)?;
        self.head = Some(OutputHead {
            id,
            sequence,
            length,
            written: false,
            acknowledged: false,
        });
        Ok(OutputOffer::Encode(id))
    }

    pub(super) fn fully_written(&mut self, id: NonZeroU64) -> Result<()> {
        let head = self
            .head
            .as_mut()
            .filter(|head| head.id == id && !head.written)
            .ok_or(OUTPUT)?;
        head.written = true;
        Ok(())
    }

    pub(super) fn ack(&mut self, id: NonZeroU64) -> Result<()> {
        if id.get() == self.committed {
            return Ok(());
        }
        let head = self
            .head
            .as_mut()
            .filter(|head| head.id == id && head.written && !head.acknowledged)
            .ok_or(OUTPUT)?;
        head.acknowledged = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "terminal_socket_state_tests.rs"]
mod tests;
