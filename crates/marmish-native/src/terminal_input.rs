//! Local detach recognition for the controlling terminal.

use std::time::Duration;

const PREFIX_LIMIT: Duration = Duration::from_millis(32);
const PASTE_LIMIT: Duration = Duration::from_secs(5);
const PASTE_BYTES: usize = 64 * 1024;
const PASTE_BEGIN: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

pub(super) struct Input {
    observer: bool,
    line_start: bool,
    bracketed_paste: bool,
    paste: Option<Paste>,
    prefix: Prefix,
    prefix_age: Duration,
    detached: bool,
}

struct Paste {
    age: Duration,
    bytes: usize,
}

#[derive(Default)]
enum Prefix {
    #[default]
    None,
    Tilde,
    Marker(usize),
}

impl Input {
    pub(super) fn new(observer: bool) -> Self {
        Self {
            observer,
            line_start: true,
            bracketed_paste: false,
            paste: None,
            prefix: Prefix::None,
            prefix_age: Duration::ZERO,
            detached: false,
        }
    }

    pub(super) fn set_paste_mode(&mut self, enabled: bool, output: &mut Vec<u8>) {
        if !enabled && self.bracketed_paste {
            self.flush_marker(output);
            self.paste = None;
        }
        self.bracketed_paste = enabled;
    }

    /// Prefixes have an absolute deadline. A paste ages only while reads can proceed.
    pub(super) fn advance(
        &mut self,
        elapsed: Duration,
        read_elapsed: Duration,
        output: &mut Vec<u8>,
    ) {
        self.prefix_age = self.prefix_age.saturating_add(elapsed);
        if self.prefix_age >= PREFIX_LIMIT {
            self.flush(output);
        }
        if let Some(paste) = &mut self.paste {
            paste.age = paste.age.saturating_add(read_elapsed);
            if paste.age >= PASTE_LIMIT {
                self.end_expired_paste(output);
            }
        }
    }

    pub(super) fn deadline(&self, reading: bool) -> Option<Duration> {
        let prefix = (!matches!(self.prefix, Prefix::None))
            .then(|| PREFIX_LIMIT.saturating_sub(self.prefix_age));
        let paste = self
            .paste
            .as_ref()
            .filter(|_| reading)
            .map(|paste| PASTE_LIMIT.saturating_sub(paste.age));
        prefix.into_iter().chain(paste).min()
    }

    /// True means detach. The caller discards the remainder of the read.
    pub(super) fn push(&mut self, byte: u8, output: &mut Vec<u8>) -> bool {
        if self.detached {
            return true;
        }

        match std::mem::take(&mut self.prefix) {
            Prefix::Tilde => {
                self.prefix_age = Duration::ZERO;
                if byte == b'.' {
                    self.detached = true;
                    return true;
                }
                self.emit(b'~', output);
                if byte != b'~' {
                    self.start_or_emit(byte, output);
                }
            }
            Prefix::Marker(len) => {
                let marker = self.marker();
                if marker[len] != byte {
                    self.emit_marker_prefix(len, output);
                    self.start_or_emit(byte, output);
                } else if len + 1 == marker.len() {
                    for byte in marker {
                        self.emit(*byte, output);
                    }
                    self.paste = self.paste.is_none().then_some(Paste {
                        age: Duration::ZERO,
                        bytes: 0,
                    });
                } else {
                    self.prefix = Prefix::Marker(len + 1);
                }
            }
            Prefix::None => self.start_or_emit(byte, output),
        }
        if let Some(paste) = &mut self.paste {
            paste.bytes += 1;
            if paste.bytes >= PASTE_BYTES {
                self.end_expired_paste(output);
            }
        }
        false
    }

    pub(super) fn flush(&mut self, output: &mut Vec<u8>) {
        match std::mem::take(&mut self.prefix) {
            Prefix::None => {}
            Prefix::Tilde => self.emit(b'~', output),
            Prefix::Marker(len) => self.emit_marker_prefix(len, output),
        }
    }

    fn marker(&self) -> &'static [u8] {
        if self.paste.is_some() {
            PASTE_END
        } else {
            PASTE_BEGIN
        }
    }

    fn emit_marker_prefix(&mut self, len: usize, output: &mut Vec<u8>) {
        for byte in &self.marker()[..len] {
            self.emit(*byte, output);
        }
    }

    fn flush_marker(&mut self, output: &mut Vec<u8>) {
        if let Prefix::Marker(len) = self.prefix {
            self.prefix = Prefix::None;
            self.emit_marker_prefix(len, output);
        }
    }

    fn end_expired_paste(&mut self, output: &mut Vec<u8>) {
        self.flush_marker(output);
        self.paste = None;
        self.line_start = true;
    }

    fn start_or_emit(&mut self, byte: u8, output: &mut Vec<u8>) {
        let prefix = if self.bracketed_paste && byte == 0x1b {
            Prefix::Marker(1)
        } else if self.paste.is_none() && self.line_start && byte == b'~' {
            Prefix::Tilde
        } else {
            self.emit(byte, output);
            return;
        };
        self.prefix_age = Duration::ZERO;
        self.prefix = prefix;
    }

    fn emit(&mut self, byte: u8, output: &mut Vec<u8>) {
        self.line_start = matches!(byte, b'\r' | b'\n');
        if !self.observer {
            output.push(byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(input: &mut Input, bytes: &[u8], output: &mut Vec<u8>) -> bool {
        bytes.iter().copied().any(|byte| input.push(byte, output))
    }

    #[test]
    fn core_tests_terminal_frontend_detach_and_observer() {
        for observer in [true, false] {
            let mut input = Input::new(observer);
            let mut out = Vec::new();
            assert!(!feed(&mut input, b"a~.\n~~abc\r~x\n~", &mut out));
            assert!(feed(&mut input, b".ignored", &mut out));
            assert!(feed(&mut input, b"ignored", &mut out));
            assert_eq!(
                out,
                if observer {
                    b"".as_slice()
                } else {
                    b"a~.\n~abc\r~x\n"
                }
            );
        }
    }

    #[test]
    fn core_tests_terminal_frontend_detach_deadline_and_eof() {
        let mut input = Input::new(false);
        let mut out = Vec::new();
        assert!(!input.push(b'~', &mut out));
        input.advance(
            PREFIX_LIMIT - Duration::from_nanos(1),
            Duration::ZERO,
            &mut out,
        );
        assert!(out.is_empty());
        input.advance(Duration::from_nanos(1), Duration::ZERO, &mut out);
        assert!(!feed(&mut input, b".\r~", &mut out));
        input.flush(&mut out);
        input.flush(&mut out);
        assert_eq!(out, b"~.\r~");
    }

    #[test]
    fn core_tests_terminal_frontend_paste_after_leading_tilde() {
        let bytes = b"~\x1b[200~line\n~.more\x1b[201~";
        for observer in [true, false] {
            let mut input = Input::new(observer);
            let mut output = Vec::new();
            input.set_paste_mode(true, &mut output);
            assert!(!feed(&mut input, bytes, &mut output));
            assert_eq!(output, if observer { b"".as_slice() } else { bytes });
            assert!(feed(&mut input, b"\r~.", &mut output));
        }
    }

    #[test]
    fn core_tests_terminal_frontend_paste_delimiters_and_expiry() {
        let mut input = Input::new(false);
        let mut out = Vec::new();
        input.set_paste_mode(true, &mut out);
        for bytes in [b"\x1b[20".as_slice(), b"0~\r~.\n\x1b[", b"201~\r"] {
            assert!(!feed(&mut input, bytes, &mut out));
        }
        assert!(feed(&mut input, b"~.", &mut out));
        assert_eq!(out, b"\x1b[200~\r~.\n\x1b[201~\r");

        for expires_by_bytes in [true, false] {
            let mut input = Input::new(false);
            let mut out = Vec::new();
            input.set_paste_mode(true, &mut out);
            assert!(!feed(&mut input, PASTE_BEGIN, &mut out));
            input.advance(PASTE_LIMIT * 2, Duration::ZERO, &mut out);
            assert!(!feed(&mut input, b"\r~.", &mut out));
            if expires_by_bytes {
                let remaining = PASTE_BYTES - input.paste.as_ref().unwrap().bytes;
                assert!(!feed(&mut input, &vec![b'x'; remaining], &mut out));
            } else {
                input.advance(Duration::ZERO, PASTE_LIMIT, &mut out);
            }
            assert!(feed(&mut input, b"~.", &mut out));
        }
    }

    #[test]
    fn core_tests_terminal_frontend_incomplete_paste_marker() {
        let mut input = Input::new(false);
        let mut out = Vec::new();
        input.set_paste_mode(true, &mut out);
        assert!(!feed(&mut input, b"\x1b[20\x1b[2", &mut out));
        input.advance(PREFIX_LIMIT, Duration::ZERO, &mut out);
        assert_eq!(out, b"\x1b[20\x1b[2");
        assert!(!feed(&mut input, b"\x1b[20", &mut out));
        input.set_paste_mode(false, &mut out);
        assert_eq!(out, b"\x1b[20\x1b[2\x1b[20");
    }
}
