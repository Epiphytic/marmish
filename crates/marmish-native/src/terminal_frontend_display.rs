//! Bounded display writes and fixed recovery for a foreground attachment.

use super::terminal_output::{DisplayUnit, Filter};
use super::{Error, Result};
use std::collections::VecDeque;
use std::time::Duration;

const UNIT_CAPACITY: usize = 1024;
const BATCH_BYTES: usize = 4096;
const OUTPUT: Error = Error::Invalid("terminal_display");

#[derive(Default)]
pub(super) struct Display {
    filter: Filter,
    pending: VecDeque<DisplayUnit>,
    partial: usize,
    alternate: bool,
    paste: bool,
}

impl Display {
    /// Consume only the prefix that fits. The caller retains the remainder.
    pub(super) fn ingest(&mut self, bytes: &[u8]) -> Result<usize> {
        let count = bytes.len().min(UNIT_CAPACITY - self.pending.len());
        for &byte in &bytes[..count] {
            if let Some(unit) = self.filter.push(byte)? {
                self.pending.push_back(unit);
            }
        }
        Ok(count)
    }

    pub(super) fn can_ingest(&self) -> bool {
        self.pending.len() < UNIT_CAPACITY
    }

    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(super) fn record_committed(&self, remaining: usize) -> Result<bool> {
        if remaining != 0 {
            return Ok(false);
        }
        if !self.filter.is_ground() {
            return Err(OUTPUT);
        }
        Ok(!self.has_pending())
    }

    pub(super) fn advance(&mut self, elapsed: Duration) -> Result<()> {
        if self.can_ingest() {
            self.filter.advance(elapsed)?;
        }
        Ok(())
    }

    pub(super) fn deadline(&self) -> Option<Duration> {
        if self.can_ingest() {
            self.filter.deadline()
        } else {
            None
        }
    }

    pub(super) fn paste_enabled(&self) -> bool {
        self.paste
    }

    pub(super) fn write(
        &mut self,
        mut sink: impl FnMut(&[u8]) -> Result<Option<usize>>,
    ) -> Result<()> {
        let mut batch = [0; BATCH_BYTES];
        let mut len = 0;
        for (index, unit) in self.pending.iter().enumerate() {
            let offset = if index == 0 { self.partial } else { 0 };
            let bytes = &unit.bytes()[offset..];
            if len + bytes.len() > batch.len() {
                break;
            }
            batch[len..len + bytes.len()].copy_from_slice(bytes);
            len += bytes.len();
        }
        if len != 0 {
            let written = write_count(&mut sink, &batch[..len])?;
            self.commit(written);
        }
        Ok(())
    }

    fn commit(&mut self, mut count: usize) {
        while count > 0 {
            let head = self.pending.front().expect("bounded by the offered output");
            let remaining = head.bytes().len() - self.partial;
            if count < remaining {
                self.partial += count;
                return;
            }
            count -= remaining;
            self.partial = 0;
            let unit = self.pending.pop_front().expect("front exists");
            match unit.bytes() {
                b"\x1b[?1049h" => self.alternate = true,
                b"\x1b[?1049l" => self.alternate = false,
                b"\x1b[?2004h" => self.paste = true,
                b"\x1b[?2004l" => self.paste = false,
                _ => {}
            }
        }
    }

    /// Stop peer output after the current partial unit. The local poll owner
    /// gives the whole recovery one deadline and restores termios even on error.
    pub(super) fn cleanup(mut self, marker: Marker) -> Cleanup {
        self.pending.truncate(usize::from(self.partial != 0));
        Cleanup {
            display: self,
            marker,
            trailer: None,
            offset: 0,
            incomplete: false,
        }
    }
}

pub(super) enum Marker {
    Detached,
    Ended,
    Suspended,
    Interrupted,
    TerminalError,
}

pub(super) struct Cleanup {
    display: Display,
    marker: Marker,
    trailer: Option<Vec<u8>>,
    offset: usize,
    incomplete: bool,
}

impl Cleanup {
    /// At most one syscall per call. The caller may explicitly abandon a
    /// stalled partial unit and continue with the local CAN recovery.
    pub(super) fn write(
        &mut self,
        mut sink: impl FnMut(&[u8]) -> Result<Option<usize>>,
    ) -> Result<bool> {
        if self.display.partial != 0 {
            self.display.write(sink)?;
            return Ok(false);
        }
        let trailer = self.trailer.get_or_insert_with(|| {
            normalization(self.display.alternate, self.incomplete, &self.marker)
        });
        if self.offset < trailer.len() {
            self.offset += write_count(&mut sink, &trailer[self.offset..])?;
        }
        Ok(self.offset == trailer.len())
    }

    pub(super) fn abandon_partial(&mut self) {
        if self.display.partial != 0 {
            if self
                .display
                .pending
                .front()
                .is_some_and(|unit| unit.bytes() == b"\x1b[?1049h")
            {
                self.display.alternate = true;
            }
            self.display.pending.clear();
            self.display.partial = 0;
            self.incomplete = true;
        }
    }

    pub(super) fn complete(&self) -> bool {
        !self.incomplete
            && self
                .trailer
                .as_ref()
                .is_some_and(|bytes| bytes.len() == self.offset)
    }
}

fn write_count(
    sink: &mut impl FnMut(&[u8]) -> Result<Option<usize>>,
    bytes: &[u8],
) -> Result<usize> {
    match sink(bytes)? {
        Some(count) if count > 0 && count <= bytes.len() => Ok(count),
        Some(_) => Err(OUTPUT),
        None => Ok(0),
    }
}

fn normalization(alternate: bool, incomplete: bool, marker: &Marker) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(128);
    if incomplete {
        bytes.push(0x18); // CAN terminates an incomplete emulator control sequence.
    }
    if alternate {
        bytes.extend_from_slice(b"\x1b[?1049l");
    }
    bytes.extend_from_slice(
        b"\x1b[0m\x1b[0 q\x1b[?25h\x1b[r\x1b[?6l\x1b[?7h\x1b[4l\x1b(B\x1b)B\x0f\x1b[?1l\x1b>\x1b[?2004l",
    );
    bytes.extend_from_slice(match marker {
        Marker::Detached => b"\r\n[marmish: detached]\r\n",
        Marker::Ended => b"\r\n[marmish: terminal ended]\r\n",
        Marker::Suspended => b"\r\n[marmish: suspended]\r\n",
        Marker::Interrupted => b"\r\n[marmish: interrupted]\r\n",
        Marker::TerminalError => b"\r\n[marmish: terminal error]\r\n",
    });
    bytes
}

#[cfg(test)]
mod core_tests_terminal_frontend {
    use super::*;

    #[test]
    fn terminal_socket_display_core_ack_waits_for_independent_written_records() -> Result<()> {
        let mut display = Display::default();
        let bytes = b"\xe2\x98\x83\x1b[?1049h";
        assert_eq!(display.ingest(bytes)?, bytes.len());
        assert!(!display.record_committed(0)?);
        display.write(|pending| {
            assert_eq!(pending, bytes);
            Ok(Some(1))
        })?;
        assert!(!display.record_committed(0)?);
        display.write(|pending| {
            assert_eq!(pending, &bytes[1..]);
            Ok(Some(pending.len()))
        })?;
        assert!(display.record_committed(0)?);
        assert!(!display.record_committed(1)?);
        let discarded = b"\x1b]52;c;AA==\x07";
        assert_eq!(display.ingest(discarded)?, discarded.len());
        assert!(display.record_committed(0)?);
        let incomplete = b"\x1b[";
        assert_eq!(display.ingest(incomplete)?, incomplete.len());
        assert!(display.record_committed(0).is_err());
        Ok(())
    }

    #[test]
    fn partial_controls_commit_only_after_the_boundary() -> Result<()> {
        let mut display = Display::default();
        let data = b"\x1b]52;c;discard\x07\x1b[?02004htext";
        assert_eq!(display.ingest(data)?, data.len());
        display.write(|batch| {
            assert_eq!(batch, b"\x1b[?2004htext");
            Ok(Some(3))
        })?;
        assert!(!display.paste_enabled());
        display.write(|batch| {
            assert_eq!(batch, b"2004htext");
            Ok(None)
        })?;
        assert!(!display.paste_enabled());
        display.write(|_| Ok(Some(5)))?;
        assert!(display.paste_enabled());
        let mut output = Vec::new();
        display.write(|batch| {
            output.extend_from_slice(batch);
            Ok(Some(batch.len()))
        })?;
        assert_eq!(output, b"text");
        Ok(())
    }

    #[test]
    fn backpressure_preserves_the_exact_unconsumed_prefix() -> Result<()> {
        let mut display = Display::default();
        let data = vec![b'x'; UNIT_CAPACITY + 8];
        assert_eq!(display.ingest(&data)?, UNIT_CAPACITY);
        assert!(!display.can_ingest());
        assert_eq!(display.ingest(b"\x1b]52")?, 0);
        display.write(|batch| {
            assert_eq!(batch, &data[..UNIT_CAPACITY]);
            Ok(Some(8))
        })?;
        assert!(display.can_ingest());
        assert_eq!(display.ingest(b"last\x1b[?2")?, 8);
        assert_eq!(display.ingest(b"004h")?, 4);
        Ok(())
    }

    #[test]
    fn cleanup_finishes_only_the_partial_unit_and_uses_local_markers() -> Result<()> {
        for (marker, ending) in [
            (Marker::Detached, b"\r\n[marmish: detached]\r\n".as_slice()),
            (Marker::Ended, b"\r\n[marmish: terminal ended]\r\n"),
            (Marker::Suspended, b"\r\n[marmish: suspended]\r\n"),
            (Marker::Interrupted, b"\r\n[marmish: interrupted]\r\n"),
            (Marker::TerminalError, b"\r\n[marmish: terminal error]\r\n"),
        ] {
            let mut display = Display::default();
            display.ingest(b"\x1b[?1049hpeer remainder")?;
            display.write(|_| Ok(Some(1)))?;
            let mut cleanup = display.cleanup(marker);
            let mut output = Vec::new();
            while !cleanup.write(|bytes| {
                output.push(bytes[0]);
                Ok(Some(1))
            })? {}
            assert!(cleanup.complete());
            assert!(output.starts_with(b"[?1049h\x1b[?1049l"));
            assert!(output.ends_with(ending));
            assert!(!output.windows(4).any(|bytes| bytes == b"peer"));
        }
        Ok(())
    }

    #[test]
    fn abandoned_partial_is_reported_even_after_can_recovery() -> Result<()> {
        let mut display = Display::default();
        display.ingest(b"\x1b[?1049htext")?;
        display.write(|_| Ok(Some(6)))?;
        let mut cleanup = display.cleanup(Marker::TerminalError);
        cleanup.abandon_partial();
        assert!(cleanup.write(|batch| {
            assert!(batch.starts_with(b"\x18\x1b[?1049l"));
            Ok(Some(batch.len()))
        })?);
        assert!(!cleanup.complete());
        Ok(())
    }

    #[test]
    fn active_time_bounds_even_a_display_with_no_output() -> Result<()> {
        let mut display = Display::default();
        display.ingest(b"x\x1b]52;c;unfinished")?;
        display.advance(Duration::from_millis(7999))?;
        assert!(display.advance(Duration::from_millis(1)).is_err());
        Ok(())
    }
}
