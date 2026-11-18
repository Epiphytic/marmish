//! Strict framing, index fields and resource bounds for the raw socket proof.

use std::{borrow::Cow, fmt};

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

use super::peer_control_wire::MAX_OUTER_BYTES;

pub(super) const CARRIER: u16 = 7435;
pub(super) const MESSAGE_LIMIT: usize = MAX_OUTER_BYTES + 256;
pub(super) const RECORD_LIMIT: usize = 4;
const RESERVE_BYTES: usize = RECORD_LIMIT * MAX_OUTER_BYTES;
const FRAME_LIMIT: u8 = 24;
const CONTROL_LIMIT: u8 = 8;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum Failure {
    Binding,
    Size,
    Grammar,
    Capacity,
    Deadline,
    Transport,
    Retired,
    Ambiguous,
}

pub(super) type Result<T> = std::result::Result<T, Failure>;

pub(super) enum Packet<'a> {
    Challenge(String),
    Event(String, &'a RawValue),
    End(String),
    Acknowledged([u8; 32], bool),
}

pub(super) fn packet(text: &str) -> Result<Packet<'_>> {
    if text.len() > MESSAGE_LIMIT {
        return Err(Failure::Size);
    }
    let Array(fields) = serde_json::from_str(text).map_err(|_| Failure::Grammar)?;
    let [verb, one, two, three] = fields;
    let verb: &str = decode(verb.ok_or(Failure::Grammar)?)?;
    match (verb, one, two, three) {
        ("AUTH", Some(challenge), None, None) => {
            let challenge: String = decode(challenge)?;
            if !(1..=256).contains(&challenge.len())
                || challenge.bytes().any(|byte| byte.is_ascii_control())
            {
                return Err(Failure::Grammar);
            }
            Ok(Packet::Challenge(challenge))
        }
        ("EOSE", Some(id), None, None) => Ok(Packet::End(decode(id)?)),
        ("EVENT", Some(id), Some(body), None) => Ok(Packet::Event(decode(id)?, body)),
        ("OK", Some(id), Some(positive), Some(detail)) => {
            let id: &str = decode(id)?;
            let _: String = decode(detail)?;
            Ok(Packet::Acknowledged(fixed_hex(id)?, decode(positive)?))
        }
        _ => Err(Failure::Grammar),
    }
}

fn decode<'a, T: Deserialize<'a>>(raw: &'a RawValue) -> Result<T> {
    serde_json::from_str(raw.get()).map_err(|_| Failure::Grammar)
}

struct Array<'a>([Option<&'a RawValue>; 4]);

impl<'de> Deserialize<'de> for Array<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct ArrayReader;
        impl<'de> Visitor<'de> for ArrayReader {
            type Value = Array<'de>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded relay array")
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut slots = [None; 4];
                for slot in &mut slots {
                    *slot = sequence.next_element()?;
                    if slot.is_none() {
                        return Ok(Array(slots));
                    }
                }
                if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::invalid_length(5, &self));
                }
                Ok(Array(slots))
            }
        }
        deserializer.deserialize_seq(ArrayReader)
    }
}

fn fixed_hex<const N: usize>(text: &str) -> Result<[u8; N]> {
    let mut bytes = [0; N];
    if text.len() != N * 2
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(Failure::Grammar);
    }
    hex::decode_to_slice(text, &mut bytes).map_err(|_| Failure::Grammar)?;
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields<'a> {
    #[serde(borrow)]
    id: Cow<'a, str>,
    #[serde(borrow)]
    pubkey: Cow<'a, str>,
    created_at: u64,
    kind: u16,
    tags: [[String; 2]; 2],
    #[serde(rename = "content", borrow)]
    _content: Cow<'a, str>,
    #[serde(borrow)]
    sig: Cow<'a, str>,
}

#[derive(Debug)]
pub(super) struct Arrival {
    pub(super) id: [u8; 32],
    pub(super) stamp: u64,
    pub(super) body: Box<RawValue>,
}

impl Arrival {
    pub(super) fn parse(body: &RawValue, recipient: &[u8; 32]) -> Result<Self> {
        let (id, stamp) = Self::index(body, recipient)?;
        Ok(Self {
            id,
            stamp,
            body: body.to_owned(),
        })
    }

    pub(super) fn index(body: &RawValue, recipient: &[u8; 32]) -> Result<([u8; 32], u64)> {
        if body.get().len() > MAX_OUTER_BYTES {
            return Err(Failure::Size);
        }
        let fields: Fields<'_> = decode(body)?;
        let [[name, addressed], [expiration, until]] = &fields.tags;
        let canonical_expiry = until.parse::<u64>().is_ok_and(|value| {
            value >= 21_600 && value % 21_600 == 0 && value.to_string() == *until
        });
        if fields.kind != CARRIER
            || name != "p"
            || fixed_hex::<32>(addressed)? != *recipient
            || fixed_hex::<32>(&fields.pubkey)? == *recipient
            || expiration != "expiration"
            || !canonical_expiry
        {
            return Err(Failure::Grammar);
        }
        fixed_hex::<64>(&fields.sig)?;
        Ok((fixed_hex(&fields.id)?, fields.created_at))
    }
}

#[derive(Default)]
pub(super) struct Reserve {
    records: usize,
    bytes: usize,
}

impl Reserve {
    pub(super) fn charge(&mut self, size: usize) -> Result<()> {
        if size > MAX_OUTER_BYTES {
            return Err(Failure::Size);
        }
        if self.records == RECORD_LIMIT || self.bytes + size > RESERVE_BYTES {
            return Err(Failure::Capacity);
        }
        self.records += 1;
        self.bytes += size;
        Ok(())
    }

    pub(super) fn release(&mut self, size: usize) {
        self.records -= 1;
        self.bytes -= size;
    }
}

#[derive(Default)]
pub(super) struct Budget {
    total: u8,
    control: u8,
}

impl Budget {
    pub(super) fn admit(&mut self, control: bool) -> Result<()> {
        if self.total >= FRAME_LIMIT || control && self.control >= CONTROL_LIMIT {
            return Err(Failure::Capacity);
        }
        self.total += 1;
        self.control += u8::from(control);
        Ok(())
    }
}
