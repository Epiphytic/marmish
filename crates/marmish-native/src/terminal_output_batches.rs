//! Bounded batches of complete, independently safe terminal display units.

use crate::Result;
use crate::terminal_ipc_wire::MAX_DATA;
use crate::terminal_output::{DisplayUnit, Filter};
use std::time::Duration;

const FLUSH_AFTER: Duration = Duration::from_millis(8);

#[derive(Default)]
pub(crate) struct Batches {
    filter: Filter,
    pending: Vec<u8>,
    next: Option<DisplayUnit>,
    flush_at: Option<Duration>,
}

impl Batches {
    /// The caller retains unconsumed PTY bytes when a complete batch is ready.
    pub(crate) fn push(&mut self, input: &[u8], now: Duration) -> Result<usize> {
        let mut consumed = 0;
        for &byte in input {
            if self.ready(now) {
                break;
            }
            consumed += 1;
            if let Some(unit) = self.filter.push(byte)? {
                if self.pending.len() + unit.bytes().len() > MAX_DATA {
                    self.next = Some(unit);
                } else {
                    self.append(unit, now);
                }
            }
        }
        Ok(consumed)
    }

    fn append(&mut self, unit: DisplayUnit, now: Duration) {
        self.flush_at.get_or_insert(now.saturating_add(FLUSH_AFTER));
        self.pending.extend_from_slice(unit.bytes());
    }

    pub(crate) fn advance(&mut self, elapsed: Duration) -> Result<()> {
        self.filter.advance(elapsed)
    }

    pub(crate) fn ready(&self, now: Duration) -> bool {
        self.next.is_some()
            || self.pending.len() == MAX_DATA
            || self.flush_at.is_some_and(|deadline| now >= deadline)
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty() || self.next.is_some()
    }

    pub(crate) fn take(&mut self, now: Duration) -> Option<Vec<u8>> {
        if !self.ready(now) {
            return None;
        }
        self.flush_at = None;
        let batch = std::mem::take(&mut self.pending);
        if let Some(unit) = self.next.take() {
            self.append(unit, now);
        }
        Some(batch)
    }

    #[cfg(test)]
    pub(crate) fn deadline(&self, now: Duration) -> Option<Duration> {
        if self.ready(now) {
            return Some(Duration::ZERO);
        }
        self.flush_at
            .map(|deadline| deadline.saturating_sub(now))
            .into_iter()
            .chain(self.filter.deadline())
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_socket_display_core_canonical_batches_are_idempotent() -> Result<()> {
        let mut output = Batches::default();
        let now = Duration::from_millis(10);
        let prefix = vec![b'a'; MAX_DATA - 1];
        assert_eq!(output.push(&prefix, now)?, prefix.len());
        assert_eq!(output.push("☃".as_bytes(), now)?, 3);
        let full = output.take(now).unwrap();
        assert_eq!(full, prefix);
        Filter::validate_batch(&full)?;

        let tail = b"\x1b]0;untrusted title\x07\x1b[?1049h";
        assert_eq!(output.push(tail, now)?, tail.len());
        let complete = output.take(now + FLUSH_AFTER).unwrap();
        assert_eq!(complete, "☃\x1b[?1049h".as_bytes());
        Filter::validate_batch(&complete)?;
        for invalid in [b"\x1b[".as_slice(), tail.as_slice(), b"\xe2\x98"] {
            assert!(Filter::validate_batch(invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn terminal_socket_display_core_batches_preserve_split_display_units() -> Result<()> {
        let mut output = Batches::default();
        let start = Duration::from_millis(20);
        let bytes = vec![b'x'; MAX_DATA - 1];
        assert_eq!(output.push(&bytes, start)?, bytes.len());
        assert_eq!(output.deadline(start), Some(FLUSH_AFTER));
        assert_eq!(output.push(b"\xe2", start)?, 1);
        assert_eq!(output.push(b"\x98", start)?, 1);
        assert_eq!(output.push(b"\x83\x1b[", start)?, 1);
        assert_eq!(output.deadline(start), Some(Duration::ZERO));
        assert_eq!(output.take(start).unwrap(), bytes);

        assert_eq!(output.push(b"\x1b[", start)?, 2);
        assert_eq!(output.push(b"?1049h", start)?, 6);
        assert_eq!(output.take(start + Duration::from_millis(7)), None);
        assert_eq!(
            output.take(start + FLUSH_AFTER).unwrap(),
            b"\xe2\x98\x83\x1b[?1049h",
        );
        assert_eq!(output.deadline(start), None);
        Ok(())
    }

    #[test]
    fn terminal_socket_display_core_discards_and_bounds_incomplete_units() -> Result<()> {
        let mut output = Batches::default();
        let now = Duration::ZERO;
        assert_eq!(output.push(b"\x1b]52;c;discard", now)?, 14);
        assert_eq!(output.take(now), None);
        let deadline = output.deadline(now).unwrap();
        output.advance(deadline - Duration::from_millis(1))?;
        assert_eq!(output.push(b"\x07", now)?, 1);
        assert_eq!(output.take(now), None);
        assert_eq!(output.deadline(now), None);

        output.push(b"\x1b", now)?;
        let deadline = output.deadline(now).unwrap();
        assert!(output.advance(deadline).is_err());
        Ok(())
    }
}

#[cfg(test)]
#[path = "terminal_redraw_evaluation_tests.rs"]
mod terminal_redraw_evaluation_tests;
