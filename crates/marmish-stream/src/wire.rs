//! Closed canonical stream schema. Payload identities are not authentication.
use crate::codec::{Codec, EncodedBlock};
use crate::{Error, MAX_BLOCK, MAX_CONTENT, MAX_OFFSET, Result, WINDOW_BYTES};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub const PROTOCOL: &str = "marmish-stream/1";
pub const FORMAT: u32 = 1;
pub type Id = [u8; 16];
pub type Account = [u8; 32];
pub type Digest = [u8; 32];
const MAX_ARGV_BYTES: usize = 16384;
const MAX_ARGV_ITEMS: usize = 128;
const MAX_AGE_SECONDS: u64 = 7 * 86400;
const FUTURE_SKEW_SECONDS: u64 = 60;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    #[serde(with = "hex16")]
    pub stream: Id,
    #[serde(with = "hex16")]
    pub session: Id,
    #[serde(with = "hex32")]
    pub client: Account,
    #[serde(with = "hex32")]
    pub server: Account,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    pub binding: Binding,
    pub sequence: u64,
    pub body: RequestBody,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    pub binding: Binding,
    pub sequence: u64,
    pub request_sha256: Digest,
    pub body: ResponseBody,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "op",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum RequestBody {
    Hello {
        codecs: Vec<Codec>,
    },
    CopyHello {
        versions: Vec<u32>,
        codecs: Vec<Codec>,
    },
    OpenCopy {
        launch: crate::copy_protocol::Launch,
        codec: Codec,
    },
    Open {
        #[serde(with = "argv_encoding")]
        argv: Vec<Vec<u8>>,
        codec: Codec,
    },
    Exchange {
        stdin: Option<EncodedBlock>,
        stdin_eof: Option<u64>,
        stdout_ack: u64,
        stderr_ack: u64,
    },
    Cancel,
    Status,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "op",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ResponseBody {
    CopyHello {
        version: u32,
        codecs: Vec<Codec>,
        frame_bytes: u32,
        window_bytes: u32,
    },
    CopyRefused {
        code: CopyAdmissionError,
        prior_binding: Option<Binding>,
    },
    Hello {
        codecs: Vec<Codec>,
        frame_bytes: u32,
        window_bytes: u32,
    },
    Opened {
        codec: Codec,
        #[serde(with = "hex32")]
        open_sha256: Digest,
        disposition: OpenDisposition,
    },
    Exchange {
        stdin_accepted: u64,
        stdin_delivered: u64,
        stdin_closed: bool,
        stdout: Option<EncodedBlock>,
        stderr: Option<EncodedBlock>,
        stdout_eof: Option<u64>,
        stderr_eof: Option<u64>,
        cancel_requested: bool,
        terminal: Option<Terminal>,
    },
    Error {
        code: WireErrorCode,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyAdmissionError {
    Unsupported,
    Denied,
    Busy,
    Conflict,
    Capacity,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenDisposition {
    Created,
    Existing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireErrorCode {
    Unsupported,
    Invalid,
    Denied,
    Conflict,
    Gap,
    Expired,
    Capacity,
    Uncertain,
    NotFound,
    Closed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Terminal {
    Exited { code: u8 },
    Signaled { signal: u8 },
    Cancelled,
    Failed { code: WireErrorCode },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    protocol: String,
    version: u32,
    binding: Binding,
    sequence: u64,
    message: Message,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "direction",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Message {
    Request(RequestBody),
    Response {
        #[serde(with = "hex32")]
        request_sha256: Digest,
        body: ResponseBody,
    },
}
fn invalid() -> Error {
    Error::Invalid("stream_wire")
}
fn offset(value: u64) -> Result<()> {
    if value > MAX_OFFSET {
        return Err(Error::Offset);
    }
    Ok(())
}
fn codecs(values: &[Codec]) -> Result<()> {
    if values.is_empty() || values.len() > 2 || (values.len() == 2 && values[0] == values[1]) {
        return Err(invalid());
    }
    Ok(())
}
fn validate_argv(argv: &[Vec<u8>]) -> Result<()> {
    let total = argv.iter().try_fold(0usize, |sum, arg| {
        sum.checked_add(arg.len())?.checked_add(1)
    });
    if argv.is_empty()
        || argv.len() > MAX_ARGV_ITEMS
        || total.is_none_or(|total| total > MAX_ARGV_BYTES)
    {
        return Err(Error::Limit);
    }
    if argv[0].is_empty() || argv.iter().any(|arg| arg.contains(&0)) {
        return Err(invalid());
    }
    Ok(())
}
impl Request {
    pub fn validate_at(&self, now_unix_seconds: u64) -> Result<()> {
        validate_request(self)?;
        if matches!(
            self.body,
            RequestBody::Open { .. } | RequestBody::OpenCopy { .. }
        ) {
            let timestamp =
                u64::from_be_bytes(self.binding.stream[..8].try_into().map_err(|_| invalid())?);
            if timestamp > now_unix_seconds.saturating_add(FUTURE_SKEW_SECONDS)
                || now_unix_seconds.saturating_sub(timestamp) > MAX_AGE_SECONDS
            {
                return Err(Error::Invalid("stream_age"));
            }
        }
        Ok(())
    }
}
fn validate_request(request: &Request) -> Result<()> {
    match &request.body {
        RequestBody::Hello { codecs: values } => codecs(values),
        RequestBody::CopyHello {
            versions,
            codecs: values,
        } => {
            if versions.is_empty()
                || versions.contains(&0)
                || versions.len() > 4
                || versions
                    .iter()
                    .enumerate()
                    .any(|(i, v)| versions[..i].contains(v))
            {
                return Err(invalid());
            }
            codecs(values)
        }
        RequestBody::OpenCopy { launch, .. } => {
            launch.validate()?;
            if launch.client != request.binding.client || launch.server != request.binding.server {
                return Err(invalid());
            }
            Ok(())
        }
        RequestBody::Open { argv, .. } => validate_argv(argv),
        RequestBody::Exchange {
            stdin,
            stdin_eof,
            stdout_ack,
            stderr_ack,
        } => {
            offset(*stdout_ack)?;
            offset(*stderr_ack)?;
            if let Some(end) = stdin_eof {
                offset(*end)?;
            }
            if let Some(block) = stdin {
                block.validate()?;
                if stdin_eof.is_some_and(|end| block.end().is_ok_and(|block_end| block_end != end))
                {
                    return Err(Error::Eof);
                }
            }
            Ok(())
        }
        RequestBody::Cancel | RequestBody::Status => Ok(()),
    }
}
fn validate_output(block: &Option<EncodedBlock>, eof: Option<u64>) -> Result<()> {
    if let Some(end) = eof {
        offset(end)?;
    }
    if let Some(block) = block {
        block.validate()?;
        if eof.is_some_and(|end| block.end().is_ok_and(|block_end| block_end > end)) {
            return Err(Error::Eof);
        }
    }
    Ok(())
}
fn validate_response(response: &Response) -> Result<()> {
    match &response.body {
        ResponseBody::CopyHello { version, .. } if *version != crate::copy_protocol::VERSION => {
            Err(invalid())
        }
        ResponseBody::CopyHello {
            codecs: values,
            frame_bytes,
            window_bytes,
            ..
        }
        | ResponseBody::Hello {
            codecs: values,
            frame_bytes,
            window_bytes,
        } => {
            codecs(values)?;
            if *frame_bytes == 0
                || *frame_bytes as usize > MAX_BLOCK
                || *window_bytes == 0
                || *window_bytes as usize > WINDOW_BYTES
                || window_bytes < frame_bytes
            {
                return Err(Error::Limit);
            }
            Ok(())
        }
        ResponseBody::Exchange {
            stdin_accepted,
            stdin_delivered,
            stdin_closed,
            stdout,
            stderr,
            stdout_eof,
            stderr_eof,
            cancel_requested,
            terminal,
        } => {
            offset(*stdin_accepted)?;
            offset(*stdin_delivered)?;
            if stdin_delivered > stdin_accepted {
                return Err(Error::Ack);
            }
            if (terminal.is_some() || *cancel_requested) && !stdin_closed {
                return Err(invalid());
            }
            validate_output(stdout, *stdout_eof)?;
            validate_output(stderr, *stderr_eof)?;
            if terminal.is_some() && (stdout_eof.is_none() || stderr_eof.is_none()) {
                return Err(Error::Eof);
            }
            if matches!(terminal, Some(Terminal::Signaled { signal: 0 })) {
                return Err(invalid());
            }
            if matches!(terminal, Some(Terminal::Cancelled)) && !cancel_requested {
                return Err(invalid());
            }
            Ok(())
        }
        ResponseBody::CopyRefused {
            code,
            prior_binding,
        } => {
            if prior_binding.is_some()
                && !matches!(
                    code,
                    CopyAdmissionError::Busy | CopyAdmissionError::RecoveryRequired
                )
            {
                return Err(invalid());
            }
            if prior_binding.as_ref().is_some_and(|prior| {
                prior.client != response.binding.client || prior.server != response.binding.server
            }) {
                return Err(invalid());
            }
            Ok(())
        }
        ResponseBody::Opened { .. } | ResponseBody::Error { .. } => Ok(()),
    }
}
fn encode(envelope: &Envelope) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(envelope).map_err(|_| invalid())?;
    if bytes.len() > MAX_CONTENT {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<Envelope> {
    if bytes.len() > MAX_CONTENT {
        return Err(Error::Limit);
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if envelope.protocol != PROTOCOL
        || envelope.version != FORMAT
        || encode(&envelope)?.as_slice() != bytes
    {
        return Err(invalid());
    }
    Ok(envelope)
}
pub fn encode_request(request: &Request) -> Result<Vec<u8>> {
    validate_request(request)?;
    encode(&Envelope {
        protocol: PROTOCOL.into(),
        version: FORMAT,
        binding: request.binding.clone(),
        sequence: request.sequence,
        message: Message::Request(request.body.clone()),
    })
}
/// Decode and structurally validate one canonical request.
///
/// This does not authenticate the payload or apply clock/replay admission policy.
/// Before admitting a NEW Open, the caller MUST call [`Request::validate_at`]
/// with its trusted current Unix time, in addition to transport authentication
/// and durable launch/replay fencing. Existing-stream recovery must use its
/// persisted identity and launch state instead of treating retries as new work.
pub fn decode_request(bytes: &[u8]) -> Result<Request> {
    let envelope = decode(bytes)?;
    let Message::Request(body) = envelope.message else {
        return Err(invalid());
    };
    let request = Request {
        binding: envelope.binding,
        sequence: envelope.sequence,
        body,
    };
    validate_request(&request)?;
    Ok(request)
}
pub fn encode_response(response: &Response) -> Result<Vec<u8>> {
    validate_response(response)?;
    encode(&Envelope {
        protocol: PROTOCOL.into(),
        version: FORMAT,
        binding: response.binding.clone(),
        sequence: response.sequence,
        message: Message::Response {
            request_sha256: response.request_sha256,
            body: response.body.clone(),
        },
    })
}
pub fn decode_response(bytes: &[u8]) -> Result<Response> {
    let envelope = decode(bytes)?;
    let Message::Response {
        request_sha256,
        body,
    } = envelope.message
    else {
        return Err(invalid());
    };
    let response = Response {
        binding: envelope.binding,
        sequence: envelope.sequence,
        request_sha256,
        body,
    };
    validate_response(&response)?;
    Ok(response)
}
pub fn request_digest(bytes: &[u8]) -> Digest {
    Sha256::digest(bytes).into()
}
pub fn open_digest(argv: &[Vec<u8>], codec: Codec) -> Result<Digest> {
    validate_argv(argv)?;
    let bytes = serde_json::to_vec(&RequestBody::Open {
        argv: argv.to_vec(),
        codec,
    })
    .map_err(|_| invalid())?;
    Ok(request_digest(&bytes))
}

fn serialize_hex<S: serde::Serializer>(
    bytes: &[u8],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 15) as usize] as char);
    }
    serializer.serialize_str(&text)
}
fn deserialize_hex<'de, D: serde::Deserializer<'de>, const N: usize>(
    deserializer: D,
) -> std::result::Result<[u8; N], D::Error> {
    use serde::de::Error as _;
    let text = String::deserialize(deserializer)?;
    if text.len() != N * 2 {
        return Err(D::Error::custom("hex"));
    }
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let mut bytes = [0; N];
    for (out, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        *out = (nibble(pair[0]).ok_or_else(|| D::Error::custom("hex"))? << 4)
            | nibble(pair[1]).ok_or_else(|| D::Error::custom("hex"))?;
    }
    Ok(bytes)
}
mod hex16 {
    pub fn serialize<S: serde::Serializer>(
        value: &[u8; 16],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        super::serialize_hex(value, serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; 16], D::Error> {
        super::deserialize_hex(deserializer)
    }
}
pub(crate) mod hex32 {
    pub fn serialize<S: serde::Serializer>(
        value: &[u8; 32],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        super::serialize_hex(value, serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; 32], D::Error> {
        super::deserialize_hex(deserializer)
    }
}
mod argv_encoding {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        argv: &[Vec<u8>],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        argv.iter()
            .map(|arg| STANDARD.encode(arg))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<Vec<u8>>, D::Error> {
        use serde::de::Error as _;
        let encoded = Vec::<String>::deserialize(deserializer)?;
        if encoded.len() > MAX_ARGV_ITEMS {
            return Err(D::Error::custom("argv_limit"));
        }
        let mut argv = Vec::with_capacity(encoded.len());
        let mut total = 0usize;
        for value in encoded {
            let arg = crate::codec::base64_bytes::decode_bytes(&value, MAX_ARGV_BYTES)
                .map_err(D::Error::custom)?;
            total = total
                .checked_add(arg.len() + 1)
                .ok_or_else(|| D::Error::custom("argv_limit"))?;
            if total > MAX_ARGV_BYTES {
                return Err(D::Error::custom("argv_limit"));
            }
            argv.push(arg);
        }
        Ok(argv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> Binding {
        Binding {
            stream: [1; 16],
            session: [2; 16],
            client: [3; 32],
            server: [4; 32],
        }
    }
    fn request(body: RequestBody) -> Request {
        Request {
            binding: binding(),
            sequence: u64::MAX,
            body,
        }
    }
    fn response(body: ResponseBody) -> Response {
        Response {
            binding: binding(),
            sequence: u64::MAX,
            request_sha256: [255; 32],
            body,
        }
    }
    fn max_block() -> EncodedBlock {
        crate::codec::encode(
            MAX_OFFSET - MAX_BLOCK as u64,
            &vec![255; MAX_BLOCK],
            Codec::None,
        )
        .unwrap()
    }
    fn exchange(terminal: Option<Terminal>) -> ResponseBody {
        ResponseBody::Exchange {
            stdin_accepted: MAX_OFFSET,
            stdin_delivered: MAX_OFFSET,
            stdin_closed: true,
            stdout: Some(max_block()),
            stderr: Some(max_block()),
            stdout_eof: Some(MAX_OFFSET),
            stderr_eof: Some(MAX_OFFSET),
            cancel_requested: true,
            terminal,
        }
    }
    fn copy_launch() -> crate::copy_protocol::Launch {
        use crate::copy_protocol::{AttemptMode, Direction, Launch, VERSION};
        Launch {
            version: VERSION,
            mode: AttemptMode::Create,
            transfer: [5; 32],
            client: binding().client,
            server: binding().server,
            direction: Direction::Put,
            source: b"a\xff;$(x)".to_vec(),
            destination: b"/tmp/d".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
        }
    }
    #[test]
    fn copy_wire_is_closed_bounded_and_byte_preserving() {
        for body in [
            RequestBody::CopyHello {
                versions: vec![1, 2],
                codecs: vec![Codec::None, Codec::Zlib],
            },
            RequestBody::OpenCopy {
                launch: copy_launch(),
                codec: Codec::Zlib,
            },
        ] {
            let r = request(body);
            let bytes = encode_request(&r).unwrap();
            assert!(decode_request(&bytes).unwrap() == r);
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(!text.contains("[97,"));
        }
        let r = request(RequestBody::OpenCopy {
            launch: copy_launch(),
            codec: Codec::None,
        });
        let mut value: serde_json::Value =
            serde_json::from_slice(&encode_request(&r).unwrap()).unwrap();
        value["message"]["data"]["data"]["launch"]["extra"] = true.into();
        assert!(decode_request(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut l = copy_launch();
        l.client[0] ^= 1;
        assert!(
            encode_request(&request(RequestBody::OpenCopy {
                launch: l,
                codec: Codec::None
            }))
            .is_err()
        );
        // A future-only offer is syntactically valid and negotiates Unsupported;
        // malformed offers must fail before reaching server negotiation.
        assert!(
            encode_request(&request(RequestBody::CopyHello {
                versions: vec![2],
                codecs: vec![Codec::None],
            }))
            .is_ok()
        );
        for versions in [vec![], vec![0], vec![1, 1], vec![1, 2, 3, 4, 5]] {
            assert!(
                encode_request(&request(RequestBody::CopyHello {
                    versions,
                    codecs: vec![Codec::None]
                }))
                .is_err()
            );
        }
        for body in [
            ResponseBody::CopyHello {
                version: 1,
                codecs: vec![Codec::None],
                frame_bytes: MAX_BLOCK as u32,
                window_bytes: WINDOW_BYTES as u32,
            },
            ResponseBody::CopyRefused {
                code: CopyAdmissionError::Busy,
                prior_binding: Some(binding()),
            },
        ] {
            let r = response(body);
            let b = encode_response(&r).unwrap();
            assert!(decode_response(&b).unwrap() == r);
        }
        assert!(
            encode_response(&response(ResponseBody::CopyRefused {
                code: CopyAdmissionError::Conflict,
                prior_binding: Some(binding())
            }))
            .is_err()
        );
    }
    #[test]
    fn worst_case_fit_and_roundtrip_every_variant() {
        let mut argv = vec![Vec::new(); MAX_ARGV_ITEMS];
        argv[0] = vec![255; MAX_ARGV_BYTES - MAX_ARGV_ITEMS];
        for body in [
            RequestBody::Hello {
                codecs: vec![Codec::None, Codec::Zlib],
            },
            RequestBody::Open {
                argv,
                codec: Codec::Zlib,
            },
            RequestBody::Exchange {
                stdin: Some(max_block()),
                stdin_eof: Some(MAX_OFFSET),
                stdout_ack: MAX_OFFSET,
                stderr_ack: MAX_OFFSET,
            },
            RequestBody::Cancel,
            RequestBody::Status,
        ] {
            let message = request(body);
            let bytes = encode_request(&message).unwrap();
            assert!(bytes.len() <= MAX_CONTENT);
            assert!(decode_request(&bytes).unwrap() == message);
            assert!(decode_response(&bytes).is_err());
        }
        for body in [
            ResponseBody::Hello {
                codecs: vec![Codec::None, Codec::Zlib],
                frame_bytes: MAX_BLOCK as u32,
                window_bytes: WINDOW_BYTES as u32,
            },
            ResponseBody::Opened {
                codec: Codec::Zlib,
                open_sha256: [255; 32],
                disposition: OpenDisposition::Created,
            },
            ResponseBody::Opened {
                codec: Codec::None,
                open_sha256: [255; 32],
                disposition: OpenDisposition::Existing,
            },
            exchange(None),
            exchange(Some(Terminal::Exited { code: 255 })),
            exchange(Some(Terminal::Signaled { signal: 127 })),
            exchange(Some(Terminal::Cancelled)),
            exchange(Some(Terminal::Failed {
                code: WireErrorCode::Unsupported,
            })),
            ResponseBody::Error {
                code: WireErrorCode::Unsupported,
            },
            ResponseBody::Error {
                code: WireErrorCode::Invalid,
            },
            ResponseBody::Error {
                code: WireErrorCode::Denied,
            },
            ResponseBody::Error {
                code: WireErrorCode::Conflict,
            },
            ResponseBody::Error {
                code: WireErrorCode::Gap,
            },
            ResponseBody::Error {
                code: WireErrorCode::Expired,
            },
            ResponseBody::Error {
                code: WireErrorCode::Capacity,
            },
            ResponseBody::Error {
                code: WireErrorCode::Uncertain,
            },
            ResponseBody::Error {
                code: WireErrorCode::NotFound,
            },
            ResponseBody::Error {
                code: WireErrorCode::Closed,
            },
        ] {
            let message = response(body);
            let bytes = encode_response(&message).unwrap();
            assert!(bytes.len() <= MAX_CONTENT);
            assert!(decode_response(&bytes).unwrap() == message);
            assert!(decode_request(&bytes).is_err());
        }
    }
    #[test]
    fn rejects_noncanonical_unknown_and_oversized_envelopes() {
        let bytes = encode_request(&request(RequestBody::Status)).unwrap();
        let mut whitespace = bytes.clone();
        whitespace.push(b' ');
        assert!(decode_request(&whitespace).is_err());
        for path in [0, 1, 2] {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let target = match path {
                0 => &mut value,
                1 => &mut value["binding"],
                _ => &mut value["message"],
            };
            target
                .as_object_mut()
                .unwrap()
                .insert("unknown".into(), true.into());
            assert!(decode_request(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let duplicate = String::from_utf8(bytes.clone()).unwrap().replacen(
            "\"version\":1",
            "\"version\":1,\"version\":1",
            1,
        );
        assert!(decode_request(duplicate.as_bytes()).is_err());
        assert!(decode_request(&vec![b' '; MAX_CONTENT + 1]).is_err());
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["binding"]["stream"] = "AA000000000000000000000000000000".into();
        assert!(decode_request(&serde_json::to_vec(&value).unwrap()).is_err());
        value["version"] = 2.into();
        assert!(decode_request(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    #[test]
    fn rejects_invalid_argv_offsets_and_snapshots() {
        for argv in [
            vec![],
            vec![vec![]],
            vec![b"a\0".to_vec()],
            vec![vec![1; MAX_ARGV_BYTES]],
            vec![b"x".to_vec(); MAX_ARGV_ITEMS + 1],
        ] {
            assert!(
                encode_request(&request(RequestBody::Open {
                    argv,
                    codec: Codec::None
                }))
                .is_err()
            );
        }
        assert!(
            encode_request(&request(RequestBody::Hello {
                codecs: vec![Codec::None, Codec::None]
            }))
            .is_err()
        );
        assert!(
            encode_request(&request(RequestBody::Exchange {
                stdin: None,
                stdin_eof: None,
                stdout_ack: u64::MAX,
                stderr_ack: 0
            }))
            .is_err()
        );
        assert!(
            encode_request(&request(RequestBody::Exchange {
                stdin: Some(max_block()),
                stdin_eof: Some(0),
                stdout_ack: 0,
                stderr_ack: 0
            }))
            .is_err()
        );
        let mut body = exchange(None);
        if let ResponseBody::Exchange {
            stdin_delivered, ..
        } = &mut body
        {
            *stdin_delivered = MAX_OFFSET + 1;
        }
        assert!(encode_response(&response(body)).is_err());
        let mut body = exchange(Some(Terminal::Cancelled));
        if let ResponseBody::Exchange { stdout_eof, .. } = &mut body {
            *stdout_eof = None;
        }
        assert!(encode_response(&response(body)).is_err());
        let mut body = exchange(None);
        if let ResponseBody::Exchange { stdout_eof, .. } = &mut body {
            *stdout_eof = Some(0);
        }
        assert!(encode_response(&response(body)).is_err());
    }
    #[test]
    fn cancelled_terminal_requires_accepted_cancellation() {
        let accepted = response(exchange(Some(Terminal::Cancelled)));
        let canonical = encode_response(&accepted).unwrap();
        assert!(decode_response(&canonical).is_ok());
        let rejected = String::from_utf8(canonical)
            .unwrap()
            .replace("\"cancel_requested\":true", "\"cancel_requested\":false");
        assert!(decode_response(rejected.as_bytes()).is_err());
        let mut rejected = accepted;
        if let ResponseBody::Exchange {
            cancel_requested, ..
        } = &mut rejected.body
        {
            *cancel_requested = false;
        }
        assert!(encode_response(&rejected).is_err());
    }
    #[test]
    fn input_closed_is_required_and_truthful_for_terminal_and_cancel() {
        let canonical = encode_response(&response(exchange(None))).unwrap();
        let missing = String::from_utf8(canonical.clone())
            .unwrap()
            .replace("\"stdin_closed\":true,", "");
        assert!(decode_response(missing.as_bytes()).is_err());
        for terminal in [None, Some(Terminal::Exited { code: 0 })] {
            let mut body = exchange(terminal);
            if let ResponseBody::Exchange { stdin_closed, .. } = &mut body {
                *stdin_closed = false;
            }
            assert!(encode_response(&response(body)).is_err());
        }
        let mut open = exchange(None);
        if let ResponseBody::Exchange {
            stdin_closed,
            cancel_requested,
            ..
        } = &mut open
        {
            *stdin_closed = false;
            *cancel_requested = false;
        }
        let open = response(open);
        let bytes = encode_response(&open).unwrap();
        assert!(decode_response(&bytes).unwrap() == open);
        assert!(bytes.len() <= MAX_CONTENT);
    }

    #[test]
    fn age_only_checks_open_and_immutable_digest_excludes_sequence() {
        let now = 1_800_000_000;
        let mut message = request(RequestBody::Open {
            argv: vec![b"cat".to_vec()],
            codec: Codec::Zlib,
        });
        for timestamp in [now, now - MAX_AGE_SECONDS, now + FUTURE_SKEW_SECONDS] {
            message.binding.stream[..8].copy_from_slice(&timestamp.to_be_bytes());
            assert!(message.validate_at(now).is_ok());
        }
        for timestamp in [now - MAX_AGE_SECONDS - 1, now + FUTURE_SKEW_SECONDS + 1] {
            message.binding.stream[..8].copy_from_slice(&timestamp.to_be_bytes());
            assert!(message.validate_at(now).is_err());
        }
        message.body = RequestBody::Status;
        assert!(message.validate_at(now).is_ok());
        let digest = open_digest(&[b"cat".to_vec()], Codec::Zlib).unwrap();
        assert_eq!(
            digest,
            open_digest(&[b"cat".to_vec()], Codec::Zlib).unwrap()
        );
        assert_ne!(
            digest,
            open_digest(&[b"cat".to_vec()], Codec::None).unwrap()
        );
        let first = request_digest(&encode_request(&message).unwrap());
        message.sequence = 0;
        assert_ne!(first, request_digest(&encode_request(&message).unwrap()));
    }
    #[test]
    fn base64_is_strict_and_nested_unknown_fields_fail() {
        let bytes = encode_request(&request(RequestBody::Open {
            argv: vec![b"x".to_vec()],
            codec: Codec::None,
        }))
        .unwrap();
        let text = String::from_utf8(bytes).unwrap();
        for bad in ["eA", "eB==", "eA===", "eA==\\n"] {
            assert!(decode_request(text.replace("eA==", bad).as_bytes()).is_err());
        }
        let extra = text.replace("\"codec\":\"none\"", "\"codec\":\"none\",\"surprise\":true");
        assert!(decode_request(extra.as_bytes()).is_err());
    }
}
