use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf, time::Duration};
use zeroize::Zeroizing;

macro_rules! id {
    ($name:ident, $n:literal) => {
        #[derive(Clone, Copy, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
        pub struct $name(pub [u8; $n]);
    };
}
id!(PrincipalId, 32);
id!(EndpointId, 32);
id!(ServerId, 32);
id!(InvocationId, 16);
id!(JobId, 16);
id!(InputGeneration, 16);

#[derive(Clone, Debug)]
pub struct AuthorizedContext {
    pub(crate) principal: PrincipalId,
    pub(crate) endpoint: EndpointId,
    pub(crate) server: ServerId,
    pub(crate) target_uid: u32,
}
#[derive(Clone)]
pub struct ExecuteToken(Zeroizing<[u8; 32]>);
impl ExecuteToken {
    /// The caller must zeroize its exported copy and never log it.
    pub fn expose_bytes(&self) -> [u8; 32] {
        *self.0
    }
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }
}
impl fmt::Debug for ExecuteToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExecuteToken([REDACTED])")
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnginePolicy {
    pub state_root: PathBuf,
    pub worker_executable: PathBuf,
    pub server: ServerId,
    pub allowed_principals: Vec<PrincipalId>,
    pub allow_shell_command: bool,
    pub cwd: Option<PathBuf>,
    pub limits: EngineLimits,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineLimits {
    pub active_jobs: u32,
    pub invocation_records: u32,
    pub outstanding_tokens: u32,
    pub starting_workers: u32,
    pub token_lifetime: Duration,
    pub spool_bytes_per_stream: u64,
    pub aggregate_spool_bytes: u64,
    #[serde(with = "retention_encoding")]
    pub output_retention: marmish_core::Retention,
    pub tombstone_retention: Duration,
    pub cancel_grace: Duration,
    pub drain_grace: Duration,
}
impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            active_jobs: 32,
            invocation_records: 10_000,
            outstanding_tokens: 1_024,
            starting_workers: 4,
            token_lifetime: Duration::from_secs(60),
            spool_bytes_per_stream: 8 * 1024 * 1024,
            aggregate_spool_bytes: 512 * 1024 * 1024,
            output_retention: Default::default(),
            tombstone_retention: Duration::from_secs(48 * 3600),
            cancel_grace: Duration::from_secs(5),
            drain_grace: Duration::from_secs(2),
        }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandRequest {
    Exec { argv: Vec<Vec<u8>> },
    Shell { command: Vec<u8> },
}
impl fmt::Debug for CommandRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CommandRequest([REDACTED])")
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvocationState {
    Accepted,
    Launching,
    Running,
    Exited,
    UnknownLaunch,
    Lost,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitOutcome {
    Code(i32),
    Signal(i32),
    NotStarted,
}
#[derive(Clone, Debug)]
pub struct InvocationStatus {
    pub invocation: InvocationId,
    pub state: InvocationState,
    pub outcome: Option<ExitOutcome>,
    pub cancel_requested: bool,
    pub input: InputStatus,
    pub stdout: OutputStatus,
    pub stderr: OutputStatus,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputHealth {
    Open,
    EofPending,
    Closed,
    Gap,
    Uncertain,
}
#[derive(Clone, Debug)]
pub struct InputStatus {
    pub generation: Option<InputGeneration>,
    pub accepted: u64,
    pub delivered: u64,
    pub available_credit: u32,
    pub health: InputHealth,
    pub gap_count: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissingReason {
    Limit,
    StorageFailure,
    Expired,
    UncertainTail,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingRange {
    pub start: u64,
    pub end: Option<u64>,
    pub reason: MissingReason,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutputStatus {
    pub retained: u64,
    pub produced: u64,
    pub closed: bool,
    pub gaps: Vec<MissingRange>,
}
#[derive(Clone, Debug)]
pub struct OutputChunk {
    pub bytes: Vec<u8>,
    pub next_cursor: u64,
    pub status: OutputStatus,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitDisposition {
    Accepted,
    Existing,
}
#[derive(Clone, Debug)]
pub struct Submission {
    pub disposition: SubmitDisposition,
    pub status: InvocationStatus,
}
#[derive(Clone, Debug)]
pub struct InputReceipt {
    pub accepted: u64,
    pub available_credit: u32,
    pub duplicate: bool,
}
#[derive(Clone, Debug, Default)]
pub struct MaintenanceReport {
    pub records_removed: u32,
    pub spool_bytes_removed: u64,
}
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum EngineError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("root execution forbidden")]
    RootForbidden,
    #[error("invalid engine policy")]
    InvalidPolicy,
    #[error("unsafe state path")]
    UnsafePath,
    #[error("engine already open")]
    AlreadyOpen,
    #[error("invalid command request")]
    InvalidRequest,
    #[error("invocation not found")]
    NotFound,
    #[error("invocation content conflict")]
    InvocationConflict,
    #[error("invalid execute token")]
    InvalidToken,
    #[error("execute token expired")]
    ExpiredToken,
    #[error("engine quota exceeded")]
    QuotaExceeded,
    #[error("stale input generation")]
    StaleGeneration,
    #[error("input expired")]
    InputExpired,
    #[error("input conflict")]
    InputConflict,
    #[error("input closed")]
    InputClosed,
    #[error("input gap")]
    InputGap,
    #[error("worker unavailable")]
    WorkerUnavailable,
    #[error("recovery outcome uncertain")]
    RecoveryUncertain,
    #[error("storage failure")]
    StorageFailure,
    #[error("engine busy")]
    Busy,
    #[error("input backpressure")]
    InputBackpressure {
        next_offset: u64,
        available_credit: u32,
    },
}
pub type Result<T> = std::result::Result<T, EngineError>;

mod retention_encoding {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &marmish_core::Retention, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_some(&v.seconds())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<marmish_core::Retention, D::Error> {
        marmish_core::Retention::from_seconds(Option::<u64>::deserialize(d)?)
            .map_err(serde::de::Error::custom)
    }
}
