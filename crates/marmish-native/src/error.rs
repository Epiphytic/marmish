/// Finite public categories. External provider and relay diagnostics must not
/// carry secrets into the CLI or local IPC.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Bounded finite diagnostic relayed over the private startup channel.
    #[error("{0}")]
    DaemonStartup(String),
    #[error("local I/O failed")]
    Io(#[from] std::io::Error),
    #[error("invalid input: {0}")]
    Invalid(&'static str),
    #[error("identity: {0}")]
    Identity(&'static str),
    #[error("storage: {0}")]
    Storage(&'static str),
    #[error("transport: {0}")]
    Transport(&'static str),
    #[error("marmot: {0}")]
    Mdk(&'static str),
    #[error("copy: {}", copy_category(.0))]
    Copy(marmish_stream::copy_protocol::CopyError),
    #[error("stream: {}", stream_category(.0))]
    Stream(marmish_stream::wire::WireErrorCode),
    #[error("operation deadline exceeded")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Stable finite file categories; never include a peer-provided path or message.
pub(crate) fn copy_category(code: &marmish_stream::copy_protocol::CopyError) -> &'static str {
    use marmish_stream::copy_protocol::CopyError::*;
    match code {
        Unsupported => "copy_unsupported",
        Protocol => "copy_protocol_error",
        PermissionDenied => "file_permission_denied",
        NotFound => "file_not_found",
        UnsafePath => "unsafe_path",
        Busy => "transfer_busy",
        Conflict => "transfer_conflict",
        SourceChanged => "source_changed",
        HashMismatch => "hash_mismatch",
        DiskFull => "disk_full",
        Quota => "capacity",
        CommitUncertain => "commit_uncertain",
        LocalIo => "file_io_error",
        IdentityChanged => "identity_changed",
    }
}

pub(crate) fn stream_category(code: &marmish_stream::wire::WireErrorCode) -> &'static str {
    use marmish_stream::wire::WireErrorCode::*;
    match code {
        Unsupported => "copy_unsupported",
        Invalid => "preparation_failed",
        Denied => "authorization_denied",
        Conflict => "stream_conflict",
        Gap => "stream_gap",
        Expired => "stream_expired",
        Capacity => "capacity",
        Uncertain => "stream_uncertain",
        NotFound => "stream_not_found",
        Closed => "stream_closed",
    }
}
