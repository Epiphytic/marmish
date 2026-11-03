//! Closed native command protocol. Decoding is not authentication or authority.
use crate::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

pub(crate) const PROTOCOL: &str = "marmish-command/1";
pub(crate) const KIND: u64 = 30100;
pub(crate) const TAG: [&str; 2] = ["marmish", "command/1"];
pub(crate) const MAX_CONTENT: usize = 32768;
pub(crate) const MAX_BYTES: usize = 16384;
pub(crate) const MAX_METADATA: usize = 4096;
const MAX_CURSOR: u64 = i64::MAX as u64;
pub(crate) type Account = [u8; 32];
pub(crate) type Id = [u8; 16];
pub(crate) type Digest = [u8; 32];

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    pub session: Id,
    pub client: Account,
    pub server: Account,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Submit {
    #[serde(with = "argv_encoding")]
    pub argv: Vec<Vec<u8>>,
    #[serde(with = "hex16")]
    pub invocation: Id,
    #[serde(with = "bytes_encoding")]
    pub stdin: Vec<u8>,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Op {
    Submit,
    Status,
    Poll,
    Cancel,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum RequestOp {
    Submit(Submit),
    Status {
        invocation: Id,
    },
    Poll {
        invocation: Id,
        stdout_cursor: u64,
        stderr_cursor: u64,
        max_bytes: u32,
    },
    Cancel {
        invocation: Id,
    },
}
impl RequestOp {
    pub fn op(&self) -> Op {
        match self {
            Self::Submit(_) => Op::Submit,
            Self::Status { .. } => Op::Status,
            Self::Poll { .. } => Op::Poll,
            Self::Cancel { .. } => Op::Cancel,
        }
    }
    pub fn invocation(&self) -> Id {
        match self {
            Self::Submit(s) => s.invocation,
            Self::Status { invocation }
            | Self::Poll { invocation, .. }
            | Self::Cancel { invocation } => *invocation,
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Request {
    pub binding: Binding,
    pub id: Id,
    pub op: RequestOp,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Response {
    pub binding: Binding,
    pub id: Id,
    pub op: Op,
    pub request_sha256: Digest,
    pub result: std::result::Result<ResponseResult, WireError>,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ResponseResult {
    Submit {
        disposition: Disposition,
        status: Status,
    },
    Status(Status),
    Poll {
        status: Status,
        stdout: Chunk,
        stderr: Chunk,
    },
    Cancel(Status),
}
impl ResponseResult {
    pub fn op(&self) -> Op {
        match self {
            Self::Submit { .. } => Op::Submit,
            Self::Status(_) => Op::Status,
            Self::Poll { .. } => Op::Poll,
            Self::Cancel(_) => Op::Cancel,
        }
    }
    pub fn status(&self) -> &Status {
        match self {
            Self::Submit { status, .. } | Self::Poll { status, .. } => status,
            Self::Status(s) | Self::Cancel(s) => s,
        }
    }
}
macro_rules! closed_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub(crate) enum $name { $($variant),+ }
    };
}
closed_enum!(Disposition { Accepted, Existing });
closed_enum!(Admission {
    Accepted,
    Dispatching,
    Engine
});
closed_enum!(State {
    Accepted,
    Launching,
    Running,
    Exited,
    UnknownLaunch,
    Lost
});
closed_enum!(OutcomeKind {
    Code,
    Signal,
    NotStarted
});
closed_enum!(InputHealth {
    Open,
    EofPending,
    Closed,
    Gap,
    Uncertain
});
closed_enum!(GapReason {
    Limit,
    StorageFailure,
    Expired,
    UncertainTail
});
closed_enum!(Warning {
    StdinUnconsumed,
    InputFailed
});
closed_enum!(ErrorCode {
    InvalidRequest,
    Unauthorized,
    RoleDisabled,
    CommandDenied,
    NoncanonicalExecutable,
    InvocationConflict,
    Expired,
    FutureDeferred,
    FutureExcessive,
    ClockBehindFloor,
    QuotaExceeded,
    Busy,
    NotFound,
    ResultExpired,
    RecoveryUncertain,
    InputFailed,
    StorageFailure,
    TransportDegraded,
    AuthorityRevoked
});
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireError {
    pub code: ErrorCode,
    #[serde(deserialize_with = "required_nullable")]
    pub detail: Option<String>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Outcome {
    pub kind: OutcomeKind,
    #[serde(deserialize_with = "required_nullable")]
    pub value: Option<u8>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputStatus {
    pub accepted: u32,
    pub delivered: u32,
    pub eof_sent: bool,
    pub health: InputHealth,
    pub length: u32,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Gap {
    #[serde(deserialize_with = "required_nullable")]
    pub end: Option<u64>,
    pub reason: GapReason,
    pub start: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutputStatus {
    pub closed: bool,
    pub gaps: Vec<Gap>,
    pub produced: u64,
    pub retained: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Status {
    pub admission: Admission,
    pub authority_valid: bool,
    pub cancel_requested: bool,
    pub engine_known: bool,
    pub input: InputStatus,
    #[serde(with = "hex16")]
    pub invocation: Id,
    #[serde(deserialize_with = "required_nullable")]
    pub outcome: Option<Outcome>,
    pub state: State,
    pub stderr: OutputStatus,
    pub stdout: OutputStatus,
    #[serde(deserialize_with = "required_nullable")]
    pub warning: Option<Warning>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Chunk {
    #[serde(with = "bytes_encoding")]
    pub bytes: Vec<u8>,
    pub next_cursor: u64,
}
fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

// The outer object is typed; each operation body is decoded separately. Canonical
// byte equality also rejects duplicates inside the temporary body Value.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    body: serde_json::Value,
    #[serde(with = "hex32")]
    client: Account,
    #[serde(with = "hex16")]
    id: Id,
    op: Op,
    protocol: String,
    #[serde(with = "hex32")]
    server: Account,
    #[serde(with = "hex16")]
    session: Id,
    #[serde(rename = "type")]
    kind: MessageType,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MessageType {
    Request,
    Response,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InvocationBody {
    #[serde(with = "hex16")]
    invocation: Id,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PollBody {
    #[serde(with = "hex16")]
    invocation: Id,
    max_bytes: u32,
    stderr_cursor: u64,
    stdout_cursor: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseBody {
    #[serde(deserialize_with = "required_nullable")]
    error: Option<WireError>,
    ok: bool,
    #[serde(with = "hex32")]
    request_sha256: Digest,
    #[serde(deserialize_with = "required_nullable")]
    result: Option<serde_json::Value>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitResult {
    disposition: Disposition,
    status: Status,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusResult {
    status: Status,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PollResult {
    status: Status,
    stderr: Chunk,
    stdout: Chunk,
}

fn invalid() -> Error {
    Error::Invalid("command_wire")
}
fn value<T: Serialize>(v: &T) -> Result<serde_json::Value> {
    serde_json::to_value(v).map_err(|_| invalid())
}
fn body<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| invalid())
}
fn read(bytes: &[u8], kind: MessageType) -> Result<Wire> {
    if bytes.len() > MAX_CONTENT {
        return Err(Error::Invalid("command_envelope_limit"));
    }
    let w: Wire = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if w.protocol != PROTOCOL || w.kind != kind || w.client == w.server {
        return Err(invalid());
    }
    Ok(w)
}
fn write(
    binding: &Binding,
    id: Id,
    op: Op,
    kind: MessageType,
    body: serde_json::Value,
) -> Result<Vec<u8>> {
    if binding.client == binding.server {
        return Err(invalid());
    }
    let w = Wire {
        body,
        client: binding.client,
        id,
        op,
        protocol: PROTOCOL.into(),
        server: binding.server,
        session: binding.session,
        kind,
    };
    let bytes = serde_json::to_vec(&w).map_err(|_| invalid())?;
    if bytes.len() > MAX_CONTENT {
        return Err(Error::Invalid("command_envelope_limit"));
    }
    Ok(bytes)
}
fn binding(w: &Wire) -> Binding {
    Binding {
        session: w.session,
        client: w.client,
        server: w.server,
    }
}
fn validate_submit(s: &Submit) -> Result<()> {
    let total = s
        .argv
        .iter()
        .try_fold(0usize, |n, a| n.checked_add(a.len())?.checked_add(1));
    if s.argv.is_empty()
        || s.argv.len() > 128
        || total.is_none_or(|n| n > MAX_BYTES)
        || s.stdin.len() > MAX_BYTES
    {
        return Err(Error::Invalid("command_bytes_limit"));
    }
    if s.argv[0].first() != Some(&b'/') || s.argv.iter().any(|a| a.contains(&0)) {
        return Err(Error::Invalid("command_argv"));
    }
    Ok(())
}
pub(crate) fn encode_request(r: &Request) -> Result<Vec<u8>> {
    let b = match &r.op {
        RequestOp::Submit(s) => {
            validate_submit(s)?;
            value(s)?
        }
        RequestOp::Status { invocation } | RequestOp::Cancel { invocation } => {
            value(&InvocationBody {
                invocation: *invocation,
            })?
        }
        RequestOp::Poll {
            invocation,
            stdout_cursor,
            stderr_cursor,
            max_bytes,
        } => {
            if *stdout_cursor > MAX_CURSOR
                || *stderr_cursor > MAX_CURSOR
                || !(1..=MAX_BYTES as u32).contains(max_bytes)
            {
                return Err(invalid());
            }
            value(&PollBody {
                invocation: *invocation,
                stdout_cursor: *stdout_cursor,
                stderr_cursor: *stderr_cursor,
                max_bytes: *max_bytes,
            })?
        }
    };
    write(&r.binding, r.id, r.op.op(), MessageType::Request, b)
}
pub(crate) fn decode_request(bytes: &[u8]) -> Result<Request> {
    let w = read(bytes, MessageType::Request)?;
    let b = binding(&w);
    let op = match w.op {
        Op::Submit => RequestOp::Submit(body(w.body)?),
        Op::Status => RequestOp::Status {
            invocation: body::<InvocationBody>(w.body)?.invocation,
        },
        Op::Cancel => RequestOp::Cancel {
            invocation: body::<InvocationBody>(w.body)?.invocation,
        },
        Op::Poll => {
            let p: PollBody = body(w.body)?;
            RequestOp::Poll {
                invocation: p.invocation,
                stdout_cursor: p.stdout_cursor,
                stderr_cursor: p.stderr_cursor,
                max_bytes: p.max_bytes,
            }
        }
    };
    let r = Request {
        binding: b,
        id: w.id,
        op,
    };
    if encode_request(&r)? != bytes {
        return Err(Error::Invalid("command_noncanonical"));
    }
    Ok(r)
}
fn validate_status(s: &Status) -> Result<()> {
    if s.input.length > MAX_BYTES as u32
        || s.input.accepted > MAX_BYTES as u32
        || s.input.delivered > MAX_BYTES as u32
    {
        return Err(invalid());
    }
    if !s.engine_known
        && (s.state != State::Accepted
            || s.input.accepted != 0
            || s.input.delivered != 0
            || s.input.health != InputHealth::Open
            || s.input.eof_sent)
    {
        return Err(invalid());
    }
    if let Some(o) = &s.outcome {
        match (o.kind, o.value) {
            (OutcomeKind::Code, Some(_))
            | (OutcomeKind::Signal, Some(1..=127))
            | (OutcomeKind::NotStarted, None) => {}
            _ => return Err(invalid()),
        }
    }
    for out in [&s.stdout, &s.stderr] {
        if out.produced > MAX_CURSOR || out.retained > MAX_CURSOR || out.gaps.len() > 4 {
            return Err(invalid());
        }
        for gap in &out.gaps {
            if gap.start > MAX_CURSOR
                || gap
                    .end
                    .is_some_and(|end| end > MAX_CURSOR || end <= gap.start)
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
fn validate_error(e: &WireError) -> Result<()> {
    if let Some(detail) = &e.detail
        && (e.code != ErrorCode::NoncanonicalExecutable
            || !detail.starts_with('/')
            || detail.contains('\0')
            || serde_json::to_vec(detail).map_err(|_| invalid())?.len() > 1024)
    {
        return Err(invalid());
    }
    Ok(())
}
fn result_value(r: &ResponseResult) -> Result<serde_json::Value> {
    validate_status(r.status())?;
    match r {
        ResponseResult::Submit {
            disposition,
            status,
        } => value(&SubmitResult {
            disposition: *disposition,
            status: status.clone(),
        }),
        ResponseResult::Status(s) | ResponseResult::Cancel(s) => {
            value(&StatusResult { status: s.clone() })
        }
        ResponseResult::Poll {
            status,
            stdout,
            stderr,
        } => {
            if stdout.next_cursor > MAX_CURSOR
                || stderr.next_cursor > MAX_CURSOR
                || stdout
                    .bytes
                    .len()
                    .checked_add(stderr.bytes.len())
                    .is_none_or(|n| n > MAX_BYTES)
            {
                return Err(invalid());
            }
            value(&PollResult {
                status: status.clone(),
                stderr: stderr.clone(),
                stdout: stdout.clone(),
            })
        }
    }
}
pub(crate) fn encode_response(r: &Response) -> Result<Vec<u8>> {
    let (result, error) = match &r.result {
        Ok(result) => {
            if result.op() != r.op {
                return Err(invalid());
            }
            (Some(result_value(result)?), None)
        }
        Err(e) => {
            validate_error(e)?;
            (None, Some(e.clone()))
        }
    };
    let b = ResponseBody {
        error,
        ok: r.result.is_ok(),
        request_sha256: r.request_sha256,
        result,
    };
    let bytes = write(&r.binding, r.id, r.op, MessageType::Response, value(&b)?)?;
    if matches!(&r.result, Ok(ResponseResult::Poll { .. })) {
        let mut metadata = r.clone();
        // Keep actual cursors and every other field; remove only encoded data.
        if let Ok(ResponseResult::Poll { stdout, stderr, .. }) = &mut metadata.result {
            stdout.bytes.clear();
            stderr.bytes.clear();
        }
        let metadata_body = ResponseBody {
            error: None,
            ok: true,
            request_sha256: metadata.request_sha256,
            result: Some(result_value(
                metadata.result.as_ref().map_err(|_| invalid())?,
            )?),
        };
        if write(
            &metadata.binding,
            metadata.id,
            metadata.op,
            MessageType::Response,
            value(&metadata_body)?,
        )?
        .len()
            > MAX_METADATA
        {
            return Err(Error::Invalid("command_metadata_limit"));
        }
    }
    Ok(bytes)
}
pub(crate) fn decode_response(bytes: &[u8]) -> Result<Response> {
    let w = read(bytes, MessageType::Response)?;
    let b = binding(&w);
    let response: ResponseBody = body(w.body)?;
    let result = match (response.ok, response.result, response.error) {
        (true, Some(v), None) => Ok(match w.op {
            Op::Submit => {
                let p: SubmitResult = body(v)?;
                ResponseResult::Submit {
                    disposition: p.disposition,
                    status: p.status,
                }
            }
            Op::Status => ResponseResult::Status(body::<StatusResult>(v)?.status),
            Op::Cancel => ResponseResult::Cancel(body::<StatusResult>(v)?.status),
            Op::Poll => {
                let p: PollResult = body(v)?;
                ResponseResult::Poll {
                    status: p.status,
                    stdout: p.stdout,
                    stderr: p.stderr,
                }
            }
        }),
        (false, None, Some(e)) => Err(e),
        _ => return Err(invalid()),
    };
    let r = Response {
        binding: b,
        id: w.id,
        op: w.op,
        request_sha256: response.request_sha256,
        result,
    };
    if encode_response(&r)? != bytes {
        return Err(Error::Invalid("command_noncanonical"));
    }
    Ok(r)
}
pub(crate) fn submit_digest(group: &[u8], binding: &Binding, s: &Submit) -> Result<Digest> {
    validate_submit(s)?;
    let length: u32 = group.len().try_into().map_err(|_| invalid())?;
    let mut h = Sha256::new();
    h.update(b"marmish-command\0\x01");
    h.update(length.to_be_bytes());
    h.update(group);
    h.update(binding.session);
    h.update(binding.client);
    h.update(binding.server);
    h.update(s.invocation);
    h.update((s.argv.len() as u32).to_be_bytes());
    for arg in &s.argv {
        h.update((arg.len() as u32).to_be_bytes());
        h.update(arg);
    }
    h.update((s.stdin.len() as u32).to_be_bytes());
    h.update(&s.stdin);
    Ok(h.finalize().into())
}
pub(crate) fn request_digest(canonical: &[u8]) -> Digest {
    Sha256::digest(canonical).into()
}
pub(crate) fn id_timestamp(id: Id) -> u64 {
    u64::from_be_bytes([0, 0, id[0], id[1], id[2], id[3], id[4], id[5]])
}

fn parse_hex<const N: usize>(s: &str) -> std::result::Result<[u8; N], &'static str> {
    if !crate::foundation::valid_hex(s, N * 2) {
        return Err("invalid_hex");
    }
    let mut out = [0; N];
    hex::decode_to_slice(s, &mut out).map_err(|_| "invalid_hex")?;
    Ok(out)
}
macro_rules! hex_encoding {
    ($module:ident, $size:literal) => {
        mod $module {
            use super::*;
            pub fn serialize<S: Serializer>(
                v: &[u8; $size],
                s: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                s.serialize_str(&hex::encode(v))
            }
            pub fn deserialize<'de, D: Deserializer<'de>>(
                d: D,
            ) -> std::result::Result<[u8; $size], D::Error> {
                parse_hex(&String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    };
}
hex_encoding!(hex16, 16);
hex_encoding!(hex32, 32);
fn decode_bytes<E: serde::de::Error>(s: String) -> std::result::Result<Vec<u8>, E> {
    if s.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err(E::custom("bytes_limit"));
    }
    let bytes = STANDARD.decode(&s).map_err(|_| E::custom("base64"))?;
    if bytes.len() > MAX_BYTES || STANDARD.encode(&bytes) != s {
        return Err(E::custom("base64"));
    }
    Ok(bytes)
}
mod bytes_encoding {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<u8>, D::Error> {
        decode_bytes(String::deserialize(d)?)
    }
}
mod argv_encoding {
    use super::*;
    pub fn serialize<S: Serializer>(v: &[Vec<u8>], s: S) -> std::result::Result<S::Ok, S::Error> {
        v.iter()
            .map(|b| STANDARD.encode(b))
            .collect::<Vec<_>>()
            .serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<Vec<Vec<u8>>, D::Error> {
        let encoded = Vec::<String>::deserialize(d)?;
        if encoded.len() > 128 {
            return Err(serde::de::Error::custom("argv_limit"));
        }
        encoded.into_iter().map(decode_bytes).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> Binding {
        Binding {
            session: [0x44; 16],
            client: [0x11; 32],
            server: [0x22; 32],
        }
    }
    fn submit() -> Submit {
        Submit {
            invocation: [0x55; 16],
            argv: vec![b"/usr/bin/true".to_vec()],
            stdin: vec![],
        }
    }
    fn request(op: RequestOp) -> Request {
        Request {
            binding: binding(),
            id: [0x66; 16],
            op,
        }
    }
    fn status() -> Status {
        let out = OutputStatus {
            closed: false,
            gaps: vec![],
            produced: 0,
            retained: 0,
        };
        Status {
            admission: Admission::Accepted,
            authority_valid: true,
            cancel_requested: false,
            engine_known: false,
            input: InputStatus {
                accepted: 0,
                delivered: 0,
                eof_sent: false,
                health: InputHealth::Open,
                length: 0,
            },
            invocation: [0x55; 16],
            outcome: None,
            state: State::Accepted,
            stderr: out.clone(),
            stdout: out,
            warning: None,
        }
    }
    fn response(result: ResponseResult) -> Response {
        Response {
            binding: binding(),
            id: [0x66; 16],
            op: result.op(),
            request_sha256: [0x77; 32],
            result: Ok(result),
        }
    }
    fn request_roundtrip(r: Request) {
        let bytes = encode_request(&r).unwrap();
        assert!(decode_request(&bytes).unwrap() == r);
    }
    fn response_roundtrip(r: Response) {
        let bytes = encode_response(&r).unwrap();
        assert!(decode_response(&bytes).unwrap() == r);
    }
    fn mutate(bytes: &[u8], f: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let mut v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        f(&mut v);
        serde_json::to_vec(&v).unwrap()
    }
    #[test]
    fn normative_digest_and_request_identity() {
        let b = Binding {
            session: parse_hex("00006a00000000000000000000000002").unwrap(),
            client: [0x11; 32],
            server: [0x22; 32],
        };
        let s = Submit {
            invocation: parse_hex("00006a00000000000000000000000001").unwrap(),
            ..submit()
        };
        assert!(
            hex::encode(submit_digest(&[0x33; 32], &b, &s).unwrap())
                == "783fa59ee2d2c19d7408cfcf3963e8ed542572648ce6a9b993a17ac0e5bf50be"
        );
        assert!(id_timestamp(s.invocation) == 0x6a000000);
        let mut r = Request {
            binding: b,
            id: [0x66; 16],
            op: RequestOp::Submit(s.clone()),
        };
        let first = encode_request(&r).unwrap();
        assert!(
            String::from_utf8(first.clone())
                .unwrap()
                .contains("L3Vzci9iaW4vdHJ1ZQ==")
        );
        r.id[15] ^= 1;
        let second = encode_request(&r).unwrap();
        assert!(request_digest(&first) != request_digest(&second));
        assert!(
            submit_digest(&[0x33; 32], &r.binding, &s).unwrap()
                != submit_digest(&[0x34; 32], &r.binding, &s).unwrap()
        );
        let mut changed = s.clone();
        changed.stdin.push(0);
        assert!(
            submit_digest(&[0x33; 32], &r.binding, &s).unwrap()
                != submit_digest(&[0x33; 32], &r.binding, &changed).unwrap()
        );
        assert!(r.op.invocation() == s.invocation);
        assert!(KIND == 30100 && TAG == ["marmish", "command/1"]);
    }
    #[test]
    fn independent_canonical_status_request_vector() {
        let expected = concat!(
            "{\"body\":{\"invocation\":\"01010101010101010101010101010101\"},",
            "\"client\":\"1111111111111111111111111111111111111111111111111111111111111111\",",
            "\"id\":\"44444444444444444444444444444444\",\"op\":\"status\",",
            "\"protocol\":\"marmish-command/1\",",
            "\"server\":\"2222222222222222222222222222222222222222222222222222222222222222\",",
            "\"session\":\"33333333333333333333333333333333\",\"type\":\"request\"}"
        );
        let r = Request {
            binding: Binding {
                session: [0x33; 16],
                client: [0x11; 32],
                server: [0x22; 32],
            },
            id: [0x44; 16],
            op: RequestOp::Status {
                invocation: [1; 16],
            },
        };
        assert!(encode_request(&r).unwrap() == expected.as_bytes());
        assert!(decode_request(expected.as_bytes()).unwrap() == r);
    }
    #[test]
    fn requests_preserve_binary_arguments_and_input() {
        let binary: Vec<u8> = (0..=255).collect();
        let mut s = submit();
        s.argv.extend([
            vec![],
            b"quotes'\"\n* ** $(echo no);\\".to_vec(),
            (1..=255).collect(),
        ]);
        s.stdin = binary;
        request_roundtrip(request(RequestOp::Submit(s)));
        request_roundtrip(request(RequestOp::Status {
            invocation: [1; 16],
        }));
        request_roundtrip(request(RequestOp::Cancel {
            invocation: [1; 16],
        }));
        request_roundtrip(request(RequestOp::Poll {
            invocation: [1; 16],
            stdout_cursor: MAX_CURSOR,
            stderr_cursor: 0,
            max_bytes: MAX_BYTES as u32,
        }));
        for size in [4095, 4096, 4097, MAX_BYTES] {
            let mut s = submit();
            s.stdin = vec![0xff; size];
            request_roundtrip(request(RequestOp::Submit(s)));
        }
    }
    #[test]
    fn combined_envelope_and_decoded_bounds() {
        let mut s = submit();
        s.argv.push(vec![b'x'; MAX_BYTES - 15]);
        assert!(encode_request(&request(RequestOp::Submit(s.clone()))).is_ok());
        s.stdin = vec![b'y'; MAX_BYTES];
        assert!(matches!(
            encode_request(&request(RequestOp::Submit(s))),
            Err(Error::Invalid("command_envelope_limit"))
        ));
        for argv in [
            vec![],
            vec![b"relative".to_vec()],
            vec![vec![]],
            vec![b"/a\0b".to_vec()],
            vec![b"/x".to_vec(); 129],
            vec![vec![b'/'; MAX_BYTES]],
        ] {
            let s = Submit { argv, ..submit() };
            assert!(encode_request(&request(RequestOp::Submit(s))).is_err());
        }
        let mut s = submit();
        s.stdin = vec![0; MAX_BYTES + 1];
        assert!(encode_request(&request(RequestOp::Submit(s))).is_err());
        for (cursor, budget) in [(MAX_CURSOR + 1, 1), (0, 0), (0, MAX_BYTES as u32 + 1)] {
            assert!(
                encode_request(&request(RequestOp::Poll {
                    invocation: [0; 16],
                    stdout_cursor: cursor,
                    stderr_cursor: 0,
                    max_bytes: budget
                }))
                .is_err()
            );
        }
        assert!(decode_request(&vec![b' '; MAX_CONTENT + 1]).is_err());
        // Empty arguments vary JSON overhead modulo four; payloads remain canonical.
        let mut exact = None;
        for empty_arguments in 0..4 {
            let mut s = submit();
            s.argv.push(vec![b'x'; 8192]);
            s.argv.extend(vec![Vec::new(); empty_arguments]);
            let base = encode_request(&request(RequestOp::Submit(s.clone()))).unwrap();
            let remaining = MAX_CONTENT - base.len();
            if remaining.is_multiple_of(4) {
                s.stdin = vec![b'y'; remaining / 4 * 3];
                exact = Some(s);
                break;
            }
        }
        let mut s = exact.unwrap();
        let encoded = encode_request(&request(RequestOp::Submit(s.clone()))).unwrap();
        assert!(encoded.len() == MAX_CONTENT);
        assert!(decode_request(&encoded).is_ok());
        s.stdin.push(b'y');
        assert!(matches!(
            encode_request(&request(RequestOp::Submit(s))),
            Err(Error::Invalid("command_envelope_limit"))
        ));
        let mut padded = encoded;
        padded.push(b' ');
        assert!(decode_request(&padded).is_err());
    }
    #[test]
    fn strict_request_schema_and_canonical_bytes() {
        let bytes = encode_request(&request(RequestOp::Submit(submit()))).unwrap();
        for f in [
            |v: &mut serde_json::Value| {
                v["body"]["unknown"] = 0.into();
            },
            |v: &mut serde_json::Value| {
                v["body"].as_object_mut().unwrap().remove("stdin");
            },
            |v: &mut serde_json::Value| {
                v["protocol"] = "marmish/1".into();
            },
            |v: &mut serde_json::Value| {
                v["client"] = "AA".repeat(32).into();
            },
            |v: &mut serde_json::Value| {
                v["id"] = "00".repeat(15).into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["stdin"] = "Zh==".into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["stdin"] = "Zg".into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["stdin"] = "Zg==\n".into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["stdin"] = "-_==".into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["argv"] = serde_json::json!(["AA=="]);
            },
            |v: &mut serde_json::Value| {
                v["body"]["group"] = "33".into();
            },
        ] {
            assert!(decode_request(&mutate(&bytes, f)).is_err());
        }
        let text = String::from_utf8(bytes.clone()).unwrap();
        for bad in [
            format!(" {text}"),
            format!("{text}\n"),
            format!("{text}{text}"),
            text.replacen(
                "\"body\":",
                "\"id\":\"00000000000000000000000000000000\",\"body\":",
                1,
            ),
            text.replacen("\"stdin\":", "\"stdin\":\"\",\"stdin\":", 1),
            text.replace("/", "\\/"),
        ] {
            if bad.as_bytes() != bytes {
                assert!(decode_request(bad.as_bytes()).is_err());
            }
        }
        let p = encode_request(&request(RequestOp::Poll {
            invocation: [0; 16],
            stdout_cursor: 0,
            stderr_cursor: 0,
            max_bytes: 1,
        }))
        .unwrap();
        for x in [
            serde_json::json!(true),
            serde_json::json!(-1),
            serde_json::json!(1.0),
            serde_json::json!("1"),
        ] {
            assert!(decode_request(&mutate(&p, |v| v["body"]["max_bytes"] = x)).is_err());
        }
        let mut equal = request(RequestOp::Submit(submit()));
        equal.binding.server = equal.binding.client;
        assert!(encode_request(&equal).is_err());
    }
    #[test]
    fn every_response_shape_and_closed_enum_value() {
        for disposition in [Disposition::Accepted, Disposition::Existing] {
            response_roundtrip(response(ResponseResult::Submit {
                disposition,
                status: status(),
            }));
        }
        response_roundtrip(response(ResponseResult::Status(status())));
        response_roundtrip(response(ResponseResult::Cancel(status())));
        for state in [
            State::Accepted,
            State::Launching,
            State::Running,
            State::Exited,
            State::UnknownLaunch,
            State::Lost,
        ] {
            let mut s = status();
            s.engine_known = true;
            s.admission = Admission::Engine;
            s.state = state;
            response_roundtrip(response(ResponseResult::Status(s)));
        }
        for health in [
            InputHealth::Open,
            InputHealth::EofPending,
            InputHealth::Closed,
            InputHealth::Gap,
            InputHealth::Uncertain,
        ] {
            let mut s = status();
            s.engine_known = true;
            s.admission = Admission::Dispatching;
            s.input.health = health;
            response_roundtrip(response(ResponseResult::Status(s)));
        }
        for outcome in [
            Outcome {
                kind: OutcomeKind::Code,
                value: Some(255),
            },
            Outcome {
                kind: OutcomeKind::Signal,
                value: Some(127),
            },
            Outcome {
                kind: OutcomeKind::NotStarted,
                value: None,
            },
        ] {
            let mut s = status();
            s.engine_known = true;
            s.outcome = Some(outcome);
            s.state = State::Exited;
            response_roundtrip(response(ResponseResult::Status(s)));
        }
        for warning in [Warning::StdinUnconsumed, Warning::InputFailed] {
            let mut s = status();
            s.warning = Some(warning);
            response_roundtrip(response(ResponseResult::Status(s)));
        }
        for reason in [
            GapReason::Limit,
            GapReason::StorageFailure,
            GapReason::Expired,
            GapReason::UncertainTail,
        ] {
            let mut s = status();
            s.stdout.gaps = vec![
                Gap {
                    start: MAX_CURSOR - 1,
                    end: Some(MAX_CURSOR),
                    reason,
                },
                Gap {
                    start: MAX_CURSOR,
                    end: None,
                    reason,
                },
            ];
            s.stdout.produced = MAX_CURSOR;
            s.stderr.retained = MAX_CURSOR;
            response_roundtrip(response(ResponseResult::Status(s)));
        }
        let all = [
            ErrorCode::InvalidRequest,
            ErrorCode::Unauthorized,
            ErrorCode::RoleDisabled,
            ErrorCode::CommandDenied,
            ErrorCode::NoncanonicalExecutable,
            ErrorCode::InvocationConflict,
            ErrorCode::Expired,
            ErrorCode::FutureDeferred,
            ErrorCode::FutureExcessive,
            ErrorCode::ClockBehindFloor,
            ErrorCode::QuotaExceeded,
            ErrorCode::Busy,
            ErrorCode::NotFound,
            ErrorCode::ResultExpired,
            ErrorCode::RecoveryUncertain,
            ErrorCode::InputFailed,
            ErrorCode::StorageFailure,
            ErrorCode::TransportDegraded,
            ErrorCode::AuthorityRevoked,
        ];
        for code in all {
            for op in [Op::Submit, Op::Status, Op::Poll, Op::Cancel] {
                let mut r = response(ResponseResult::Status(status()));
                r.op = op;
                r.result = Err(WireError { code, detail: None });
                response_roundtrip(r);
            }
        }
    }
    #[test]
    fn poll_combined_budget_and_maximum_metadata() {
        let mut s = status();
        s.engine_known = true;
        s.admission = Admission::Engine;
        s.state = State::Lost;
        s.outcome = Some(Outcome {
            kind: OutcomeKind::Signal,
            value: Some(127),
        });
        s.input = InputStatus {
            length: 16384,
            accepted: 16384,
            delivered: 16384,
            eof_sent: true,
            health: InputHealth::Uncertain,
        };
        s.warning = Some(Warning::StdinUnconsumed);
        let out = OutputStatus {
            closed: true,
            retained: MAX_CURSOR,
            produced: MAX_CURSOR,
            gaps: (0..4)
                .map(|i| Gap {
                    start: MAX_CURSOR - 8 + i,
                    end: Some(MAX_CURSOR),
                    reason: GapReason::StorageFailure,
                })
                .collect(),
        };
        s.stdout = out.clone();
        s.stderr = out;
        let chunk = Chunk {
            bytes: (0..=255).cycle().take(8192).collect(),
            next_cursor: MAX_CURSOR,
        };
        let mut r = response(ResponseResult::Poll {
            status: s,
            stdout: chunk.clone(),
            stderr: chunk,
        });
        let bytes = encode_response(&r).unwrap();
        assert!(bytes.len() <= 25944);
        let mut metadata: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        metadata["body"]["result"]["stdout"]["bytes"] = "".into();
        metadata["body"]["result"]["stderr"]["bytes"] = "".into();
        assert!(serde_json::to_vec(&metadata).unwrap().len() <= MAX_METADATA);
        response_roundtrip(r.clone());
        if let Ok(ResponseResult::Poll { stdout, .. }) = &mut r.result {
            stdout.bytes.push(0);
        }
        assert!(encode_response(&r).is_err());
    }
    #[test]
    fn required_nullables_and_status_ranges_are_enforced() {
        let bytes = encode_response(&response(ResponseResult::Status(status()))).unwrap();
        for path in [
            "/body/error",
            "/body/result/status/outcome",
            "/body/result/status/warning",
        ] {
            let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let (parent, key) = path.rsplit_once('/').unwrap();
            v.pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(decode_response(&serde_json::to_vec(&v).unwrap()).is_err());
        }
        for f in [
            |v: &mut serde_json::Value| {
                v["body"]["ok"] = false.into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["state"] = "running".into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["input"]["accepted"] = 1.into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["input"]["delivered"] = 1.into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["input"]["health"] = "closed".into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["input"]["eof_sent"] = true.into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["input"]["length"] = 16385.into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["outcome"] =
                    serde_json::json!({"kind":"signal","value":0});
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["outcome"] =
                    serde_json::json!({"kind":"not_started","value":0});
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["stdout"]["produced"] = serde_json::json!(u64::MAX);
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["stdout"]["gaps"] =
                    serde_json::json!([{"start":1,"end":1,"reason":"limit"}]);
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["stdout"]["gaps"] =
                    serde_json::json!([{"start":1,"reason":"limit"}]);
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["stdout"]["gaps"] = serde_json::json!(vec![
                    serde_json::json!({"start":1,"end":2,"reason":"limit"});
                    5
                ]);
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["outcome"] =
                    serde_json::json!({"kind":"code","value":256});
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["outcome"] =
                    serde_json::json!({"kind":"not_started"});
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["extra"] = true.into();
            },
            |v: &mut serde_json::Value| {
                v["body"]["result"]["status"]["engine_known"] = 0.into();
            },
        ] {
            assert!(decode_response(&mutate(&bytes, f)).is_err());
        }
        let text = String::from_utf8(bytes.clone()).unwrap();
        let duplicate = text.replacen("\"outcome\":", "\"outcome\":null,\"outcome\":", 1);
        assert!(decode_response(duplicate.as_bytes()).is_err());
        let mut mismatch = response(ResponseResult::Status(status()));
        mismatch.op = Op::Submit;
        assert!(encode_response(&mismatch).is_err());
        assert!(
            decode_response(
                &encode_request(&request(RequestOp::Status {
                    invocation: [1; 16]
                }))
                .unwrap()
            )
            .is_err()
        );
        assert!(decode_request(&bytes).is_err());
    }
    #[test]
    fn error_details_are_closed_and_json_encoded_bounded() {
        let mut r = response(ResponseResult::Status(status()));
        for (code, detail, ok) in [
            (
                ErrorCode::NoncanonicalExecutable,
                Some("relative".into()),
                false,
            ),
            (
                ErrorCode::NoncanonicalExecutable,
                Some("/a\0b".into()),
                false,
            ),
            (
                ErrorCode::NoncanonicalExecutable,
                Some("\x1b[31mrelative".into()),
                false,
            ),
            // Unix permits control bytes in names; only the eventual CLI display
            // escapes this remote path, rather than inventing a wire restriction.
            (
                ErrorCode::NoncanonicalExecutable,
                Some("/a\x1b[31m".into()),
                true,
            ),
            (
                ErrorCode::NoncanonicalExecutable,
                Some("/".repeat(1022)),
                true,
            ),
            (
                ErrorCode::NoncanonicalExecutable,
                Some("/".repeat(1023)),
                false,
            ),
            (
                ErrorCode::NoncanonicalExecutable,
                Some(format!("/{}", "\n".repeat(512))),
                false,
            ),
            (
                ErrorCode::CommandDenied,
                Some("/usr/bin/true".into()),
                false,
            ),
        ] {
            r.result = Err(WireError { code, detail });
            assert!(encode_response(&r).is_ok() == ok);
        }
        r.result = Err(WireError {
            code: ErrorCode::NoncanonicalExecutable,
            detail: Some("/usr/bin/true".into()),
        });
        response_roundtrip(r.clone());
        let bytes = encode_response(&r).unwrap();
        for detail in ["relative", "/a\0b", "\x1b[31mrelative"] {
            assert!(
                decode_response(&mutate(&bytes, |v| {
                    v["body"]["error"]["detail"] = detail.into();
                }))
                .is_err()
            );
        }
        assert!(
            decode_response(&mutate(&bytes, |v| {
                v["body"]["error"]["detail"] = "/a\x1b[31m".into();
            }))
            .is_ok()
        );
        for change in ["unknown", "local_busy", "worker_unavailable"] {
            assert!(
                decode_response(&mutate(&bytes, |v| v["body"]["error"]["code"] = change.into()))
                    .is_err()
            );
        }
        assert!(
            decode_response(&mutate(&bytes, |v| {
                v["body"]["error"].as_object_mut().unwrap().remove("detail");
            }))
            .is_err()
        );
    }
    // These are hand-written protocol bytes, not output captured from the codec.
    // Shared literal fragments only avoid copying the exact same status/envelope.
    macro_rules! literal_status {
        () => {
            concat!(
                r#"{"admission":"accepted","authority_valid":true,"cancel_requested":false,"engine_known":false,"input":{"accepted":0,"delivered":0,"eof_sent":false,"health":"open","length":0},"invocation":"55555555555555555555555555555555","outcome":null,"state":"accepted","stderr":{"closed":false,"gaps":[],"produced":0,"retained":0},"stdout":{"closed":false,"gaps":[],"produced":0,"retained":0},"warning":null}"#
            )
        };
    }
    macro_rules! literal_response {
        ($op:literal, $body:expr) => {
            concat!(
                r#"{"body":"#, $body,
                r#","client":"1111111111111111111111111111111111111111111111111111111111111111","id":"66666666666666666666666666666666","op":""#,
                $op,
                r#"","protocol":"marmish-command/1","server":"2222222222222222222222222222222222222222222222222222222222222222","session":"44444444444444444444444444444444","type":"response"}"#
            )
        };
    }
    #[test]
    fn independent_canonical_response_vectors() {
        const SUBMIT: &str = literal_response!(
            "submit",
            concat!(
                r#"{"error":null,"ok":true,"request_sha256":"7777777777777777777777777777777777777777777777777777777777777777","result":{"disposition":"accepted","status":"#,
                literal_status!(),
                "}}"
            )
        );
        const STATUS: &str = literal_response!(
            "status",
            concat!(
                r#"{"error":null,"ok":true,"request_sha256":"7777777777777777777777777777777777777777777777777777777777777777","result":{"status":"#,
                literal_status!(),
                "}}"
            )
        );
        const POLL: &str = literal_response!(
            "poll",
            concat!(
                r#"{"error":null,"ok":true,"request_sha256":"7777777777777777777777777777777777777777777777777777777777777777","result":{"status":"#,
                literal_status!(),
                r#","stderr":{"bytes":"","next_cursor":0},"stdout":{"bytes":"","next_cursor":0}}}"#
            )
        );
        const POLL_BINARY: &str = literal_response!(
            "poll",
            r#"{"error":null,"ok":true,"request_sha256":"7777777777777777777777777777777777777777777777777777777777777777","result":{"status":{"admission":"engine","authority_valid":true,"cancel_requested":false,"engine_known":true,"input":{"accepted":0,"delivered":0,"eof_sent":false,"health":"open","length":0},"invocation":"55555555555555555555555555555555","outcome":{"kind":"signal","value":9},"state":"exited","stderr":{"closed":true,"gaps":[{"end":null,"reason":"uncertain_tail","start":1}],"produced":1,"retained":0},"stdout":{"closed":true,"gaps":[{"end":1,"reason":"limit","start":0}],"produced":3,"retained":0},"warning":null},"stderr":{"bytes":"/w==","next_cursor":1},"stdout":{"bytes":"AAE=","next_cursor":3}}}"#
        );
        const CANCEL: &str = literal_response!(
            "cancel",
            concat!(
                r#"{"error":null,"ok":true,"request_sha256":"7777777777777777777777777777777777777777777777777777777777777777","result":{"status":"#,
                literal_status!(),
                "}}"
            )
        );
        const ERROR_NULL: &str = literal_response!(
            "status",
            r#"{"error":{"code":"not_found","detail":null},"ok":false,"request_sha256":"7777777777777777777777777777777777777777777777777777777777777777","result":null}"#
        );
        const ERROR_PATH: &str = literal_response!(
            "submit",
            r#"{"error":{"code":"noncanonical_executable","detail":"/usr/bin/true"},"ok":false,"request_sha256":"7777777777777777777777777777777777777777777777777777777777777777","result":null}"#
        );
        let cases = [
            (
                SUBMIT,
                Ok(ResponseResult::Submit {
                    disposition: Disposition::Accepted,
                    status: status(),
                }),
                Op::Submit,
            ),
            (STATUS, Ok(ResponseResult::Status(status())), Op::Status),
            (
                POLL,
                Ok(ResponseResult::Poll {
                    status: status(),
                    stdout: Chunk {
                        bytes: vec![],
                        next_cursor: 0,
                    },
                    stderr: Chunk {
                        bytes: vec![],
                        next_cursor: 0,
                    },
                }),
                Op::Poll,
            ),
            (
                POLL_BINARY,
                Ok(ResponseResult::Poll {
                    status: Status {
                        admission: Admission::Engine,
                        engine_known: true,
                        state: State::Exited,
                        outcome: Some(Outcome {
                            kind: OutcomeKind::Signal,
                            value: Some(9),
                        }),
                        stdout: OutputStatus {
                            closed: true,
                            gaps: vec![Gap {
                                end: Some(1),
                                reason: GapReason::Limit,
                                start: 0,
                            }],
                            produced: 3,
                            retained: 0,
                        },
                        stderr: OutputStatus {
                            closed: true,
                            gaps: vec![Gap {
                                end: None,
                                reason: GapReason::UncertainTail,
                                start: 1,
                            }],
                            produced: 1,
                            retained: 0,
                        },
                        ..status()
                    },
                    stdout: Chunk {
                        bytes: vec![0, 1],
                        next_cursor: 3,
                    },
                    stderr: Chunk {
                        bytes: vec![255],
                        next_cursor: 1,
                    },
                }),
                Op::Poll,
            ),
            (CANCEL, Ok(ResponseResult::Cancel(status())), Op::Cancel),
            (
                ERROR_NULL,
                Err(WireError {
                    code: ErrorCode::NotFound,
                    detail: None,
                }),
                Op::Status,
            ),
            (
                ERROR_PATH,
                Err(WireError {
                    code: ErrorCode::NoncanonicalExecutable,
                    detail: Some("/usr/bin/true".into()),
                }),
                Op::Submit,
            ),
        ];
        for (literal, result, op) in cases {
            let expected = Response {
                binding: binding(),
                id: [0x66; 16],
                op,
                request_sha256: [0x77; 32],
                result,
            };
            assert!(encode_response(&expected).unwrap() == literal.as_bytes());
            assert!(decode_response(literal.as_bytes()).unwrap() == expected);
        }
    }
    fn enum_literals<T: Serialize + serde::de::DeserializeOwned + PartialEq>(cases: &[(T, &str)]) {
        for (value, literal) in cases {
            assert!(serde_json::to_vec(value).unwrap() == literal.as_bytes());
            assert!(serde_json::from_str::<T>(literal).unwrap() == *value);
        }
    }
    #[test]
    fn independent_closed_enum_literals() {
        enum_literals(&[
            (Op::Submit, r#""submit""#),
            (Op::Status, r#""status""#),
            (Op::Poll, r#""poll""#),
            (Op::Cancel, r#""cancel""#),
        ]);
        enum_literals(&[
            (MessageType::Request, r#""request""#),
            (MessageType::Response, r#""response""#),
        ]);
        enum_literals(&[
            (Disposition::Accepted, r#""accepted""#),
            (Disposition::Existing, r#""existing""#),
        ]);
        enum_literals(&[
            (Admission::Accepted, r#""accepted""#),
            (Admission::Dispatching, r#""dispatching""#),
            (Admission::Engine, r#""engine""#),
        ]);
        enum_literals(&[
            (State::Accepted, r#""accepted""#),
            (State::Launching, r#""launching""#),
            (State::Running, r#""running""#),
            (State::Exited, r#""exited""#),
            (State::UnknownLaunch, r#""unknown_launch""#),
            (State::Lost, r#""lost""#),
        ]);
        enum_literals(&[
            (OutcomeKind::Code, r#""code""#),
            (OutcomeKind::Signal, r#""signal""#),
            (OutcomeKind::NotStarted, r#""not_started""#),
        ]);
        enum_literals(&[
            (InputHealth::Open, r#""open""#),
            (InputHealth::EofPending, r#""eof_pending""#),
            (InputHealth::Closed, r#""closed""#),
            (InputHealth::Gap, r#""gap""#),
            (InputHealth::Uncertain, r#""uncertain""#),
        ]);
        enum_literals(&[
            (GapReason::Limit, r#""limit""#),
            (GapReason::StorageFailure, r#""storage_failure""#),
            (GapReason::Expired, r#""expired""#),
            (GapReason::UncertainTail, r#""uncertain_tail""#),
        ]);
        enum_literals(&[
            (Warning::StdinUnconsumed, r#""stdin_unconsumed""#),
            (Warning::InputFailed, r#""input_failed""#),
        ]);
        enum_literals(&[
            (ErrorCode::InvalidRequest, r#""invalid_request""#),
            (ErrorCode::Unauthorized, r#""unauthorized""#),
            (ErrorCode::RoleDisabled, r#""role_disabled""#),
            (ErrorCode::CommandDenied, r#""command_denied""#),
            (
                ErrorCode::NoncanonicalExecutable,
                r#""noncanonical_executable""#,
            ),
            (ErrorCode::InvocationConflict, r#""invocation_conflict""#),
            (ErrorCode::Expired, r#""expired""#),
            (ErrorCode::FutureDeferred, r#""future_deferred""#),
            (ErrorCode::FutureExcessive, r#""future_excessive""#),
            (ErrorCode::ClockBehindFloor, r#""clock_behind_floor""#),
            (ErrorCode::QuotaExceeded, r#""quota_exceeded""#),
            (ErrorCode::Busy, r#""busy""#),
            (ErrorCode::NotFound, r#""not_found""#),
            (ErrorCode::ResultExpired, r#""result_expired""#),
            (ErrorCode::RecoveryUncertain, r#""recovery_uncertain""#),
            (ErrorCode::InputFailed, r#""input_failed""#),
            (ErrorCode::StorageFailure, r#""storage_failure""#),
            (ErrorCode::TransportDegraded, r#""transport_degraded""#),
            (ErrorCode::AuthorityRevoked, r#""authority_revoked""#),
        ]);
    }
}
