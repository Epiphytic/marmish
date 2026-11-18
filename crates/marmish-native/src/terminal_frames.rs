//! Test-gated Terminal v1 framing. Callers establish MLS and grant authority.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use cgka_traits::app_event::{AppMessageRetentionDecision, MarmotAppEvent};
use std::time::Duration;

pub(crate) const KIND: u64 = 24862;
pub(crate) const MAX_APP: usize = 6144;
pub(crate) const MAX_DATA: usize = 4096;
pub(crate) const MAX_BASE64: usize = 5552;
pub(crate) const RETENTION: u64 = 60;
const MAX_AGE: u64 = 15;
const FUTURE_SLACK: u64 = 5;
const HEADER: usize = 60;

#[cfg(test)]
#[path = "terminal_frames_core_tests.rs"]
mod core_tests;
#[path = "terminal_frames_delivery.rs"]
mod delivery;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    ToServer,
    FromServer,
}

impl Direction {
    fn reverse(self) -> Self {
        match self {
            Self::ToServer => Self::FromServer,
            Self::FromServer => Self::ToServer,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Writer,
    Observer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) transcript: [u8; 32],
    pub(crate) attachment: [u8; 16],
    pub(crate) generation: u64,
    pub(crate) role: Role,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Size {
    pub(crate) rows: u16,
    pub(crate) cols: u16,
}

impl Size {
    pub(crate) fn valid(self) -> bool {
        (2..=250).contains(&self.rows) && (2..=500).contains(&self.cols)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Data {
    Input(Vec<u8>),
    Output(Vec<u8>),
    Resize(Size),
}

impl Data {
    fn len(&self) -> usize {
        match self {
            Self::Input(bytes) | Self::Output(bytes) => bytes.len(),
            Self::Resize(_) => 0,
        }
    }

    fn allowed(&self, direction: Direction, role: Role) -> bool {
        match (self, direction, role) {
            (Self::Input(bytes), Direction::ToServer, Role::Writer)
            | (Self::Output(bytes), Direction::FromServer, _) => {
                (1..=MAX_DATA).contains(&bytes.len())
            }
            (Self::Resize(size), Direction::ToServer, Role::Writer) => size.valid(),
            _ => false,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Record {
    Ordered(u64, Data),
    Ack(u64),
    Pulse(u64),
    Close,
}

impl Record {
    fn valid(&self, binding: Binding, direction: Direction) -> bool {
        if binding.generation == 0 {
            return false;
        }
        match self {
            Self::Ordered(sequence, data) => {
                *sequence != 0 && data.allowed(direction, binding.role)
            }
            Self::Ack(_) | Self::Close => true,
            Self::Pulse(pulse) => *pulse != 0 && direction == Direction::ToServer,
        }
    }
}

pub(crate) fn encode_app(
    author: &[u8; 32],
    created_at: u64,
    binding: Binding,
    direction: Direction,
    record: &Record,
) -> Option<Vec<u8>> {
    if !record.valid(binding, direction) || created_at.checked_add(RETENTION).is_none() {
        return None;
    }
    let variant = match record {
        Record::Ordered(_, Data::Input(_)) => 1,
        Record::Ordered(_, Data::Output(_)) => 2,
        Record::Ordered(_, Data::Resize(_)) => 3,
        Record::Ack(_) => 4,
        Record::Pulse(_) => 5,
        Record::Close => 6,
    };
    let body_len = match record {
        Record::Ordered(_, Data::Resize(_)) => 12,
        Record::Ordered(_, data) => 8 + data.len(),
        Record::Ack(_) | Record::Pulse(_) => 8,
        Record::Close => 0,
    };
    let mut bytes = Vec::with_capacity(HEADER + body_len);
    bytes.extend_from_slice(&[b'M', b'T', 1, variant]);
    bytes.extend_from_slice(&binding.transcript);
    bytes.extend_from_slice(&binding.attachment);
    bytes.extend_from_slice(&binding.generation.to_be_bytes());
    match record {
        Record::Ordered(sequence, data) => {
            bytes.extend_from_slice(&sequence.to_be_bytes());
            match data {
                Data::Input(data) | Data::Output(data) => bytes.extend_from_slice(data),
                Data::Resize(size) => {
                    bytes.extend_from_slice(&size.rows.to_be_bytes());
                    bytes.extend_from_slice(&size.cols.to_be_bytes());
                }
            }
        }
        Record::Ack(value) | Record::Pulse(value) => bytes.extend_from_slice(&value.to_be_bytes()),
        Record::Close => {}
    }
    let app = MarmotAppEvent::new(
        hex::encode(author),
        created_at,
        KIND,
        vec![],
        STANDARD.encode(bytes),
    )
    .encode()
    .ok()?;
    (app.len() <= MAX_APP).then_some(app)
}

/// Metadata comes from the authenticated native event and from local clock reads.
#[derive(Clone, Copy)]
pub(crate) struct Arrival {
    pub(crate) retention: Option<AppMessageRetentionDecision>,
    pub(crate) wall: Option<u64>,
    pub(crate) monotonic: Option<Duration>,
}

pub(crate) struct Admitted {
    binding: Binding,
    direction: Direction,
    pub(crate) record: Record,
    received: Duration,
    deadline: Duration,
}

pub(crate) fn decode_app(
    bytes: &[u8],
    authenticated_author: &[u8; 32],
    binding: Binding,
    direction: Direction,
    arrival: Arrival,
) -> Option<Admitted> {
    if bytes.len() > MAX_APP {
        return None;
    }
    let app = MarmotAppEvent::decode(bytes).ok()?;
    if app.kind != KIND || !app.tags.is_empty() || app.content.len() > MAX_BASE64 {
        return None;
    }
    app.validate_sender(&hex::encode(authenticated_author))
        .ok()?;
    let (received, deadline) = freshness(app.created_at, arrival)?;
    let raw = STANDARD.decode(&app.content).ok()?;
    if STANDARD.encode(&raw) != app.content {
        return None;
    }
    Some(Admitted {
        binding,
        direction,
        record: decode_record(&raw, binding, direction)?,
        received,
        deadline,
    })
}

fn freshness(created: u64, arrival: Arrival) -> Option<(Duration, Duration)> {
    let wall = arrival.wall?;
    let monotonic = arrival.monotonic?;
    let retention = arrival.retention?;
    let expires = created.checked_add(RETENTION)?;
    if retention.retention_seconds != RETENTION
        || retention.expires_at != Some(expires)
        || wall >= expires
        || created.saturating_sub(wall) > FUTURE_SLACK
    {
        return None;
    }
    let remaining = MAX_AGE.checked_sub(wall.saturating_sub(created))?;
    if remaining == 0 {
        return None;
    }
    Some((
        monotonic,
        monotonic.checked_add(Duration::from_secs(remaining))?,
    ))
}

fn decode_record(raw: &[u8], binding: Binding, direction: Direction) -> Option<Record> {
    if raw.get(..3)? != b"MT\x01"
        || raw.get(4..36)? != binding.transcript
        || raw.get(36..52)? != binding.attachment
        || raw.get(52..HEADER)? != binding.generation.to_be_bytes()
    {
        return None;
    }
    let body = raw.get(HEADER..)?;
    let word = |bytes: &[u8]| -> Option<u64> { Some(u64::from_be_bytes(bytes.try_into().ok()?)) };
    let record = match raw[3] {
        kind @ (1 | 2) if (9..=MAX_DATA + 8).contains(&body.len()) => {
            let bytes = body[8..].to_vec();
            let data = if kind == 1 {
                Data::Input(bytes)
            } else {
                Data::Output(bytes)
            };
            Record::Ordered(word(&body[..8])?, data)
        }
        3 if body.len() == 12 => Record::Ordered(
            word(&body[..8])?,
            Data::Resize(Size {
                rows: u16::from_be_bytes(body[8..10].try_into().ok()?),
                cols: u16::from_be_bytes(body[10..12].try_into().ok()?),
            }),
        ),
        4 => Record::Ack(word(body)?),
        5 => Record::Pulse(word(body)?),
        6 if body.is_empty() => Record::Close,
        _ => return None,
    };
    record.valid(binding, direction).then_some(record)
}
