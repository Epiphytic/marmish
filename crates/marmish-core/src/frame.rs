use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Immutable protocol resource bounds; callers cannot enlarge them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits;

impl Limits {
    pub const RAW_BYTES: usize = 16 * 1024;
    pub const JSON_BYTES: usize = 32 * 1024;
    pub const NDJSON_BYTES: usize = 64 * 1024;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("probe frame exceeds its size limit")]
    SizeLimit,
    #[error("invalid probe JSON")]
    InvalidJson,
    #[error("unsupported probe version")]
    InvalidVersion,
    #[error("invalid canonical base64 payload")]
    InvalidEncoding,
}

/// Opaque probe content only: decoding never implies permission to execute it.
pub struct ProbeEnvelope;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    payload: String,
}

impl ProbeEnvelope {
    pub fn encode(bytes: &[u8]) -> Result<Vec<u8>, FrameError> {
        if bytes.len() > Limits::RAW_BYTES {
            return Err(FrameError::SizeLimit);
        }
        let frame = serde_json::to_vec(&Envelope {
            version: 1,
            payload: STANDARD.encode(bytes),
        })
        .map_err(|_| FrameError::InvalidJson)?;
        if frame.len() > Limits::JSON_BYTES {
            return Err(FrameError::SizeLimit);
        }
        Ok(frame)
    }

    pub fn decode(frame: &[u8]) -> Result<Vec<u8>, FrameError> {
        if frame.len() > Limits::JSON_BYTES {
            return Err(FrameError::SizeLimit);
        }
        let envelope: Envelope =
            serde_json::from_slice(frame).map_err(|_| FrameError::InvalidJson)?;
        if envelope.version != 1 {
            return Err(FrameError::InvalidVersion);
        }
        let max_encoded = Limits::RAW_BYTES.div_ceil(3) * 4;
        if envelope.payload.len() > max_encoded {
            return Err(FrameError::SizeLimit);
        }
        // Length is bounded before decode allocation. STANDARD requires padding
        // and rejects nonzero unused bits; re-encoding enforces canonical form.
        let bytes = STANDARD
            .decode(&envelope.payload)
            .map_err(|_| FrameError::InvalidEncoding)?;
        if bytes.len() > Limits::RAW_BYTES {
            return Err(FrameError::SizeLimit);
        }
        if STANDARD.encode(&bytes) != envelope.payload {
            return Err(FrameError::InvalidEncoding);
        }
        Ok(bytes)
    }
}
