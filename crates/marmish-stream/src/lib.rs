//! Pure bounded stream primitives. Wire identities are claims, never authority.
//! Durable launch fencing and authenticated transport belong to the native owner.
pub mod codec;
pub mod windows;
pub mod wire;

pub const MAX_BLOCK: usize = 8192;
pub const MAX_ENCODED: usize = MAX_BLOCK;
pub const MAX_CONTENT: usize = 32768;
pub const WINDOW_BYTES: usize = 256 * 1024;
pub const MAX_OFFSET: u64 = i64::MAX as u64;
pub const MAX_RECORDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid stream input: {0}")]
    Invalid(&'static str),
    #[error("invalid compressed stream block")]
    Codec,
    #[error("stream capacity reached")]
    Limit,
    #[error("invalid stream offset")]
    Offset,
    #[error("conflicting stream retry")]
    Conflict,
    #[error("stream byte gap")]
    Gap,
    #[error("stream retry outside retained history")]
    OldRetry,
    #[error("stream EOF conflict")]
    Eof,
    #[error("invalid stream acknowledgement")]
    Ack,
}
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn end(offset: u64, len: usize) -> Result<u64> {
    offset
        .checked_add(len as u64)
        .filter(|n| *n <= MAX_OFFSET)
        .ok_or(Error::Offset)
}

pub mod copy_protocol;
