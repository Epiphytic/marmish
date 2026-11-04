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
    #[error("operation deadline exceeded")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, Error>;
