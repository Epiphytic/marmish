//! Closed, bounded regular-file copy records. Identity fields are claims: callers
//! must compare them to the authenticated stream before filesystem access.
use crate::{Error, MAX_BLOCK, MAX_OFFSET, Result, codec::Codec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const VERSION: u32 = 1;
pub const MAX_PATH: usize = 4096;
pub const MAX_CONTROL: usize = 16 * 1024;
pub const DURABLE_WINDOW: u64 = 2 * 1024 * 1024;
pub const CHECKPOINT_INTERVAL: u64 = 1024 * 1024;
pub const MIN_RETENTION_SECS: i64 = 7 * 24 * 60 * 60 + 60;
/// Same lifecycle ceiling as the project retention configuration.
pub const MAX_RETENTION_SECS: u64 = 365 * 86_400;

macro_rules! numbered_enum {
    ($name:ident { $($variant:ident = $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        #[repr(u8)]
        pub enum $name { $($variant = $value),+ }
        impl TryFrom<u8> for $name {
            type Error = Error;
            fn try_from(value: u8) -> Result<Self> {
                match value { $($value => Ok(Self::$variant),)+ _ => Err(Error::Invalid("copy_enum")) }
            }
        }
    };
}
numbered_enum!(AttemptMode { Create = 1, Resume = 2 });
numbered_enum!(Direction { Put = 1, Get = 2 });
numbered_enum!(Phase { Created = 1, Receiving = 2, Verifying = 3, CommitPrepared = 4, Committed = 5 });
numbered_enum!(Stage { Hashing = 1, PrefixHash = 2, Verifying = 3, Syncing = 4 });
numbered_enum!(CopyError {
    Unsupported = 1, Protocol = 2, PermissionDenied = 3, NotFound = 4,
    UnsafePath = 5, Busy = 6, Conflict = 7, SourceChanged = 8,
    HashMismatch = 9, DiskFull = 10, Quota = 11, CommitUncertain = 12,
    LocalIo = 13, IdentityChanged = 14
});

pub const CONTEXT_VERSION: u32 = 1;
pub const MAX_CONTEXT: usize = 4096;
pub const HELPER_ENTRYPOINT: &str = "__copy-helper-v1";
const CONTEXT_DOMAIN: &[u8] = b"marmish-copy-context/1\0";

/// Server-produced consistency context. This is deliberately not a capability:
/// same-UID processes can forge pipes; authenticated admission remains upstream.
#[derive(Clone, PartialEq, Eq)]
pub struct HelperContext {
    pub context_version: u32,
    pub helper_protocol: u32,
    pub client: [u8; 32],
    pub server: [u8; 32],
    pub group: Vec<u8>,
    pub session: [u8; 16],
    pub stream: [u8; 16],
    pub transfer: [u8; 32],
    pub direction: Direction,
    pub launch_digest: [u8; 32],
    pub retention_seconds: u64,
    pub expected_version: String,
    pub expected_target: String,
    pub expected_commit: [u8; 20],
}
impl HelperContext {
    pub fn validate(&self) -> Result<()> {
        if self.context_version != CONTEXT_VERSION
            || self.helper_protocol != VERSION
            || self.group.is_empty()
            || self.group.len() > 1024
            || !(MIN_RETENTION_SECS as u64..=MAX_RETENTION_SECS).contains(&self.retention_seconds)
            || !build_atom(&self.expected_version)
            || !build_atom(&self.expected_target)
        {
            return Err(Error::Invalid("copy_context"));
        }
        Ok(())
    }
    pub fn validate_start(&self, launch: &Launch, codec: Codec) -> Result<()> {
        self.validate()?;
        if launch.client != self.client
            || launch.server != self.server
            || launch.transfer != self.transfer
            || launch.direction != self.direction
            || launch.digest(codec)? != self.launch_digest
        {
            return Err(Error::Invalid("copy_context_launch"));
        }
        Ok(())
    }
    pub fn validate_build(&self, version: &str, target: &str, commit: [u8; 20]) -> Result<()> {
        self.validate()?;
        if self.expected_version != version
            || self.expected_target != target
            || self.expected_commit != commit
        {
            return Err(Error::Invalid("copy_context_build"));
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut bytes = CONTEXT_DOMAIN.to_vec();
        put_u32(&mut bytes, self.context_version);
        put_u32(&mut bytes, self.helper_protocol);
        bytes.extend(self.client);
        bytes.extend(self.server);
        put_bytes(&mut bytes, &self.group);
        bytes.extend(self.session);
        bytes.extend(self.stream);
        bytes.extend(self.transfer);
        bytes.push(self.direction as u8);
        bytes.extend(self.launch_digest);
        put_u64(&mut bytes, self.retention_seconds);
        put_bytes(&mut bytes, self.expected_version.as_bytes());
        put_bytes(&mut bytes, self.expected_target.as_bytes());
        bytes.extend(self.expected_commit);
        put_bytes(&mut bytes, HELPER_ENTRYPOINT.as_bytes());
        if bytes.len() > MAX_CONTEXT {
            return Err(Error::Limit);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_CONTEXT || !bytes.starts_with(CONTEXT_DOMAIN) {
            return Err(Error::Invalid("copy_context"));
        }
        let mut r = Reader(&bytes[CONTEXT_DOMAIN.len()..]);
        let context = Self {
            context_version: r.u32()?,
            helper_protocol: r.u32()?,
            client: r.array()?,
            server: r.array()?,
            group: r.bytes(1024)?.to_vec(),
            session: r.array()?,
            stream: r.array()?,
            transfer: r.array()?,
            direction: r.u8()?.try_into()?,
            launch_digest: r.array()?,
            retention_seconds: r.u64()?,
            expected_version: read_build_atom(&mut r)?,
            expected_target: read_build_atom(&mut r)?,
            expected_commit: r.array()?,
        };
        if r.bytes(64)? != HELPER_ENTRYPOINT.as_bytes() {
            return Err(Error::Invalid("copy_context_entrypoint"));
        }
        r.finish()?;
        context.validate()?;
        Ok(context)
    }
}
fn build_atom(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
}
fn read_build_atom(r: &mut Reader<'_>) -> Result<String> {
    let value =
        std::str::from_utf8(r.bytes(64)?).map_err(|_| Error::Invalid("copy_context_build"))?;
    if !build_atom(value) {
        return Err(Error::Invalid("copy_context_build"));
    }
    Ok(value.to_owned())
}

// No Debug implementations: operands and data can contain private information.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub version: u32,
    pub mode: AttemptMode,
    #[serde(with = "crate::wire::hex32")]
    pub transfer: [u8; 32],
    #[serde(with = "crate::wire::hex32")]
    pub client: [u8; 32],
    #[serde(with = "crate::wire::hex32")]
    pub server: [u8; 32],
    pub direction: Direction,
    #[serde(with = "path_encoding")]
    pub source: Vec<u8>,
    #[serde(with = "path_encoding")]
    pub destination: Vec<u8>,
    pub overwrite: bool,
    pub preserve_mode: bool,
    pub preserve_mtime: bool,
}

impl Launch {
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return Err(Error::Invalid("copy_version"));
        }
        validate_path(&self.source)?;
        validate_path(&self.destination)
    }

    pub fn digest(&self, codec: Codec) -> Result<[u8; 32]> {
        self.validate()?;
        let mut bytes = Vec::new();
        encode_launch(&mut bytes, self);
        bytes.push(codec_number(codec));
        let mut hash = Sha256::new();
        hash.update(b"marmish-copy-launch/1\0");
        hash.update(bytes);
        Ok(hash.finalize().into())
    }

    pub fn namespace(&self) -> String {
        namespace(self.client, self.server, self.transfer, self.direction)
    }

    /// Mode is per attempt; all remaining launch fields bind immutable content.
    pub fn matches_manifest(&self, manifest: &Manifest) -> bool {
        self.version == manifest.version
            && self.transfer == manifest.transfer
            && self.client == manifest.client
            && self.server == manifest.server
            && self.direction == manifest.direction
            && self.source == manifest.source
            && self.destination == manifest.destination
            && self.overwrite == manifest.overwrite
            && self.preserve_mode == manifest.preserve_mode
            && self.preserve_mtime == manifest.preserve_mtime
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mtime {
    pub seconds: i64,
    pub nanos: u32,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: u32,
    pub transfer: [u8; 32],
    pub client: [u8; 32],
    pub server: [u8; 32],
    pub direction: Direction,
    pub source: Vec<u8>,
    pub destination: Vec<u8>,
    pub overwrite: bool,
    pub preserve_mode: bool,
    pub preserve_mtime: bool,
    pub size: u64,
    pub sha256: [u8; 32],
    pub mode: Option<u32>,
    pub mtime: Option<Mtime>,
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return Err(Error::Invalid("copy_version"));
        }
        validate_path(&self.source)?;
        validate_path(&self.destination)?;
        offset(self.size)?;
        if self.preserve_mode != self.mode.is_some() || self.preserve_mtime != self.mtime.is_some()
        {
            return Err(Error::Invalid("copy_metadata"));
        }
        if self
            .mode
            .is_some_and(|m| m & !0o777 != 0 || m & 0o600 != 0o600)
            || self.mtime.is_some_and(|m| m.nanos >= 1_000_000_000)
        {
            return Err(Error::Invalid("copy_metadata"));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut bytes = Vec::new();
        encode_manifest(&mut bytes, self);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_CONTROL {
            return Err(Error::Limit);
        }
        let mut reader = Reader(bytes);
        let manifest = read_manifest(&mut reader)?;
        reader.finish()?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Account and direction separation, not an authorization capability.
    pub fn namespace(&self) -> String {
        namespace(self.client, self.server, self.transfer, self.direction)
    }
}

fn namespace(
    client: [u8; 32],
    server: [u8; 32],
    transfer: [u8; 32],
    direction: Direction,
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"marmish-copy-state/1\0");
    hash.update(client);
    hash.update(server);
    hash.update(transfer);
    hash.update([direction as u8]);
    let mut output = String::from(".marmish-copy-v1-");
    for byte in hash.finalize() {
        use std::fmt::Write;
        write!(output, "{byte:02x}").expect("write to String");
    }
    output
}

#[derive(Clone, PartialEq, Eq)]
pub enum Frame {
    Start(Launch, Codec),
    Manifest(Manifest),
    Resume {
        phase: Phase,
        offset: u64,
        sha256: [u8; 32],
    },
    Prefix {
        offset: u64,
        sha256: [u8; 32],
    },
    Data {
        offset: u64,
        bytes: Vec<u8>,
    },
    Checkpoint {
        offset: u64,
        sha256: [u8; 32],
    },
    Done {
        size: u64,
        sha256: [u8; 32],
    },
    Commit {
        transfer: [u8; 32],
        size: u64,
        sha256: [u8; 32],
    },
    Finish {
        transfer: [u8; 32],
        sha256: [u8; 32],
    },
    Progress {
        stage: Stage,
        examined: u64,
        total: u64,
    },
    Error(CopyError),
}

impl Frame {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        let tag = match self {
            Self::Start(launch, codec) => {
                launch.validate()?;
                encode_launch(&mut payload, launch);
                payload.push(codec_number(*codec));
                1
            }
            Self::Manifest(manifest) => {
                payload = manifest.encode()?;
                2
            }
            Self::Resume {
                phase,
                offset: n,
                sha256,
            } => {
                offset(*n)?;
                payload.push(*phase as u8);
                put_offset_hash(&mut payload, *n, sha256);
                3
            }
            Self::Prefix { offset: n, sha256 } => {
                offset(*n)?;
                put_offset_hash(&mut payload, *n, sha256);
                4
            }
            Self::Data { offset: n, bytes } => {
                if bytes.is_empty() || bytes.len() > MAX_BLOCK {
                    return Err(Error::Limit);
                }
                crate::end(*n, bytes.len())?;
                put_u64(&mut payload, *n);
                payload.extend(bytes);
                5
            }
            Self::Checkpoint { offset: n, sha256 } => {
                offset(*n)?;
                put_offset_hash(&mut payload, *n, sha256);
                6
            }
            Self::Done { size, sha256 } => {
                offset(*size)?;
                put_offset_hash(&mut payload, *size, sha256);
                7
            }
            Self::Commit {
                transfer,
                size,
                sha256,
            } => {
                offset(*size)?;
                payload.extend(transfer);
                put_offset_hash(&mut payload, *size, sha256);
                payload.push(1);
                8
            }
            Self::Finish { transfer, sha256 } => {
                payload.extend(transfer);
                payload.extend(sha256);
                9
            }
            Self::Progress {
                stage,
                examined,
                total,
            } => {
                offset(*total)?;
                if examined > total {
                    return Err(Error::Offset);
                }
                payload.push(*stage as u8);
                put_u64(&mut payload, *examined);
                put_u64(&mut payload, *total);
                10
            }
            Self::Error(code) => {
                payload.push(*code as u8);
                payload.push(0);
                11
            }
        };
        if payload.len() > MAX_CONTROL {
            return Err(Error::Limit);
        }
        let mut bytes = Vec::with_capacity(payload.len() + 5);
        bytes.push(tag);
        bytes.extend((payload.len() as u32).to_be_bytes());
        bytes.extend(payload);
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 5 {
            return Err(Error::Invalid("copy_frame"));
        }
        let length = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
        if length > MAX_CONTROL {
            return Err(Error::Limit);
        }
        if bytes.len() != length + 5 {
            return Err(Error::Invalid("copy_frame_length"));
        }
        let mut r = Reader(&bytes[5..]);
        let frame = match bytes[0] {
            1 => {
                let launch = read_launch(&mut r)?;
                launch.validate()?;
                Self::Start(launch, read_codec(r.u8()?)?)
            }
            2 => {
                let manifest = read_manifest(&mut r)?;
                manifest.validate()?;
                Self::Manifest(manifest)
            }
            3 => Self::Resume {
                phase: r.u8()?.try_into()?,
                offset: r.offset()?,
                sha256: r.array()?,
            },
            4 => Self::Prefix {
                offset: r.offset()?,
                sha256: r.array()?,
            },
            5 => {
                let offset = r.offset()?;
                if r.0.is_empty() || r.0.len() > MAX_BLOCK {
                    return Err(Error::Limit);
                }
                crate::end(offset, r.0.len())?;
                let bytes = r.0.to_vec();
                r.0 = &[];
                Self::Data { offset, bytes }
            }
            6 => Self::Checkpoint {
                offset: r.offset()?,
                sha256: r.array()?,
            },
            7 => Self::Done {
                size: r.offset()?,
                sha256: r.array()?,
            },
            8 => {
                let transfer = r.array()?;
                let size = r.offset()?;
                let sha256 = r.array()?;
                if r.u8()? != 1 {
                    return Err(Error::Invalid("copy_disposition"));
                }
                Self::Commit {
                    transfer,
                    size,
                    sha256,
                }
            }
            9 => Self::Finish {
                transfer: r.array()?,
                sha256: r.array()?,
            },
            10 => {
                let stage = r.u8()?.try_into()?;
                let examined = r.offset()?;
                let total = r.offset()?;
                if examined > total {
                    return Err(Error::Offset);
                }
                Self::Progress {
                    stage,
                    examined,
                    total,
                }
            }
            11 => {
                let code = r.u8()?.try_into()?;
                if r.u8()? != 0 {
                    return Err(Error::Invalid("copy_error_owner"));
                }
                Self::Error(code)
            }
            _ => return Err(Error::Invalid("copy_frame_tag")),
        };
        r.finish()?;
        Ok(frame)
    }
}

/// At most one incomplete frame retained. Caller handles a completed frame before
/// feeding further input; no unbounded decoded-frame queue can accumulate.
#[derive(Default)]
pub struct FrameDecoder {
    pending: Vec<u8>,
    expected: Option<usize>,
    failed: bool,
}
impl FrameDecoder {
    pub fn feed(&mut self, input: &[u8]) -> Result<(usize, Option<Frame>)> {
        if self.failed {
            return Err(Error::Invalid("copy_decoder_failed"));
        }
        let result = self.feed_inner(input);
        if result.is_err() {
            self.failed = true;
            self.pending.clear();
        }
        result
    }
    fn feed_inner(&mut self, input: &[u8]) -> Result<(usize, Option<Frame>)> {
        let mut consumed = 0;
        if self.pending.len() < 5 {
            let n = (5 - self.pending.len()).min(input.len());
            self.pending.extend_from_slice(&input[..n]);
            consumed += n;
            if self.pending.len() < 5 {
                return Ok((consumed, None));
            }
            let length = u32::from_be_bytes(self.pending[1..5].try_into().unwrap()) as usize;
            if length > MAX_CONTROL {
                return Err(Error::Limit);
            }
            self.expected = Some(length + 5);
        }
        let expected = self.expected.ok_or(Error::Invalid("copy_decoder"))?;
        let n = (expected - self.pending.len()).min(input.len() - consumed);
        self.pending
            .extend_from_slice(&input[consumed..consumed + n]);
        consumed += n;
        if self.pending.len() != expected {
            return Ok((consumed, None));
        }
        let frame = Frame::decode(&self.pending)?;
        self.pending.clear();
        self.expected = None;
        Ok((consumed, Some(frame)))
    }
    pub fn finish(&self) -> Result<()> {
        if self.failed || !self.pending.is_empty() {
            Err(Error::Invalid("copy_truncated"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub manifest: Manifest,
    pub phase: Phase,
    pub normalized_path: Vec<u8>,
    pub parent: FileIdentity,
    pub private: FileIdentity,
    pub data: FileIdentity,
    pub durable_offset: u64,
    pub prefix_sha256: [u8; 32],
    pub last_stream: [u8; 16],
    pub created: i64,
    pub updated: i64,
    pub expires: i64,
    pub committed_final: Option<FileIdentity>,
}
impl Checkpoint {
    pub fn validate(&self) -> Result<()> {
        self.manifest.validate()?;
        if self.normalized_path.is_empty()
            || self.normalized_path.len() > MAX_PATH + 8
            || self.normalized_path.contains(&0)
        {
            return Err(Error::Invalid("copy_local_path"));
        }
        if self.durable_offset > self.manifest.size
            || self.updated < self.created
            || self
                .expires
                .checked_sub(self.updated)
                .is_none_or(|age| age < MIN_RETENTION_SECS)
            || (self.phase == Phase::Created && self.durable_offset != 0)
            || (matches!(
                self.phase,
                Phase::Verifying | Phase::CommitPrepared | Phase::Committed
            ) && self.durable_offset != self.manifest.size)
            || (self.phase == Phase::Committed) != self.committed_final.is_some()
            || self.committed_final.is_some_and(|id| id != self.data)
            || (self.durable_offset == 0 && self.prefix_sha256 != Sha256::digest([]).as_slice())
            || (self.durable_offset == self.manifest.size
                && self.prefix_sha256 != self.manifest.sha256)
        {
            return Err(Error::Invalid("copy_checkpoint"));
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut bytes = Vec::new();
        put_u32(&mut bytes, VERSION);
        put_bytes(&mut bytes, &self.manifest.encode()?);
        bytes.push(self.phase as u8);
        put_bytes(&mut bytes, &self.normalized_path);
        for id in [self.parent, self.private, self.data] {
            put_identity(&mut bytes, id);
        }
        put_u64(&mut bytes, self.durable_offset);
        bytes.extend(self.prefix_sha256);
        bytes.extend(self.last_stream);
        for n in [self.created, self.updated, self.expires] {
            bytes.extend(n.to_be_bytes());
        }
        put_option(&mut bytes, self.committed_final, put_identity);
        let mut hash = Sha256::new();
        hash.update(b"marmish-copy-checkpoint/1\0");
        hash.update(&bytes);
        bytes.extend(hash.finalize());
        if bytes.len() > MAX_CONTROL {
            return Err(Error::Limit);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_CONTROL || bytes.len() < 32 {
            return Err(Error::Limit);
        }
        let (payload, checksum) = bytes.split_at(bytes.len() - 32);
        let mut hash = Sha256::new();
        hash.update(b"marmish-copy-checkpoint/1\0");
        hash.update(payload);
        if hash.finalize().as_slice() != checksum {
            return Err(Error::Invalid("copy_checkpoint_checksum"));
        }
        let mut r = Reader(payload);
        if r.u32()? != VERSION {
            return Err(Error::Invalid("copy_checkpoint_version"));
        }
        let manifest = Manifest::decode(r.bytes(MAX_CONTROL)?)?;
        let checkpoint = Self {
            manifest,
            phase: r.u8()?.try_into()?,
            normalized_path: r.bytes(MAX_PATH + 8)?.to_vec(),
            parent: r.identity()?,
            private: r.identity()?,
            data: r.identity()?,
            durable_offset: r.offset()?,
            prefix_sha256: r.array()?,
            last_stream: r.array()?,
            created: r.i64()?,
            updated: r.i64()?,
            expires: r.i64()?,
            committed_final: r.option(|r| r.identity())?,
        };
        r.finish()?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }
}

fn validate_path(path: &[u8]) -> Result<()> {
    if path.is_empty() || path.len() > MAX_PATH || path.contains(&0) {
        Err(Error::Invalid("copy_path"))
    } else {
        Ok(())
    }
}
fn offset(value: u64) -> Result<()> {
    if value > MAX_OFFSET {
        Err(Error::Offset)
    } else {
        Ok(())
    }
}
fn codec_number(codec: Codec) -> u8 {
    match codec {
        Codec::None => 0,
        Codec::Zlib => 1,
    }
}
fn read_codec(n: u8) -> Result<Codec> {
    match n {
        0 => Ok(Codec::None),
        1 => Ok(Codec::Zlib),
        _ => Err(Error::Invalid("copy_codec")),
    }
}
fn put_u32(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_be_bytes());
}
fn put_u64(out: &mut Vec<u8>, n: u64) {
    out.extend(n.to_be_bytes());
}
fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len() as u32);
    out.extend(bytes);
}
fn put_option<T>(out: &mut Vec<u8>, value: Option<T>, f: impl FnOnce(&mut Vec<u8>, T)) {
    match value {
        Some(value) => {
            out.push(1);
            f(out, value);
        }
        None => out.push(0),
    }
}
fn put_identity(out: &mut Vec<u8>, id: FileIdentity) {
    put_u64(out, id.device);
    put_u64(out, id.inode);
}
fn put_offset_hash(out: &mut Vec<u8>, offset: u64, hash: &[u8; 32]) {
    put_u64(out, offset);
    out.extend(hash);
}
fn encode_launch(out: &mut Vec<u8>, v: &Launch) {
    put_u32(out, v.version);
    out.push(v.mode as u8);
    out.extend(v.transfer);
    out.extend(v.client);
    out.extend(v.server);
    out.push(v.direction as u8);
    put_bytes(out, &v.source);
    put_bytes(out, &v.destination);
    out.extend([
        u8::from(v.overwrite),
        u8::from(v.preserve_mode),
        u8::from(v.preserve_mtime),
    ]);
}
fn encode_manifest(out: &mut Vec<u8>, v: &Manifest) {
    put_u32(out, v.version);
    out.extend(v.transfer);
    out.extend(v.client);
    out.extend(v.server);
    out.push(v.direction as u8);
    put_bytes(out, &v.source);
    put_bytes(out, &v.destination);
    out.extend([
        u8::from(v.overwrite),
        u8::from(v.preserve_mode),
        u8::from(v.preserve_mtime),
    ]);
    put_u64(out, v.size);
    out.extend(v.sha256);
    put_option(out, v.mode, put_u32);
    put_option(out, v.mtime, |out, time| {
        out.extend(time.seconds.to_be_bytes());
        put_u32(out, time.nanos);
    });
}
fn read_launch(r: &mut Reader<'_>) -> Result<Launch> {
    Ok(Launch {
        version: r.u32()?,
        mode: r.u8()?.try_into()?,
        transfer: r.array()?,
        client: r.array()?,
        server: r.array()?,
        direction: r.u8()?.try_into()?,
        source: r.bytes(MAX_PATH)?.to_vec(),
        destination: r.bytes(MAX_PATH)?.to_vec(),
        overwrite: r.boolean()?,
        preserve_mode: r.boolean()?,
        preserve_mtime: r.boolean()?,
    })
}
fn read_manifest(r: &mut Reader<'_>) -> Result<Manifest> {
    Ok(Manifest {
        version: r.u32()?,
        transfer: r.array()?,
        client: r.array()?,
        server: r.array()?,
        direction: r.u8()?.try_into()?,
        source: r.bytes(MAX_PATH)?.to_vec(),
        destination: r.bytes(MAX_PATH)?.to_vec(),
        overwrite: r.boolean()?,
        preserve_mode: r.boolean()?,
        preserve_mtime: r.boolean()?,
        size: r.offset()?,
        sha256: r.array()?,
        mode: r.option(|r| r.u32())?,
        mtime: r.option(|r| {
            Ok(Mtime {
                seconds: r.i64()?,
                nanos: r.u32()?,
            })
        })?,
    })
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.0.len() {
            return Err(Error::Invalid("copy_truncated"));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.array()?))
    }
    fn offset(&mut self) -> Result<u64> {
        let n = self.u64()?;
        offset(n)?;
        Ok(n)
    }
    fn boolean(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Invalid("copy_boolean")),
        }
    }
    fn bytes(&mut self, max: usize) -> Result<&'a [u8]> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(Error::Limit);
        }
        self.take(n)
    }
    fn identity(&mut self) -> Result<FileIdentity> {
        Ok(FileIdentity {
            device: self.u64()?,
            inode: self.u64()?,
        })
    }
    fn option<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<Option<T>> {
        match self.u8()? {
            0 => Ok(None),
            1 => f(self).map(Some),
            _ => Err(Error::Invalid("copy_option")),
        }
    }
    fn finish(&self) -> Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid("copy_trailing"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn launch() -> Launch {
        Launch {
            version: VERSION,
            mode: AttemptMode::Create,
            transfer: [1; 32],
            client: [2; 32],
            server: [3; 32],
            direction: Direction::Put,
            source: b"-file\xff \n'$*".to_vec(),
            destination: b"out".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
        }
    }
    fn manifest() -> Manifest {
        let l = launch();
        Manifest {
            version: l.version,
            transfer: l.transfer,
            client: l.client,
            server: l.server,
            direction: l.direction,
            source: l.source,
            destination: l.destination,
            overwrite: l.overwrite,
            preserve_mode: false,
            preserve_mtime: false,
            size: 3,
            sha256: Sha256::digest(b"abc").into(),
            mode: None,
            mtime: None,
        }
    }
    fn checkpoint() -> Checkpoint {
        Checkpoint {
            manifest: manifest(),
            phase: Phase::Receiving,
            normalized_path: b"out".to_vec(),
            parent: FileIdentity {
                device: 1,
                inode: 2,
            },
            private: FileIdentity {
                device: 1,
                inode: 3,
            },
            data: FileIdentity {
                device: 1,
                inode: 4,
            },
            durable_offset: 0,
            prefix_sha256: Sha256::digest([]).into(),
            last_stream: [4; 16],
            created: 10,
            updated: 20,
            expires: 20 + MIN_RETENTION_SECS,
            committed_final: None,
        }
    }
    fn frames() -> Vec<Frame> {
        vec![
            Frame::Start(launch(), Codec::Zlib),
            Frame::Manifest(manifest()),
            Frame::Resume {
                phase: Phase::Receiving,
                offset: 1,
                sha256: [4; 32],
            },
            Frame::Prefix {
                offset: 1,
                sha256: [4; 32],
            },
            Frame::Data {
                offset: 1,
                bytes: vec![0, 255, 10, 13],
            },
            Frame::Checkpoint {
                offset: 5,
                sha256: [5; 32],
            },
            Frame::Done {
                size: 5,
                sha256: [5; 32],
            },
            Frame::Commit {
                transfer: [1; 32],
                size: 5,
                sha256: [5; 32],
            },
            Frame::Finish {
                transfer: [1; 32],
                sha256: [5; 32],
            },
            Frame::Progress {
                stage: Stage::Verifying,
                examined: 5,
                total: 5,
            },
            Frame::Error(CopyError::Busy),
        ]
    }
    #[test]
    fn all_frames_roundtrip_every_split_without_buffering_next_frame() {
        for frame in frames() {
            let encoded = frame.encode().unwrap();
            assert!(Frame::decode(&encoded).unwrap() == frame);
            for split in 0..encoded.len() {
                let mut decoder = FrameDecoder::default();
                let (n, first) = decoder.feed(&encoded[..split]).unwrap();
                assert_eq!(n, split);
                assert!(first.is_none());
                let mut suffix = encoded[split..].to_vec();
                suffix.extend([0; 100]);
                let (n, decoded) = decoder.feed(&suffix).unwrap();
                assert_eq!(n, encoded.len() - split);
                assert!(decoded == Some(frame.clone()));
                decoder.finish().unwrap();
            }
        }
    }
    #[test]
    fn one_byte_reassembly_and_multiple_frames() {
        let mut decoder = FrameDecoder::default();
        let mut count = 0;
        for frame in frames() {
            for byte in frame.encode().unwrap() {
                let (n, item) = decoder.feed(&[byte]).unwrap();
                assert_eq!(n, 1);
                if let Some(item) = item {
                    assert!(item == frame);
                    count += 1;
                }
            }
        }
        assert_eq!(count, 11);
        decoder.finish().unwrap();
    }
    #[test]
    fn malformed_lengths_unknown_tags_enums_and_options_fail_closed() {
        for frame in frames() {
            let valid = frame.encode().unwrap();
            for prefix in 0..valid.len() {
                assert!(Frame::decode(&valid[..prefix]).is_err());
            }
            let mut extra = valid;
            extra.push(0);
            assert!(Frame::decode(&extra).is_err());
        }
        assert!(Frame::decode(&[99, 0, 0, 0, 0]).is_err());
        assert!(Frame::decode(&[11, 0, 0, 0, 2, 255, 0]).is_err());
        assert!(Frame::decode(&[11, 0, 0, 0, 2, 6, 1]).is_err());
        let mut decoder = FrameDecoder::default();
        assert!(decoder.feed(&[1, 255, 255, 255, 255]).is_err());
        assert!(decoder.feed(&[]).is_err());
        let mut decoder = FrameDecoder::default();
        decoder.feed(&[1]).unwrap();
        assert!(decoder.finish().is_err());
    }
    #[test]
    fn sizes_offsets_and_metadata_are_checked_before_encoding() {
        assert!(
            Frame::Data {
                offset: MAX_OFFSET,
                bytes: vec![1]
            }
            .encode()
            .is_err()
        );
        assert!(
            Frame::Data {
                offset: 0,
                bytes: vec![]
            }
            .encode()
            .is_err()
        );
        assert!(
            Frame::Data {
                offset: 0,
                bytes: vec![1; MAX_BLOCK + 1]
            }
            .encode()
            .is_err()
        );
        assert!(
            Frame::Progress {
                stage: Stage::Hashing,
                examined: 2,
                total: 1
            }
            .encode()
            .is_err()
        );
        let mut m = manifest();
        m.source.push(0);
        assert!(m.encode().is_err());
        let mut m = manifest();
        m.mode = Some(0o600);
        assert!(m.encode().is_err());
        m.preserve_mode = true;
        m.mode = Some(0o400);
        assert!(m.encode().is_err());
        m.mode = Some(0o4600);
        assert!(m.encode().is_err());
        m.mode = Some(0o600);
        m.preserve_mtime = true;
        m.mtime = Some(Mtime {
            seconds: 0,
            nanos: 1_000_000_000,
        });
        assert!(m.encode().is_err());
    }
    #[test]
    fn launch_digest_binds_codec_mode_paths_and_metadata_but_not_attempt_binding() {
        let l = launch();
        let digest = l.digest(Codec::None).unwrap();
        assert_ne!(digest, l.digest(Codec::Zlib).unwrap());
        let mut changed = l.clone();
        changed.mode = AttemptMode::Resume;
        assert_ne!(digest, changed.digest(Codec::None).unwrap());
        changed = l.clone();
        changed.source.push(b'x');
        assert_ne!(digest, changed.digest(Codec::None).unwrap());
        changed = l.clone();
        changed.overwrite = true;
        assert_ne!(digest, changed.digest(Codec::None).unwrap());
        assert!(l.matches_manifest(&manifest()));
        assert!(
            Launch {
                mode: AttemptMode::Resume,
                ..l
            }
            .matches_manifest(&manifest())
        );
    }
    #[test]
    fn namespace_separates_principals_direction_and_transfer() {
        let m = manifest();
        let initial = m.namespace();
        assert_eq!(initial.len(), 81);
        let mut changed = m.clone();
        changed.client[0] ^= 1;
        assert_ne!(initial, changed.namespace());
        changed = m.clone();
        changed.server[0] ^= 1;
        assert_ne!(initial, changed.namespace());
        changed = m.clone();
        changed.transfer[0] ^= 1;
        assert_ne!(initial, changed.namespace());
        changed.direction = Direction::Get;
        assert_ne!(initial, changed.namespace());
    }
    #[test]
    fn checkpoint_rejects_corruption_trailing_fields_and_impossible_states() {
        let c = checkpoint();
        let bytes = c.encode().unwrap();
        assert!(Checkpoint::decode(&bytes).unwrap() == c);
        for n in 0..bytes.len() {
            let mut corrupt = bytes.clone();
            corrupt[n] ^= 1;
            assert!(Checkpoint::decode(&corrupt).is_err());
        }
        let mut c = checkpoint();
        c.durable_offset = 4;
        assert!(c.encode().is_err());
        c = checkpoint();
        c.phase = Phase::Verifying;
        assert!(c.encode().is_err());
        c = checkpoint();
        c.phase = Phase::Committed;
        assert!(c.encode().is_err());
        c = checkpoint();
        c.expires -= 1;
        assert!(c.encode().is_err());
        c = checkpoint();
        c.prefix_sha256 = [0; 32];
        assert!(c.encode().is_err());
        c = checkpoint();
        c.phase = Phase::Committed;
        c.durable_offset = c.manifest.size;
        c.prefix_sha256 = c.manifest.sha256;
        c.committed_final = Some(c.data);
        assert!(Checkpoint::decode(&c.encode().unwrap()).unwrap() == c);
    }
    #[test]
    fn maximum_checkpoint_stays_below_control_cap() {
        let mut c = checkpoint();
        c.manifest.source = vec![b's'; MAX_PATH];
        c.manifest.destination = vec![b'd'; MAX_PATH];
        c.manifest.preserve_mode = true;
        c.manifest.mode = Some(0o777);
        c.manifest.preserve_mtime = true;
        c.manifest.mtime = Some(Mtime {
            seconds: i64::MAX,
            nanos: 999_999_999,
        });
        c.normalized_path = vec![b'x'; MAX_PATH + 8];
        assert_eq!(c.manifest.encode().unwrap().len(), 8362);
        let bytes = c.encode().unwrap();
        assert_eq!(bytes.len(), 12640); // no final identity yet: 16 bytes smaller than maximum
        assert!(Checkpoint::decode(&bytes).unwrap() == c);
        c.phase = Phase::Committed;
        c.durable_offset = c.manifest.size;
        c.prefix_sha256 = c.manifest.sha256;
        c.committed_final = Some(c.data);
        assert_eq!(c.encode().unwrap().len(), 12656);
        c.normalized_path.push(b'x');
        assert!(c.encode().is_err());
    }
    fn context() -> HelperContext {
        HelperContext {
            context_version: CONTEXT_VERSION,
            helper_protocol: VERSION,
            client: [2; 32],
            server: [3; 32],
            group: vec![9; 32],
            session: [5; 16],
            stream: [6; 16],
            transfer: [1; 32],
            direction: Direction::Put,
            launch_digest: launch().digest(Codec::Zlib).unwrap(),
            retention_seconds: MIN_RETENTION_SECS as u64,
            expected_version: "0.2.3".into(),
            expected_target: "aarch64-apple-darwin".into(),
            expected_commit: [7; 20],
        }
    }
    #[test]
    fn helper_context_roundtrip_limits_and_start_binding() {
        let c = context();
        let bytes = c.encode().unwrap();
        assert!(HelperContext::decode(&bytes).unwrap() == c);
        c.validate_start(&launch(), Codec::Zlib).unwrap();
        assert!(c.validate_start(&launch(), Codec::None).is_err());
        c.validate_build("0.2.3", "aarch64-apple-darwin", [7; 20])
            .unwrap();
        assert!(
            c.validate_build("0.2.4", "aarch64-apple-darwin", [7; 20])
                .is_err()
        );
        let mut modified = launch();
        modified.client[0] ^= 1;
        assert!(c.validate_start(&modified, Codec::Zlib).is_err());
        let mut c = context();
        c.group = vec![255; 1024];
        c.expected_version = "x".repeat(64);
        c.expected_target = "a".repeat(64);
        assert!(c.encode().unwrap().len() < MAX_CONTEXT);
        c.group.push(0);
        assert!(c.encode().is_err());
    }
    #[test]
    fn helper_context_retention_has_replay_floor_and_config_ceiling() {
        let mut c = context();
        c.retention_seconds = MIN_RETENTION_SECS as u64 - 1;
        assert!(c.encode().is_err());
        c.retention_seconds = MAX_RETENTION_SECS;
        assert!(HelperContext::decode(&c.encode().unwrap()).unwrap() == c);
        c.validate_start(&launch(), Codec::Zlib).unwrap();
        c.retention_seconds += 1;
        assert!(c.encode().is_err());
        let mut bytes = context().encode().unwrap();
        let retention_offset = CONTEXT_DOMAIN.len() + 8 + 64 + 4 + 32 + 32 + 32 + 1 + 32;
        bytes[retention_offset..retention_offset + 8].copy_from_slice(&u64::MAX.to_be_bytes());
        assert!(HelperContext::decode(&bytes).is_err());
        bytes[retention_offset..retention_offset + 8]
            .copy_from_slice(&(MIN_RETENTION_SECS as u64 - 1).to_be_bytes());
        assert!(HelperContext::decode(&bytes).is_err());
    }
    #[test]
    fn helper_context_rejects_missing_trailing_unknown_and_entrypoint_fields() {
        let bytes = context().encode().unwrap();
        for prefix in 0..bytes.len() {
            assert!(HelperContext::decode(&bytes[..prefix]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(HelperContext::decode(&extra).is_err());
        let mut invalid = bytes.clone();
        *invalid.last_mut().unwrap() ^= 1;
        assert!(HelperContext::decode(&invalid).is_err());
        let mut c = context();
        c.context_version = 2;
        assert!(c.encode().is_err());
        c = context();
        c.expected_target = "target\0suffix".into();
        assert!(c.encode().is_err());
    }
}

mod path_encoding {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    pub fn serialize<S: serde::Serializer>(
        path: &[u8],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(path))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<u8>, D::Error> {
        use serde::de::Error as _;
        let value = String::deserialize(deserializer)?;
        crate::codec::base64_bytes::decode_bytes(&value, MAX_PATH).map_err(D::Error::custom)
    }
}
