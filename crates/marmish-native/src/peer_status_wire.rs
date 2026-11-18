//! Bounded application records for private server status in an accepted Peer room.

use base64::{Engine, engine::general_purpose::STANDARD};
use marmish_engine::CommandPolicyDescriptionV1;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, SeqAccess, Visitor},
};
use sha2::{Digest, Sha256};
use std::{fmt, net::IpAddr, num::NonZeroU64};

#[path = "peer_status_state.rs"]
pub(crate) mod state;

const VERSION: u8 = 1;
const HEARTBEAT_LIMIT: usize = 4 * 1024;
const RECORD_LIMIT: usize = 8 * 1024;
const PAGE_BYTES: usize = 5760;
const PAGE_COUNT: usize = 64;
const DESCRIPTION_LIMIT: usize = PAGE_BYTES * PAGE_COUNT;
// Match marmish-engine's validated effective command-policy limits.
const MAX_LAYERS: usize = 129;
const MAX_PATTERNS: usize = 256;
const MAX_PATTERN_BYTES: usize = 1024;
const OVERSIZE_LIMIT: usize = MAX_PATTERNS * (2 * MAX_PATTERN_BYTES + 3) + MAX_LAYERS * 64 + 128;
const DOMAIN: &[u8] = b"marmish/peer/permission-description/v1\0";
pub(crate) const MAX_DISKS: usize = 8;
pub(crate) const MAX_IPS: usize = 8;
pub(crate) const MAX_UPTIME: u64 = 31_557_600_000;
pub(crate) const MAX_LOAD: u32 = 1_000_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WireError {
    Invalid,
    TooLong,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Hex32(pub(crate) [u8; 32]);

impl Serialize for Hex32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Hex32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(de::Error::custom("invalid canonical identifier"));
        }
        let mut bytes = [0; 32];
        hex::decode_to_slice(value, &mut bytes)
            .map_err(|_| de::Error::custom("invalid identifier length"))?;
        Ok(Self(bytes))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Generation {
    pub(crate) ordinal: NonZeroU64,
    pub(crate) digest: Hex32,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(deny_unknown_fields)]
pub(crate) struct Heartbeat {
    pub(crate) sequence: NonZeroU64,
    pub(crate) sampled_at: u64,
    pub(crate) interval: u32,
    pub(crate) host: HostStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) permission: Option<Generation>,
}

#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(deny_unknown_fields)]
pub(crate) struct HostStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) uptime: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) load: Option<[u32; 3]>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "decode_disks"
    )]
    pub(crate) disks: Option<Vec<DiskStatus>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "decode_ips"
    )]
    pub(crate) ips: Option<Vec<IpAddr>>,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(deny_unknown_fields)]
pub(crate) struct DiskStatus {
    pub(crate) label: String,
    pub(crate) used: u64,
    pub(crate) available: u64,
}

fn decode_disks<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<DiskStatus>>, D::Error> {
    Ok(
        Option::<BoundedVec<DiskStatus, MAX_DISKS>>::deserialize(deserializer)?
            .map(|disks| disks.0),
    )
}

fn decode_ips<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Vec<IpAddr>>, D::Error> {
    let Some(addresses) = Option::<BoundedVec<String, MAX_IPS>>::deserialize(deserializer)? else {
        return Ok(None);
    };
    addresses
        .0
        .into_iter()
        .map(|address| {
            let ip = address
                .parse::<IpAddr>()
                .map_err(|_| de::Error::custom("invalid IP address"))?;
            if ip.to_string() != address {
                return Err(de::Error::custom("noncanonical IP address"));
            }
            Ok(ip)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub(crate) fn safe_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(|character| {
        character.is_control() || matches!(character,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    })
}

impl Heartbeat {
    fn valid(&self) -> bool {
        self.sampled_at != 0 && (300..=86_400).contains(&self.interval) && self.host.valid()
    }
}

impl HostStatus {
    pub(crate) fn valid(&self) -> bool {
        self.username
            .as_ref()
            .is_none_or(|name| safe_text(name, 64))
            && self.uptime.is_none_or(|uptime| uptime <= MAX_UPTIME)
            && self
                .load
                .is_none_or(|load| load.into_iter().all(|entry| entry <= MAX_LOAD))
            && self.disks.as_ref().is_none_or(|disks| {
                (1..=MAX_DISKS).contains(&disks.len())
                    && disks.windows(2).all(|pair| pair[0].label < pair[1].label)
                    && disks.iter().all(DiskStatus::valid)
            })
            && self.ips.as_ref().is_none_or(|ips| {
                (1..=MAX_IPS).contains(&ips.len()) && ips.windows(2).all(|pair| pair[0] < pair[1])
            })
    }
}

impl DiskStatus {
    pub(crate) fn valid(&self) -> bool {
        safe_text(&self.label, 32)
            && !self.label.contains('/')
            && self
                .used
                .checked_add(self.available)
                .is_some_and(|total| total != 0)
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    version: u8,
    pub(crate) room: Hex32,
    pub(crate) payload: Payload,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(
    tag = "kind",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Payload {
    Heartbeat(Heartbeat),
    Permission(PermissionRecord),
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(deny_unknown_fields)]
pub(crate) struct PermissionRecord {
    pub(crate) issued_at: u64,
    pub(crate) generation: Generation,
    pub(crate) content: PageContent,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PageContent {
    Chunk {
        page: u8,
        total: u8,
        encoded: PageBytes,
    },
    TooLarge {
        description_len: u32,
    },
}

#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct PageBytes(Vec<u8>);

impl Serialize for PageBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for PageBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() > PAGE_BYTES.div_ceil(3) * 4 {
            return Err(de::Error::custom("oversized page"));
        }
        let bytes = STANDARD
            .decode(&encoded)
            .map_err(|_| de::Error::custom("invalid page encoding"))?;
        if STANDARD.encode(&bytes) != encoded {
            return Err(de::Error::custom("noncanonical page encoding"));
        }
        Ok(Self(bytes))
    }
}

impl Record {
    pub(crate) fn new(room: Hex32, payload: Payload) -> Self {
        Self {
            version: VERSION,
            room,
            payload,
        }
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, WireError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| WireError::Invalid)?;
        self.validate_length(bytes.len())?;
        Ok(bytes)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.len() > RECORD_LIMIT {
            return Err(WireError::TooLong);
        }
        let record: Self = serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)?;
        record.validate_length(bytes.len())?;
        record.validate()?;
        Ok(record)
    }

    fn validate_length(&self, length: usize) -> Result<(), WireError> {
        let cap = match &self.payload {
            Payload::Heartbeat(_) => HEARTBEAT_LIMIT,
            Payload::Permission(_) => RECORD_LIMIT,
        };
        if length > cap {
            Err(WireError::TooLong)
        } else {
            Ok(())
        }
    }

    fn validate(&self) -> Result<(), WireError> {
        let valid_payload = match &self.payload {
            Payload::Heartbeat(heartbeat) => heartbeat.valid(),
            Payload::Permission(permission) => {
                permission.issued_at != 0 && permission.content.valid()
            }
        };
        if self.version == VERSION && valid_payload {
            Ok(())
        } else {
            Err(WireError::Invalid)
        }
    }
}

impl PageContent {
    fn valid(&self) -> bool {
        match self {
            Self::Chunk {
                page,
                total,
                encoded,
            } => {
                (1..=PAGE_COUNT as u8).contains(total)
                    && page < total
                    && (1..=PAGE_BYTES).contains(&encoded.0.len())
                    && (*page + 1 == *total || encoded.0.len() == PAGE_BYTES)
            }
            Self::TooLarge { description_len } => {
                (DESCRIPTION_LIMIT + 1..=OVERSIZE_LIMIT).contains(&(*description_len as usize))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Capability {
    Unsupported,
    Denied,
    Granted,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(deny_unknown_fields)]
pub(crate) struct Description {
    version: u8,
    #[serde(deserialize_with = "decode_layers")]
    layers: Vec<RuleLayer>,
    interactive: Capability,
    shell_command: Capability,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(deny_unknown_fields)]
struct RuleLayer {
    allow_exec: Option<BoundedVec<String, MAX_PATTERNS>>,
    deny_exec: BoundedVec<String, MAX_PATTERNS>,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(Debug))]
#[serde(transparent)]
struct BoundedVec<T, const MAX: usize>(Vec<T>);

impl<'de, T: Deserialize<'de>, const MAX: usize> Deserialize<'de> for BoundedVec<T, MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Sequence<T, const MAX: usize>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>, const MAX: usize> Visitor<'de> for Sequence<T, MAX> {
            type Value = BoundedVec<T, MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "at most {MAX} elements")
            }

            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut values = Vec::new();
                while values.len() < MAX {
                    match seq.next_element()? {
                        Some(value) => values.push(value),
                        None => return Ok(BoundedVec(values)),
                    }
                }
                if seq.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::custom("oversized sequence"));
                }
                Ok(BoundedVec(values))
            }
        }

        deserializer.deserialize_seq(Sequence(std::marker::PhantomData))
    }
}

fn decode_layers<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<RuleLayer>, D::Error> {
    struct Layers;

    impl<'de> Visitor<'de> for Layers {
        type Value = Vec<RuleLayer>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("bounded effective layers")
        }

        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
            let mut layers = Vec::new();
            let mut patterns = 0;
            while layers.len() < MAX_LAYERS {
                let Some(layer) = seq.next_element::<RuleLayer>()? else {
                    return Ok(layers);
                };
                patterns += layer.patterns().count();
                if patterns > MAX_PATTERNS {
                    return Err(de::Error::custom("too many aggregate patterns"));
                }
                layers.push(layer);
            }
            if seq.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("too many layers"));
            }
            Ok(layers)
        }
    }

    deserializer.deserialize_seq(Layers)
}

impl RuleLayer {
    fn patterns(&self) -> impl Iterator<Item = &String> {
        self.allow_exec
            .iter()
            .flat_map(|allow| &allow.0)
            .chain(&self.deny_exec.0)
    }
}

impl Description {
    pub(crate) fn from_engine(
        policy: CommandPolicyDescriptionV1,
        interactive: Capability,
        shell_command: Capability,
    ) -> Result<Self, WireError> {
        if policy.version != u32::from(VERSION) {
            return Err(WireError::Invalid);
        }
        let description = Self {
            version: VERSION,
            interactive,
            shell_command,
            layers: policy
                .layers
                .into_iter()
                .map(|layer| RuleLayer {
                    allow_exec: layer.allow_exec.map(BoundedVec),
                    deny_exec: BoundedVec(layer.deny_exec),
                })
                .collect(),
        };
        description.validate()?;
        Ok(description)
    }

    fn validate(&self) -> Result<(), WireError> {
        let valid_layers = self.layers.len() <= MAX_LAYERS
            && self
                .layers
                .iter()
                .all(|layer| layer.allow_exec.is_some() || !layer.deny_exec.0.is_empty());
        let patterns = || self.layers.iter().flat_map(RuleLayer::patterns);
        if self.version != VERSION
            || !valid_layers
            || patterns().count() > MAX_PATTERNS
            || !patterns().all(|pattern| valid_pattern(pattern))
        {
            return Err(WireError::Invalid);
        }
        Ok(())
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, WireError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|_| WireError::Invalid)
    }

    fn decode_canonical(bytes: &[u8], expected: Hex32) -> Result<Self, WireError> {
        if description_digest(bytes) != expected {
            return Err(WireError::Invalid);
        }
        let description: Self = serde_json::from_slice(bytes).map_err(|_| WireError::Invalid)?;
        if description.canonical_bytes()? != bytes {
            return Err(WireError::Invalid);
        }
        Ok(description)
    }
}

fn valid_pattern(pattern: &str) -> bool {
    if pattern.len() > MAX_PATTERN_BYTES
        || !pattern.starts_with('/')
        || pattern.chars().any(char::is_control)
    {
        return false;
    }
    let mut bytes = pattern.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' && !matches!(bytes.next(), Some(b'*' | b'\\')) {
            return false;
        }
    }
    true
}

fn description_digest(bytes: &[u8]) -> Hex32 {
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(bytes);
    Hex32(hash.finalize().into())
}

pub(crate) struct PermissionPlan {
    pub(crate) generation: Generation,
    data: PlanData,
}

enum PlanData {
    Pages(Vec<u8>),
    TooLarge(u32),
}

impl PermissionPlan {
    pub(crate) fn new(ordinal: NonZeroU64, description: &Description) -> Result<Self, WireError> {
        let bytes = description.canonical_bytes()?;
        let generation = Generation {
            ordinal,
            digest: description_digest(&bytes),
        };
        let data = if bytes.len() <= DESCRIPTION_LIMIT {
            PlanData::Pages(bytes)
        } else {
            PlanData::TooLarge(u32::try_from(bytes.len()).map_err(|_| WireError::TooLong)?)
        };
        Ok(Self { generation, data })
    }

    pub(crate) fn records(
        &self,
        room: Hex32,
        issued_at: u64,
    ) -> impl Iterator<Item = Result<Vec<u8>, WireError>> + '_ {
        let contents: Box<dyn Iterator<Item = PageContent>> = match &self.data {
            PlanData::Pages(bytes) => {
                let total = bytes.len().div_ceil(PAGE_BYTES) as u8;
                Box::new(
                    bytes
                        .chunks(PAGE_BYTES)
                        .enumerate()
                        .map(move |(page, bytes)| PageContent::Chunk {
                            page: page as u8,
                            total,
                            encoded: PageBytes(bytes.to_vec()),
                        }),
                )
            }
            PlanData::TooLarge(description_len) => {
                Box::new(std::iter::once(PageContent::TooLarge {
                    description_len: *description_len,
                }))
            }
        };
        contents.map(move |content| {
            Record::new(
                room,
                Payload::Permission(PermissionRecord {
                    issued_at,
                    generation: self.generation,
                    content,
                }),
            )
            .encode()
        })
    }
}

#[cfg(test)]
#[path = "peer_status_wire_tests.rs"]
mod tests;
