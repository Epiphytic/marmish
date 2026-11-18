//! Streaming display policy for bytes from an authenticated remote terminal.

use super::{Error, Result};
use std::time::Duration;

const UNIT_LIMIT: usize = 96;
const STRING_LIMIT: usize = 64 * 1024;
const TIME_LIMIT: Duration = Duration::from_secs(8);
const POLICY: Error = Error::Invalid("terminal_output_policy");

#[derive(Default)]
pub(super) struct Filter {
    state: State,
    elapsed: Duration,
}

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Utf8(Utf8),
    Short(Short),
    String(DiscardString),
}

/// An independently safe, complete display unit. It owns its encoded bytes.
pub(super) struct DisplayUnit {
    bytes: [u8; UNIT_LIMIT],
    len: usize,
}

impl DisplayUnit {
    fn new(bytes: &[u8]) -> Self {
        let mut unit = Self {
            bytes: [0; UNIT_LIMIT],
            len: bytes.len(),
        };
        unit.bytes[..unit.len].copy_from_slice(bytes);
        unit
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Filter {
    pub(super) fn is_ground(&self) -> bool {
        matches!(self.state, State::Ground)
    }

    /// Reject records that end inside a control or would change on display.
    pub(super) fn validate_batch(bytes: &[u8]) -> Result<()> {
        let mut filter = Self::default();
        let mut rest = bytes;
        for &byte in bytes {
            if let Some(unit) = filter.push(byte)? {
                rest = rest.strip_prefix(unit.bytes()).ok_or(POLICY)?;
            }
        }
        if filter.is_ground() && rest.is_empty() {
            Ok(())
        } else {
            Err(POLICY)
        }
    }

    /// The frontend calls this only for elapsed time when it can consume data.
    pub(super) fn advance(&mut self, elapsed: Duration) -> Result<()> {
        if self.is_ground() {
            return Ok(());
        }
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if self.elapsed >= TIME_LIMIT {
            return Err(POLICY);
        }
        Ok(())
    }

    pub(super) fn deadline(&self) -> Option<Duration> {
        (!self.is_ground()).then(|| TIME_LIMIT.saturating_sub(self.elapsed))
    }

    pub(super) fn push(&mut self, byte: u8) -> Result<Option<DisplayUnit>> {
        let state = std::mem::take(&mut self.state);
        let (state, output) = match state {
            State::Ground => return Ok(self.ground(byte)),
            State::Utf8(mut utf8) => match utf8.push(byte) {
                Utf8Result::Continue => (State::Utf8(utf8), None),
                Utf8Result::Complete(ch) => (
                    State::Ground,
                    (!('\u{80}'..='\u{9f}').contains(&ch)).then(|| DisplayUnit::new(utf8.bytes())),
                ),
                Utf8Result::Invalid => return Ok(self.ground(byte)),
            },
            State::Short(short) => {
                if byte == 0x1b || (0x80..=0x9f).contains(&byte) {
                    return Ok(self.ground(byte));
                }
                if matches!(byte, 0x18 | 0x1a) {
                    (State::Ground, None)
                } else if byte < 0x20 {
                    return Ok(self.ground(byte));
                } else {
                    short.push(byte)
                }
            }
            State::String(mut string) => {
                if string.push(byte)? {
                    (State::Ground, None)
                } else {
                    (State::String(string), None)
                }
            }
        };
        self.state = state;
        Ok(output)
    }

    fn ground(&mut self, byte: u8) -> Option<DisplayUnit> {
        self.elapsed = Duration::ZERO;
        self.state = match byte {
            0x1b => State::Short(Short::new(ShortKind::Escape)),
            0x9b => State::Short(Short::new(ShortKind::DeniedCsi)),
            0x90 | 0x98 | 0x9d..=0x9f => {
                State::String(DiscardString::new(byte == 0x9d, byte == 0x90))
            }
            0xc2..=0xf4 => State::Utf8(Utf8::new(byte)),
            _ => State::Ground,
        };
        matches!(byte, 0x08..=0x0a | 0x0d..=0x0f | 0x20..=0x7e).then(|| DisplayUnit::new(&[byte]))
    }
}

struct Utf8 {
    data: [u8; 4],
    len: usize,
    total: usize,
}

enum Utf8Result {
    Continue,
    Complete(char),
    Invalid,
}

impl Utf8 {
    fn new(first: u8) -> Self {
        let total = match first {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            _ => 4,
        };
        Self {
            data: [first, 0, 0, 0],
            len: 1,
            total,
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }

    fn push(&mut self, byte: u8) -> Utf8Result {
        let allowed = match (self.len, self.data[0]) {
            (1, 0xe0) => 0xa0..=0xbf,
            (1, 0xed) => 0x80..=0x9f,
            (1, 0xf0) => 0x90..=0xbf,
            (1, 0xf4) => 0x80..=0x8f,
            _ => 0x80..=0xbf,
        };
        if !allowed.contains(&byte) {
            return Utf8Result::Invalid;
        }
        self.data[self.len] = byte;
        self.len += 1;
        if self.len < self.total {
            return Utf8Result::Continue;
        }
        std::str::from_utf8(self.bytes())
            .ok()
            .and_then(|text| text.chars().next())
            .map_or(Utf8Result::Invalid, Utf8Result::Complete)
    }
}

#[derive(Clone, Copy)]
enum ShortKind {
    Escape,
    Csi,
    DeniedCsi,
}

struct Short {
    bytes: [u8; UNIT_LIMIT - 2],
    len: usize,
    kind: ShortKind,
}

impl Short {
    fn new(kind: ShortKind) -> Self {
        Self {
            bytes: [0; UNIT_LIMIT - 2],
            len: 0,
            kind,
        }
    }

    fn push(mut self, byte: u8) -> (State, Option<DisplayUnit>) {
        if self.len == 0 && matches!(self.kind, ShortKind::Escape) {
            match byte {
                b'[' => {
                    self.kind = ShortKind::Csi;
                    return (State::Short(self), None);
                }
                b']' | b'P' | b'X' | b'^' | b'_' | b'k' => {
                    return (
                        State::String(DiscardString::new(byte == b']', byte == b'P')),
                        None,
                    );
                }
                _ => {}
            }
        }
        let final_byte = match self.kind {
            ShortKind::Escape => (0x30..=0x7e).contains(&byte),
            ShortKind::Csi | ShortKind::DeniedCsi => (0x40..=0x7e).contains(&byte),
        };
        self.pending(byte, final_byte)
    }

    fn pending(mut self, byte: u8, final_byte: bool) -> (State, Option<DisplayUnit>) {
        // Once full, keep consuming the same unit without storing another byte.
        if self.len >= self.bytes.len() {
            return if final_byte {
                (State::Ground, None)
            } else {
                (State::Short(self), None)
            };
        }
        if byte >= 0x7f {
            self.len = self.bytes.len();
            return (State::Short(self), None);
        }
        self.bytes[self.len] = byte;
        self.len += 1;
        if !final_byte {
            return (State::Short(self), None);
        }
        let raw = &self.bytes[..self.len];
        let unit = match self.kind {
            ShortKind::Escape => escape(raw),
            ShortKind::Csi => csi(raw),
            ShortKind::DeniedCsi => None,
        };
        (State::Ground, unit)
    }
}

fn escape(raw: &[u8]) -> Option<DisplayUnit> {
    if !matches!(
        raw,
        [b'7' | b'8' | b'D' | b'E' | b'M' | b'=' | b'>'] | [b'(' | b')', b'B' | b'0']
    ) {
        return None;
    }
    let mut bytes = [0x1b, 0, 0];
    bytes[1..=raw.len()].copy_from_slice(raw);
    Some(DisplayUnit::new(&bytes[..=raw.len()]))
}

struct DiscardString {
    osc: bool,
    tmux_prefix: Option<usize>,
    in_tmux: bool,
    escaped: bool,
    utf8: Option<Utf8>,
    len: usize,
}

impl DiscardString {
    fn new(osc: bool, dcs: bool) -> Self {
        Self {
            osc,
            tmux_prefix: dcs.then_some(0),
            in_tmux: false,
            escaped: false,
            utf8: None,
            len: 0,
        }
    }

    fn push(&mut self, byte: u8) -> Result<bool> {
        const PREFIX: &[u8] = b"tmux;";
        self.len += 1;
        if self.len > STRING_LIMIT {
            return Err(POLICY);
        }
        if let Some(index) = self.tmux_prefix.take()
            && PREFIX[index] == byte
        {
            if index + 1 == PREFIX.len() {
                self.in_tmux = true;
            } else {
                self.tmux_prefix = Some(index + 1);
            }
        }
        if self.escaped {
            self.escaped = false;
            if byte == b'\\' {
                return Ok(true);
            }
            if byte == 0x1b {
                return Ok(false);
            }
        }
        if let Some(mut utf8) = self.utf8.take() {
            match utf8.push(byte) {
                Utf8Result::Continue => {
                    self.utf8 = Some(utf8);
                    return Ok(false);
                }
                Utf8Result::Complete(_) => return Ok(false),
                Utf8Result::Invalid => {}
            }
        }
        match byte {
            0x1b => self.escaped = true,
            0xc2..=0xf4 => self.utf8 = Some(Utf8::new(byte)),
            0x9c if !self.in_tmux => return Ok(true),
            0x07 if self.osc => return Ok(true),
            _ => {}
        }
        Ok(false)
    }
}

fn csi(raw: &[u8]) -> Option<DisplayUnit> {
    let (&final_byte, parameters) = raw.split_last()?;
    let (private, parameters) = parameters
        .strip_prefix(b"?")
        .map_or((false, parameters), |rest| (true, rest));
    let (cursor_style, parameters) = parameters
        .strip_suffix(b" ")
        .map_or((false, parameters), |rest| (true, rest));

    let mut fields = [None; 16];
    let count = if parameters.is_empty() {
        0
    } else {
        let mut count = 0;
        for raw in parameters.split(|&byte| byte == b';') {
            let field = fields.get_mut(count)?;
            if !raw.is_empty() {
                let mut value = 0u16;
                for byte in raw {
                    let digit = byte.checked_sub(b'0').filter(|value| *value <= 9)?;
                    value = value.checked_mul(10)?.checked_add(u16::from(digit))?;
                    if value > 4096 {
                        return None;
                    }
                }
                *field = Some(value);
            }
            count += 1;
        }
        count
    };
    let fields = &fields[..count];
    if !csi_allowed(final_byte, private, cursor_style, fields) {
        return None;
    }
    let mut out = [0; UNIT_LIMIT];
    out[..2].copy_from_slice(b"\x1b[");
    let mut end = 2;
    if private {
        out[end] = b'?';
        end += 1;
    }
    for (index, field) in fields.iter().enumerate() {
        if index != 0 {
            out[end] = b';';
            end += 1;
        }
        if let Some(value) = field {
            end += decimal(*value, &mut out[end..]);
        }
    }
    if cursor_style {
        out[end] = b' ';
        end += 1;
    }
    out[end] = final_byte;
    Some(DisplayUnit::new(&out[..=end]))
}

fn decimal(mut value: u16, output: &mut [u8]) -> usize {
    let mut digits = [0; 4];
    let mut used = digits.len();
    loop {
        used -= 1;
        digits[used] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    let bytes = &digits[used..];
    output[..bytes.len()].copy_from_slice(bytes);
    bytes.len()
}

fn csi_allowed(final_byte: u8, private: bool, cursor_style: bool, fields: &[Option<u16>]) -> bool {
    if cursor_style {
        return !private && final_byte == b'q' && matches!(fields, [Some(0..=6)]);
    }
    if private {
        return matches!(final_byte, b'h' | b'l')
            && matches!(fields, [Some(1 | 6 | 7 | 25 | 1049 | 2004)]);
    }
    match final_byte {
        b'A' | b'B' | b'C' | b'D' | b'E' | b'F' | b'G' | b'I' | b'Z' | b'd' | b'@' | b'L'
        | b'M' | b'P' | b'S' | b'T' | b'X' | b'b' => fields.len() <= 1,
        b'H' | b'f' => fields.len() <= 2,
        b'J' | b'K' => matches!(fields, [] | [None | Some(0..=2)]),
        b'r' => fields.len() <= 2 && fields.iter().flatten().all(|value| *value <= 250),
        b's' | b'u' => fields.is_empty(),
        b'h' | b'l' => fields == [Some(4)],
        b'm' => sgr_allowed(fields),
        _ => false,
    }
}

fn sgr_allowed(mut fields: &[Option<u16>]) -> bool {
    while let Some((first, rest)) = fields.split_first() {
        fields = match (first, rest) {
            (Some(38 | 48), [Some(5), Some(0..=255), remainder @ ..])
            | (
                Some(38 | 48),
                [
                    Some(2),
                    Some(0..=255),
                    Some(0..=255),
                    Some(0..=255),
                    remainder @ ..,
                ],
            ) => remainder,
            (
                None | Some(0..=7 | 9 | 21..=29 | 30..=37 | 39..=47 | 49 | 90..=97 | 100..=107),
                _,
            ) => rest,
            _ => return false,
        };
    }
    true
}

#[cfg(test)]
#[path = "terminal_output_tests.rs"]
mod tests;
