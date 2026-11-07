//! Local stream projections and bounded actor-owned live state.
use crate::stream_runner::RunnerHandle;
use marmish_stream::{codec::Codec, wire};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StreamLocalRequest {
    Inspect {},
    CopyReceiver {
        request: Box<crate::copy_local_gc::Request>,
    },
    Frame {
        frame_base64: String,
        relays: Vec<String>,
    },
    List {
        remote: Option<String>,
    },
    AdminCancel {
        stream: String,
    },
    AdminReconcile {
        stream: String,
        acknowledge_possible_live: bool,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StreamLocalResult {
    Inspection {
        snapshot: Box<StreamInspection>,
    },
    CopyReceiver {
        response: Box<crate::copy_local_gc::Response>,
    },
    Pending,
    Failure {
        category: String,
    },
    Frame {
        frame_base64: String,
    },
    Entries {
        entries: Vec<StreamEntry>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamEntry {
    pub stream: String,
    pub session: String,
    pub client: String,
    pub server: String,
    pub direction: String,
    pub state: String,
    pub open_digest: Option<String>,
    pub group_sha256: String,
    pub active: bool,
    pub cancel_requested: bool,
    pub terminal: Option<wire::Terminal>,
    #[serde(default)]
    pub child_disposition: Option<ChildDisposition>,
}
/// Private SAME-UID journal projection. Never log bindings without redaction.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamInspection {
    pub schema: u8,
    pub entries: Vec<StreamInspectionEntry>,
    pub records_truncated: bool,
    pub outbox_truncated: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamInspectionEntry {
    pub binding: Option<wire::Binding>,
    pub direction: Option<InspectionDirection>,
    /// Invalid if the record or any selected slot fails validation. A bad slot
    /// does not clear independently validated record progress below; inspect
    /// each slot's integrity before interpreting its projected fields.
    pub integrity: InspectionIntegrity,
    pub phase: Option<InspectionPhase>,
    pub server_hello_validated: Option<bool>,
    pub open_present: Option<bool>,
    pub active: Option<bool>,
    pub cancel_requested: Option<bool>,
    pub projection_invalidated: Option<bool>,
    pub record_sequence: Option<u64>,
    pub record_stdin_accepted: Option<u64>,
    pub record_stdin_delivered: Option<u64>,
    pub stdout_eof: Option<u64>,
    pub stderr_eof: Option<u64>,
    pub terminal: Option<wire::Terminal>,
    pub slots: Vec<InspectionSlot>,
    pub slots_truncated: bool,
    pub outbox: Vec<InspectionOutbox>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionSlot {
    /// Invalid slots retain only the cached-response presence flag; kind is
    /// Invalid and sequence, exchange and response projections are null.
    pub integrity: InspectionIntegrity,
    pub kind: InspectionKind,
    pub sequence: Option<u64>,
    pub response_cached: bool,
    pub exchange: Option<InspectionExchange>,
    pub response: Option<InspectionExchangeResponse>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionExchange {
    pub stdin_offset: Option<u64>,
    pub stdin_end: Option<u64>,
    pub stdin_eof: Option<u64>,
    pub stdout_ack: u64,
    pub stderr_ack: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionExchangeResponse {
    pub stdin_accepted: u64,
    pub stdin_delivered: u64,
    pub stdout_end: Option<u64>,
    pub stderr_end: Option<u64>,
    pub stdout_eof: Option<u64>,
    pub stderr_eof: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InspectionOutbox {
    pub bucket: InspectionPublicationState,
    pub count: u32,
    pub native_id_present: u32,
    pub intent_present: u32,
    pub evidence_present: u32,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InspectionDirection {
    Client,
    Server,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InspectionIntegrity {
    Valid,
    Invalid,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InspectionKind {
    Hello,
    Open,
    Exchange,
    Status,
    Cancel,
    CopyHello,
    OpenCopy,
    Other,
    Invalid,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InspectionPublicationState {
    Unsubmitted,
    Prepared,
    Queued,
    Published,
    Uncertain,
    Failed,
    Acked,
    RetiredFailed,
    Other,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InspectionPhase {
    Hello,
    Opening,
    Preparing,
    Launching,
    Running,
    Cancelling,
    Terminal,
    AuthorityRevoked,
    Lost,
    UnknownLaunch,
    TransportUncertain,
    ReconciledPossibleLive,
    CleanupUncertain,
    ServerStopping,
    ServerStopped,
    ProjectionInvalidated,
    Other,
}
/// Startup inspection of the recorded leader; never evidence of group cleanup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChildDisposition {
    Absent,
    PresentUnowned,
    Unknown,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StreamLaunch {
    Exec {
        argv: Vec<Vec<u8>>,
    },
    NativeCopy {
        launch: marmish_stream::copy_protocol::Launch,
    },
}
impl Default for StreamLaunch {
    fn default() -> Self {
        Self::Exec { argv: Vec::new() }
    }
}
impl StreamLaunch {
    pub fn clear_exec(&mut self) {
        if let Self::Exec { argv } = self {
            argv.clear();
        }
    }
    pub fn has_exec_payload(&self) -> bool {
        matches!(self, Self::Exec { argv } if !argv.is_empty())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StreamRecord {
    pub binding: wire::Binding,
    pub group: Vec<u8>,
    pub server_side: bool,
    #[serde(default)]
    pub hello: bool,
    #[serde(default)]
    pub copy_hello: bool,
    pub open_digest: Option<[u8; 32]>,
    pub launch: StreamLaunch,
    pub codec: Codec,
    pub state: String,
    pub cancel: bool,
    pub active: bool,
    pub terminal: Option<wire::Terminal>,
    pub identity: Option<Vec<u8>>,
    #[serde(default)]
    pub child_disposition: Option<ChildDisposition>,
    pub revision: u64,
    pub sequence: u64,
    #[serde(default)]
    pub stdin_accepted: u64,
    #[serde(default)]
    pub stdin_delivered: u64,
    #[serde(default)]
    pub stdout_eof: Option<u64>,
    #[serde(default)]
    pub stderr_eof: Option<u64>,
    #[serde(default)]
    pub completed_at: Option<u64>,
    #[serde(default)]
    pub last_exchange_digest: Option<[u8; 32]>,
    #[serde(default)]
    pub payload_drained: bool,
    #[serde(default)]
    pub payload_expired: bool,
    #[serde(default)]
    pub projection_invalidated: bool,
}
impl StreamRecord {
    pub fn key(&self) -> String {
        key(&self.group, &self.binding.stream)
    }
    pub fn entry(&self) -> StreamEntry {
        StreamEntry {
            stream: hex::encode(self.binding.stream),
            session: hex::encode(self.binding.session),
            client: hex::encode(self.binding.client),
            server: hex::encode(self.binding.server),
            direction: if self.server_side { "server" } else { "client" }.into(),
            state: self.state.clone(),
            open_digest: self.open_digest.map(hex::encode),
            group_sha256: {
                use sha2::{Digest, Sha256};
                hex::encode(Sha256::digest(&self.group))
            },
            active: self.active,
            cancel_requested: self.cancel,
            terminal: self.terminal,
            child_disposition: self.child_disposition,
        }
    }
}
pub(crate) fn key(group: &[u8], stream: &wire::Id) -> String {
    format!("{}:{}", hex::encode(group), hex::encode(stream))
}
pub(crate) struct LiveStream {
    pub runner: Option<RunnerHandle>,
    pub preparing: bool,
    pub operation_id: Option<[u8; 16]>,
    pub identity_committed: bool,
    pub revision: u64,
    pub data_rate: Rate,
    pub control_rate: Rate,
}
impl LiveStream {
    pub fn new() -> Self {
        Self {
            runner: None,
            preparing: false,
            operation_id: None,
            identity_committed: false,
            revision: 0,
            data_rate: Rate::new(8),
            control_rate: Rate::new(2),
        }
    }
}
pub(crate) struct StreamState {
    pub last_maintenance: Option<Instant>,
    pub stopping: bool,
    pub live: BTreeMap<String, LiveStream>,
    pub peer_rates: BTreeMap<[u8; 32], (Rate, Rate)>,
    pub data_rate: Rate,
    pub control_rate: Rate,
}
impl Default for StreamState {
    fn default() -> Self {
        Self {
            last_maintenance: None,
            stopping: false,
            live: BTreeMap::new(),
            peer_rates: BTreeMap::new(),
            data_rate: Rate::new(128),
            control_rate: Rate::new(32),
        }
    }
}
pub(crate) struct Rate {
    tokens: f64,
    maximum: u32,
    updated: Instant,
}
impl Rate {
    pub fn new(maximum: u32) -> Self {
        Self {
            tokens: f64::from(maximum),
            maximum,
            updated: Instant::now(),
        }
    }
    pub fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens
            + now.duration_since(self.updated).as_secs_f64() * f64::from(self.maximum))
        .min(f64::from(self.maximum));
        self.updated = now;
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }
}

/// Process cleanup observation, separate from filesystem receipt reclamation.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamCleanupStatus {
    pub active: u32,
    pub cancelling: u32,
    pub uncertain: u32,
    pub owned_runners: u32,
    pub unfinished_runners: u32,
    pub observation_pending: bool,
    pub cleanup_ready: bool,
    pub stop_complete: bool,
}
