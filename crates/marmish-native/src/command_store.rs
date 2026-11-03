//! Command ledger on the actor's existing SQLCipher connection.
//! No engine authority, network work or second connection is created here.
use super::{Journal, db, sql_u64};
use crate::command_evidence::PublishedEvidence;
use crate::command_wire::{
    self as wire, Account, Admission, Binding, Chunk, Digest, ErrorCode, Id, InputHealth, Op,
    Request, RequestOp, Response, State, Status, Submit, Warning, WireError,
};
use crate::mdk_adapter::{AuthenticatedCommandContext, CommandDirection};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;

const WEEK: u64 = 604800;
const ACTIVE_GLOBAL: usize = 32;
const ACTIVE_PRINCIPAL: usize = 4;
const RETAINED_GLOBAL: usize = 10000;
const RETAINED_PRINCIPAL: usize = 1024;
const SLOTS_GLOBAL: usize = 256;
const SLOTS_PRINCIPAL: usize = 64;
const MAX_SOURCES: usize = 16;
const MAX_BINDINGS: usize = 2;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationKey {
    pub server: Account,
    pub principal: Account,
    pub invocation: Id,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OperationId(pub [u8; 16]);
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Owner {
    group: Vec<u8>,
    transport: Vec<u8>,
    binding: Binding,
    uid: u32,
    authority: Digest,
    server_side: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerDisk {
    group: Vec<u8>,
    transport: Vec<u8>,
    session: Id,
    client: Account,
    server: Account,
    uid: u32,
    authority: Digest,
    server_side: bool,
}
impl Owner {
    pub(crate) fn from_context(ctx: &AuthenticatedCommandContext) -> Self {
        let server_side = ctx.direction() == CommandDirection::Server;
        let (client, server) = if server_side {
            (ctx.peer_account(), ctx.local_account())
        } else {
            (ctx.local_account(), ctx.peer_account())
        };
        Self {
            group: ctx.mls_group().to_vec(),
            transport: ctx.transport_group().to_vec(),
            binding: Binding {
                session: ctx.session(),
                client,
                server,
            },
            uid: ctx.target_uid(),
            authority: ctx.authority_digest(),
            server_side,
        }
    }
    pub(crate) fn group(&self) -> &[u8] {
        &self.group
    }
    pub(crate) fn binding(&self) -> &Binding {
        &self.binding
    }
    pub(crate) fn uid(&self) -> u32 {
        self.uid
    }
    pub(crate) fn server_side(&self) -> bool {
        self.server_side
    }
    fn key(&self, invocation: Id) -> InvocationKey {
        InvocationKey {
            server: self.binding.server,
            principal: self.binding.client,
            invocation,
        }
    }
    fn local(&self) -> Account {
        if self.server_side {
            self.binding.server
        } else {
            self.binding.client
        }
    }
    fn encode(&self) -> Result<Vec<u8>> {
        encode(&OwnerDisk {
            group: self.group.clone(),
            transport: self.transport.clone(),
            session: self.binding.session,
            client: self.binding.client,
            server: self.binding.server,
            uid: self.uid,
            authority: self.authority,
            server_side: self.server_side,
        })
    }
    fn from_ledger(record: ValidatedOwnerRecord) -> Self {
        record.0
    }
}
struct ValidatedOwnerRecord(Owner);
fn decode_owner(bytes: &[u8], key: &InvocationKey) -> Result<Owner> {
    let disk: OwnerDisk = decode(bytes)?;
    if disk.group.is_empty()
        || disk.group.len() > 1024
        || disk.transport.is_empty()
        || disk.transport.len() > 1024
        || disk.client != key.principal
        || disk.server != key.server
        || disk.client == disk.server
        || disk.uid != unsafe { libc::geteuid() }
    {
        return Err(invalid("command_owner"));
    }
    Ok(Owner::from_ledger(ValidatedOwnerRecord(Owner {
        group: disk.group,
        transport: disk.transport,
        binding: Binding {
            session: disk.session,
            client: disk.client,
            server: disk.server,
        },
        uid: disk.uid,
        authority: disk.authority,
        server_side: disk.server_side,
    })))
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum FeederState {
    NeverBegun,
    Beginning,
    Ready {
        generation: [u8; 16],
        credit: u64,
    },
    WriteIntended {
        generation: [u8; 16],
        offset: u64,
        digest: Digest,
    },
    Accepted {
        generation: [u8; 16],
        offset: u64,
        credit: u64,
    },
    EofIntended {
        generation: [u8; 16],
        offset: u64,
    },
    Complete,
    Failed,
}
#[derive(Clone)]
pub(crate) struct OperationLedgerSnapshot {
    pub stdin_length: u32,
    pub feeder: FeederState,
    pub warning: Option<Warning>,
    pub authority_valid: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum EngineOperation {
    Observe,
    FinalObserve,
    Dispatch {
        submit: Submit,
    },
    BeginInput,
    WriteInput {
        generation: [u8; 16],
        offset: u64,
        bytes: Vec<u8>,
    },
    Eof {
        generation: [u8; 16],
        offset: u64,
    },
    Poll {
        stdout_cursor: u64,
        stderr_cursor: u64,
        max_bytes: u32,
    },
    Cancel,
}
#[derive(Clone)]
pub(crate) struct OperationRecord {
    pub id: OperationId,
    pub key: InvocationKey,
    pub owner: Owner,
    pub revision: u64,
    pub engine_known: bool,
    pub ledger: OperationLedgerSnapshot,
    pub operation: EngineOperation,
    pub request_id: Option<Id>,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkerState {
    Running,
    Exited,
    Unknown,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationalFailure {
    Unavailable,
    #[cfg(test)]
    Timeout,
    Storage,
    Uncertain,
}
#[derive(Clone)]
pub(crate) enum EngineObservation {
    Missing,
    Found {
        status: Status,
        worker: WorkerState,
    },
    Dispatched {
        status: Status,
        worker: WorkerState,
    },
    Input {
        status: Status,
        generation: Option<[u8; 16]>,
        credit: u64,
        worker: WorkerState,
    },
    Polled {
        status: Status,
        stdout: Chunk,
        stderr: Chunk,
        worker: WorkerState,
    },
    Refused(WireError),
    Failed(OperationalFailure),
}
pub(crate) struct EngineCompletion {
    pub operation: OperationId,
    pub key: InvocationKey,
    pub revision: u64,
    pub result: EngineObservation,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommittedDecision {
    KnownFound,
    DefinitelyUnsubmitted,
    DefinitivelyRefused,
    StillUncertain,
}
pub(crate) struct CommitAck {
    pub operation: OperationId,
    pub key: InvocationKey,
    pub revision: u64,
    pub decision: CommittedDecision,
    pub answered: Vec<Id>,
    pub suppressed: Vec<Id>,
}
#[derive(Clone)]
pub(crate) struct InvocationRecord {
    pub key: InvocationKey,
    pub owner: Owner,
    pub submit: Option<Submit>,
    pub stdin_length: u32,
    pub digest: Digest,
    pub admission: Admission,
    pub revision: u64,
    pub engine_known: bool,
    pub status: Option<Status>,
    pub refusal: Option<WireError>,
    pub feeder: FeederState,
    pub authority_valid: bool,
    pub warning: Option<Warning>,
    pub worker: WorkerState,
    pub observed_seq: u64,
    pub cursor: u64,
    pub uncertain: bool,
}
impl InvocationRecord {
    pub(crate) fn ledger(&self) -> OperationLedgerSnapshot {
        OperationLedgerSnapshot {
            stdin_length: self.stdin_length,
            feeder: self.feeder.clone(),
            warning: self.warning,
            authority_valid: self.authority_valid,
        }
    }
    fn payload(&self) -> Result<&Submit> {
        self.submit
            .as_ref()
            .ok_or(Error::Storage("command_payload_expired"))
    }
    fn payload_required(&self) -> bool {
        self.owner.server_side
            && ((!self.engine_known && self.refusal.is_none())
                || !matches!(self.feeder, FeederState::Complete | FeederState::Failed))
    }
    pub(crate) fn active(&self) -> bool {
        self.refusal.is_none()
            && (self.uncertain
                || !self.engine_known
                || self
                    .status
                    .as_ref()
                    .is_none_or(|s| s.state != State::Exited)
                || !matches!(self.feeder, FeederState::Complete | FeederState::Failed))
    }
}
pub(crate) struct RecoveryRecord {
    pub key: InvocationKey,
    pub engine_known: bool,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PublicationKey {
    pub group: Vec<u8>,
    pub app_event_id: String,
}
#[derive(Clone)]
pub(crate) struct CommandSource {
    pub group: Vec<u8>,
    pub native_id: Vec<u8>,
    pub app_event_id: String,
    pub author: Account,
    pub authority_metadata: Vec<u8>,
}
#[derive(Clone)]
pub(crate) struct PreparedCommandPublication {
    pub key: PublicationKey,
    pub binding: Binding,
    pub request_id: Id,
    pub request_sha256: Digest,
    pub encoded_wire: Vec<u8>,
    pub encoded_app_event: Vec<u8>,
}
impl PreparedCommandPublication {
    pub(crate) fn validate(&self, local: Account) -> Result<()> {
        crate::mdk_adapter::validate_command_publication(self, local)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublicationState {
    Unsubmitted,
    Prepared,
    #[cfg(test)]
    Published,
    #[cfg(test)]
    Invalidated,
}
impl PublicationState {
    fn text(self) -> &'static str {
        match self {
            Self::Unsubmitted => "unsubmitted",
            Self::Prepared => "prepared",
            #[cfg(test)]
            Self::Published => "published",
            #[cfg(test)]
            Self::Invalidated => "invalidated",
        }
    }
}
pub(crate) enum CommandNativeUpdate {
    Queued {
        key: PublicationKey,
        intent: Vec<u8>,
    },
    Published {
        key: PublicationKey,
        native_id: Vec<u8>,
        evidence: PublishedEvidence,
    },
    Failed {
        key: PublicationKey,
    },
    RefusedQueued {
        group: Vec<u8>,
        intent: Vec<u8>,
    },
    Invalidated {
        group: Vec<u8>,
        native_id: Vec<u8>,
        possible_publication: Option<PublicationKey>,
    },
}
#[derive(Clone)]
pub(crate) enum AdmissionDecision {
    Existing,
    New,
    Refused(WireError),
}
pub(crate) struct CommandProjection {
    pub owner: Owner,
    pub source: CommandSource,
    pub request: Request,
    pub decision: AdmissionDecision,
    pub response: Option<PreparedCommandPublication>,
}
pub(crate) struct ProjectedRequest {
    #[cfg(test)]
    pub key: InvocationKey,
    #[cfg(test)]
    pub existing: bool,
}
pub(crate) struct ProjectedResponse {
    #[cfg(test)]
    pub existing: bool,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PublishedKey {
    pub key: PublicationKey,
    pub native_id: Vec<u8>,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SourceKey {
    pub group: Vec<u8>,
    pub native_id: Vec<u8>,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SourceKind {
    Incoming,
    Outgoing,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceCursor {
    pub kind: SourceKind,
    pub row_id: u64,
}
pub(crate) struct NativeBinding {
    pub cursor: SourceCursor,
    pub source: SourceKey,
    pub publication: Option<PublicationKey>,
    pub acked: bool,
    pub invalidated: bool,
    pub quarantined: bool,
    pub evidence: Option<PublishedEvidence>,
}
pub(crate) struct SourcePage {
    pub rows: Vec<NativeBinding>,
    pub next: Option<SourceCursor>,
}
#[derive(Default)]
pub(crate) struct CommandProtection {
    pub invalidation_groups: BTreeSet<Vec<u8>>,
    pub groups: BTreeSet<Vec<u8>>,
    pub publications: BTreeSet<PublicationKey>,
    pub queued: BTreeSet<(Vec<u8>, Vec<u8>)>,
}

fn invalid(category: &'static str) -> Error {
    Error::Invalid(category)
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| invalid("command_storage_encoding"))
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > 65536 {
        return Err(invalid("command_storage_encoding"));
    }
    serde_json::from_slice(bytes).map_err(|_| invalid("command_storage_encoding"))
}
fn bound(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| invalid("command_counter"))
}
fn limit(value: usize) -> Result<i64> {
    if !(1..=64).contains(&value) {
        return Err(invalid("command_page_limit"));
    }
    Ok(value as i64)
}
fn key_bytes(key: &InvocationKey) -> Vec<u8> {
    [
        key.server.as_slice(),
        key.principal.as_slice(),
        key.invocation.as_slice(),
    ]
    .concat()
}
fn sha(bytes: &[u8]) -> Digest {
    Sha256::digest(bytes).into()
}
fn blob32(bytes: Vec<u8>) -> Result<Digest> {
    bytes
        .try_into()
        .map_err(|_| invalid("command_storage_encoding"))
}

#[derive(Clone, Copy)]
pub(crate) struct CommandClientFinishState {
    pub confirmed: bool,
    #[cfg(test)]
    pub attempts: u8,
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FinishFacts {
    client_finished: bool,
    fingerprint: Option<Digest>,
    cursors: Option<(u64, u64)>,
    attempts: u8,
    latest_request: Option<Id>,
    #[serde(default)]
    latest_busy: bool,
    #[serde(default)]
    confirmed: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Facts {
    admission: Admission,
    revision: u64,
    engine_known: bool,
    status: Option<Status>,
    refusal: Option<WireError>,
    feeder: FeederState,
    authority_valid: bool,
    warning: Option<Warning>,
    worker: WorkerState,
    observed_seq: u64,
    uncertain: bool,
    #[serde(default)]
    finish: FinishFacts,
}
impl Facts {
    fn new(refusal: Option<WireError>) -> Self {
        Self {
            admission: Admission::Accepted,
            revision: 0,
            engine_known: false,
            status: None,
            feeder: if refusal.is_some() {
                FeederState::Failed
            } else {
                FeederState::NeverBegun
            },
            refusal,
            authority_valid: true,
            warning: None,
            worker: WorkerState::Unknown,
            observed_seq: 0,
            uncertain: false,
            finish: FinishFacts::default(),
        }
    }
    fn from_record(r: &InvocationRecord) -> Self {
        Self {
            admission: r.admission,
            revision: r.revision,
            engine_known: r.engine_known,
            status: r.status.clone(),
            refusal: r.refusal.clone(),
            feeder: r.feeder.clone(),
            authority_valid: r.authority_valid,
            warning: r.warning,
            worker: r.worker,
            observed_seq: r.observed_seq,
            uncertain: r.uncertain,
            finish: FinishFacts::default(),
        }
    }
}
impl Journal {
    pub(crate) fn command_init(&mut self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS command_replay_state(id INTEGER PRIMARY KEY CHECK(id=1),floor INTEGER NOT NULL CHECK(floor>=0),sequence INTEGER NOT NULL CHECK(sequence>=0),admitted INTEGER NOT NULL DEFAULT 0,finished INTEGER NOT NULL DEFAULT 0 CHECK(finished>=0),payload_cursor INTEGER NOT NULL DEFAULT 0);
        INSERT OR IGNORE INTO command_replay_state(id,floor,sequence) VALUES(1,0,0);
        CREATE TABLE IF NOT EXISTS command_settings(id INTEGER PRIMARY KEY CHECK(id=1),settings BLOB NOT NULL);
        CREATE TABLE IF NOT EXISTS command_invocations(cursor INTEGER PRIMARY KEY AUTOINCREMENT,k BLOB NOT NULL UNIQUE,server BLOB NOT NULL,principal BLOB NOT NULL,invocation BLOB NOT NULL,owner BLOB NOT NULL,submit BLOB,stdin_length INTEGER NOT NULL CHECK(stdin_length BETWEEN 0 AND 16384),stdout_delivered INTEGER NOT NULL DEFAULT 0,stderr_delivered INTEGER NOT NULL DEFAULT 0,digest BLOB NOT NULL,facts BLOB NOT NULL,active INTEGER NOT NULL,created INTEGER NOT NULL,operation BLOB,operation_revision INTEGER,operation_kind TEXT,operation_request BLOB,server_side INTEGER NOT NULL,submit_request_id BLOB,submit_request_digest BLOB,CHECK(server_side=1 OR (submit_request_id IS NOT NULL AND length(submit_request_id)=16 AND submit_request_digest IS NOT NULL AND length(submit_request_digest)=32)));
        CREATE INDEX IF NOT EXISTS command_active ON command_invocations(server_side,active,principal);
        CREATE TABLE IF NOT EXISTS command_requests(group_id BLOB NOT NULL,request_id BLOB NOT NULL,principal BLOB NOT NULL,k BLOB NOT NULL,owner BLOB NOT NULL,authority_valid INTEGER NOT NULL DEFAULT 1,request BLOB NOT NULL,digest BLOB NOT NULL,op TEXT NOT NULL,response_id TEXT,client INTEGER NOT NULL,projected INTEGER NOT NULL DEFAULT 0,received_response BLOB,response_digest BLOB,local_consumed INTEGER NOT NULL DEFAULT 0,slot_state TEXT NOT NULL DEFAULT 'pending',failed_attempts INTEGER NOT NULL DEFAULT 0,slot_operation BLOB,slot_revision INTEGER,created INTEGER NOT NULL,PRIMARY KEY(group_id,request_id));
        CREATE TABLE IF NOT EXISTS command_publications(group_id BLOB NOT NULL,event_id TEXT NOT NULL,request_id BLOB NOT NULL,request_digest BLOB NOT NULL,session BLOB NOT NULL,client BLOB NOT NULL,server BLOB NOT NULL,wire BLOB NOT NULL,native BLOB NOT NULL,state TEXT NOT NULL,PRIMARY KEY(group_id,event_id));
        CREATE TABLE IF NOT EXISTS command_incoming_sources(row_id INTEGER PRIMARY KEY AUTOINCREMENT,group_id BLOB NOT NULL,native_id BLOB NOT NULL,request_id BLOB NOT NULL,event_id TEXT NOT NULL,author BLOB NOT NULL,authority BLOB NOT NULL,acked INTEGER NOT NULL DEFAULT 0,UNIQUE(group_id,native_id));
        CREATE TABLE IF NOT EXISTS command_outgoing_sources(row_id INTEGER PRIMARY KEY AUTOINCREMENT,group_id BLOB NOT NULL,native_id BLOB NOT NULL,event_id TEXT NOT NULL,acked INTEGER NOT NULL DEFAULT 0,invalidated INTEGER NOT NULL DEFAULT 0,quarantined INTEGER NOT NULL DEFAULT 0,evidence BLOB NOT NULL,UNIQUE(group_id,native_id));
        CREATE TABLE IF NOT EXISTS command_early_invalidations(group_id BLOB NOT NULL,native_id BLOB NOT NULL,PRIMARY KEY(group_id,native_id));
        CREATE TABLE IF NOT EXISTS command_queued_bindings(group_id BLOB NOT NULL,intent BLOB NOT NULL,event_id TEXT NOT NULL,PRIMARY KEY(group_id,intent));") .map_err(db)?;
        let columns: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("PRAGMA table_info(command_replay_state)")
                .map_err(db)?;
            stmt.query_map([], |row| row.get(1))
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?
        };
        if !columns.iter().any(|name| name == "finished") {
            let tx = self.conn.transaction().map_err(db)?;
            tx.execute_batch("ALTER TABLE command_replay_state ADD COLUMN finished INTEGER NOT NULL DEFAULT 0 CHECK(finished>=0)").map_err(db)?;
            tx.commit().map_err(db)?;
        }
        // Schemas 3/4 predate durable local Submit-handle metadata. Nullable
        // additions preserve old rows without guessing an original request ID.
        let columns: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("PRAGMA table_info(command_invocations)")
                .map_err(db)?;
            stmt.query_map([], |row| row.get(1))
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?
        };
        let tx = self.conn.transaction().map_err(db)?;
        if !columns.iter().any(|n| n == "submit_request_id") {
            tx.execute_batch("ALTER TABLE command_invocations ADD COLUMN submit_request_id BLOB CHECK(submit_request_id IS NULL OR length(submit_request_id)=16)").map_err(db)?;
        }
        if !columns.iter().any(|n| n == "submit_request_digest") {
            tx.execute_batch("ALTER TABLE command_invocations ADD COLUMN submit_request_digest BLOB CHECK(submit_request_digest IS NULL OR length(submit_request_digest)=32)").map_err(db)?;
        }
        tx.commit().map_err(db)?;
        // A new engine frontend cannot resume an old input generation. Preserve
        // uncertain launch facts; only the feeder is failed, never replayed.
        let keys = self.command_keys("WHERE server_side=1 AND active=1", ACTIVE_GLOBAL + 1)?;
        if keys.len() > ACTIVE_GLOBAL {
            return Err(invalid("command_recovery_capacity"));
        }
        self.conn
            .execute(
                "INSERT OR IGNORE INTO command_settings(id,settings) VALUES(1,?1)",
                [encode(&StoreLimits::default())?],
            )
            .map_err(db)?;
        let tx = self.conn.transaction().map_err(db)?;
        for key in keys {
            let mut r = load_record(&tx, &key)?.ok_or(invalid("command_record"))?;
            if !matches!(
                r.feeder,
                FeederState::NeverBegun | FeederState::Complete | FeederState::Failed
            ) {
                r.feeder = FeederState::Failed;
                r.warning = Some(Warning::InputFailed);
                r.revision = increment(r.revision)?;
                save_record(&tx, &r)?;
            }
        }
        // Registrations belong to the dead frontend. Recovery snapshots retain
        // dispatch uncertainty, preventing a second Submit on a missing result.
        tx.execute("UPDATE command_invocations SET operation=NULL,operation_revision=NULL,operation_kind=NULL,operation_request=NULL WHERE operation_request IS NULL",[]).map_err(db)?;
        tx.commit().map_err(db)
    }
    pub(crate) fn command_floor(&self) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT floor FROM command_replay_state WHERE id=1",
                [],
                |r| sql_u64(r, 0),
            )
            .map_err(db)
    }
    pub(crate) fn command_get(&self, key: &InvocationKey) -> Result<Option<InvocationRecord>> {
        load_record(&self.conn, key)
    }
    fn command_keys(&self, clause: &str, max: usize) -> Result<Vec<InvocationKey>> {
        let sql = format!(
            "SELECT server,principal,invocation FROM command_invocations {clause} ORDER BY cursor LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(db)?;
        let rows = stmt
            .query_map([max as i64], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(db)?;
        rows.map(|row| {
            let (s, p, i) = row.map_err(db)?;
            Ok(InvocationKey {
                server: blob32(s)?,
                principal: blob32(p)?,
                invocation: i.try_into().map_err(|_| invalid("command_record"))?,
            })
        })
        .collect()
    }
    pub(crate) fn command_recovery(&self) -> Result<Vec<RecoveryRecord>> {
        let keys = self.command_keys("WHERE server_side=1 AND active=1", ACTIVE_GLOBAL + 1)?;
        if keys.len() > ACTIVE_GLOBAL {
            return Err(invalid("command_recovery_capacity"));
        }
        keys.into_iter()
            .map(|key| {
                let r = self.command_get(&key)?.ok_or(invalid("command_record"))?;
                Ok(RecoveryRecord {
                    key: r.key,
                    engine_known: r.engine_known,
                })
            })
            .collect()
    }
    pub(crate) fn command_historical_principals(&self) -> Result<Vec<Account>> {
        let mut stmt=self.conn.prepare("SELECT DISTINCT principal FROM command_invocations WHERE server_side=1 ORDER BY principal LIMIT 4097").map_err(db)?;
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0)).map_err(db)?;
        let result: Result<Vec<Account>> = rows.map(|r| blob32(r.map_err(db)?)).collect();
        let result = result?;
        if result.len() > 4096 {
            return Err(invalid("command_principals_capacity"));
        }
        Ok(result)
    }
    pub(crate) fn command_project_request(
        &mut self,
        plan: &CommandProjection,
        now: u64,
    ) -> Result<ProjectedRequest> {
        if !plan.owner.server_side || plan.source.author != plan.owner.binding.client {
            return Err(invalid("command_owner"));
        }
        validate_source(&plan.source, &plan.owner)?;
        let bytes = wire::encode_request(&plan.request)?;
        check_request_owner(&plan.request, &plan.owner)?;
        if let Some(response) = &plan.response {
            response.validate(plan.owner.local())?;
            validate_response_for(response, &plan.owner, &plan.request, &bytes)?;
        }
        let key = plan.owner.key(plan.request.op.invocation());
        let tx = self.conn.transaction().map_err(db)?;
        let existing = check_slot(&tx, &plan.owner, &plan.request, &bytes)?;
        let record = load_record(&tx, &key)?;
        if existing && plan.response.is_some() {
            return Err(invalid("command_duplicate_response"));
        }
        if !existing
            && plan.response.is_none()
            && (!matches!(plan.decision, AdmissionDecision::Existing)
                || matches!(plan.request.op, RequestOp::Submit(_))
                || record.as_ref().is_none_or(|r| !r.authority_valid))
        {
            return Err(invalid("command_pending_response"));
        }
        if let RequestOp::Submit(submit) = &plan.request.op {
            let digest = wire::submit_digest(plan.owner.group(), plan.owner.binding(), submit)?;
            if let Some(r) = &record {
                check_record_owner(r, &plan.owner)?;
                if r.digest != digest
                    && !existing
                    && !matches!(
                        &plan.decision,
                        AdmissionDecision::Refused(WireError {
                            code: ErrorCode::InvocationConflict,
                            detail: None,
                        })
                    )
                {
                    return Err(invalid("command_invocation_conflict"));
                }
            } else {
                if matches!(
                    &plan.decision,
                    AdmissionDecision::Refused(WireError {
                        code: ErrorCode::InvocationConflict,
                        ..
                    })
                ) {
                    return Err(invalid("command_invocation_conflict"));
                }
                freshness(&tx, submit.invocation, now)?;
                if matches!(plan.decision, AdmissionDecision::Existing) {
                    return Err(invalid("command_not_found"));
                }
                let refusal = match &plan.decision {
                    AdmissionDecision::Refused(e) => Some(e.clone()),
                    _ => None,
                };
                if refusal.as_ref().is_some_and(|e| {
                    matches!(
                        e.code,
                        ErrorCode::Busy
                            | ErrorCode::FutureDeferred
                            | ErrorCode::ClockBehindFloor
                            | ErrorCode::StorageFailure
                            | ErrorCode::TransportDegraded
                    )
                }) {
                    return Err(invalid("command_transient_refusal"));
                }
                insert_invocation(&tx, &plan.owner, submit, digest, refusal, now)?;
            }
        } else {
            if let Some(r) = record.as_ref() {
                check_record_owner(r, &plan.owner)?;
            } else if !existing
                && (!matches!(plan.decision, AdmissionDecision::Refused(_))
                    || plan.response.is_none())
            {
                return Err(invalid("command_not_found"));
            }
        }
        if let AdmissionDecision::Refused(error) = &plan.decision
            && !existing
        {
            let response = plan
                .response
                .as_ref()
                .ok_or(invalid("command_refusal_response"))?;
            if wire::decode_response(&response.encoded_wire)?.result != Err(error.clone()) {
                return Err(invalid("command_refusal_response"));
            }
        }
        if !existing {
            freshness(&tx, plan.request.id, now)?;
            insert_slot(&tx, &plan.owner, &plan.request, &bytes, false, now)?;
        }
        if !existing && matches!(plan.request.op, RequestOp::Finish { .. }) {
            let result = finish_result(&tx, &plan.owner, &plan.request)?;
            let response = plan
                .response
                .as_ref()
                .ok_or(invalid("command_finish_response"))?;
            if wire::decode_response(&response.encoded_wire)?.result != result {
                return Err(invalid("command_finish_response"));
            }
            if result == Ok(wire::ResponseResult::Finish { finished: true }) {
                commit_finished(&tx, &plan.owner, &plan.request)?;
            }
        }
        attach_source(&tx, &plan.source, plan.request.id)?;
        if let Some(response) = &plan.response {
            insert_publication(&tx, response)?;
            tx.execute("UPDATE command_requests SET response_id=?3,slot_state='answered' WHERE group_id=?1 AND request_id=?2",params![plan.owner.group(),plan.request.id.as_slice(),response.key.app_event_id]).map_err(db)?;
        }
        tx.commit().map_err(db)?;
        Ok(ProjectedRequest {
            #[cfg(test)]
            key,
            #[cfg(test)]
            existing,
        })
    }
    pub(crate) fn command_submit_client(
        &mut self,
        owner: &Owner,
        request: &Request,
        publication: &PreparedCommandPublication,
        now: u64,
    ) -> Result<InvocationRecord> {
        if owner.server_side {
            return Err(invalid("command_owner"));
        }
        check_request_owner(request, owner)?;
        publication.validate(owner.local())?;
        let bytes = wire::encode_request(request)?;
        if publication.key.group != owner.group
            || publication.binding != owner.binding
            || publication.request_id != request.id
            || publication.request_sha256 != wire::request_digest(&bytes)
            || publication.encoded_wire != bytes
        {
            return Err(invalid("command_publication_binding"));
        }
        let key = owner.key(request.op.invocation());
        let tx = self.conn.transaction().map_err(db)?;
        let existing = check_slot(&tx, owner, request, &bytes)?;
        let current = load_record(&tx, &key)?;
        if let RequestOp::Submit(submit) = &request.op {
            let digest = wire::submit_digest(owner.group(), owner.binding(), submit)?;
            if let Some(r) = &current {
                check_record_owner(r, owner)?;
                if r.digest != digest {
                    return Err(invalid("command_invocation_conflict"));
                }
                check_original_submit(&tx, &key, request.id)?;
            } else {
                freshness(&tx, submit.invocation, now)?;
                insert_invocation_with_client_request(
                    &tx,
                    owner,
                    submit,
                    digest,
                    None,
                    InvocationAdmission {
                        now,
                        client_request: Some((request.id, wire::request_digest(&bytes))),
                    },
                )?;
            }
        } else {
            check_record_owner(current.as_ref().ok_or(invalid("command_not_found"))?, owner)?;
        }
        if !existing {
            if let RequestOp::Poll {
                stdout_cursor,
                stderr_cursor,
                ..
            } = &request.op
            {
                let pair = delivery(&tx, &key)?;
                if pair.stdout != *stdout_cursor || pair.stderr != *stderr_cursor {
                    return Err(invalid("command_cursor_stale"));
                }
            }
            freshness(&tx, request.id, now)?;
            insert_slot(&tx, owner, request, &bytes, true, now)?;
        }
        insert_publication(&tx, publication)?;
        tx.commit().map_err(db)?;
        self.command_get(&key)?.ok_or(invalid("command_record"))
    }
    pub(crate) fn command_project_response(
        &mut self,
        source: &CommandSource,
        response: &Response,
        now: u64,
    ) -> Result<ProjectedResponse> {
        let tx = self.conn.transaction().map_err(db)?;
        type ClientSlot = (Vec<u8>, Vec<u8>, Vec<u8>, i64);
        let stored:Option<ClientSlot>=tx.query_row("SELECT k,request,digest,projected FROM command_requests WHERE group_id=?1 AND request_id=?2 AND client=1",params![source.group,response.id.as_slice()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db)?;
        let (encoded_key, request_bytes, digest, projected) =
            stored.ok_or(invalid("command_not_found"))?;
        let response_bytes = wire::encode_response(response)?;
        let previous:Option<Vec<u8>>=tx.query_row("SELECT received_response FROM command_requests WHERE group_id=?1 AND request_id=?2",params![source.group,response.id.as_slice()],|r|r.get(0)).map_err(db)?;
        if previous
            .as_ref()
            .is_some_and(|bytes| *bytes != response_bytes)
        {
            return Err(invalid("command_response_conflict"));
        }
        let request = wire::decode_request(&request_bytes)?;
        let key = key_from_bytes(&encoded_key)?;
        let mut record = load_record(&tx, &key)?.ok_or(invalid("command_record"))?;
        validate_source(source, &record.owner)?;
        if source.author != record.owner.binding.server
            || response.binding != record.owner.binding
            || response.op != request.op.op()
            || response.request_sha256.as_slice() != digest
            || response.id != request.id
        {
            return Err(invalid("command_response_binding"));
        }
        wire::encode_response(response)?;
        // Response request association, not response arrival time, defines replay.
        let _ = now;
        attach_source(&tx, source, response.id)?;
        if projected != 0 {
            tx.commit().map_err(db)?;
            return Ok(ProjectedResponse {
                #[cfg(test)]
                existing: true,
            });
        }
        if let Ok(result) = &response.result {
            if let Some(status) = result.status() {
                if status.invocation != key.invocation {
                    return Err(invalid("command_response_binding"));
                }
                record.engine_known |= status.engine_known;
                let mut status = status.clone();
                status.engine_known = record.engine_known;
                record.authority_valid &= status.authority_valid;
                status.authority_valid = record.authority_valid;
                record.admission = if record.engine_known {
                    Admission::Engine
                } else {
                    status.admission
                };
                status.admission = record.admission;
                record.status = Some(status.clone());
                record.warning = status.warning;
                record.feeder = if status.input.eof_sent {
                    FeederState::Complete
                } else if matches!(
                    status.input.health,
                    InputHealth::Gap | InputHealth::Uncertain
                ) {
                    FeederState::Failed
                } else {
                    record.feeder
                };
            }
        } else if let Err(error) = &response.result
            && response.op == Op::Submit
        {
            record.refusal = Some(error.clone());
        }
        if !record.authority_valid {
            revoke_slots(&tx, &record.key)?;
        }
        record.revision = increment(record.revision)?;
        save_record(&tx, &record)?;
        if response.op == Op::Finish {
            let mut f = facts(&tx, &record.key)?;
            if f.finish.latest_request == Some(response.id) {
                f.finish.confirmed |= matches!(
                    response.result,
                    Ok(wire::ResponseResult::Finish { finished: true })
                );
                f.finish.latest_busy = matches!(
                    response.result,
                    Err(WireError {
                        code: ErrorCode::Busy,
                        ..
                    })
                );
                tx.execute(
                    "UPDATE command_invocations SET facts=?2 WHERE k=?1",
                    params![key_bytes(&record.key), encode(&f)?],
                )
                .map_err(db)?;
            }
        }
        tx.execute("UPDATE command_requests SET projected=1,received_response=?3,response_digest=?4,slot_state='answered' WHERE group_id=?1 AND request_id=?2",params![source.group,response.id.as_slice(),response_bytes,sha(&response_bytes).as_slice()]).map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(ProjectedResponse {
            #[cfg(test)]
            existing: false,
        })
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ClientDelivery {
    pub stdout: u64,
    pub stderr: u64,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientSlotState {
    Absent,
    Pending,
    Answered,
    Consumed,
}
pub(crate) struct ClientInvocation {
    pub cursor: u64,
    pub owner: Owner,
    pub invocation: Id,
    #[cfg(test)]
    pub submit_request_id: Id,
    pub submit_request_sha256: Digest,
    pub group_submit_sha256: Digest,
    pub created: u64,
    pub admission: Admission,
    pub state: Option<State>,
    #[cfg(test)]
    pub engine_known: bool,
    pub authority_valid: bool,
    pub refusal: Option<ErrorCode>,
}
pub(crate) struct ClientInvocationPage {
    pub entries: Vec<ClientInvocation>,
    pub next: Option<u64>,
}
fn check_original_submit(conn: &Connection, key: &InvocationKey, id: Id) -> Result<()> {
    let original: Option<Vec<u8>> = conn
        .query_row(
            "SELECT submit_request_id FROM command_invocations WHERE k=?1",
            [key_bytes(key)],
            |r| r.get(0),
        )
        .map_err(db)?;
    let original = original.ok_or(invalid("command_legacy_handle_unavailable"))?;
    if original != id {
        return Err(invalid("command_submit_request_conflict"));
    }
    Ok(())
}

fn client_invocation(conn: &Connection, record: InvocationRecord) -> Result<ClientInvocation> {
    let (request,digest,created): (Option<Vec<u8>>,Option<Vec<u8>>,u64) = conn.query_row("SELECT submit_request_id,submit_request_digest,created FROM command_invocations WHERE k=?1 AND server_side=0", [key_bytes(&record.key)], |r|Ok((r.get(0)?,r.get(1)?,sql_u64(r,2)?))).map_err(db)?;
    let request = request.ok_or(invalid("command_legacy_handle_unavailable"))?;
    let digest = digest.ok_or(invalid("command_legacy_handle_unavailable"))?;
    if request.len() != 16 {
        return Err(invalid("command_record"));
    }
    #[cfg(test)]
    let submit_request_id = request.try_into().map_err(|_| invalid("command_record"))?;
    Ok(ClientInvocation {
        cursor: record.cursor,
        owner: record.owner,
        invocation: record.key.invocation,
        #[cfg(test)]
        submit_request_id,
        submit_request_sha256: blob32(digest)?,
        group_submit_sha256: record.digest,
        created,
        admission: record.admission,
        state: record.status.map(|s| s.state),
        #[cfg(test)]
        engine_known: record.engine_known,
        authority_valid: record.authority_valid,
        refusal: record.refusal.map(|r| r.code),
    })
}
fn client_record(conn: &Connection, owner: &Owner, invocation: Id) -> Result<InvocationRecord> {
    if owner.server_side {
        return Err(invalid("command_owner"));
    }
    let record = load_record(conn, &owner.key(invocation))?.ok_or(invalid("command_not_found"))?;
    check_record_owner(&record, owner)?;
    if !record.authority_valid {
        return Err(invalid("command_authority_revoked"));
    }
    Ok(record)
}
fn slot_authority(conn: &Connection, group: &[u8], id: Id) -> Result<bool> {
    conn.query_row(
        "SELECT authority_valid FROM command_requests WHERE group_id=?1 AND request_id=?2",
        params![group, id.as_slice()],
        |r| r.get(0),
    )
    .map_err(db)
}
fn delivery(conn: &Connection, key: &InvocationKey) -> Result<ClientDelivery> {
    conn.query_row(
        "SELECT stdout_delivered,stderr_delivered FROM command_invocations WHERE k=?1",
        [key_bytes(key)],
        |r| {
            Ok(ClientDelivery {
                stdout: sql_u64(r, 0)?,
                stderr: sql_u64(r, 1)?,
            })
        },
    )
    .map_err(db)
}
fn decode_client_response(bytes: &[u8], slot: &PendingCommandRequest) -> Result<Response> {
    let response = wire::decode_response(bytes)?;
    if response.id != slot.request.id
        || response.binding != slot.owner.binding
        || response.op != slot.request.op.op()
        || response.request_sha256 != slot.digest
        || response.result.as_ref().is_ok_and(|r| {
            r.status()
                .is_some_and(|s| s.invocation != slot.request.op.invocation())
        })
    {
        return Err(invalid("command_response_binding"));
    }
    Ok(response)
}
fn chunk_progress(chunk: &Chunk, start: u64, gaps: &[wire::Gap]) -> bool {
    let next = chunk.next_cursor;
    if next < start || next > i64::MAX as u64 || chunk.bytes.len() as u64 > next - start {
        return false;
    }
    chunk.bytes.len() as u64 == next - start
        || gaps
            .iter()
            .any(|g| g.start < next && g.end.is_none_or(|end| end > start))
}
impl Journal {
    pub(crate) fn command_server_records(
        &self,
        after: Option<u64>,
        max: usize,
    ) -> Result<Vec<InvocationRecord>> {
        let mut stmt=self.conn.prepare("SELECT k FROM command_invocations WHERE server_side=1 AND cursor>?1 ORDER BY cursor LIMIT ?2").map_err(db)?;
        let keys = stmt
            .query_map(params![bound(after.unwrap_or(0))?, limit(max)?], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        keys.into_iter()
            .map(|k| {
                load_record(&self.conn, &key_from_bytes(&k)?)?.ok_or(invalid("command_record"))
            })
            .collect()
    }
    pub(crate) fn command_cleanup_fresh(&self, fence: u64) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT facts FROM command_invocations WHERE server_side=1 LIMIT 10001")
            .map_err(db)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, Vec<u8>>(0))
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        if rows.len() > RETAINED_GLOBAL {
            return Err(invalid("command_retained_capacity"));
        }
        for bytes in rows {
            let row: Facts = decode(&bytes)?;
            if row.observed_seq < fence
                || !row.authority_valid
                || row.uncertain
                || !(row.refusal.is_some() && !row.engine_known
                    || row.engine_known
                        && row.worker == WorkerState::Exited
                        && row
                            .status
                            .as_ref()
                            .is_some_and(|s| s.state == State::Exited))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Call `command_client_invocation` with the validated handle first. After
    /// slot retirement, this owner/request lookup cannot identify the invocation
    /// whose retained authority must be checked before returning `Absent`.
    pub(crate) fn command_client_slot_state(
        &self,
        owner: &Owner,
        id: Id,
    ) -> Result<ClientSlotState> {
        if owner.server_side {
            return Err(invalid("command_owner"));
        }
        check_local_binding(&self.conn, owner)?;
        if self
            .command_room(owner.group())?
            .is_some_and(|room| room.revoked)
        {
            return Err(invalid("command_authority_revoked"));
        }
        let Some(slot) = load_request(&self.conn, owner.group(), id.as_slice())? else {
            return Ok(ClientSlotState::Absent);
        };
        client_record(&self.conn, owner, slot.request.op.invocation())?;
        if slot.owner != *owner || !slot_authority(&self.conn, owner.group(), id)? {
            return Err(invalid("command_owner"));
        }
        let (projected, consumed): (bool,bool) = self.conn.query_row("SELECT projected,local_consumed FROM command_requests WHERE group_id=?1 AND request_id=?2 AND client=1", params![owner.group(),id.as_slice()], |r| Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
        Ok(if consumed {
            ClientSlotState::Consumed
        } else if projected {
            ClientSlotState::Answered
        } else {
            ClientSlotState::Pending
        })
    }
    pub(crate) fn command_client_invocation(
        &self,
        owner: &Owner,
        invocation: Id,
    ) -> Result<ClientInvocation> {
        let record = client_record(&self.conn, owner, invocation)?;
        client_invocation(&self.conn, record)
    }
    pub(crate) fn command_client_invocations(
        &self,
        after: Option<u64>,
        max: usize,
    ) -> Result<ClientInvocationPage> {
        let sql_max = limit(max)?;
        let mut statement = self.conn.prepare("SELECT k FROM command_invocations WHERE server_side=0 AND cursor>?1 ORDER BY cursor LIMIT ?2").map_err(db)?;
        let keys = statement
            .query_map(params![bound(after.unwrap_or(0))?, sql_max + 1], |r| {
                r.get::<_, Vec<u8>>(0)
            })
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        let more = keys.len() > max;
        let entries: Result<Vec<_>> = keys
            .into_iter()
            .take(max)
            .map(|key| {
                let record = load_record(&self.conn, &key_from_bytes(&key)?)?
                    .ok_or(invalid("command_record"))?;
                if record.owner.server_side {
                    return Err(invalid("command_owner"));
                }
                check_local_binding(&self.conn, &record.owner)?;
                client_invocation(&self.conn, record)
            })
            .collect();
        let entries = entries?;
        let next = more.then(|| entries.last().expect("nonempty page").cursor);
        Ok(ClientInvocationPage { entries, next })
    }
    pub(crate) fn command_client_cursors(
        &self,
        owner: &Owner,
        invocation: Id,
    ) -> Result<ClientDelivery> {
        let record = client_record(&self.conn, owner, invocation)?;
        delivery(&self.conn, &record.key)
    }
    pub(crate) fn command_client_response(
        &self,
        owner: &Owner,
        id: Id,
    ) -> Result<Option<(Response, Digest)>> {
        let slot = load_request(&self.conn, owner.group(), id.as_slice())?
            .ok_or(invalid("command_response_retired"))?;
        client_record(&self.conn, owner, slot.request.op.invocation())?;
        if slot.owner != *owner || !slot_authority(&self.conn, owner.group(), id)? {
            return Err(invalid("command_owner"));
        }
        let row:Option<(Vec<u8>,Vec<u8>)>=self.conn.query_row("SELECT received_response,response_digest FROM command_requests WHERE group_id=?1 AND request_id=?2 AND client=1 AND local_consumed=0 AND projected=1",params![owner.group(),id.as_slice()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
        row.map(|(bytes, digest)| {
            let digest = blob32(digest)?;
            if sha(&bytes) != digest {
                return Err(invalid("command_response_conflict"));
            }
            Ok((decode_client_response(&bytes, &slot)?, digest))
        })
        .transpose()
    }
    pub(crate) fn command_client_unconsumed(
        &self,
        owner: &Owner,
        invocation: Id,
        max: usize,
    ) -> Result<Vec<(Id, Op, Digest)>> {
        client_record(&self.conn, owner, invocation)?;
        let mut stmt=self.conn.prepare("SELECT request_id FROM command_requests WHERE group_id=?1 AND k=?2 AND client=1 AND projected=1 AND local_consumed=0 AND authority_valid=1 ORDER BY created,request_id LIMIT ?3").map_err(db)?;
        let ids = stmt
            .query_map(
                params![
                    owner.group(),
                    key_bytes(&owner.key(invocation)),
                    limit(max)?
                ],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        let mut result = Vec::new();
        for id in ids {
            let id: Id = id.try_into().map_err(|_| invalid("command_request"))?;
            if let Some((response, digest)) = self.command_client_response(owner, id)? {
                result.push((id, response.op, digest));
            }
        }
        Ok(result)
    }
    pub(crate) fn command_client_request_invocation(
        &self,
        owner: &Owner,
        id: Id,
    ) -> Result<Option<Id>> {
        let Some(slot) = load_request(&self.conn, owner.group(), id.as_slice())? else {
            return Ok(None);
        };
        client_record(&self.conn, owner, slot.request.op.invocation())?;
        if slot.owner != *owner || !slot_authority(&self.conn, owner.group(), id)? {
            return Err(invalid("command_owner"));
        }
        Ok(Some(slot.request.op.invocation()))
    }
    pub(crate) fn command_client_finish_state(
        &self,
        owner: &Owner,
        invocation: Id,
    ) -> Result<CommandClientFinishState> {
        let record = client_record(&self.conn, owner, invocation)?;
        let finish = facts(&self.conn, &record.key)?.finish;
        Ok(CommandClientFinishState {
            confirmed: finish.confirmed,
            #[cfg(test)]
            attempts: finish.attempts,
        })
    }
    pub(crate) fn command_finish_result(
        &self,
        owner: &Owner,
        request: &Request,
    ) -> Result<std::result::Result<wire::ResponseResult, WireError>> {
        finish_result(&self.conn, owner, request)
    }
    pub(crate) fn command_admission_budget(&self) -> Result<CommandAdmissionBudget> {
        admission_budget(&self.conn)
    }
    pub(crate) fn command_publications_terminal(&self) -> Result<bool> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM command_publications WHERE state NOT IN ('published','invalidated','expired')", [], |r| r.get(0)).map_err(db)?;
        Ok(n == 0)
    }
    /// Drain proof uses durable native acknowledgements, never relay acceptance.
    pub(crate) fn command_sources_all_acknowledged(&self) -> Result<bool> {
        let pending:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM command_incoming_sources WHERE acked=0) OR EXISTS(SELECT 1 FROM command_outgoing_sources WHERE acked=0 OR quarantined=1) OR EXISTS(SELECT 1 FROM command_early_invalidations) OR EXISTS(SELECT 1 FROM command_queued_bindings)",[],|row|row.get(0)).map_err(db)?;
        Ok(!pending)
    }
    pub(crate) fn command_finish_candidate(
        &self,
        owner: &Owner,
        id: Id,
        response_sha256: Digest,
        expected: Option<ClientDelivery>,
        new_id: Id,
    ) -> Result<Option<Request>> {
        finish_candidate(&self.conn, owner, id, response_sha256, expected, new_id)
    }
    /// The exact final Poll consumption and prepared Finish publication share one commit.
    pub(crate) fn command_consume_and_finish(
        &mut self,
        owner: &Owner,
        id: Id,
        response_sha256: Digest,
        expected: Option<ClientDelivery>,
        publication: Option<&PreparedCommandPublication>,
        now: u64,
    ) -> Result<(ClientDelivery, Option<Id>)> {
        let tx = self.conn.transaction().map_err(db)?;
        let candidate = finish_candidate(
            &tx,
            owner,
            id,
            response_sha256,
            expected,
            publication.map_or([0; 16], |p| p.request_id),
        )?;
        if candidate.is_some() != publication.is_some() {
            return Err(invalid("command_finish_preparation"));
        }
        if let Some(request) = candidate.as_ref() {
            let p = publication.ok_or(invalid("command_finish_preparation"))?;
            p.validate(owner.local())?;
            if p.key.group != owner.group
                || p.binding != owner.binding
                || p.request_id != request.id
                || p.encoded_wire != wire::encode_request(request)?
                || p.request_sha256 != wire::request_digest(&p.encoded_wire)
            {
                return Err(invalid("command_publication_binding"));
            }
            freshness(&tx, request.id, now)?;
        }
        let (current, bad) = consume_response_in(&tx, owner, id, response_sha256, expected)?;
        let mut finish_id = None;
        if let Some(request) = candidate {
            insert_slot(
                &tx,
                owner,
                &request,
                &wire::encode_request(&request)?,
                true,
                now,
            )?;
            insert_publication(
                &tx,
                publication.ok_or(invalid("command_finish_preparation"))?,
            )?;
            let key = owner.key(request.op.invocation());
            let mut f = facts(&tx, &key)?;
            f.finish.attempts = f
                .finish
                .attempts
                .checked_add(1)
                .filter(|n| *n <= 4)
                .ok_or(invalid("command_finish_attempts"))?;
            f.finish.latest_request = Some(request.id);
            f.finish.latest_busy = false;
            tx.execute(
                "UPDATE command_invocations SET facts=?2 WHERE k=?1",
                params![key_bytes(&key), encode(&f)?],
            )
            .map_err(db)?;
            finish_id = Some(request.id);
        }
        if finish_id.is_none() {
            let slot = load_request(&tx, owner.group(), id.as_slice())?
                .ok_or(invalid("command_response_retired"))?;
            let finish = facts(&tx, &owner.key(slot.request.op.invocation()))?.finish;
            if !finish.confirmed && !finish.latest_busy {
                finish_id = finish.latest_request;
            }
        }
        tx.commit().map_err(db)?;
        if bad {
            return Err(invalid("command_output_cursor_invalid"));
        }
        Ok((current, finish_id))
    }
    pub(crate) fn command_consume_response(
        &mut self,
        owner: &Owner,
        id: Id,
        response_sha256: Digest,
        expected: Option<ClientDelivery>,
    ) -> Result<ClientDelivery> {
        let tx = self.conn.transaction().map_err(db)?;
        let (current, invalid_cursor) =
            consume_response_in(&tx, owner, id, response_sha256, expected)?;
        tx.commit().map_err(db)?;
        if invalid_cursor {
            return Err(invalid("command_output_cursor_invalid"));
        }
        Ok(current)
    }
}

fn consume_response_in(
    conn: &Connection,
    owner: &Owner,
    id: Id,
    response_sha256: Digest,
    expected: Option<ClientDelivery>,
) -> Result<(ClientDelivery, bool)> {
    let slot = load_request(conn, owner.group(), id.as_slice())?
        .ok_or(invalid("command_response_retired"))?;
    let record = client_record(conn, owner, slot.request.op.invocation())?;
    if slot.owner != *owner || !slot_authority(conn, owner.group(), id)? {
        return Err(invalid("command_owner"));
    }
    type Received = (Vec<u8>, Vec<u8>, bool);
    let row:Option<Received>=conn.query_row("SELECT received_response,response_digest,local_consumed FROM command_requests WHERE group_id=?1 AND request_id=?2 AND client=1 AND projected=1",params![owner.group(),id.as_slice()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db)?;
    let (bytes, digest, consumed) = row.ok_or(invalid("command_response_pending"))?;
    if digest != response_sha256 || sha(&bytes) != response_sha256 {
        return Err(invalid("command_response_conflict"));
    }
    let mut current = delivery(conn, &record.key)?;
    match &slot.request.op {
        RequestOp::Poll {
            stdout_cursor,
            stderr_cursor,
            ..
        } => {
            let start = ClientDelivery {
                stdout: *stdout_cursor,
                stderr: *stderr_cursor,
            };
            if expected != Some(start) || (!consumed && current != start) {
                return Err(invalid("command_cursor_stale"));
            }
        }
        _ if expected.is_some() => return Err(invalid("command_cursor_stale")),
        _ => {}
    }
    if consumed {
        return Ok((current, false));
    }
    let response = decode_client_response(&bytes, &slot)?;
    let mut invalid_cursor = false;
    if let Ok(wire::ResponseResult::Poll {
        status,
        stdout,
        stderr,
    }) = &response.result
    {
        if !chunk_progress(stdout, current.stdout, &status.stdout.gaps)
            || !chunk_progress(stderr, current.stderr, &status.stderr.gaps)
        {
            invalid_cursor = true;
        } else {
            current = ClientDelivery {
                stdout: stdout.next_cursor,
                stderr: stderr.next_cursor,
            };
        }
    }
    conn.execute(
        "UPDATE command_invocations SET stdout_delivered=?2,stderr_delivered=?3 WHERE k=?1",
        params![
            key_bytes(&record.key),
            bound(current.stdout)?,
            bound(current.stderr)?
        ],
    )
    .map_err(db)?;
    conn.execute(
        "UPDATE command_requests SET local_consumed=1 WHERE group_id=?1 AND request_id=?2",
        params![owner.group(), id.as_slice()],
    )
    .map_err(db)?;
    Ok((current, invalid_cursor))
}
fn finish_candidate(
    conn: &Connection,
    owner: &Owner,
    id: Id,
    response_sha256: Digest,
    expected: Option<ClientDelivery>,
    new_id: Id,
) -> Result<Option<Request>> {
    let slot = load_request(conn, owner.group(), id.as_slice())?
        .ok_or(invalid("command_response_retired"))?;
    let record = client_record(conn, owner, slot.request.op.invocation())?;
    if slot.owner != *owner || !slot_authority(conn, owner.group(), id)? {
        return Err(invalid("command_owner"));
    }
    let RequestOp::Poll {
        stdout_cursor,
        stderr_cursor,
        ..
    } = slot.request.op
    else {
        return Ok(None);
    };
    let row:Option<(Vec<u8>,Vec<u8>,bool)>=conn.query_row("SELECT received_response,response_digest,local_consumed FROM command_requests WHERE group_id=?1 AND request_id=?2 AND client=1 AND projected=1",params![owner.group(),id.as_slice()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db)?;
    let (bytes, digest, consumed) = row.ok_or(invalid("command_response_pending"))?;
    if digest != response_sha256 || sha(&bytes) != response_sha256 {
        return Err(invalid("command_response_conflict"));
    }
    if consumed {
        return Ok(None);
    }
    let current = delivery(conn, &record.key)?;
    if expected
        != Some(ClientDelivery {
            stdout: stdout_cursor,
            stderr: stderr_cursor,
        })
        || current != expected.unwrap_or_default()
    {
        return Err(invalid("command_cursor_stale"));
    }
    let response = decode_client_response(&bytes, &slot)?;
    let Ok(wire::ResponseResult::Poll {
        status,
        stdout,
        stderr,
    }) = response.result
    else {
        return Ok(None);
    };
    if !chunk_progress(&stdout, current.stdout, &status.stdout.gaps)
        || !chunk_progress(&stderr, current.stderr, &status.stderr.gaps)
    {
        return Ok(None);
    }
    if stdout.next_cursor != status.stdout.produced || stderr.next_cursor != status.stderr.produced
    {
        return Ok(None);
    }
    let terminal_sha256 =
        match wire::finish_digest(owner.group(), owner.binding(), &record.digest, &status) {
            Ok(d) => d,
            Err(_) => return Ok(None),
        };
    let f = facts(conn, &record.key)?;
    if f.finish.attempts >= 4 {
        return Ok(None);
    }
    if f.finish.latest_request.is_some() && !f.finish.latest_busy {
        return Ok(None);
    }
    let others:i64=conn.query_row("SELECT COUNT(*) FROM command_requests WHERE k=?1 AND client=1 AND request_id!=?2 AND (projected=0 OR local_consumed=0)",params![key_bytes(&record.key),id.as_slice()],|r|r.get(0)).map_err(db)?;
    if others != 0 {
        return Ok(None);
    }
    Ok(Some(Request {
        binding: owner.binding().clone(),
        id: new_id,
        op: RequestOp::Finish {
            invocation: record.key.invocation,
            submit_sha256: record.digest,
            terminal_sha256,
            stdout_cursor: stdout.next_cursor,
            stderr_cursor: stderr.next_cursor,
        },
    }))
}

fn increment(n: u64) -> Result<u64> {
    n.checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(Error::Storage("command_counter"))
}
fn key_from_bytes(b: &[u8]) -> Result<InvocationKey> {
    if b.len() != 80 {
        return Err(invalid("command_record"));
    }
    Ok(InvocationKey {
        server: b[..32].try_into().map_err(|_| invalid("command_record"))?,
        principal: b[32..64]
            .try_into()
            .map_err(|_| invalid("command_record"))?,
        invocation: b[64..].try_into().map_err(|_| invalid("command_record"))?,
    })
}
fn load_record(conn: &Connection, key: &InvocationKey) -> Result<Option<InvocationRecord>> {
    type Stored = (
        Vec<u8>,
        Option<Vec<u8>>,
        Vec<u8>,
        Vec<u8>,
        u64,
        u32,
        Option<Vec<u8>>,
    );
    let data: Option<Stored> = conn
        .query_row(
            "SELECT owner,submit,digest,facts,cursor,stdin_length,operation_kind FROM command_invocations WHERE k=?1",
            [key_bytes(key)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, sql_u64(r, 4)?, r.get(5)?, r.get(6)?)),
        )
        .optional()
        .map_err(db)?;
    data.map(|(o, s, d, f, cursor, stdin_length, operation)| {
        let owner = decode_owner(&o, key)?;
        check_local_binding(conn, &owner)?;
        let submit: Option<Submit> = s.as_deref().map(decode).transpose()?;
        let digest = blob32(d)?;
        if stdin_length > 16384 {
            return Err(invalid("command_record"));
        }
        if let Some(submit) = &submit
            && (submit.invocation != key.invocation
                || submit.stdin.len() != stdin_length as usize
                || wire::submit_digest(owner.group(), owner.binding(), submit)? != digest)
        {
            return Err(invalid("command_record"));
        }
        let f: Facts = decode(&f)?;
        let record = InvocationRecord {
            key: key.clone(),
            owner,
            submit,
            stdin_length,
            digest,
            admission: f.admission,
            revision: f.revision,
            engine_known: f.engine_known,
            status: f.status,
            refusal: f.refusal,
            feeder: f.feeder,
            authority_valid: f.authority_valid,
            warning: f.warning,
            worker: f.worker,
            observed_seq: f.observed_seq,
            cursor,
            uncertain: f.uncertain,
        };
        let registered_payload = operation
            .as_deref()
            .map(decode::<EngineOperation>)
            .transpose()?
            .is_some_and(|op| {
                matches!(
                    op,
                    EngineOperation::Dispatch { .. } | EngineOperation::WriteInput { .. }
                )
            });
        if (record.payload_required() || registered_payload) && record.submit.is_none() {
            return Err(Error::Storage("command_payload_expired"));
        }
        Ok(record)
    })
    .transpose()
}
fn facts(conn: &Connection, key: &InvocationKey) -> Result<Facts> {
    let bytes: Vec<u8> = conn
        .query_row(
            "SELECT facts FROM command_invocations WHERE k=?1",
            [key_bytes(key)],
            |r| r.get(0),
        )
        .map_err(db)?;
    decode(&bytes)
}
fn save_record(conn: &Connection, r: &InvocationRecord) -> Result<()> {
    let mut updated = Facts::from_record(r);
    updated.finish = facts(conn, &r.key)?.finish;
    conn.execute(
        "UPDATE command_invocations SET facts=?2,active=?3 WHERE k=?1",
        params![key_bytes(&r.key), encode(&updated)?, r.active()],
    )
    .map_err(db)?;
    Ok(())
}
fn count(conn: &Connection, sql: &str, principal: Option<&Account>) -> Result<usize> {
    let n: i64 = match principal {
        Some(p) => conn.query_row(sql, [p.as_slice()], |r| r.get(0)),
        None => conn.query_row(sql, [], |r| r.get(0)),
    }
    .map_err(db)?;
    usize::try_from(n).map_err(|_| invalid("command_counter"))
}
fn insert_invocation(
    conn: &Connection,
    owner: &Owner,
    submit: &Submit,
    digest: Digest,
    refusal: Option<WireError>,
    now: u64,
) -> Result<()> {
    insert_invocation_with_client_request(
        conn,
        owner,
        submit,
        digest,
        refusal,
        InvocationAdmission {
            now,
            client_request: None,
        },
    )
}
struct InvocationAdmission {
    now: u64,
    client_request: Option<(Id, Digest)>,
}
fn insert_invocation_with_client_request(
    conn: &Connection,
    owner: &Owner,
    submit: &Submit,
    digest: Digest,
    refusal: Option<WireError>,
    admission: InvocationAdmission,
) -> Result<()> {
    let InvocationAdmission {
        now,
        client_request,
    } = admission;
    let limits = store_limits(conn)?;
    if count(conn, "SELECT count(*) FROM command_invocations", None)? >= limits.retained_global
        || count(
            conn,
            "SELECT count(*) FROM command_invocations WHERE principal=?1",
            Some(&owner.binding.client),
        )? >= limits.retained_per_principal
    {
        return Err(invalid("command_retained_capacity"));
    }
    let facts = Facts::new(refusal);
    let active = facts.refusal.is_none();
    if owner.server_side
        && active
        && (count(
            conn,
            "SELECT count(*) FROM command_invocations WHERE server_side=1 AND active=1",
            None,
        )? >= limits.active_global
            || count(
                conn,
                "SELECT count(*) FROM command_invocations WHERE server_side=1 AND active=1 AND principal=?1",
                Some(&owner.binding.client),
            )? >= limits.active_per_principal)
    {
        return Err(invalid("command_active_capacity"));
    }
    if owner.server_side && active {
        reserve_credit(conn, &limits)?;
    }
    conn.execute("INSERT INTO command_invocations(k,server,principal,invocation,owner,submit,digest,facts,active,created,server_side,stdin_length,submit_request_id,submit_request_digest) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![key_bytes(&owner.key(submit.invocation)),owner.binding.server.as_slice(),owner.binding.client.as_slice(),submit.invocation.as_slice(),owner.encode()?,encode(submit)?,digest.as_slice(),encode(&facts)?,active,bound(now)?,owner.server_side,submit.stdin.len() as i64,client_request.as_ref().map(|(id,_)|id.as_slice()),client_request.as_ref().map(|(_,digest)|digest.as_slice())]).map_err(db)?;
    Ok(())
}
fn check_record_owner(r: &InvocationRecord, owner: &Owner) -> Result<()> {
    if r.owner != *owner {
        return Err(invalid("command_invocation_binding"));
    }
    Ok(())
}
fn check_request_owner(r: &Request, owner: &Owner) -> Result<()> {
    if r.binding != owner.binding {
        return Err(invalid("command_request_binding"));
    }
    Ok(())
}
fn freshness(conn: &Connection, id: Id, now: u64) -> Result<()> {
    let floor: u64 = conn
        .query_row(
            "SELECT floor FROM command_replay_state WHERE id=1",
            [],
            |r| sql_u64(r, 0),
        )
        .map_err(db)?;
    let timestamp = wire::id_timestamp(id);
    if now < floor {
        return Err(invalid("command_clock_behind_floor"));
    }
    if timestamp < floor {
        return Err(invalid("command_expired"));
    }
    if timestamp > now.saturating_add(300) {
        return Err(invalid(
            if timestamp <= now.max(floor).saturating_add(WEEK) {
                "command_future_deferred"
            } else {
                "command_future_excessive"
            },
        ));
    }
    Ok(())
}
fn check_slot(conn: &Connection, owner: &Owner, request: &Request, bytes: &[u8]) -> Result<bool> {
    let old: Option<Vec<u8>> = conn
        .query_row(
            "SELECT request FROM command_requests WHERE group_id=?1 AND request_id=?2",
            params![owner.group(), request.id.as_slice()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    if let Some(old) = old {
        if old != bytes {
            return Err(invalid("command_request_conflict"));
        }
        Ok(true)
    } else {
        Ok(false)
    }
}
fn op_text(op: wire::Op) -> &'static str {
    match op {
        wire::Op::Submit => "submit",
        wire::Op::Status => "status",
        wire::Op::Poll => "poll",
        wire::Op::Cancel => "cancel",
        wire::Op::Finish => "finish",
    }
}
fn insert_slot(
    conn: &Connection,
    owner: &Owner,
    request: &Request,
    bytes: &[u8],
    client: bool,
    now: u64,
) -> Result<()> {
    if count(conn, "SELECT count(*) FROM command_requests", None)? >= SLOTS_GLOBAL
        || count(
            conn,
            "SELECT count(*) FROM command_requests WHERE principal=?1",
            Some(&owner.binding.client),
        )? >= SLOTS_PRINCIPAL
    {
        return Err(invalid("command_slot_capacity"));
    }
    let key = owner.key(request.op.invocation());
    if matches!(request.op, RequestOp::Poll { .. }) {
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM command_requests WHERE k=?1 AND op='poll'",
                [key_bytes(&key)],
                |r| r.get(0),
            )
            .map_err(db)?;
        if n != 0 {
            return Err(invalid("command_poll_busy"));
        }
    }
    conn.execute("INSERT INTO command_requests(group_id,request_id,principal,k,request,digest,op,client,created,owner) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![owner.group(),request.id.as_slice(),owner.binding.client.as_slice(),key_bytes(&key),bytes,wire::request_digest(bytes).as_slice(),op_text(request.op.op()),client,bound(now)?,owner.encode()?]).map_err(db)?;
    Ok(())
}
fn validate_source(source: &CommandSource, owner: &Owner) -> Result<()> {
    if source.group != owner.group
        || source.native_id.is_empty()
        || source.native_id.len() > 1024
        || !crate::foundation::valid_hex(&source.app_event_id, 64)
        || source.authority_metadata.len() > 4096
    {
        return Err(invalid("command_source"));
    }
    Ok(())
}
fn attach_source(conn: &Connection, s: &CommandSource, request_id: Id) -> Result<()> {
    type SourceRow = (Vec<u8>, String, Vec<u8>, Vec<u8>);
    let old:Option<SourceRow>=conn.query_row("SELECT request_id,event_id,author,authority FROM command_incoming_sources WHERE group_id=?1 AND native_id=?2",params![s.group,s.native_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db)?;
    if let Some((id, event, author, authority)) = old {
        if id != request_id
            || event != s.app_event_id
            || author != s.author
            || authority != s.authority_metadata
        {
            return Err(invalid("command_source_conflict"));
        }
        return Ok(());
    }
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM command_incoming_sources WHERE group_id=?1 AND request_id=?2",
            params![s.group, request_id.as_slice()],
            |r| r.get(0),
        )
        .map_err(db)?;
    if n >= MAX_SOURCES as i64 {
        return Err(invalid("command_source_capacity"));
    }
    conn.execute("INSERT INTO command_incoming_sources(group_id,native_id,request_id,event_id,author,authority) VALUES(?1,?2,?3,?4,?5,?6)",params![s.group,s.native_id,request_id.as_slice(),s.app_event_id,s.author.as_slice(),s.authority_metadata]).map_err(db)?;
    Ok(())
}
fn validate_response_for(
    p: &PreparedCommandPublication,
    owner: &Owner,
    request: &Request,
    request_bytes: &[u8],
) -> Result<()> {
    let response = wire::decode_response(&p.encoded_wire)?;
    if p.key.group != owner.group
        || p.binding != owner.binding
        || p.request_id != request.id
        || p.request_sha256 != wire::request_digest(request_bytes)
        || response.id != request.id
        || response.op != request.op.op()
        || response.binding != owner.binding
        || response.request_sha256 != p.request_sha256
    {
        return Err(invalid("command_publication_binding"));
    }
    Ok(())
}
fn insert_publication(conn: &Connection, p: &PreparedCommandPublication) -> Result<()> {
    let old: Option<(Vec<u8>, Vec<u8>)> = conn
        .query_row(
            "SELECT wire,native FROM command_publications WHERE group_id=?1 AND event_id=?2",
            params![p.key.group, p.key.app_event_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    if let Some((wire, native)) = old {
        if wire != p.encoded_wire || native != p.encoded_app_event {
            return Err(invalid("command_publication_conflict"));
        }
        return Ok(());
    }
    let association: Option<(Vec<u8>, Option<String>)> = conn
        .query_row(
            "SELECT digest,response_id FROM command_requests WHERE group_id=?1 AND request_id=?2",
            params![p.key.group, p.request_id.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    let (digest, response_id) = association.ok_or(invalid("command_publication_binding"))?;
    if digest != p.request_sha256
        || response_id
            .as_ref()
            .is_some_and(|id| id != &p.key.app_event_id)
    {
        return Err(invalid("command_publication_binding"));
    }
    conn.execute("INSERT INTO command_publications(group_id,event_id,request_id,request_digest,session,client,server,wire,native,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'unsubmitted')",params![p.key.group,p.key.app_event_id,p.request_id.as_slice(),p.request_sha256.as_slice(),p.binding.session.as_slice(),p.binding.client.as_slice(),p.binding.server.as_slice(),p.encoded_wire,p.encoded_app_event]).map_err(db)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreLimits {
    active_per_principal: usize,
    active_global: usize,
    retained_per_principal: usize,
    retained_global: usize,
    max_commands: Option<u64>,
    payload_retention: u64,
}
impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            active_per_principal: ACTIVE_PRINCIPAL,
            active_global: ACTIVE_GLOBAL,
            retained_per_principal: RETAINED_PRINCIPAL,
            retained_global: RETAINED_GLOBAL,
            max_commands: None,
            payload_retention: 86400,
        }
    }
}
fn store_limits(conn: &Connection) -> Result<StoreLimits> {
    let bytes: Vec<u8> = conn
        .query_row(
            "SELECT settings FROM command_settings WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(db)?;
    decode(&bytes)
}
pub(crate) struct CommandAdmissionBudget {
    pub maximum: Option<u64>,
    pub admitted: u64,
    pub reserved: u64,
    pub finished: u64,
}
impl CommandAdmissionBudget {
    pub(crate) fn exhausted(&self) -> bool {
        self.maximum.is_some_and(|n| self.admitted >= n)
    }
    pub(crate) fn reserved_full(&self) -> bool {
        self.maximum
            .is_some_and(|n| self.admitted.saturating_add(self.reserved) >= n)
    }
}
fn admission_budget(conn: &Connection) -> Result<CommandAdmissionBudget> {
    let limits = store_limits(conn)?;
    let (admitted, finished) = conn
        .query_row(
            "SELECT admitted,finished FROM command_replay_state WHERE id=1",
            [],
            |r| Ok((sql_u64(r, 0)?, sql_u64(r, 1)?)),
        )
        .map_err(db)?;
    let mut statement = conn
        .prepare("SELECT facts FROM command_invocations WHERE server_side=1 AND active=1 LIMIT 33")
        .map_err(db)?;
    let rows = statement
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db)?;
    let mut reserved = 0;
    for row in rows {
        let f: Facts = decode(&row.map_err(db)?)?;
        if !f.engine_known && f.refusal.is_none() {
            reserved = increment(reserved)?;
        }
    }
    Ok(CommandAdmissionBudget {
        maximum: limits.max_commands,
        admitted,
        reserved,
        finished,
    })
}
fn finish_error(code: ErrorCode) -> std::result::Result<wire::ResponseResult, WireError> {
    Err(WireError { code, detail: None })
}
fn finish_result(
    conn: &Connection,
    owner: &Owner,
    request: &Request,
) -> Result<std::result::Result<wire::ResponseResult, WireError>> {
    let RequestOp::Finish {
        invocation,
        submit_sha256,
        terminal_sha256,
        stdout_cursor,
        stderr_cursor,
    } = &request.op
    else {
        return Err(invalid("command_finish_request"));
    };
    check_request_owner(request, owner)?;
    let key = owner.key(*invocation);
    let Some(record) = load_record(conn, &key)? else {
        return Ok(finish_error(ErrorCode::NotFound));
    };
    check_record_owner(&record, owner)?;
    if !record.authority_valid {
        return Err(invalid("command_authority_revoked"));
    }
    if record.digest != *submit_sha256 {
        return Ok(finish_error(ErrorCode::InvocationConflict));
    }
    let f = facts(conn, &key)?;
    if f.finish.client_finished {
        return Ok(
            if f.finish.fingerprint == Some(*terminal_sha256)
                && f.finish.cursors == Some((*stdout_cursor, *stderr_cursor))
            {
                Ok(wire::ResponseResult::Finish { finished: true })
            } else {
                finish_error(ErrorCode::InvocationConflict)
            },
        );
    }
    if !record.engine_known {
        return Ok(match record.refusal {
            Some(e) => Err(e),
            None => finish_error(ErrorCode::InvalidRequest),
        });
    }
    let Some(status) = record.status.as_ref() else {
        return Ok(finish_error(ErrorCode::ResultExpired));
    };
    let fingerprint =
        match wire::finish_digest(owner.group(), owner.binding(), submit_sha256, status) {
            Ok(d) => d,
            Err(_) => return Ok(finish_error(ErrorCode::InvalidRequest)),
        };
    if *stdout_cursor != status.stdout.produced || *stderr_cursor != status.stderr.produced {
        return Ok(finish_error(ErrorCode::InvalidRequest));
    }
    if fingerprint != *terminal_sha256 {
        return Ok(finish_error(ErrorCode::Busy));
    }
    Ok(Ok(wire::ResponseResult::Finish { finished: true }))
}
fn commit_finished(conn: &Connection, owner: &Owner, request: &Request) -> Result<()> {
    let RequestOp::Finish {
        invocation,
        terminal_sha256,
        stdout_cursor,
        stderr_cursor,
        ..
    } = &request.op
    else {
        return Err(invalid("command_finish_request"));
    };
    let key = owner.key(*invocation);
    let mut f = facts(conn, &key)?;
    if f.finish.client_finished {
        return Ok(());
    }
    f.finish.client_finished = true;
    f.finish.fingerprint = Some(*terminal_sha256);
    f.finish.cursors = Some((*stdout_cursor, *stderr_cursor));
    conn.execute(
        "UPDATE command_invocations SET facts=?2 WHERE k=?1",
        params![key_bytes(&key), encode(&f)?],
    )
    .map_err(db)?;
    if conn
        .execute(
            "UPDATE command_replay_state SET finished=finished+1 WHERE id=1 AND finished<admitted",
            [],
        )
        .map_err(db)?
        != 1
    {
        return Err(Error::Storage("command_counter"));
    }
    Ok(())
}
fn reserve_credit(conn: &Connection, _limits: &StoreLimits) -> Result<()> {
    if admission_budget(conn)?.reserved_full() {
        return Err(invalid("command_admission_closed"));
    }
    Ok(())
}
#[derive(Clone)]
pub(crate) struct PendingCommandRequest {
    pub owner: Owner,
    pub request: Request,
    pub digest: Digest,
    pub response: Option<PublicationKey>,
    pub failed_attempts: u8,
    pub operation: Option<OperationId>,
    pub operation_revision: Option<u64>,
}
impl Journal {
    pub(crate) fn command_configure(
        &mut self,
        limits: &crate::command_config::CommandLimits,
    ) -> Result<()> {
        let settings = StoreLimits {
            active_per_principal: limits.active_per_principal as usize,
            active_global: limits.active_global as usize,
            retained_per_principal: limits.retained_per_principal as usize,
            retained_global: limits.retained_global as usize,
            max_commands: limits.max_commands.map(|n| n.get()),
            payload_retention: limits.payload_retention.as_secs(),
        };
        if settings.active_per_principal == 0
            || settings.active_per_principal > ACTIVE_PRINCIPAL
            || settings.active_global == 0
            || settings.active_global > ACTIVE_GLOBAL
            || settings.retained_per_principal == 0
            || settings.retained_per_principal > RETAINED_PRINCIPAL
            || settings.retained_global == 0
            || settings.retained_global > RETAINED_GLOBAL
            || settings.payload_retention == 0
            || settings.payload_retention > WEEK
        {
            return Err(invalid("command_limits"));
        }
        self.conn
            .execute(
                "UPDATE command_settings SET settings=?1 WHERE id=1",
                [encode(&settings)?],
            )
            .map_err(db)?;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn command_requests(
        &self,
        key: &InvocationKey,
        max: usize,
    ) -> Result<Vec<PendingCommandRequest>> {
        let max = limit(max)?;
        let mut statement=self.conn.prepare("SELECT group_id,request_id FROM command_requests WHERE k=?1 ORDER BY created,request_id LIMIT ?2").map_err(db)?;
        let ids = statement
            .query_map(params![key_bytes(key), max], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        ids.into_iter()
            .map(|(group, id)| {
                load_request(&self.conn, &group, &id)?.ok_or(invalid("command_record"))
            })
            .collect()
    }
    pub(crate) fn command_begin_operation(
        &mut self,
        key: &InvocationKey,
        expected_revision: u64,
        operation: EngineOperation,
    ) -> Result<OperationRecord> {
        self.command_begin_operation_for(key, expected_revision, operation, None)
    }
    pub(crate) fn command_begin_request_operation(
        &mut self,
        key: &InvocationKey,
        expected_revision: u64,
        request_id: Id,
        operation: EngineOperation,
    ) -> Result<OperationRecord> {
        self.command_begin_operation_for(key, expected_revision, operation, Some(request_id))
    }
    fn command_begin_operation_for(
        &mut self,
        key: &InvocationKey,
        expected_revision: u64,
        operation: EngineOperation,
        request_id: Option<Id>,
    ) -> Result<OperationRecord> {
        let tx = self.conn.transaction().map_err(db)?;
        let mut record = load_record(&tx, key)?.ok_or(invalid("command_not_found"))?;
        if let Some(id) = request_id {
            let bytes:Vec<u8>=tx.query_row("SELECT request FROM command_requests WHERE group_id=?1 AND request_id=?2 AND k=?3 AND response_id IS NULL AND slot_state='pending' AND failed_attempts<3 AND slot_operation IS NULL",params![record.owner.group(),id.as_slice(),key_bytes(key)],|r|r.get(0)).optional().map_err(db)?.ok_or(invalid("command_operation_request"))?;
            let request = wire::decode_request(&bytes)?;
            check_request_operation(&request, &operation)?;
        }
        let registered: bool = tx
            .query_row(
                "SELECT operation IS NOT NULL FROM command_invocations WHERE k=?1",
                [key_bytes(key)],
                |r| r.get(0),
            )
            .map_err(db)?;
        if registered {
            return Err(invalid("command_operation_busy"));
        }
        if record.revision != expected_revision {
            return Err(invalid("command_revision"));
        }
        if !record.owner.server_side {
            return Err(invalid("command_owner"));
        }
        if !record.authority_valid
            && !matches!(
                operation,
                EngineOperation::Observe | EngineOperation::FinalObserve | EngineOperation::Cancel
            )
        {
            return Err(invalid("command_authority_revoked"));
        }
        match &operation {
            EngineOperation::Dispatch { submit } => {
                if record.engine_known
                    || record.refusal.is_some()
                    || record.admission != Admission::Accepted
                    || submit != record.payload()?
                {
                    return Err(invalid("command_dispatch_barrier"));
                }
                record.admission = Admission::Dispatching;
            }
            EngineOperation::BeginInput => {
                if !record.engine_known || !matches!(record.feeder, FeederState::NeverBegun) {
                    return Err(invalid("command_input_generation"));
                }
                record.feeder = FeederState::Beginning;
            }
            EngineOperation::WriteInput {
                generation,
                offset,
                bytes,
            } => {
                let (expected, credit) = match record.feeder {
                    FeederState::Ready {
                        generation: g,
                        credit,
                    } if g == *generation => (0, credit),
                    FeederState::Accepted {
                        generation: g,
                        offset,
                        credit,
                    } if g == *generation => (offset, credit),
                    _ => return Err(invalid("command_input_generation")),
                };
                let end = offset
                    .checked_add(bytes.len() as u64)
                    .ok_or(invalid("command_input_bounds"))?;
                if *offset != expected
                    || bytes.is_empty()
                    || bytes.len() > wire::MAX_BYTES
                    || bytes.len() as u64 > credit
                    || end > record.stdin_length as u64
                    || record.payload()?.stdin[*offset as usize..end as usize] != *bytes
                {
                    return Err(invalid("command_input_bounds"));
                }
                record.feeder = FeederState::WriteIntended {
                    generation: *generation,
                    offset: *offset,
                    digest: sha(bytes),
                };
            }
            EngineOperation::Eof { generation, offset } => {
                let accepted = match record.feeder {
                    FeederState::Ready { generation: g, .. } if g == *generation => 0,
                    FeederState::Accepted {
                        generation: g,
                        offset,
                        ..
                    } if g == *generation => offset,
                    _ => return Err(invalid("command_input_generation")),
                };
                if *offset != accepted
                    || *offset != record.stdin_length as u64
                    || record.status.as_ref().is_none_or(|s| {
                        s.input.delivered as u64 != *offset
                            || matches!(s.input.health, InputHealth::Gap | InputHealth::Uncertain)
                    })
                {
                    return Err(invalid("command_input_eof"));
                }
                record.feeder = FeederState::EofIntended {
                    generation: *generation,
                    offset: *offset,
                };
            }
            EngineOperation::Poll { max_bytes, .. }
                if *max_bytes == 0 || *max_bytes as usize > wire::MAX_BYTES =>
            {
                return Err(invalid("command_poll_bounds"));
            }
            _ => {}
        }
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|_| invalid("command_random"))?;
        save_record(&tx, &record)?;
        tx.execute("UPDATE command_invocations SET operation=?2,operation_revision=?3,operation_kind=?4,operation_request=?5 WHERE k=?1",params![key_bytes(key),id.as_slice(),bound(expected_revision)?,encode(&operation)?,request_id.as_ref().map(|i|i.as_slice())]).map_err(db)?;
        if let Some(request_id) = request_id {
            let changed=tx.execute("UPDATE command_requests SET slot_operation=?3,slot_revision=?4 WHERE group_id=?1 AND request_id=?2 AND slot_operation IS NULL AND slot_state='pending'",params![record.owner.group(),request_id.as_slice(),id.as_slice(),bound(expected_revision)?]).map_err(db)?;
            if changed != 1 {
                return Err(invalid("command_operation_request"));
            }
        }
        tx.commit().map_err(db)?;
        Ok(OperationRecord {
            id: OperationId(id),
            key: key.clone(),
            owner: record.owner.clone(),
            revision: expected_revision,
            engine_known: record.engine_known,
            ledger: record.ledger(),
            operation,
            request_id,
        })
    }
    pub(crate) fn command_commit_completion(
        &mut self,
        completion: &EngineCompletion,
        publications: &[PreparedCommandPublication],
    ) -> Result<CommitAck> {
        if publications.len() > 1 {
            return Err(invalid("command_operation_response"));
        }
        let tx = self.conn.transaction().map_err(db)?;
        let record = load_record(&tx, &completion.key)?.ok_or(invalid("command_not_found"))?;
        let operation = load_operation(&tx, completion.operation)?
            .ok_or(invalid("command_operation_unknown"))?;
        let pending = load_operation_request(&tx, completion.operation)?;
        let projection = project_completion(&record, &operation, completion, pending.as_ref())?;
        match (&projection.response, publications) {
            (Some(expected), [publication]) => {
                publication.validate(record.owner.local())?;
                if publication.encoded_wire != wire::encode_response(expected)?
                    || publication.key.group != record.owner.group
                    || publication.binding != record.owner.binding
                    || publication.request_id != expected.id
                    || publication.request_sha256 != expected.request_sha256
                {
                    return Err(invalid("command_operation_response"));
                }
            }
            (None, []) => {}
            _ => return Err(invalid("command_operation_response")),
        }
        let mut updated = projection.record;
        if !record.engine_known && updated.engine_known {
            let n=tx.execute("UPDATE command_replay_state SET admitted=admitted+1 WHERE id=1 AND admitted<9223372036854775807",[]).map_err(db)?;
            if n != 1 {
                return Err(invalid("command_counter"));
            }
        }
        let n=tx.execute("UPDATE command_replay_state SET sequence=sequence+1 WHERE id=1 AND sequence<9223372036854775807",[]).map_err(db)?;
        if n != 1 {
            return Err(invalid("command_counter"));
        }
        updated.observed_seq = tx
            .query_row(
                "SELECT sequence FROM command_replay_state WHERE id=1",
                [],
                |r| sql_u64(r, 0),
            )
            .map_err(db)?;
        updated.revision = increment(updated.revision)?;
        save_record(&tx, &updated)?;
        let mut answered = Vec::new();
        let mut suppressed = Vec::new();
        if let Some(pending) = &pending {
            let id = pending.request.id;
            if projection.suppressed {
                tx.execute("UPDATE command_requests SET slot_state='suppressed',slot_operation=NULL,slot_revision=NULL WHERE group_id=?1 AND request_id=?2",params![record.owner.group(),id.as_slice()]).map_err(db)?;
                suppressed.push(id);
            } else if let Some(publication) = publications.first() {
                insert_publication(&tx, publication)?;
                tx.execute("UPDATE command_requests SET slot_state='answered',response_id=?3,slot_operation=NULL,slot_revision=NULL,failed_attempts=?4 WHERE group_id=?1 AND request_id=?2",params![record.owner.group(),id.as_slice(),publication.key.app_event_id,projection.failed_attempts]).map_err(db)?;
                answered.push(id);
            } else {
                tx.execute("UPDATE command_requests SET failed_attempts=?3,slot_operation=NULL,slot_revision=NULL WHERE group_id=?1 AND request_id=?2",params![record.owner.group(),id.as_slice(),projection.failed_attempts]).map_err(db)?;
            }
        }
        tx.execute("UPDATE command_invocations SET operation=NULL,operation_revision=NULL,operation_kind=NULL,operation_request=NULL WHERE k=?1",[key_bytes(&completion.key)]).map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(CommitAck {
            operation: completion.operation,
            key: completion.key.clone(),
            revision: completion.revision,
            decision: projection.decision,
            answered,
            suppressed,
        })
    }
}
fn advance_feeder(
    record: &mut InvocationRecord,
    operation: &EngineOperation,
    status: &Status,
    generation: Option<[u8; 16]>,
    credit: u64,
) -> Result<()> {
    if matches!(
        status.input.health,
        InputHealth::Gap | InputHealth::Uncertain
    ) {
        record.feeder = FeederState::Failed;
        record.warning = Some(Warning::InputFailed);
        return Ok(());
    }
    if matches!(operation, EngineOperation::Observe) {
        let completed = status.input.eof_sent
            && status.input.accepted == record.stdin_length
            && status.input.delivered == record.stdin_length
            && matches!(
                status.input.health,
                InputHealth::EofPending | InputHealth::Closed
            );
        match record.feeder.clone() {
            FeederState::Beginning => {
                if let Some(generation) = generation
                    && status.input.accepted == 0
                {
                    record.feeder = FeederState::Ready { generation, credit };
                    return Ok(());
                }
            }
            FeederState::WriteIntended {
                generation: expected,
                offset,
                digest,
            } => {
                let end = status.input.accepted as usize;
                let offset =
                    usize::try_from(offset).map_err(|_| invalid("command_input_bounds"))?;
                if generation == Some(expected)
                    && end > offset
                    && end <= record.stdin_length as usize
                    && sha(&record.payload()?.stdin[offset..end]) == digest
                {
                    record.feeder = FeederState::Accepted {
                        generation: expected,
                        offset: end as u64,
                        credit,
                    };
                    return Ok(());
                }
            }
            FeederState::EofIntended {
                generation: expected,
                ..
            } if generation == Some(expected) && completed => {
                record.feeder = FeederState::Complete;
                return Ok(());
            }
            FeederState::Failed if generation.is_some() && completed => {
                record.feeder = FeederState::Complete;
                record.warning = None;
                return Ok(());
            }
            FeederState::Complete
            | FeederState::NeverBegun
            | FeederState::Ready { .. }
            | FeederState::Accepted { .. }
            | FeederState::Failed => return Ok(()),
            _ => {}
        }
        record.feeder = FeederState::Failed;
        record.warning = Some(Warning::InputFailed);
        return Ok(());
    }
    match (operation, &record.feeder) {
        (EngineOperation::BeginInput, FeederState::Beginning) => {
            let generation = generation.ok_or(invalid("command_input_generation"))?;
            record.feeder = FeederState::Ready { generation, credit };
        }
        (
            EngineOperation::WriteInput {
                generation: g,
                offset,
                bytes,
            },
            FeederState::WriteIntended {
                generation: expected,
                ..
            },
        ) if g == expected && Some(*g) == generation => {
            let end = offset
                .checked_add(bytes.len() as u64)
                .ok_or(invalid("command_input_bounds"))?;
            if status.input.accepted as u64 != end {
                record.feeder = FeederState::Failed;
                record.warning = Some(Warning::InputFailed);
            } else {
                record.feeder = FeederState::Accepted {
                    generation: *g,
                    offset: end,
                    credit,
                };
            }
        }
        (
            EngineOperation::Eof { generation: g, .. },
            FeederState::EofIntended {
                generation: expected,
                ..
            },
        ) if g == expected && Some(*g) == generation => {
            if status.input.eof_sent {
                record.feeder = FeederState::Complete;
            } else {
                record.feeder = FeederState::Failed;
                record.warning = Some(Warning::InputFailed);
            }
        }
        _ => {}
    }
    Ok(())
}

fn load_publication(
    conn: &Connection,
    key: &PublicationKey,
) -> Result<Option<PreparedCommandPublication>> {
    type StoredPublication = (
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
    );
    let row:Option<StoredPublication>=conn.query_row("SELECT request_id,request_digest,session,client,server,wire,native FROM command_publications WHERE group_id=?1 AND event_id=?2",params![key.group,key.app_event_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(db)?;
    row.map(|(id, digest, session, client, server, wire, native)| {
        Ok(PreparedCommandPublication {
            key: key.clone(),
            binding: Binding {
                session: session.try_into().map_err(|_| invalid("command_record"))?,
                client: blob32(client)?,
                server: blob32(server)?,
            },
            request_id: id.try_into().map_err(|_| invalid("command_record"))?,
            request_sha256: blob32(digest)?,
            encoded_wire: wire,
            encoded_app_event: native,
        })
    })
    .transpose()
}
impl Journal {
    pub(crate) fn command_claim_publication(
        &mut self,
        key: &PublicationKey,
    ) -> Result<Option<PreparedCommandPublication>> {
        let tx = self.conn.transaction().map_err(db)?;
        let publication = load_publication(&tx, key)?;
        if publication.is_none() {
            return Ok(None);
        }
        let changed=tx.execute("UPDATE command_publications SET state='prepared' WHERE group_id=?1 AND event_id=?2 AND state='unsubmitted' AND EXISTS(SELECT 1 FROM command_requests r WHERE r.group_id=command_publications.group_id AND r.request_id=command_publications.request_id AND r.authority_valid=1)",params![key.group,key.app_event_id]).map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(if changed == 1 { publication } else { None })
    }
    pub(crate) fn command_publications(
        &self,
        state: PublicationState,
        max: usize,
    ) -> Result<Vec<PreparedCommandPublication>> {
        let mut stmt=self.conn.prepare("SELECT group_id,event_id FROM command_publications WHERE state=?1 ORDER BY group_id,event_id LIMIT ?2").map_err(db)?;
        let rows = stmt
            .query_map(params![state.text(), limit(max)?], |r| {
                Ok(PublicationKey {
                    group: r.get(0)?,
                    app_event_id: r.get(1)?,
                })
            })
            .map_err(db)?;
        rows.map(|r| {
            load_publication(&self.conn, &r.map_err(db)?)?.ok_or(invalid("command_publication"))
        })
        .collect()
    }
    pub(crate) fn command_publication_owned(&self, key: &PublicationKey) -> Result<bool> {
        self.conn.query_row("SELECT EXISTS(SELECT 1 FROM command_publications WHERE group_id=?1 AND event_id=?2)",params![key.group,key.app_event_id],|r|r.get(0)).map_err(db)
    }
    pub(crate) fn command_publication_uncertain(&mut self, key: &PublicationKey) -> Result<()> {
        self.conn.execute("UPDATE command_publications SET state='uncertain' WHERE group_id=?1 AND event_id=?2 AND state='prepared'",params![key.group,key.app_event_id]).map_err(db)?;
        Ok(())
    }
    pub(crate) fn command_apply_native(
        &mut self,
        updates: &[CommandNativeUpdate],
    ) -> Result<Vec<PublishedKey>> {
        if updates.len() > 64 {
            return Err(invalid("command_page_limit"));
        }
        let tx = self.conn.transaction().map_err(db)?;
        let mut published = Vec::new();
        // Terminal invalidation dominates confirmation regardless of batch order.
        for update in updates {
            match update {
                CommandNativeUpdate::Queued { key, intent } => {
                    if intent.is_empty() || intent.len() > 4096 {
                        return Err(invalid("command_native_binding"));
                    }
                    let state = publication_state(&tx, key)?
                        .ok_or(invalid("command_publication_unknown"))?;
                    if state == "unsubmitted" {
                        return Err(invalid("command_publication_unclaimed"));
                    }
                    let old:Option<String>=tx.query_row("SELECT event_id FROM command_queued_bindings WHERE group_id=?1 AND intent=?2",params![key.group,intent],|r|r.get(0)).optional().map_err(db)?;
                    if let Some(old) = old {
                        if old != key.app_event_id {
                            return Err(invalid("command_native_conflict"));
                        }
                    } else {
                        let n:i64=tx.query_row("SELECT count(*) FROM command_queued_bindings WHERE group_id=?1 AND event_id=?2",params![key.group,key.app_event_id],|r|r.get(0)).map_err(db)?;
                        if n >= MAX_BINDINGS as i64 {
                            return Err(invalid("command_queued_capacity"));
                        }
                        tx.execute(
                            "INSERT INTO command_queued_bindings VALUES(?1,?2,?3)",
                            params![key.group, intent, key.app_event_id],
                        )
                        .map_err(db)?;
                    }
                }
                CommandNativeUpdate::Published {
                    key,
                    native_id,
                    evidence,
                } if record_published(&tx, key, native_id, evidence)? => {
                    published.push(PublishedKey {
                        key: key.clone(),
                        native_id: native_id.clone(),
                    });
                }
                _ => {}
            }
        }
        for update in updates {
            match update {
                CommandNativeUpdate::Failed { key } => {
                    if publication_state(&tx, key)?.is_none() {
                        return Err(invalid("command_publication_unknown"));
                    }
                    invalidate_publication(&tx, key)?;
                }
                CommandNativeUpdate::RefusedQueued { group, intent } => {
                    let event:Option<String>=tx.query_row("SELECT event_id FROM command_queued_bindings WHERE group_id=?1 AND intent=?2",params![group,intent],|r|r.get(0)).optional().map_err(db)?;
                    let event = event.ok_or(invalid("command_queued_unknown"))?;
                    invalidate_publication(
                        &tx,
                        &PublicationKey {
                            group: group.clone(),
                            app_event_id: event,
                        },
                    )?;
                    tx.execute(
                        "DELETE FROM command_queued_bindings WHERE group_id=?1 AND intent=?2",
                        params![group, intent],
                    )
                    .map_err(db)?;
                }
                CommandNativeUpdate::Invalidated {
                    group,
                    native_id,
                    possible_publication,
                } => {
                    record_invalidation(&tx, group, native_id, possible_publication.as_ref())?;
                }
                _ => {}
            }
        }
        // Only durable native bindings may be ACKed; terminal evidence remains
        // invalid and cannot be resurrected by a later Published notification.
        let mut acknowledged = Vec::with_capacity(published.len());
        for binding in published {
            let safe:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM command_outgoing_sources WHERE group_id=?1 AND native_id=?2 AND event_id=?3 AND quarantined=0)",params![binding.key.group,binding.native_id,binding.key.app_event_id],|r|r.get(0)).map_err(db)?;
            if safe {
                acknowledged.push(binding);
            }
        }
        tx.commit().map_err(db)?;
        Ok(acknowledged)
    }
    pub(crate) fn command_native_bindings(
        &self,
        after: Option<SourceCursor>,
        max: usize,
    ) -> Result<SourcePage> {
        let max = limit(max)?;
        let (kind, row) = after
            .map(|c| (if c.kind == SourceKind::Incoming { 0 } else { 1 }, c.row_id))
            .unwrap_or((0, 0));
        let mut stmt=self.conn.prepare("SELECT kind,row_id,group_id,native_id,event_id,acked,invalidated,quarantined,evidence FROM (SELECT 0 AS kind,row_id,group_id,native_id,NULL AS event_id,acked,0 AS invalidated,0 AS quarantined,NULL AS evidence FROM command_incoming_sources UNION ALL SELECT 1 AS kind,row_id,group_id,native_id,event_id,acked,invalidated,quarantined,evidence FROM command_outgoing_sources) WHERE kind>?1 OR(kind=?1 AND row_id>?2) ORDER BY kind,row_id LIMIT ?3").map_err(db)?;
        let rows = stmt
            .query_map(params![kind, bound(row)?, max + 1], |r| {
                let kind = if r.get::<_, i64>(0)? == 0 {
                    SourceKind::Incoming
                } else {
                    SourceKind::Outgoing
                };
                let group: Vec<u8> = r.get(2)?;
                let event: Option<String> = r.get(4)?;
                Ok(NativeBinding {
                    cursor: SourceCursor {
                        kind,
                        row_id: sql_u64(r, 1)?,
                    },
                    source: SourceKey {
                        group: group.clone(),
                        native_id: r.get(3)?,
                    },
                    acked: r.get(5)?,
                    invalidated: r.get(6)?,
                    quarantined: r.get(7)?,
                    evidence: None,
                    publication: event.map(|app_event_id| PublicationKey {
                        group,
                        app_event_id,
                    }),
                })
            })
            .map_err(db)?;
        let mut result: Vec<_> = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?;
        for binding in &mut result {
            if binding.cursor.kind == SourceKind::Outgoing {
                let bytes: Vec<u8> = self
                    .conn
                    .query_row(
                        "SELECT evidence FROM command_outgoing_sources WHERE row_id=?1",
                        [bound(binding.cursor.row_id)?],
                        |r| r.get(0),
                    )
                    .map_err(db)?;
                binding.evidence = Some(PublishedEvidence::decode(&bytes)?);
            }
        }
        let more = result.len() > max as usize;
        if more {
            result.pop();
        }
        let next = if more {
            result.last().map(|r| r.cursor)
        } else {
            None
        };
        Ok(SourcePage { rows: result, next })
    }
    #[cfg(test)]
    pub(crate) fn command_publication_bindings(
        &self,
        key: &PublicationKey,
    ) -> Result<Vec<NativeBinding>> {
        let mut stmt=self.conn.prepare("SELECT row_id,native_id,acked,invalidated,quarantined,evidence FROM command_outgoing_sources WHERE group_id=?1 AND event_id=?2 ORDER BY row_id LIMIT 3").map_err(db)?;
        type Row = (u64, Vec<u8>, bool, bool, bool, Vec<u8>);
        let rows: Vec<Row> = stmt
            .query_map(params![key.group, key.app_event_id], |r| {
                Ok((
                    sql_u64(r, 0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        if rows.len() > MAX_BINDINGS {
            return Err(invalid("command_outgoing_capacity"));
        }
        rows.into_iter()
            .map(
                |(row_id, native_id, acked, invalidated, quarantined, evidence)| {
                    Ok(NativeBinding {
                        cursor: SourceCursor {
                            kind: SourceKind::Outgoing,
                            row_id,
                        },
                        source: SourceKey {
                            group: key.group.clone(),
                            native_id,
                        },
                        publication: Some(key.clone()),
                        acked,
                        invalidated,
                        quarantined,
                        evidence: Some(PublishedEvidence::decode(&evidence)?),
                    })
                },
            )
            .collect()
    }
    pub(crate) fn command_pending_invalidations(&self) -> Result<Vec<SourceKey>> {
        let mut stmt=self.conn.prepare("SELECT group_id,native_id FROM command_early_invalidations ORDER BY group_id,native_id LIMIT 4097").map_err(db)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SourceKey {
                    group: r.get(0)?,
                    native_id: r.get(1)?,
                })
            })
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        if rows.len() > 4096 {
            return Err(invalid("command_invalidation_capacity"));
        }
        Ok(rows)
    }
    // Caller owns the serialized native actor and has just proved absence in a
    // fresh unfiltered fanout/effect scan; a bound source cannot be pruned here.
    pub(crate) fn command_retire_invalidation(&mut self, source: &SourceKey) -> Result<()> {
        self.conn.execute("DELETE FROM command_early_invalidations WHERE group_id=?1 AND native_id=?2 AND NOT EXISTS(SELECT 1 FROM command_outgoing_sources WHERE group_id=?1 AND native_id=?2)",params![source.group,source.native_id]).map_err(db)?;
        Ok(())
    }
    pub(crate) fn command_record_native_acks(
        &mut self,
        sources: &[SourceKey],
        publications: &[PublishedKey],
    ) -> Result<()> {
        if sources.len() > 64 || publications.len() > 64 {
            return Err(invalid("command_page_limit"));
        }
        let tx = self.conn.transaction().map_err(db)?;
        for s in sources {
            let changed = tx.execute(
                "UPDATE command_incoming_sources SET acked=1 WHERE group_id=?1 AND native_id=?2",
                params![s.group, s.native_id],
            ).map_err(db)?;
            if changed == 0 {
                return Err(invalid("command_source_unknown"));
            }
        }
        for p in publications {
            let changed=tx.execute("UPDATE command_outgoing_sources SET acked=1 WHERE group_id=?1 AND native_id=?2 AND event_id=?3 AND quarantined=0 AND evidence IS NOT NULL",params![p.key.group,p.native_id,p.key.app_event_id]).map_err(db)?;
            if changed == 0 {
                return Err(invalid("command_publication_unknown"));
            }
        }
        tx.commit().map_err(db)
    }
    #[cfg(test)]
    pub(crate) fn command_revoke_group(&mut self, group: &[u8]) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        revoke_group_in(&tx, group)?;
        tx.commit().map_err(db)
    }

    pub(crate) fn command_maintain(
        &mut self,
        now: u64,
        protected: &CommandProtection,
    ) -> Result<()> {
        let tx = self.conn.transaction().map_err(db)?;
        tx.execute(
            "UPDATE command_replay_state SET floor=max(floor,?1) WHERE id=1",
            [bound(now.saturating_sub(WEEK))?],
        )
        .map_err(db)?;
        let floor = tx
            .query_row(
                "SELECT floor FROM command_replay_state WHERE id=1",
                [],
                |r| sql_u64(r, 0),
            )
            .map_err(db)?;
        let mut stmt=tx.prepare("SELECT group_id,request_id,authority_valid,client,projected,slot_state,slot_operation IS NOT NULL,local_consumed FROM command_requests ORDER BY group_id,request_id LIMIT 257").map_err(db)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, bool>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, bool>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, bool>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            })
            .map_err(db)?;
        let slots = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?;
        drop(stmt);
        if slots.len() > SLOTS_GLOBAL {
            return Err(invalid("command_slot_capacity"));
        }
        for (
            group,
            request_id,
            authority_valid,
            client,
            projected,
            slot_state,
            registered,
            consumed,
        ) in slots
        {
            if protected.invalidation_groups.contains(&group) {
                continue;
            }
            if registered || slot_state == "pending" && !client {
                continue;
            }
            // Revocation makes local consumption impossible. It must not leak
            // a slot, but all native ACK, quarantine and protection gates below
            // still apply before anything is retired.
            if client && authority_valid && (!projected || !consumed) {
                continue;
            }
            let unacked:i64=tx.query_row("SELECT count(*) FROM command_incoming_sources WHERE group_id=?1 AND request_id=?2 AND acked=0",params![group,request_id],|r|r.get(0)).map_err(db)?;
            if unacked != 0 {
                continue;
            }
            let mut stmt=tx.prepare("SELECT event_id,state FROM command_publications WHERE group_id=?1 AND request_id=?2").map_err(db)?;
            let pubs = stmt
                .query_map(params![group, request_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(db)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db)?;
            drop(stmt);
            if pubs.is_empty() && slot_state != "suppressed" {
                continue;
            }
            let mut safe = true;
            for (event, state) in &pubs {
                let key = PublicationKey {
                    group: group.clone(),
                    app_event_id: event.clone(),
                };
                if protected.publications.contains(&key)
                    || !matches!(state.as_str(), "published" | "invalidated" | "expired")
                {
                    safe = false;
                    break;
                }
                let n:i64=tx.query_row("SELECT count(*) FROM command_outgoing_sources WHERE group_id=?1 AND event_id=?2 AND (acked=0 OR quarantined=1)",params![group,event],|r|r.get(0)).map_err(db)?;
                if n != 0 {
                    safe = false;
                    break;
                }
                let mut queued=tx.prepare("SELECT intent FROM command_queued_bindings WHERE group_id=?1 AND event_id=?2").map_err(db)?;
                let intents = queued
                    .query_map(params![group, event], |r| r.get::<_, Vec<u8>>(0))
                    .map_err(db)?;
                for intent in intents {
                    if protected
                        .queued
                        .contains(&(group.clone(), intent.map_err(db)?))
                    {
                        safe = false;
                    }
                }
            }
            if !safe {
                continue;
            }
            for (event, _) in pubs {
                tx.execute(
                    "DELETE FROM command_outgoing_sources WHERE group_id=?1 AND event_id=?2",
                    params![group, event],
                )
                .map_err(db)?;
                tx.execute(
                    "DELETE FROM command_queued_bindings WHERE group_id=?1 AND event_id=?2",
                    params![group, event],
                )
                .map_err(db)?;
                tx.execute(
                    "DELETE FROM command_publications WHERE group_id=?1 AND event_id=?2",
                    params![group, event],
                )
                .map_err(db)?;
            }
            tx.execute(
                "DELETE FROM command_incoming_sources WHERE group_id=?1 AND request_id=?2",
                params![group, request_id],
            )
            .map_err(db)?;
            tx.execute(
                "DELETE FROM command_requests WHERE group_id=?1 AND request_id=?2",
                params![group, request_id],
            )
            .map_err(db)?;
        }
        expire_payloads(&tx, now, protected)?;
        let settings = store_limits(&tx)?;
        let mut stmt=tx.prepare("SELECT server,principal,invocation FROM command_invocations WHERE active=0 AND created<=?1 AND operation IS NULL AND NOT EXISTS(SELECT 1 FROM command_requests r WHERE r.k=command_invocations.k) LIMIT 10001").map_err(db)?;
        let keys = stmt
            .query_map(
                [bound(now.saturating_sub(settings.payload_retention))?],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        drop(stmt);
        for (s, p, i) in keys {
            let key = InvocationKey {
                server: blob32(s)?,
                principal: blob32(p)?,
                invocation: i.try_into().map_err(|_| invalid("command_record"))?,
            };
            if wire::id_timestamp(key.invocation) >= floor {
                continue;
            }
            let record = load_record(&tx, &key)?.ok_or(invalid("command_record"))?;
            if !protected.groups.contains(record.owner.group())
                && !protected.invalidation_groups.contains(record.owner.group())
            {
                tx.execute(
                    "DELETE FROM command_invocations WHERE k=?1",
                    [key_bytes(&key)],
                )
                .map_err(db)?;
            }
        }
        tx.commit().map_err(db)
    }
}
fn expire_payloads(conn: &Connection, now: u64, protected: &CommandProtection) -> Result<()> {
    let settings = store_limits(conn)?;
    let cursor = conn
        .query_row(
            "SELECT payload_cursor FROM command_replay_state WHERE id=1",
            [],
            |r| sql_u64(r, 0),
        )
        .map_err(db)?;
    let mut stmt=conn.prepare("SELECT k,cursor,created,operation IS NOT NULL FROM command_invocations WHERE cursor>?1 ORDER BY cursor LIMIT 256").map_err(db)?;
    type Row = (Vec<u8>, u64, u64, bool);
    let mut rows: Vec<Row> = stmt
        .query_map([bound(cursor)?], |r| {
            Ok((r.get(0)?, sql_u64(r, 1)?, sql_u64(r, 2)?, r.get(3)?))
        })
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    drop(stmt);
    if rows.is_empty() && cursor != 0 {
        conn.execute(
            "UPDATE command_replay_state SET payload_cursor=0 WHERE id=1",
            [],
        )
        .map_err(db)?;
        let mut stmt=conn.prepare("SELECT k,cursor,created,operation IS NOT NULL FROM command_invocations ORDER BY cursor LIMIT 256").map_err(db)?;
        rows = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, sql_u64(r, 1)?, sql_u64(r, 2)?, r.get(3)?))
            })
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
    }
    for (key, cursor, created, registered) in rows {
        conn.execute(
            "UPDATE command_replay_state SET payload_cursor=?1 WHERE id=1",
            [bound(cursor)?],
        )
        .map_err(db)?;
        if registered
            || created
                .checked_add(settings.payload_retention)
                .is_none_or(|at| at > now)
        {
            continue;
        }
        let record = load_record(conn, &key_from_bytes(&key)?)?.ok_or(invalid("command_record"))?;
        if record.submit.is_none()
            || record.payload_required()
            || protected.groups.contains(record.owner.group())
            || protected.invalidation_groups.contains(record.owner.group())
        {
            continue;
        }
        let pending:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM command_requests WHERE k=?1 AND ((op='submit' AND slot_state='pending') OR (client=1 AND local_consumed=0)))",[&key],|r|r.get(0)).map_err(db)?;
        if pending {
            continue;
        }
        let mut stmt=conn.prepare("SELECT p.event_id FROM command_publications p JOIN command_requests r ON p.group_id=r.group_id AND p.request_id=r.request_id WHERE r.k=?1").map_err(db)?;
        let events = stmt
            .query_map([&key], |r| r.get::<_, String>(0))
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        drop(stmt);
        let mut native = false;
        for event in events {
            let publication = PublicationKey {
                group: record.owner.group.clone(),
                app_event_id: event,
            };
            if protected.publications.contains(&publication) {
                native = true;
                break;
            }
            let held:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM command_outgoing_sources WHERE group_id=?1 AND event_id=?2 AND (acked=0 OR quarantined=1)) OR EXISTS(SELECT 1 FROM command_queued_bindings WHERE group_id=?1 AND event_id=?2)",params![publication.group,publication.app_event_id],|r|r.get(0)).map_err(db)?;
            if held {
                native = true;
                break;
            }
        }
        if !native {
            conn.execute(
                "UPDATE command_invocations SET submit=NULL WHERE k=?1",
                [key],
            )
            .map_err(db)?;
        }
    }
    Ok(())
}

fn record_published(
    conn: &Connection,
    key: &PublicationKey,
    native_id: &[u8],
    evidence: &PublishedEvidence,
) -> Result<bool> {
    if native_id.is_empty() || native_id.len() > 1024 {
        return Err(invalid("command_native_binding"));
    }
    let state = publication_state(conn, key)?.ok_or(invalid("command_publication_unknown"))?;
    if state == "unsubmitted" {
        return Err(invalid("command_publication_unclaimed"));
    }
    let encoded = evidence.encode()?;
    type Existing = (String, Vec<u8>, bool);
    let old:Option<Existing>=conn.query_row("SELECT event_id,evidence,quarantined FROM command_outgoing_sources WHERE group_id=?1 AND native_id=?2",params![key.group,native_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db)?;
    if let Some((event, previous, quarantined)) = old {
        if quarantined || event != key.app_event_id || previous != encoded {
            conn.execute("UPDATE command_outgoing_sources SET quarantined=1 WHERE group_id=?1 AND native_id=?2",params![key.group,native_id]).map_err(db)?;
            conn.execute("UPDATE command_publications SET state='uncertain' WHERE group_id=?1 AND event_id IN(?2,?3) AND state NOT IN('invalidated','expired')",params![key.group,event,key.app_event_id]).map_err(db)?;
            return Ok(false);
        }
    } else {
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM command_outgoing_sources WHERE group_id=?1 AND event_id=?2",
                params![key.group, key.app_event_id],
                |r| r.get(0),
            )
            .map_err(db)?;
        if n >= MAX_BINDINGS as i64 {
            // Never grow the bounded binding mirror or acknowledge an unknown
            // third source. Quarantine the publication's retained evidence and
            // let unrelated publications continue through this batch.
            conn.execute("UPDATE command_outgoing_sources SET quarantined=1 WHERE group_id=?1 AND event_id=?2",params![key.group,key.app_event_id]).map_err(db)?;
            conn.execute("UPDATE command_publications SET state='uncertain' WHERE group_id=?1 AND event_id=?2 AND state NOT IN('invalidated','expired')",params![key.group,key.app_event_id]).map_err(db)?;
            return Ok(false);
        }
        conn.execute("INSERT INTO command_outgoing_sources(group_id,native_id,event_id,evidence) VALUES(?1,?2,?3,?4)",params![key.group,native_id,key.app_event_id,encoded]).map_err(db)?;
    }
    let early:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM command_early_invalidations WHERE group_id=?1 AND native_id=?2)",params![key.group,native_id],|r|r.get(0)).map_err(db)?;
    if early {
        invalidate_publication(conn, key)?;
        conn.execute(
            "UPDATE command_outgoing_sources SET invalidated=1 WHERE group_id=?1 AND native_id=?2",
            params![key.group, native_id],
        )
        .map_err(db)?;
        conn.execute(
            "DELETE FROM command_early_invalidations WHERE group_id=?1 AND native_id=?2",
            params![key.group, native_id],
        )
        .map_err(db)?;
    } else {
        conn.execute("UPDATE command_publications SET state='published' WHERE group_id=?1 AND event_id=?2 AND state NOT IN('invalidated','expired')",params![key.group,key.app_event_id]).map_err(db)?;
    }
    Ok(true)
}
fn record_invalidation(
    conn: &Connection,
    group: &[u8],
    native_id: &[u8],
    possible: Option<&PublicationKey>,
) -> Result<()> {
    if group.is_empty() || group.len() > 1024 || native_id.is_empty() || native_id.len() > 1024 {
        return Err(invalid("command_native_binding"));
    }
    let outgoing: Option<String> = conn
        .query_row(
            "SELECT event_id FROM command_outgoing_sources WHERE group_id=?1 AND native_id=?2",
            params![group, native_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    if let Some(event) = outgoing {
        invalidate_publication(
            conn,
            &PublicationKey {
                group: group.to_vec(),
                app_event_id: event,
            },
        )?;
        conn.execute(
            "UPDATE command_outgoing_sources SET invalidated=1 WHERE group_id=?1 AND native_id=?2",
            params![group, native_id],
        )
        .map_err(db)?;
    } else if let Some(possible) = possible {
        if possible.group != group || publication_state(conn, possible)?.is_none() {
            return Err(invalid("command_invalidation_proof"));
        }
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM command_early_invalidations WHERE group_id=?1 AND native_id=?2)",params![group,native_id],|r|r.get(0)).map_err(db)?;
        if !exists {
            let total: i64 = conn
                .query_row(
                    "SELECT count(*) FROM command_early_invalidations",
                    [],
                    |r| r.get(0),
                )
                .map_err(db)?;
            let local: i64 = conn
                .query_row(
                    "SELECT count(*) FROM command_early_invalidations WHERE group_id=?1",
                    [group],
                    |r| r.get(0),
                )
                .map_err(db)?;
            if total >= 4096 || local >= 256 {
                return Err(Error::Storage("command_invalidation_capacity"));
            }
            conn.execute(
                "INSERT INTO command_early_invalidations VALUES(?1,?2)",
                params![group, native_id],
            )
            .map_err(db)?;
        }
    }
    let key:Option<Vec<u8>>=conn.query_row("SELECT r.k FROM command_requests r JOIN command_incoming_sources s ON s.group_id=r.group_id AND s.request_id=r.request_id WHERE s.group_id=?1 AND s.native_id=?2",params![group,native_id],|r|r.get(0)).optional().map_err(db)?;
    if let Some(key) = key {
        let key = key_from_bytes(&key)?;
        if load_record(conn, &key)?.is_some() {
            revoke_record(conn, &key)?;
        }
    }
    Ok(())
}

fn publication_state(conn: &Connection, key: &PublicationKey) -> Result<Option<String>> {
    conn.query_row(
        "SELECT state FROM command_publications WHERE group_id=?1 AND event_id=?2",
        params![key.group, key.app_event_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(db)
}
fn invalidate_publication(conn: &Connection, key: &PublicationKey) -> Result<()> {
    conn.execute("UPDATE command_publications SET state='invalidated' WHERE group_id=?1 AND event_id=?2 AND state!='expired'",params![key.group,key.app_event_id]).map_err(db)?;
    let record:Option<Vec<u8>>=conn.query_row("SELECT r.k FROM command_requests r JOIN command_publications p ON p.group_id=r.group_id AND p.request_id=r.request_id WHERE p.group_id=?1 AND p.event_id=?2",params![key.group,key.app_event_id],|r|r.get(0)).optional().map_err(db)?;
    if let Some(key) = record {
        revoke_record(conn, &key_from_bytes(&key)?)?;
    }
    Ok(())
}
fn revoke_slots(conn: &Connection, key: &InvocationKey) -> Result<()> {
    conn.execute("UPDATE command_requests SET authority_valid=0,slot_state=CASE WHEN response_id IS NULL THEN 'suppressed' ELSE slot_state END WHERE k=?1",[key_bytes(key)]).map_err(db)?;
    Ok(())
}

fn revoke_record(conn: &Connection, key: &InvocationKey) -> Result<()> {
    if let Some(mut r) = load_record(conn, key)?
        && r.authority_valid
    {
        r.authority_valid = false;
        revoke_slots(conn, key)?;
        r.revision = increment(r.revision)?;
        if let Some(status) = &mut r.status {
            status.authority_valid = false;
        }
        save_record(conn, &r)?;
    }
    Ok(())
}

pub(super) fn revoke_group_in(conn: &Connection, group: &[u8]) -> Result<()> {
    let mut statement=conn.prepare("SELECT DISTINCT k FROM command_requests WHERE group_id=?1 UNION SELECT k FROM command_invocations WHERE json_extract(CAST(owner AS TEXT),'$.group')=json(?2)").map_err(db)?;
    let keys: Vec<Vec<u8>> = statement
        .query_map(params![group, encode(&group)?], |r| r.get(0))
        .map_err(db)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db)?;
    drop(statement);
    for key in keys {
        revoke_record(conn, &key_from_bytes(&key)?)?;
    }
    conn.execute("UPDATE command_requests SET authority_valid=0,slot_state=CASE WHEN response_id IS NULL THEN 'suppressed' ELSE slot_state END WHERE group_id=?1",[group]).map_err(db)?;
    conn.execute("UPDATE command_publications SET state='invalidated' WHERE group_id=?1 AND state='unsubmitted'",[group]).map_err(db)?;
    Ok(())
}

fn check_request_operation(request: &Request, operation: &EngineOperation) -> Result<()> {
    let matches = match (&request.op, operation) {
        (RequestOp::Status { .. }, EngineOperation::Observe) => true,
        (RequestOp::Cancel { .. }, EngineOperation::Cancel) => true,
        (RequestOp::Submit(a), EngineOperation::Dispatch { submit: b }) => a == b,
        (
            RequestOp::Poll {
                stdout_cursor: a,
                stderr_cursor: b,
                max_bytes: c,
                ..
            },
            EngineOperation::Poll {
                stdout_cursor: x,
                stderr_cursor: y,
                max_bytes: z,
            },
        ) => a == x && b == y && c == z,
        _ => false,
    };
    if !matches {
        return Err(invalid("command_operation_request"));
    }
    Ok(())
}
impl Journal {
    pub(crate) fn command_operation_request(
        &self,
        operation: OperationId,
    ) -> Result<Option<PendingCommandRequest>> {
        load_operation_request(&self.conn, operation)
    }
    pub(crate) fn command_operation(
        &self,
        operation: OperationId,
    ) -> Result<Option<OperationRecord>> {
        load_operation(&self.conn, operation)
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct EvidenceStream {
    pub produced: u64,
    pub closed: bool,
    pub gaps: u32,
    pub sha256: Option<String>,
}
#[derive(Clone, Serialize)]
pub(crate) struct CommandEvidence {
    pub cursor: u64,
    pub principal: String,
    pub group_sha256: String,
    pub session: String,
    pub invocation: String,
    pub submit_sha256: String,
    pub engine_admitted: Option<u8>,
    pub refusal: Option<wire::ErrorCode>,
    pub outcome: Option<wire::Outcome>,
    pub stdout: EvidenceStream,
    pub stderr: EvidenceStream,
    pub authority_valid: bool,
    pub worker_state: WorkerState,
    pub observed_seq: u64,
}
#[derive(Serialize)]
pub(crate) struct CommandEvidencePage {
    pub rows: Vec<CommandEvidence>,
    pub next: Option<u64>,
}
#[derive(Default)]
pub(crate) struct CommandCleanupCounts {
    pub active: u32,
    pub uncertain: u32,
    pub worker_running: u32,
    pub worker_unknown: u32,
    pub observation_pending: bool,
}
fn evidence_stream(status: Option<&wire::OutputStatus>) -> EvidenceStream {
    match status {
        Some(s) => EvidenceStream {
            produced: s.produced,
            closed: s.closed,
            gaps: s.gaps.len() as u32,
            sha256: None,
        },
        None => EvidenceStream {
            produced: 0,
            closed: false,
            gaps: 0,
            sha256: None,
        },
    }
}
impl Journal {
    pub(crate) fn command_allocate_fence(&mut self) -> Result<u64> {
        let tx = self.conn.transaction().map_err(db)?;
        let changed=tx.execute("UPDATE command_replay_state SET sequence=sequence+1 WHERE id=1 AND sequence<9223372036854775807",[]).map_err(db)?;
        if changed != 1 {
            return Err(invalid("command_counter"));
        }
        let seq = tx
            .query_row(
                "SELECT sequence FROM command_replay_state WHERE id=1",
                [],
                |r| sql_u64(r, 0),
            )
            .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(seq)
    }
    pub(crate) fn command_evidence(
        &self,
        after: Option<u64>,
        max: usize,
    ) -> Result<CommandEvidencePage> {
        let max = limit(max)?;
        let mut stmt=self.conn.prepare("SELECT server,principal,invocation FROM command_invocations WHERE server_side=1 AND cursor>?1 ORDER BY cursor LIMIT ?2").map_err(db)?;
        let rows = stmt
            .query_map(params![bound(after.unwrap_or(0))?, max + 1], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(db)?;
        let keys = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?;
        drop(stmt);
        let mut page = CommandEvidencePage {
            rows: Vec::new(),
            next: None,
        };
        for (server, principal, invocation) in keys {
            let key = InvocationKey {
                server: blob32(server)?,
                principal: blob32(principal)?,
                invocation: invocation
                    .try_into()
                    .map_err(|_| invalid("command_record"))?,
            };
            let r = self.command_get(&key)?.ok_or(invalid("command_record"))?;
            let row = CommandEvidence {
                cursor: r.cursor,
                principal: hex::encode(r.key.principal),
                group_sha256: hex::encode(sha(r.owner.group())),
                session: hex::encode(r.owner.binding.session),
                invocation: hex::encode(r.key.invocation),
                submit_sha256: hex::encode(r.digest),
                engine_admitted: if r.engine_known {
                    Some(1)
                } else if r.refusal.is_some() {
                    Some(0)
                } else {
                    None
                },
                refusal: r.refusal.as_ref().map(|r| r.code),
                outcome: r.status.as_ref().and_then(|s| s.outcome.clone()),
                stdout: evidence_stream(r.status.as_ref().map(|s| &s.stdout)),
                stderr: evidence_stream(r.status.as_ref().map(|s| &s.stderr)),
                authority_valid: r.authority_valid,
                worker_state: r.worker,
                observed_seq: r.observed_seq,
            };
            if encode(&row)?.len() > 4096 {
                return Err(invalid("command_evidence_size"));
            }
            if page.rows.len() >= max as usize {
                page.next = page.rows.last().map(|r| r.cursor);
                break;
            }
            page.rows.push(row);
            if encode(&page)?.len() > 64000 {
                page.rows.pop();
                page.next = page.rows.last().map(|r| r.cursor);
                if page.rows.is_empty() {
                    return Err(invalid("command_evidence_size"));
                }
                break;
            }
        }
        Ok(page)
    }
    pub(crate) fn command_cleanup_counts(&self) -> Result<CommandCleanupCounts> {
        let mut counts = CommandCleanupCounts::default();
        let mut stmt=self.conn.prepare("SELECT facts,active,operation IS NOT NULL FROM command_invocations WHERE server_side=1 LIMIT 10001").map_err(db)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, bool>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })
            .map_err(db)?;
        let mut seen = 0;
        for row in rows {
            seen += 1;
            if seen > RETAINED_GLOBAL {
                return Err(invalid("command_retained_capacity"));
            }
            let (facts, active, operation) = row.map_err(db)?;
            let f: Facts = decode(&facts)?;
            counts.active += u32::from(active);
            counts.uncertain += u32::from(
                f.uncertain
                    || (!f.engine_known && f.admission == Admission::Dispatching)
                    || f.status
                        .as_ref()
                        .is_some_and(|s| matches!(s.state, State::UnknownLaunch | State::Lost)),
            );
            counts.worker_running += u32::from(f.worker == WorkerState::Running);
            counts.worker_unknown +=
                u32::from(f.refusal.is_none() && f.worker == WorkerState::Unknown);
            counts.observation_pending |= operation;
        }
        Ok(counts)
    }
}

pub(crate) struct CompletionProjection {
    pub record: InvocationRecord,
    pub decision: CommittedDecision,
    pub response: Option<Response>,
    pub suppressed: bool,
    pub failed_attempts: u8,
}
pub(crate) fn project_completion(
    original: &InvocationRecord,
    operation: &OperationRecord,
    completion: &EngineCompletion,
    pending: Option<&PendingCommandRequest>,
) -> Result<CompletionProjection> {
    if original.key != completion.key
        || operation.key != completion.key
        || operation.id != completion.operation
        || operation.revision != completion.revision
        || operation.owner != original.owner
    {
        return Err(invalid("command_operation_mismatch"));
    }
    let mut record = original.clone();
    let current = record.revision == completion.revision;

    let decision = match &completion.result {
        EngineObservation::Missing => {
            record.uncertain = record.engine_known;
            record.worker = WorkerState::Unknown;
            if record.engine_known {
                record.refusal = Some(WireError {
                    code: ErrorCode::ResultExpired,
                    detail: None,
                });
                record.warning = Some(Warning::InputFailed);
                record.feeder = FeederState::Failed;
                CommittedDecision::KnownFound
            } else {
                record.admission = Admission::Accepted;
                CommittedDecision::DefinitelyUnsubmitted
            }
        }
        EngineObservation::Refused(error) => {
            if !record.engine_known {
                record.refusal = Some(error.clone());
                record.feeder = FeederState::Failed;
                CommittedDecision::DefinitivelyRefused
            } else {
                CommittedDecision::KnownFound
            }
        }
        EngineObservation::Failed(failure) => {
            // Report only a closed local category; engine error text and command
            // material never enter diagnostics. All categories retain uncertainty.
            crate::diagnostics::event(
                "command_observation",
                match failure {
                    OperationalFailure::Unavailable => "unavailable",
                    OperationalFailure::Storage => "storage",
                    OperationalFailure::Uncertain => "uncertain",
                    #[cfg(test)]
                    OperationalFailure::Timeout => "timeout",
                },
            );
            record.uncertain = true;
            record.worker = WorkerState::Unknown;
            CommittedDecision::StillUncertain
        }
        EngineObservation::Found { status, worker }
        | EngineObservation::Dispatched { status, worker }
        | EngineObservation::Input { status, worker, .. }
        | EngineObservation::Polled { status, worker, .. } => {
            if status.invocation != record.key.invocation || !status.engine_known {
                return Err(invalid("command_engine_observation"));
            }
            record.engine_known = true;
            record.uncertain = false;
            record.admission = Admission::Engine;
            record.worker = *worker;
            let mut status = status.clone();
            status.authority_valid = record.authority_valid;
            status.admission = Admission::Engine;
            let empty_input_exited = status.state == State::Exited
                && record.stdin_length == 0
                && status.input.accepted == 0
                && status.input.delivered == 0;
            if empty_input_exited {
                // Completing an empty immutable input needs no EOF acceptance
                // proof once the process has exited. Preserve eof_sent as observed.
                record.feeder = FeederState::Complete;
            } else if current
                && let EngineObservation::Input {
                    generation, credit, ..
                } = &completion.result
            {
                advance_feeder(
                    &mut record,
                    &operation.operation,
                    &status,
                    *generation,
                    *credit,
                )?;
            }
            if status.state == State::Exited
                && !matches!(record.feeder, FeederState::Complete | FeederState::Failed)
            {
                if status.input.delivered < record.stdin_length
                    && !matches!(
                        status.input.health,
                        InputHealth::Gap | InputHealth::Uncertain
                    )
                {
                    record.warning = Some(Warning::StdinUnconsumed);
                } else {
                    record.warning = Some(Warning::InputFailed);
                }
                record.feeder = FeederState::Failed;
            }
            if record.warning.is_some() {
                status.warning = record.warning;
            }
            record.status = Some(status);
            CommittedDecision::KnownFound
        }
    };

    let mut projection = CompletionProjection {
        record,
        decision,
        response: None,
        suppressed: false,
        failed_attempts: pending.map(|p| p.failed_attempts).unwrap_or(0),
    };
    if let Some(pending) = pending {
        if pending.operation != Some(completion.operation)
            || pending.operation_revision != Some(completion.revision)
            || pending.owner != original.owner
            || pending.request.op.invocation() != original.key.invocation
            || operation.request_id != Some(pending.request.id)
            || pending.response.is_some()
        {
            return Err(invalid("command_operation_request"));
        }
        check_request_operation(&pending.request, &operation.operation)?;
        if !projection.record.authority_valid {
            projection.suppressed = true;
            return Ok(projection);
        }
        let value = if current {
            response_value(&projection.record, &pending.request, &completion.result)?
        } else {
            None
        };
        let result = if let Some(value) = value {
            Some(value)
        } else {
            projection.failed_attempts = pending.failed_attempts.saturating_add(1).min(3);
            (projection.failed_attempts == 3).then_some({
                Err(WireError {
                    code: ErrorCode::RecoveryUncertain,
                    detail: None,
                })
            })
        };
        projection.response = result.map(|result| Response {
            binding: pending.request.binding.clone(),
            id: pending.request.id,
            op: pending.request.op.op(),
            request_sha256: pending.digest,
            result,
        });
    }
    Ok(projection)
}
fn response_value(
    record: &InvocationRecord,
    request: &Request,
    observation: &EngineObservation,
) -> Result<Option<std::result::Result<wire::ResponseResult, WireError>>> {
    if let Some(refusal) = &record.refusal {
        return Ok(Some(Err(refusal.clone())));
    }
    if matches!(observation, EngineObservation::Failed(_)) {
        return Ok(None);
    }
    let status = record
        .status
        .clone()
        .unwrap_or_else(|| accepted_status(record));
    let value = match (&request.op, observation) {
        (
            RequestOp::Status { .. },
            EngineObservation::Found { .. }
            | EngineObservation::Input { .. }
            | EngineObservation::Missing,
        ) => wire::ResponseResult::Status(status),
        (
            RequestOp::Cancel { .. },
            EngineObservation::Found { .. } | EngineObservation::Missing,
        ) => wire::ResponseResult::Cancel(status),
        (
            RequestOp::Poll {
                stdout_cursor,
                stderr_cursor,
                max_bytes,
                ..
            },
            EngineObservation::Polled { stdout, stderr, .. },
        ) => {
            if stdout.bytes.len() + stderr.bytes.len() > *max_bytes as usize
                || stdout.next_cursor < *stdout_cursor
                || stderr.next_cursor < *stderr_cursor
            {
                return Err(invalid("command_poll_observation"));
            }
            wire::ResponseResult::Poll {
                status,
                stdout: stdout.clone(),
                stderr: stderr.clone(),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(Ok(value)))
}
fn accepted_status(record: &InvocationRecord) -> Status {
    Status {
        admission: record.admission,
        authority_valid: record.authority_valid,
        cancel_requested: false,
        engine_known: record.engine_known,
        input: wire::InputStatus {
            accepted: 0,
            delivered: 0,
            eof_sent: false,
            health: InputHealth::Open,
            length: record.stdin_length,
        },
        invocation: record.key.invocation,
        outcome: None,
        state: State::Accepted,
        stderr: wire::OutputStatus {
            closed: false,
            gaps: Vec::new(),
            produced: 0,
            retained: 0,
        },
        stdout: wire::OutputStatus {
            closed: false,
            gaps: Vec::new(),
            produced: 0,
            retained: 0,
        },
        warning: record.warning,
    }
}

fn load_request(
    conn: &Connection,
    group: &[u8],
    id: &[u8],
) -> Result<Option<PendingCommandRequest>> {
    type Row = (
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Option<String>,
        u8,
        Option<Vec<u8>>,
        Option<u64>,
    );
    let row:Option<Row>=conn.query_row("SELECT k,owner,request,digest,response_id,failed_attempts,slot_operation,slot_revision FROM command_requests WHERE group_id=?1 AND request_id=?2",params![group,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,super::sql_optional_u64(r,7)?))).optional().map_err(db)?;
    row.map(
        |(key, owner, bytes, digest, response, failed_attempts, operation, operation_revision)| {
            let key = key_from_bytes(&key)?;
            let owner = decode_owner(&owner, &key)?;
            check_local_binding(conn, &owner)?;
            let request = wire::decode_request(&bytes)?;
            check_request_owner(&request, &owner)?;
            if owner.group() != group
                || request.id.as_slice() != id
                || request.op.invocation() != key.invocation
                || wire::request_digest(&bytes) != blob32(digest.clone())?
                || failed_attempts > 3
            {
                return Err(invalid("command_record"));
            }
            Ok(PendingCommandRequest {
                owner,
                request,
                digest: blob32(digest)?,
                response: response.map(|app_event_id| PublicationKey {
                    group: group.to_vec(),
                    app_event_id,
                }),
                failed_attempts,
                operation: operation
                    .map(|b| {
                        b.try_into()
                            .map(OperationId)
                            .map_err(|_| invalid("command_record"))
                    })
                    .transpose()?,
                operation_revision,
            })
        },
    )
    .transpose()
}
fn load_operation(conn: &Connection, id: OperationId) -> Result<Option<OperationRecord>> {
    type Row = (Vec<u8>, u64, Vec<u8>, Option<Vec<u8>>);
    let row:Option<Row>=conn.query_row("SELECT k,operation_revision,operation_kind,operation_request FROM command_invocations WHERE operation=?1",[id.0.as_slice()],|r|Ok((r.get(0)?,sql_u64(r,1)?,r.get(2)?,r.get(3)?))).optional().map_err(db)?;
    row.map(|(key, revision, operation, request)| {
        let key = key_from_bytes(&key)?;
        let r = load_record(conn, &key)?.ok_or(invalid("command_record"))?;
        Ok(OperationRecord {
            id,
            key,
            owner: r.owner.clone(),
            revision,
            engine_known: r.engine_known,
            ledger: r.ledger(),
            operation: decode(&operation)?,
            request_id: request
                .map(|b| b.try_into().map_err(|_| invalid("command_record")))
                .transpose()?,
        })
    })
    .transpose()
}
fn load_operation_request(
    conn: &Connection,
    id: OperationId,
) -> Result<Option<PendingCommandRequest>> {
    let row: Option<(Vec<u8>, Vec<u8>)> = conn
        .query_row(
            "SELECT group_id,request_id FROM command_requests WHERE slot_operation=?1",
            [id.0.as_slice()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    row.map(|(g, i)| load_request(conn, &g, &i)?.ok_or(invalid("command_record")))
        .transpose()
}
impl Journal {
    pub(crate) fn command_find_request(
        &self,
        owner: &Owner,
        request: &Request,
    ) -> Result<Option<PendingCommandRequest>> {
        let bytes = wire::encode_request(request)?;
        check_request_owner(request, owner)?;
        let pending = load_request(&self.conn, owner.group(), &request.id)?;
        if let Some(p) = &pending
            && (p.owner != *owner || wire::encode_request(&p.request)? != bytes)
        {
            return Err(invalid("command_request_conflict"));
        }
        Ok(pending)
    }
    pub(crate) fn command_pending(&self, max: usize) -> Result<Vec<PendingCommandRequest>> {
        if max == 0 || max > SLOTS_GLOBAL {
            return Err(invalid("command_limit"));
        }
        let mut statement=self.conn.prepare("SELECT group_id,request_id FROM command_requests WHERE client=0 AND authority_valid=1 AND slot_state='pending' AND failed_attempts<3 AND slot_operation IS NULL ORDER BY created,request_id LIMIT ?1").map_err(db)?;
        let ids = statement
            .query_map([max as i64], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        ids.into_iter()
            .map(|(g, i)| load_request(&self.conn, &g, &i)?.ok_or(invalid("command_record")))
            .collect()
    }
    pub(crate) fn command_read_operations(&self, max: usize) -> Result<Vec<OperationRecord>> {
        if max == 0 || max > SLOTS_GLOBAL {
            return Err(invalid("command_limit"));
        }
        let mut statement=self.conn.prepare("SELECT operation FROM command_invocations WHERE operation_request IS NOT NULL ORDER BY cursor LIMIT ?1").map_err(db)?;
        let ids = statement
            .query_map([max as i64], |r| r.get::<_, Vec<u8>>(0))
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db)?;
        ids.into_iter()
            .map(|b| {
                let id = OperationId(b.try_into().map_err(|_| invalid("command_record"))?);
                let operation = load_operation(&self.conn, id)?.ok_or(invalid("command_record"))?;
                if !matches!(
                    operation.operation,
                    EngineOperation::Observe
                        | EngineOperation::Poll { .. }
                        | EngineOperation::Cancel
                ) {
                    return Err(invalid("command_recovery_operation"));
                }
                Ok(operation)
            })
            .collect()
    }
}

fn check_local_binding(conn: &Connection, owner: &Owner) -> Result<()> {
    let account: String = conn
        .query_row("SELECT account FROM binding", [], |r| r.get(0))
        .map_err(db)?;
    if account != hex::encode(owner.local()) {
        return Err(invalid("command_owner"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_evidence::PublishedEvidence;
    use crate::command_wire::{Disposition, InputStatus, OutputStatus, ResponseResult};
    use std::{fs, os::unix::fs::PermissionsExt};
    const NOW: u64 = 1_000_000;
    fn id(sequence: u16) -> Id {
        let mut id = [0; 16];
        id[..6].copy_from_slice(&NOW.to_be_bytes()[2..]);
        id[14..].copy_from_slice(&sequence.to_be_bytes());
        id
    }
    fn setup() -> (tempfile::TempDir, Journal) {
        let tmp = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
            .unwrap();
        let mut journal = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        journal.command_init().unwrap();
        (tmp, journal)
    }
    fn owner() -> Owner {
        Owner::from_context(&AuthenticatedCommandContext::test_fixture(
            [2; 32],
            [1; 32],
            b"command-group".to_vec(),
            id(0),
            CommandDirection::Server,
        ))
    }
    fn submit(sequence: u16) -> Request {
        Request {
            binding: owner().binding.clone(),
            id: id(sequence + 100),
            op: RequestOp::Submit(Submit {
                invocation: id(sequence),
                argv: vec![b"/bin/cat".to_vec(), vec![255, 1]],
                stdin: vec![0, 255, 1, 2],
            }),
        }
    }
    fn status(request: &Request) -> Status {
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
                length: match &request.op {
                    RequestOp::Submit(s) => s.stdin.len() as u32,
                    _ => 4,
                },
            },
            invocation: request.op.invocation(),
            outcome: None,
            state: State::Accepted,
            stderr: OutputStatus {
                closed: false,
                gaps: vec![],
                produced: 0,
                retained: 0,
            },
            stdout: OutputStatus {
                closed: false,
                gaps: vec![],
                produced: 0,
                retained: 0,
            },
            warning: None,
        }
    }
    fn evidence(epoch: u64) -> PublishedEvidence {
        PublishedEvidence::from_native(&marmot_session::PublishedApplicationMessage {
            group_id: cgka_traits::GroupId::new(vec![1]),
            app_event_id: "a".repeat(64),
            message_id: cgka_traits::MessageId::new(vec![1]),
            source_epoch: cgka_traits::EpochId(epoch),
            retention: cgka_traits::app_event::AppMessageRetentionDecision {
                retention_seconds: 86400,
                expires_at: None,
            },
            authority: None,
        })
    }
    fn response(request: &Request) -> Response {
        let s = status(request);
        let result = match request.op {
            RequestOp::Submit(_) => ResponseResult::Submit {
                disposition: Disposition::Accepted,
                status: s,
            },
            RequestOp::Status { .. } => ResponseResult::Status(s),
            RequestOp::Poll { .. } => ResponseResult::Poll {
                status: s,
                stdout: Chunk {
                    bytes: vec![],
                    next_cursor: 0,
                },
                stderr: Chunk {
                    bytes: vec![],
                    next_cursor: 0,
                },
            },
            RequestOp::Cancel { .. } => ResponseResult::Cancel(s),
            RequestOp::Finish { .. } => ResponseResult::Finish { finished: true },
        };
        Response {
            binding: request.binding.clone(),
            id: request.id,
            op: request.op.op(),
            request_sha256: wire::request_digest(&wire::encode_request(request).unwrap()),
            result: Ok(result),
        }
    }
    fn publication(owner: &Owner, response: &Response) -> PreparedCommandPublication {
        crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            response.id,
            response.request_sha256,
            wire::encode_response(response).unwrap(),
            owner.binding.server,
        )
        .unwrap()
    }
    fn projection(request: Request) -> CommandProjection {
        let owner = owner();
        let bytes = wire::encode_request(&request).unwrap();
        let request_event = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            request.id,
            wire::request_digest(&bytes),
            bytes,
            owner.binding.client,
        )
        .unwrap();
        CommandProjection {
            source: CommandSource {
                group: owner.group.clone(),
                native_id: request.id.to_vec(),
                app_event_id: request_event.key.app_event_id,
                author: owner.binding.client,
                authority_metadata: b"authenticated".to_vec(),
            },
            response: Some(publication(&owner, &response(&request))),
            owner,
            request,
            decision: AdmissionDecision::New,
        }
    }
    fn admitted(journal: &mut Journal, sequence: u16) -> (CommandProjection, InvocationKey) {
        let plan = projection(submit(sequence));
        let key = journal.command_project_request(&plan, NOW).unwrap().key;
        (plan, key)
    }
    fn fact(j: &mut Journal, key: &InvocationKey, state: State) -> Status {
        let r = j.command_get(key).unwrap().unwrap();
        let op = j
            .command_begin_operation(key, r.revision, EngineOperation::Observe)
            .unwrap();
        let mut s = status(&submit(1));
        s.invocation = key.invocation;
        s.engine_known = true;
        s.admission = Admission::Engine;
        s.state = state;
        j.command_commit_completion(
            &EngineCompletion {
                operation: op.id,
                key: key.clone(),
                revision: op.revision,
                result: EngineObservation::Found {
                    status: s.clone(),
                    worker: if state == State::Exited {
                        WorkerState::Exited
                    } else {
                        WorkerState::Running
                    },
                },
            },
            &[],
        )
        .unwrap();
        s
    }
    fn totals(j: &Journal) -> (usize, usize, usize) {
        (
            count(&j.conn, "SELECT count(*) FROM command_invocations", None).unwrap(),
            count(&j.conn, "SELECT count(*) FROM command_requests", None).unwrap(),
            count(&j.conn, "SELECT count(*) FROM command_publications", None).unwrap(),
        )
    }

    fn exited_status(request: &Request) -> Status {
        let mut s = status(request);
        s.engine_known = true;
        s.admission = Admission::Engine;
        s.state = State::Exited;
        s.stdout.closed = true;
        s.stderr.closed = true;
        s.outcome = Some(wire::Outcome {
            kind: wire::OutcomeKind::Code,
            value: Some(7),
        });
        s
    }
    fn finish_request(j: &Journal, key: &InvocationKey, seq: u16) -> Request {
        let r = j.command_get(key).unwrap().unwrap();
        let s = r.status.as_ref().unwrap();
        Request {
            binding: r.owner.binding.clone(),
            id: id(seq),
            op: RequestOp::Finish {
                invocation: key.invocation,
                submit_sha256: r.digest,
                terminal_sha256: wire::finish_digest(
                    r.owner.group(),
                    r.owner.binding(),
                    &r.digest,
                    s,
                )
                .unwrap(),
                stdout_cursor: s.stdout.produced,
                stderr_cursor: s.stderr.produced,
            },
        }
    }
    fn finish_projection(j: &Journal, request: Request) -> CommandProjection {
        let mut p = projection(request);
        let result = j.command_finish_result(&p.owner, &p.request).unwrap();
        let mut r = response(&p.request);
        r.result = result.clone();
        p.response = Some(publication(&p.owner, &r));
        p.decision = match result {
            Ok(_) => AdmissionDecision::Existing,
            Err(e) => AdmissionDecision::Refused(e),
        };
        p
    }
    #[test]
    fn finish_is_durable_idempotent_and_never_recounts_revocation() {
        let (tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        fact(&mut j, &key, State::Exited);
        let mut r = j.command_get(&key).unwrap().unwrap();
        r.status = Some(exited_status(&submit(1)));
        save_record(&j.conn, &r).unwrap();
        let p = finish_projection(&j, finish_request(&j, &key, 200));
        j.command_project_request(&p, NOW).unwrap();
        assert_eq!(j.command_admission_budget().unwrap().finished, 1);
        let retry = CommandProjection {
            owner: p.owner.clone(),
            source: p.source.clone(),
            request: p.request.clone(),
            decision: AdmissionDecision::Existing,
            response: None,
        };
        assert!(j.command_project_request(&retry, NOW).unwrap().existing);
        let second = finish_projection(&j, finish_request(&j, &key, 201));
        j.command_project_request(&second, NOW).unwrap();
        assert_eq!(j.command_admission_budget().unwrap().finished, 1);
        let mut conflict = finish_request(&j, &key, 202);
        if let RequestOp::Finish {
            terminal_sha256, ..
        } = &mut conflict.op
        {
            terminal_sha256[0] ^= 1;
        }
        let conflict = finish_projection(&j, conflict);
        assert!(matches!(
            wire::decode_response(&conflict.response.as_ref().unwrap().encoded_wire)
                .unwrap()
                .result,
            Err(WireError {
                code: ErrorCode::InvocationConflict,
                ..
            })
        ));
        j.command_project_request(&conflict, NOW).unwrap();
        j.command_revoke_group(owner().group()).unwrap();
        assert_eq!(j.command_admission_budget().unwrap().finished, 1);
        drop(j);
        let j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        assert_eq!(j.command_admission_budget().unwrap().finished, 1);
    }
    #[test]
    fn finish_preparation_failure_rolls_back_and_forged_claim_is_not_counted() {
        let (_tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        fact(&mut j, &key, State::Exited);
        let mut r = j.command_get(&key).unwrap().unwrap();
        r.status = Some(exited_status(&submit(1)));
        save_record(&j.conn, &r).unwrap();
        let mut request = finish_request(&j, &key, 200);
        if let RequestOp::Finish {
            terminal_sha256, ..
        } = &mut request.op
        {
            terminal_sha256[0] ^= 1;
        }
        let mut p = finish_projection(&j, request);
        assert!(matches!(
            wire::decode_response(&p.response.as_ref().unwrap().encoded_wire)
                .unwrap()
                .result,
            Err(WireError {
                code: ErrorCode::Busy,
                ..
            })
        ));
        let before = totals(&j);
        let mut forged = response(&p.request);
        forged.result = Ok(ResponseResult::Finish { finished: true });
        p.response = Some(publication(&p.owner, &forged));
        assert!(j.command_project_request(&p, NOW).is_err());
        assert_eq!(totals(&j), before);
        assert_eq!(j.command_admission_budget().unwrap().finished, 0);
    }
    #[test]
    fn client_final_poll_consumption_and_finish_are_one_commit() {
        let (tmp, mut j, owner) = client_setup();
        let request = submit(1);
        let answer = response(&request);
        client_exchange(&mut j, &owner, &request, &answer);
        let sha = j
            .command_client_response(&owner, request.id)
            .unwrap()
            .unwrap()
            .1;
        j.command_consume_response(&owner, request.id, sha, None)
            .unwrap();
        let poll = Request {
            binding: owner.binding.clone(),
            id: id(300),
            op: RequestOp::Poll {
                invocation: id(1),
                stdout_cursor: 0,
                stderr_cursor: 0,
                max_bytes: 8192,
            },
        };
        let mut answer = response(&poll);
        let mut terminal = exited_status(&poll);
        terminal.stdout.produced = 3;
        terminal.stdout.retained = 3;
        answer.result = Ok(ResponseResult::Poll {
            status: terminal,
            stdout: Chunk {
                bytes: b"abc".to_vec(),
                next_cursor: 3,
            },
            stderr: Chunk {
                bytes: vec![],
                next_cursor: 0,
            },
        });
        client_exchange(&mut j, &owner, &poll, &answer);
        let sha = j
            .command_client_response(&owner, poll.id)
            .unwrap()
            .unwrap()
            .1;
        let candidate = j
            .command_finish_candidate(
                &owner,
                poll.id,
                sha,
                Some(ClientDelivery::default()),
                id(301),
            )
            .unwrap()
            .unwrap();
        let encoded = wire::encode_request(&candidate).unwrap();
        let mut outgoing = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            candidate.id,
            wire::request_digest(&encoded),
            encoded,
            owner.local(),
        )
        .unwrap();
        let before = totals(&j);
        outgoing.request_sha256[0] ^= 1;
        assert!(
            j.command_consume_and_finish(
                &owner,
                poll.id,
                sha,
                Some(ClientDelivery::default()),
                Some(&outgoing),
                NOW
            )
            .is_err()
        );
        assert_eq!(totals(&j), before);
        assert_eq!(j.command_client_cursors(&owner, id(1)).unwrap().stdout, 0);
        outgoing.request_sha256[0] ^= 1;
        let (delivery, finish) = j
            .command_consume_and_finish(
                &owner,
                poll.id,
                sha,
                Some(ClientDelivery::default()),
                Some(&outgoing),
                NOW,
            )
            .unwrap();
        assert_eq!(delivery.stdout, 3);
        assert_eq!(finish, Some(id(301)));
        assert_eq!(
            j.command_client_request_invocation(&owner, id(301))
                .unwrap(),
            Some(id(1))
        );
        assert_eq!(
            facts(&j.conn, &owner.key(id(1))).unwrap().finish.attempts,
            1
        );
        drop(j);
        let mut j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([1; 32]),
        )
        .unwrap();
        let (delivery, finish) = j
            .command_consume_and_finish(
                &owner,
                poll.id,
                sha,
                Some(ClientDelivery::default()),
                None,
                NOW,
            )
            .unwrap();
        assert_eq!(delivery.stdout, 3);
        assert_eq!(finish, Some(id(301)));
        assert_eq!(
            facts(&j.conn, &owner.key(id(1)))
                .unwrap()
                .finish
                .latest_request,
            Some(id(301))
        );
        let pending = j.command_client_finish_state(&owner, id(1)).unwrap();
        assert!(!pending.confirmed);
        assert_eq!(pending.attempts, 1);
        let answer = response(&candidate);
        let incoming = publication(&owner, &answer);
        j.command_project_response(
            &CommandSource {
                group: owner.group.clone(),
                native_id: vec![99],
                app_event_id: incoming.key.app_event_id,
                author: owner.binding.server,
                authority_metadata: b"auth".to_vec(),
            },
            &answer,
            NOW,
        )
        .unwrap();
        assert!(
            j.command_client_finish_state(&owner, id(1))
                .unwrap()
                .confirmed
        );
    }
    #[test]
    fn schema_four_upgrade_preserves_counter_and_defaults_finish_facts() {
        let (tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        fact(&mut j, &key, State::Running);
        let mut f: serde_json::Value =
            serde_json::from_slice(&encode(&facts(&j.conn, &key).unwrap()).unwrap()).unwrap();
        f.as_object_mut().unwrap().remove("finish");
        j.conn
            .execute(
                "UPDATE command_invocations SET facts=?2 WHERE k=?1",
                params![key_bytes(&key), serde_json::to_vec(&f).unwrap()],
            )
            .unwrap();
        // Actual schema-4 shape, not a current table with just its version changed.
        j.conn.execute_batch("ALTER TABLE command_invocations RENAME TO old_invocations;
            CREATE TABLE command_invocations(cursor INTEGER PRIMARY KEY AUTOINCREMENT,k BLOB NOT NULL UNIQUE,server BLOB NOT NULL,principal BLOB NOT NULL,invocation BLOB NOT NULL,owner BLOB NOT NULL,submit BLOB,stdin_length INTEGER NOT NULL CHECK(stdin_length BETWEEN 0 AND 16384),stdout_delivered INTEGER NOT NULL DEFAULT 0,stderr_delivered INTEGER NOT NULL DEFAULT 0,digest BLOB NOT NULL,facts BLOB NOT NULL,active INTEGER NOT NULL,created INTEGER NOT NULL,operation BLOB,operation_revision INTEGER,operation_kind TEXT,operation_request BLOB,server_side INTEGER NOT NULL);
            INSERT INTO command_invocations SELECT cursor,k,server,principal,invocation,owner,submit,stdin_length,stdout_delivered,stderr_delivered,digest,facts,active,created,operation,operation_revision,operation_kind,operation_request,server_side FROM old_invocations;
            DROP TABLE old_invocations;
            ALTER TABLE command_replay_state DROP COLUMN finished; PRAGMA user_version=4;").unwrap();
        drop(j);
        let mut j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        let b = j.command_admission_budget().unwrap();
        assert_eq!(b.admitted, 1);
        assert_eq!(b.finished, 0);
        assert!(!facts(&j.conn, &key).unwrap().finish.client_finished);
        assert!(matches!(
            check_original_submit(&j.conn, &key, id(101)),
            Err(Error::Invalid("command_legacy_handle_unavailable"))
        ));
        let (_, fresh) = admitted(&mut j, 2);
        assert!(j.command_get(&fresh).unwrap().is_some());
        drop(j);
        let j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        assert_eq!(j.command_admission_budget().unwrap().admitted, 1);
        assert!(j.command_get(&fresh).unwrap().is_some());
    }
    #[test]
    fn busy_finish_repoll_has_four_persisted_attempts_and_never_returns_stale_busy_id() {
        let (tmp, mut j, owner) = client_setup();
        let request = submit(1);
        client_exchange(&mut j, &owner, &request, &response(&request));
        let sha = j
            .command_client_response(&owner, request.id)
            .unwrap()
            .unwrap()
            .1;
        j.command_consume_response(&owner, request.id, sha, None)
            .unwrap();
        for attempt in 0..=4u16 {
            let poll = Request {
                binding: owner.binding.clone(),
                id: id(300 + attempt * 2),
                op: RequestOp::Poll {
                    invocation: id(1),
                    stdout_cursor: 0,
                    stderr_cursor: 0,
                    max_bytes: 8192,
                },
            };
            let mut answer = response(&poll);
            answer.result = Ok(ResponseResult::Poll {
                status: exited_status(&poll),
                stdout: Chunk {
                    bytes: vec![],
                    next_cursor: 0,
                },
                stderr: Chunk {
                    bytes: vec![],
                    next_cursor: 0,
                },
            });
            let (poll_source, poll_publication) = client_exchange(&mut j, &owner, &poll, &answer);
            let sha = j
                .command_client_response(&owner, poll.id)
                .unwrap()
                .unwrap()
                .1;
            let candidate = j
                .command_finish_candidate(
                    &owner,
                    poll.id,
                    sha,
                    Some(ClientDelivery::default()),
                    id(301 + attempt * 2),
                )
                .unwrap();
            if attempt == 4 {
                assert!(candidate.is_none());
                assert!(
                    j.command_consume_and_finish(
                        &owner,
                        poll.id,
                        sha,
                        Some(ClientDelivery::default()),
                        None,
                        NOW
                    )
                    .unwrap()
                    .1
                    .is_none()
                );
                break;
            }
            let finish = candidate.unwrap();
            let encoded = wire::encode_request(&finish).unwrap();
            let pub_ = crate::mdk_adapter::prepare_command_publication(
                owner.group(),
                owner.binding.clone(),
                finish.id,
                wire::request_digest(&encoded),
                encoded,
                owner.local(),
            )
            .unwrap();
            assert_eq!(
                j.command_consume_and_finish(
                    &owner,
                    poll.id,
                    sha,
                    Some(ClientDelivery::default()),
                    Some(&pub_),
                    NOW
                )
                .unwrap()
                .1,
                Some(finish.id)
            );
            assert_eq!(
                j.command_consume_and_finish(
                    &owner,
                    poll.id,
                    sha,
                    Some(ClientDelivery::default()),
                    None,
                    NOW
                )
                .unwrap()
                .1,
                Some(finish.id)
            );
            let mut busy = response(&finish);
            busy.result = Err(WireError {
                code: ErrorCode::Busy,
                detail: None,
            });
            client_exchange(&mut j, &owner, &finish, &busy);
            let sha = j
                .command_client_response(&owner, finish.id)
                .unwrap()
                .unwrap()
                .1;
            j.command_consume_and_finish(&owner, finish.id, sha, None, None, NOW)
                .unwrap();
            client_settle(&mut j, &poll_source, &poll_publication);
            j.command_maintain(NOW, &CommandProtection::default())
                .unwrap();
            // Restart cannot reset the retry allowance or resurrect a consumed Busy.
            drop(j);
            j = Journal::open(
                &tmp.path().join("journal.db"),
                &[7; 32],
                &hex::encode([1; 32]),
            )
            .unwrap();
            assert_eq!(
                j.command_client_finish_state(&owner, id(1))
                    .unwrap()
                    .attempts,
                u8::try_from(attempt + 1).unwrap()
            );
        }
    }
    #[test]
    fn finish_requires_final_valid_poll_and_no_other_client_work() {
        let (_tmp, mut j, owner) = client_setup();
        let submit = submit(1);
        client_exchange(&mut j, &owner, &submit, &response(&submit));
        let submit_sha = j
            .command_client_response(&owner, submit.id)
            .unwrap()
            .unwrap()
            .1;
        let poll = Request {
            binding: owner.binding.clone(),
            id: id(300),
            op: RequestOp::Poll {
                invocation: id(1),
                stdout_cursor: 0,
                stderr_cursor: 0,
                max_bytes: 8192,
            },
        };
        let mut answer = response(&poll);
        answer.result = Ok(ResponseResult::Poll {
            status: exited_status(&poll),
            stdout: Chunk {
                bytes: vec![],
                next_cursor: 0,
            },
            stderr: Chunk {
                bytes: vec![],
                next_cursor: 0,
            },
        });
        client_exchange(&mut j, &owner, &poll, &answer);
        let sha = j
            .command_client_response(&owner, poll.id)
            .unwrap()
            .unwrap()
            .1;
        let candidate = |j: &Journal| {
            j.command_finish_candidate(
                &owner,
                poll.id,
                sha,
                Some(ClientDelivery::default()),
                id(400),
            )
            .unwrap()
        };
        assert!(candidate(&j).is_none()); // earlier Submit answer still unconsumed
        j.command_consume_response(&owner, submit.id, submit_sha, None)
            .unwrap();
        assert!(candidate(&j).is_some());
        let read = Request {
            binding: owner.binding.clone(),
            id: id(301),
            op: RequestOp::Status { invocation: id(1) },
        };
        client_request(&mut j, &owner, &read);
        assert!(candidate(&j).is_none()); // unanswered read blocks final claim
        client_exchange(
            &mut j,
            &owner,
            &read,
            &Response {
                result: Ok(ResponseResult::Status(exited_status(&read))),
                ..response(&read)
            },
        );
        assert!(candidate(&j).is_none()); // answered but unconsumed also blocks
        let sha_read = j
            .command_client_response(&owner, read.id)
            .unwrap()
            .unwrap()
            .1;
        assert!(
            j.command_finish_candidate(&owner, read.id, sha_read, None, id(401))
                .unwrap()
                .is_none()
        ); // Status cannot substitute for Poll
        j.command_consume_response(&owner, read.id, sha_read, None)
            .unwrap();
        assert!(candidate(&j).is_some());
        // Impossible chunk progress is durably consumed without advancing or
        // preparing Finish, then reports the finite diagnostic to its caller.
        let mut bad = answer;
        if let Ok(ResponseResult::Poll { status, stdout, .. }) = &mut bad.result {
            status.stdout.produced = 2;
            status.stdout.retained = 2;
            stdout.bytes = vec![1];
            stdout.next_cursor = 2;
        }
        let bytes = wire::encode_response(&bad).unwrap();
        let digest = wire::request_digest(&bytes);
        j.conn.execute("UPDATE command_requests SET received_response=?2,response_digest=?3 WHERE request_id=?1",params![poll.id.as_slice(),bytes,digest.as_slice()]).unwrap();
        assert!(
            j.command_finish_candidate(
                &owner,
                poll.id,
                digest,
                Some(ClientDelivery::default()),
                id(402)
            )
            .unwrap()
            .is_none()
        );
        assert!(matches!(
            j.command_consume_and_finish(
                &owner,
                poll.id,
                digest,
                Some(ClientDelivery::default()),
                None,
                NOW
            ),
            Err(Error::Invalid("command_output_cursor_invalid"))
        ));
        assert!(j.command_client_cursors(&owner, id(1)).unwrap() == ClientDelivery::default());
        assert!(
            j.command_client_response(&owner, poll.id)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            j.command_client_finish_state(&owner, id(1))
                .unwrap()
                .attempts,
            0
        );
    }
    #[test]
    fn count_drain_requires_terminal_publications_and_native_ack_proof() {
        let (_tmp, mut j) = setup();
        let (p, _) = admitted(&mut j, 1);
        let ready = |j: &Journal| {
            crate::command_limits::CountDrain {
                maximum: Some(1),
                admitted: 1,
                reserved: 0,
                finished: 1,
                control_idle: true,
                publications_terminal: j.command_publications_terminal().unwrap(),
            }
            .ready()
        };
        assert!(!ready(&j)); // prepared response
        let publication = &p.response.as_ref().unwrap().key;
        j.command_claim_publication(publication).unwrap();
        assert!(!ready(&j)); // uncertain publication is not a delivery proof
        j.conn
            .execute("UPDATE command_publications SET state='published'", [])
            .unwrap();
        assert!(ready(&j));
        assert!(!j.command_sources_all_acknowledged().unwrap());
        j.command_record_native_acks(
            &[SourceKey {
                group: p.source.group.clone(),
                native_id: p.source.native_id.clone(),
            }],
            &[],
        )
        .unwrap();
        assert!(j.command_sources_all_acknowledged().unwrap());
        j.conn
            .execute(
                "INSERT INTO command_early_invalidations VALUES(?1,?2)",
                params![owner().group(), vec![99u8]],
            )
            .unwrap();
        assert!(!j.command_sources_all_acknowledged().unwrap());
    }
    #[test]
    fn admission_deduplicates_and_rolls_back_every_obligation() {
        let (_tmp, mut j) = setup();
        let (mut plan, key) = admitted(&mut j, 1);
        plan.response = None;
        assert!(j.command_project_request(&plan, NOW).unwrap().existing);
        for n in 1..MAX_SOURCES {
            plan.source.native_id = vec![n as u8];
            assert!(j.command_project_request(&plan, NOW).unwrap().existing);
        }
        let before = totals(&j);
        plan.source.native_id = b"one-too-many".to_vec();
        assert!(matches!(
            j.command_project_request(&plan, NOW),
            Err(Error::Invalid("command_source_capacity"))
        ));
        assert_eq!(totals(&j), before);
        let mut changed = plan.request.clone();
        if let RequestOp::Submit(s) = &mut changed.op {
            s.argv.push(b"different".to_vec());
        }
        let conflict = projection(changed);
        assert!(matches!(
            j.command_project_request(&conflict, NOW),
            Err(Error::Invalid("command_request_conflict"))
        ));
        assert_eq!(
            j.command_get(&key).unwrap().unwrap().submit.unwrap().stdin,
            vec![0, 255, 1, 2]
        );
        let mut other = projection(submit(2));
        other.source.native_id = plan.request.id.to_vec();
        assert!(matches!(
            j.command_project_request(&other, NOW),
            Err(Error::Invalid("command_source_conflict"))
        ));
        assert!(j.command_get(&owner().key(id(2))).unwrap().is_none());
        assert_eq!(totals(&j), before);
    }
    #[test]
    fn existing_digest_precedes_freshness_and_binding_is_immutable() {
        let (_tmp, mut j) = setup();
        let (mut plan, key) = admitted(&mut j, 1);
        plan.response = None;
        j.command_maintain(NOW + WEEK + 1, &CommandProtection::default())
            .unwrap();
        assert!(
            j.command_project_request(&plan, NOW + WEEK + 1)
                .unwrap()
                .existing
        );
        let mut fresh = projection(submit(2));
        assert!(matches!(
            j.command_project_request(&fresh, NOW + WEEK + 1),
            Err(Error::Invalid("command_expired"))
        ));
        fresh.request.id = id(999);
        fresh.request.op = plan.request.op.clone();
        fresh.owner.group = b"different".to_vec();
        fresh.request.binding = fresh.owner.binding.clone();
        assert!(j.command_get(&key).unwrap().is_some());
        let mut altered = plan.request.clone();
        altered.id = id(777);
        if let RequestOp::Submit(s) = &mut altered.op {
            s.stdin.push(3);
        }
        let altered = projection(altered);
        assert!(matches!(
            j.command_project_request(&altered, NOW + WEEK + 1),
            Err(Error::Invalid("command_invocation_conflict"))
        ));
        assert!(matches!(
            freshness(&j.conn, id(3), 0),
            Err(Error::Invalid("command_clock_behind_floor"))
        ));
        let mut future = id(4);
        future[..6].copy_from_slice(&(NOW + 301).to_be_bytes()[2..]);
        assert!(freshness(&j.conn, future, NOW + 1).is_ok());
        future[..6].copy_from_slice(&(NOW + 302).to_be_bytes()[2..]);
        assert!(matches!(
            freshness(&j.conn, future, NOW + 1),
            Err(Error::Invalid("command_future_deferred"))
        ));
        assert!(matches!(
            freshness(&j.conn, future, NOW),
            Err(Error::Invalid("command_clock_behind_floor"))
        ));
    }
    #[test]
    fn missing_and_failed_observations_are_not_interchangeable() {
        let (_tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        let r = j.command_get(&key).unwrap().unwrap();
        let op = j
            .command_begin_operation(
                &key,
                r.revision,
                EngineOperation::Dispatch {
                    submit: r.submit.unwrap(),
                },
            )
            .unwrap();
        assert!(matches!(
            j.command_begin_operation(&key, 0, EngineOperation::Observe),
            Err(Error::Invalid("command_operation_busy"))
        ));
        let wrong = EngineCompletion {
            operation: OperationId([0; 16]),
            key: key.clone(),
            revision: op.revision,
            result: EngineObservation::Missing,
        };
        assert!(j.command_commit_completion(&wrong, &[]).is_err());
        let ack = j
            .command_commit_completion(
                &EngineCompletion {
                    operation: op.id,
                    key: key.clone(),
                    revision: op.revision,
                    result: EngineObservation::Failed(OperationalFailure::Timeout),
                },
                &[],
            )
            .unwrap();
        assert!(ack.decision == CommittedDecision::StillUncertain);
        let r = j.command_get(&key).unwrap().unwrap();
        assert!(r.uncertain && r.admission == Admission::Dispatching);
        assert!(j.command_get(&key).unwrap().unwrap().admission == Admission::Dispatching);
        let op = j
            .command_begin_operation(&key, r.revision, EngineOperation::Observe)
            .unwrap();
        let ack = j
            .command_commit_completion(
                &EngineCompletion {
                    operation: op.id,
                    key: key.clone(),
                    revision: op.revision,
                    result: EngineObservation::Missing,
                },
                &[],
            )
            .unwrap();
        assert!(ack.decision == CommittedDecision::DefinitelyUnsubmitted);
        assert!(j.command_get(&key).unwrap().unwrap().admission == Admission::Accepted);
        fact(&mut j, &key, State::Running);
        let r = j.command_get(&key).unwrap().unwrap();
        let op = j
            .command_begin_operation(&key, r.revision, EngineOperation::Observe)
            .unwrap();
        j.command_commit_completion(
            &EngineCompletion {
                operation: op.id,
                key: key.clone(),
                revision: op.revision,
                result: EngineObservation::Missing,
            },
            &[],
        )
        .unwrap();
        let r = j.command_get(&key).unwrap().unwrap();
        assert!(r.engine_known && r.uncertain);
        assert!(matches!(
            j.command_begin_operation(
                &key,
                r.revision,
                EngineOperation::Dispatch {
                    submit: r.submit.unwrap()
                }
            ),
            Err(Error::Invalid("command_dispatch_barrier"))
        ));
    }
    #[test]
    fn stale_completion_retains_engine_facts_without_reauthorizing() {
        let (_tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        let op = j
            .command_begin_operation(&key, 0, EngineOperation::Observe)
            .unwrap();
        j.command_revoke_group(owner().group()).unwrap();
        let mut s = status(&submit(1));
        s.engine_known = true;
        s.admission = Admission::Engine;
        s.state = State::Running;
        j.command_commit_completion(
            &EngineCompletion {
                operation: op.id,
                key: key.clone(),
                revision: op.revision,
                result: EngineObservation::Found {
                    status: s,
                    worker: WorkerState::Running,
                },
            },
            &[],
        )
        .unwrap();
        let r = j.command_get(&key).unwrap().unwrap();
        assert!(r.engine_known && !r.authority_valid);
        assert!(!r.status.unwrap().authority_valid);
        assert!(
            j.command_claim_publication(&projection(submit(1)).response.unwrap().key)
                .unwrap()
                .is_none()
        );
        assert!(!j.command_get(&key).unwrap().unwrap().authority_valid);
    }
    #[test]
    fn encrypted_reopen_preserves_dispatch_barrier_and_fails_old_input_generation() {
        let (tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        fact(&mut j, &key, State::Running);
        let r = j.command_get(&key).unwrap().unwrap();
        j.command_begin_operation(&key, r.revision, EngineOperation::BeginInput)
            .unwrap();
        drop(j);
        let mut j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        j.command_init().unwrap();
        let r = j.command_get(&key).unwrap().unwrap();
        assert!(r.feeder == FeederState::Failed && r.warning == Some(Warning::InputFailed));
        assert!(matches!(
            crate::command_engine::next_feeder_operation(&r).unwrap(),
            Some(EngineOperation::Cancel)
        ));
        assert!(
            !fs::read(tmp.path().join("journal.db"))
                .unwrap()
                .windows(8)
                .any(|b| b == b"/bin/cat")
        );
    }
    #[test]
    fn natural_exit_empty_stdin_completes_without_claiming_eof() {
        for (length, health, expected) in [
            (0, InputHealth::Closed, None),
            (0, InputHealth::Gap, None),
            (0, InputHealth::Uncertain, None),
            (4, InputHealth::Closed, Some(Warning::StdinUnconsumed)),
            (4, InputHealth::Gap, Some(Warning::InputFailed)),
        ] {
            let (_tmp, mut j) = setup();
            let mut request = submit(1);
            if let RequestOp::Submit(submit) = &mut request.op {
                submit.stdin.truncate(length);
            }
            let plan = projection(request.clone());
            let key = j.command_project_request(&plan, NOW).unwrap().key;
            let r = j.command_get(&key).unwrap().unwrap();
            let op = j
                .command_begin_operation(&key, r.revision, EngineOperation::Observe)
                .unwrap();
            let mut status = exited_status(&request);
            status.input.health = health;
            j.command_commit_completion(
                &EngineCompletion {
                    operation: op.id,
                    key: key.clone(),
                    revision: op.revision,
                    result: EngineObservation::Input {
                        status,
                        generation: None,
                        credit: 0,
                        worker: WorkerState::Exited,
                    },
                },
                &[],
            )
            .unwrap();
            let r = j.command_get(&key).unwrap().unwrap();
            assert!(r.warning == expected);
            assert!(r.status.as_ref().unwrap().warning == expected);
            assert!(!r.status.as_ref().unwrap().input.eof_sent);
            assert_eq!(r.status.as_ref().unwrap().input.accepted, 0);
            assert_eq!(r.status.as_ref().unwrap().input.delivered, 0);
            assert!(matches!(r.feeder, FeederState::Complete) == (length == 0));
        }
    }
    #[test]
    fn feeder_registers_immutable_bytes_and_reconciles_without_replaying() {
        let (_tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        let mut s = fact(&mut j, &key, State::Running);
        let r = j.command_get(&key).unwrap().unwrap();
        let op = j
            .command_begin_operation(&key, r.revision, EngineOperation::BeginInput)
            .unwrap();
        let generation = [9; 16];
        j.command_commit_completion(
            &EngineCompletion {
                operation: op.id,
                key: key.clone(),
                revision: op.revision,
                result: EngineObservation::Input {
                    status: s.clone(),
                    generation: Some(generation),
                    credit: 4,
                    worker: WorkerState::Running,
                },
            },
            &[],
        )
        .unwrap();
        let r = j.command_get(&key).unwrap().unwrap();
        assert!(matches!(
            j.command_begin_operation(
                &key,
                r.revision,
                EngineOperation::WriteInput {
                    generation,
                    offset: 0,
                    bytes: vec![9]
                }
            ),
            Err(Error::Invalid("command_input_bounds"))
        ));
        let op = j
            .command_begin_operation(
                &key,
                r.revision,
                EngineOperation::WriteInput {
                    generation,
                    offset: 0,
                    bytes: r.submit.unwrap().stdin.clone(),
                },
            )
            .unwrap();
        j.command_commit_completion(
            &EngineCompletion {
                operation: op.id,
                key: key.clone(),
                revision: op.revision,
                result: EngineObservation::Failed(OperationalFailure::Uncertain),
            },
            &[],
        )
        .unwrap();
        let r = j.command_get(&key).unwrap().unwrap();
        assert!(matches!(r.feeder, FeederState::WriteIntended { .. }));
        s.input.accepted = 4;
        s.input.delivered = 4;
        let op = j
            .command_begin_operation(&key, r.revision, EngineOperation::Observe)
            .unwrap();
        j.command_commit_completion(
            &EngineCompletion {
                operation: op.id,
                key: key.clone(),
                revision: op.revision,
                result: EngineObservation::Input {
                    status: s.clone(),
                    generation: Some(generation),
                    credit: 0,
                    worker: WorkerState::Running,
                },
            },
            &[],
        )
        .unwrap();
        let r = j.command_get(&key).unwrap().unwrap();
        assert!(matches!(r.feeder, FeederState::Accepted { offset: 4, .. }));
        let op = j
            .command_begin_operation(
                &key,
                r.revision,
                EngineOperation::Eof {
                    generation,
                    offset: 4,
                },
            )
            .unwrap();
        s.input.eof_sent = true;
        s.input.health = InputHealth::Closed;
        j.command_commit_completion(
            &EngineCompletion {
                operation: op.id,
                key: key.clone(),
                revision: op.revision,
                result: EngineObservation::Input {
                    status: s,
                    generation: Some(generation),
                    credit: 0,
                    worker: WorkerState::Running,
                },
            },
            &[],
        )
        .unwrap();
        assert!(j.command_get(&key).unwrap().unwrap().feeder == FeederState::Complete);
    }
    fn pending_read(j: &mut Journal, key: &InvocationKey, sequence: u16, op: RequestOp) -> Request {
        let mut request = submit(sequence);
        request.op = op;
        let mut plan = projection(request.clone());
        plan.decision = AdmissionDecision::Existing;
        plan.response = None;
        j.command_project_request(&plan, NOW).unwrap();
        assert_eq!(
            j.command_find_request(&owner(), &request)
                .unwrap()
                .unwrap()
                .failed_attempts,
            0
        );
        assert_eq!(request.op.invocation(), key.invocation);
        request
    }
    fn finish(
        j: &mut Journal,
        key: &InvocationKey,
        request: &Request,
        observation: EngineObservation,
    ) -> CommitAck {
        let record = j.command_get(key).unwrap().unwrap();
        let operation = match request.op {
            RequestOp::Status { .. } => EngineOperation::Observe,
            RequestOp::Cancel { .. } => EngineOperation::Cancel,
            RequestOp::Poll {
                stdout_cursor,
                stderr_cursor,
                max_bytes,
                ..
            } => EngineOperation::Poll {
                stdout_cursor,
                stderr_cursor,
                max_bytes,
            },
            _ => unreachable!(),
        };
        let operation = j
            .command_begin_request_operation(key, record.revision, request.id, operation)
            .unwrap();
        let pending = j.command_operation_request(operation.id).unwrap().unwrap();
        let completion = EngineCompletion {
            operation: operation.id,
            key: key.clone(),
            revision: operation.revision,
            result: observation,
        };
        let projected = project_completion(
            &j.command_get(key).unwrap().unwrap(),
            &operation,
            &completion,
            Some(&pending),
        )
        .unwrap();
        let publications = projected
            .response
            .as_ref()
            .map(|r| publication(&owner(), r))
            .into_iter()
            .collect::<Vec<_>>();
        j.command_commit_completion(&completion, &publications)
            .unwrap()
    }
    #[test]
    fn pending_failure_attempts_are_durable_and_third_is_finitely_answered() {
        let (tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        let request = pending_read(
            &mut j,
            &key,
            2,
            RequestOp::Status {
                invocation: key.invocation,
            },
        );
        for attempt in 1..=3 {
            let ack = finish(
                &mut j,
                &key,
                &request,
                EngineObservation::Failed(OperationalFailure::Timeout),
            );
            assert_eq!(ack.answered.len(), usize::from(attempt == 3));
            let pending = j.command_find_request(&owner(), &request).unwrap().unwrap();
            assert_eq!(pending.failed_attempts, attempt);
            assert_eq!(pending.response.is_some(), attempt == 3);
            drop(j);
            j = Journal::open(
                &tmp.path().join("journal.db"),
                &[7; 32],
                &hex::encode([2; 32]),
            )
            .unwrap();
        }
        assert!(j.command_pending(256).unwrap().is_empty());
        let pending = j.command_find_request(&owner(), &request).unwrap().unwrap();
        let response = load_publication(&j.conn, &pending.response.unwrap())
            .unwrap()
            .unwrap();
        assert!(matches!(
            wire::decode_response(&response.encoded_wire)
                .unwrap()
                .result,
            Err(WireError {
                code: ErrorCode::RecoveryUncertain,
                ..
            })
        ));
        assert!(!j.command_get(&key).unwrap().unwrap().engine_known);
    }
    #[test]
    fn unknown_read_refusal_owns_slot_without_allocating_invocation() {
        let (_tmp, mut j) = setup();
        let mut request = submit(3);
        request.op = RequestOp::Cancel { invocation: id(1) };
        let mut plan = projection(request.clone());
        let refusal = WireError {
            code: ErrorCode::NotFound,
            detail: None,
        };
        let mut answer = response(&request);
        answer.result = Err(refusal.clone());
        plan.decision = AdmissionDecision::Refused(refusal);
        plan.response = Some(publication(&owner(), &answer));
        j.command_project_request(&plan, NOW).unwrap();
        assert_eq!(totals(&j), (0, 1, 1));
        let publication = plan.response.take().unwrap();
        assert!(
            j.command_claim_publication(&publication.key)
                .unwrap()
                .is_some()
        );
        assert!(
            j.command_claim_publication(&publication.key)
                .unwrap()
                .is_none()
        );
        assert!(j.command_project_request(&plan, NOW).unwrap().existing);
        assert_eq!(totals(&j), (0, 1, 1));
        assert!(
            j.command_find_request(&owner(), &request)
                .unwrap()
                .unwrap()
                .response
                .is_some()
        );
        assert!(j.command_requests(&owner().key(id(1)), 64).unwrap().len() == 1);
    }
    #[test]
    fn exact_read_registration_survives_restart_and_rejects_wrong_publication() {
        let (tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        let status = fact(&mut j, &key, State::Exited);
        let request = pending_read(
            &mut j,
            &key,
            2,
            RequestOp::Poll {
                invocation: key.invocation,
                stdout_cursor: 3,
                stderr_cursor: 4,
                max_bytes: 8,
            },
        );
        let r = j.command_get(&key).unwrap().unwrap();
        let operation = j
            .command_begin_request_operation(
                &key,
                r.revision,
                request.id,
                EngineOperation::Poll {
                    stdout_cursor: 3,
                    stderr_cursor: 4,
                    max_bytes: 8,
                },
            )
            .unwrap();
        drop(j);
        let mut j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        let retained = j.command_read_operations(64).unwrap();
        assert_eq!(retained.len(), 1);
        assert!(retained[0].id == operation.id);
        let pending = j.command_operation_request(operation.id).unwrap().unwrap();
        assert_eq!(pending.request.id, request.id);
        let completion = EngineCompletion {
            operation: operation.id,
            key: key.clone(),
            revision: operation.revision,
            result: EngineObservation::Polled {
                status,
                stdout: Chunk {
                    bytes: vec![0, 255],
                    next_cursor: 5,
                },
                stderr: Chunk {
                    bytes: vec![1],
                    next_cursor: 5,
                },
                worker: WorkerState::Exited,
            },
        };
        let projected = project_completion(
            &j.command_get(&key).unwrap().unwrap(),
            &retained[0],
            &completion,
            Some(&pending),
        )
        .unwrap();
        assert!(
            j.command_commit_completion(&completion, &[publication(&owner(), &response(&request))])
                .is_err()
        );
        assert!(j.command_operation(operation.id).unwrap().is_some());
        j.command_commit_completion(
            &completion,
            &[publication(&owner(), &projected.response.unwrap())],
        )
        .unwrap();
        assert!(j.command_operation(operation.id).unwrap().is_none());
        assert!(j.command_pending(256).unwrap().is_empty());
    }
    #[test]
    fn revocation_suppresses_pending_read_and_never_prepares_a_response() {
        let (_tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        let request = pending_read(
            &mut j,
            &key,
            2,
            RequestOp::Status {
                invocation: key.invocation,
            },
        );
        let r = j.command_get(&key).unwrap().unwrap();
        let operation = j
            .command_begin_request_operation(&key, r.revision, request.id, EngineOperation::Observe)
            .unwrap();
        j.command_revoke_group(owner().group()).unwrap();
        let ack = j
            .command_commit_completion(
                &EngineCompletion {
                    operation: operation.id,
                    key,
                    revision: operation.revision,
                    result: EngineObservation::Failed(OperationalFailure::Storage),
                },
                &[],
            )
            .unwrap();
        assert_eq!(ack.suppressed, vec![request.id]);
        assert!(ack.answered.is_empty());
        assert!(j.command_pending(256).unwrap().is_empty());
        assert!(
            j.command_find_request(&owner(), &request)
                .unwrap()
                .unwrap()
                .response
                .is_none()
        );
    }
    fn settle(j: &mut Journal, plan: &CommandProjection, protect: bool) {
        let p = plan.response.as_ref().unwrap();
        let key = p.key.clone();
        j.command_claim_publication(&key).unwrap();
        let published = j
            .command_apply_native(&[CommandNativeUpdate::Published {
                evidence: evidence(1),
                key: key.clone(),
                native_id: plan.request.id.to_vec(),
            }])
            .unwrap();
        j.command_record_native_acks(
            &[SourceKey {
                group: plan.source.group.clone(),
                native_id: plan.source.native_id.clone(),
            }],
            &published,
        )
        .unwrap();
        let mut protection = CommandProtection::default();
        if protect {
            protection.publications.insert(key);
        }
        j.command_maintain(NOW, &protection).unwrap();
    }
    #[test]
    fn acked_native_protection_prevents_retirement_and_slots_are_reused_boundedly() {
        let (_tmp, mut j) = setup();
        let (plan, key) = admitted(&mut j, 1);
        settle(&mut j, &plan, true);
        assert_eq!(totals(&j), (1, 1, 1));
        j.command_maintain(NOW, &CommandProtection::default())
            .unwrap();
        assert_eq!(totals(&j), (1, 0, 0));
        let mut high_water = 0;
        for n in 2..1100 {
            let mut request = submit(n);
            request.op = RequestOp::Status {
                invocation: key.invocation,
            };
            let mut plan = projection(request);
            plan.decision = AdmissionDecision::Existing;
            j.command_project_request(&plan, NOW).unwrap();
            high_water = high_water.max(totals(&j).1);
            settle(&mut j, &plan, false);
            assert_eq!(totals(&j), (1, 0, 0));
        }
        assert_eq!(high_water, 1);
        assert!(
            count(
                &j.conn,
                "SELECT count(*) FROM command_incoming_sources",
                None
            )
            .unwrap()
                == 0
        );
        assert!(
            count(
                &j.conn,
                "SELECT count(*) FROM command_outgoing_sources",
                None
            )
            .unwrap()
                == 0
        );
    }
    #[test]
    fn native_batches_are_atomic_and_terminal_invalidation_cannot_be_resurrected() {
        let (_tmp, mut j) = setup();
        let (plan, key) = admitted(&mut j, 1);
        let publication = plan.response.unwrap().key;
        j.command_claim_publication(&publication).unwrap();
        for n in 0..2 {
            j.command_apply_native(&[CommandNativeUpdate::Queued {
                key: publication.clone(),
                intent: vec![n],
            }])
            .unwrap();
        }
        assert!(
            j.command_apply_native(&[CommandNativeUpdate::Queued {
                key: publication.clone(),
                intent: vec![2]
            }])
            .is_err()
        );
        let mut unknown = publication.clone();
        unknown.app_event_id = "f".repeat(64);
        assert!(
            j.command_apply_native(&[
                CommandNativeUpdate::Published {
                    evidence: evidence(1),
                    key: publication.clone(),
                    native_id: vec![8]
                },
                CommandNativeUpdate::Failed { key: unknown }
            ])
            .is_err()
        );
        assert_eq!(
            count(
                &j.conn,
                "SELECT count(*) FROM command_outgoing_sources",
                None
            )
            .unwrap(),
            0
        );
        j.command_apply_native(&[
            CommandNativeUpdate::Failed {
                key: publication.clone(),
            },
            CommandNativeUpdate::Published {
                evidence: evidence(1),
                key: publication.clone(),
                native_id: vec![9],
            },
        ])
        .unwrap();
        assert_eq!(
            publication_state(&j.conn, &publication).unwrap().unwrap(),
            "invalidated"
        );
        assert!(!j.command_get(&key).unwrap().unwrap().authority_valid);
        j.command_apply_native(&[CommandNativeUpdate::Published {
            evidence: evidence(1),
            key: publication.clone(),
            native_id: vec![10],
        }])
        .unwrap();
        assert_eq!(
            publication_state(&j.conn, &publication).unwrap().unwrap(),
            "invalidated"
        );
        assert!(
            j.command_apply_native(&[CommandNativeUpdate::Published {
                evidence: evidence(1),
                key: publication.clone(),
                native_id: vec![11]
            }])
            .unwrap()
            .is_empty()
        );
        assert!(
            j.command_publication_bindings(&publication)
                .unwrap()
                .iter()
                .all(|b| b.quarantined)
        );
    }
    fn limits() -> crate::command_config::CommandLimits {
        use std::time::Duration;
        crate::command_config::CommandLimits {
            active_per_principal: 1,
            active_global: 2,
            retained_per_principal: 3,
            retained_global: 4,
            engine_starting: 1,
            engine_tokens: 2,
            token_lifetime: Duration::from_secs(120),
            payload_retention: Duration::from_secs(86400),
            tombstone_retention: Duration::from_secs(172800),
            stream_bytes: 65536,
            spool_bytes: 65536,
            max_commands: None,
            max_uptime: None,
        }
    }
    #[test]
    fn configured_quotas_and_lifetime_admission_reservations_are_enforced() {
        let (_tmp, mut j) = setup();
        let mut settings = limits();
        j.command_configure(&settings).unwrap();
        let (_, key) = admitted(&mut j, 1);
        assert!(matches!(
            j.command_project_request(&projection(submit(2)), NOW),
            Err(Error::Invalid("command_active_capacity"))
        ));
        assert_eq!(totals(&j), (1, 1, 1));
        settings.active_per_principal = 4;
        settings.active_global = 4;
        settings.max_commands = std::num::NonZeroU64::new(1);
        j.command_configure(&settings).unwrap();
        assert!(matches!(
            j.command_project_request(&projection(submit(2)), NOW),
            Err(Error::Invalid("command_admission_closed"))
        ));
        let r = j.command_get(&key).unwrap().unwrap();
        let operation = j
            .command_begin_operation(&key, r.revision, EngineOperation::Observe)
            .unwrap();
        j.command_commit_completion(
            &EngineCompletion {
                operation: operation.id,
                key,
                revision: operation.revision,
                result: EngineObservation::Refused(WireError {
                    code: ErrorCode::CommandDenied,
                    detail: None,
                }),
            },
            &[],
        )
        .unwrap();
        let (_, key) = admitted(&mut j, 2);
        fact(&mut j, &key, State::Exited);
        assert!(matches!(
            j.command_project_request(&projection(submit(3)), NOW),
            Err(Error::Invalid("command_admission_closed"))
        ));
        assert_eq!(
            j.conn
                .query_row(
                    "SELECT admitted FROM command_replay_state",
                    [],
                    |r| sql_u64(r, 0)
                )
                .unwrap(),
            1
        );
        settings.max_commands = None;
        settings.retained_per_principal = 2;
        j.command_configure(&settings).unwrap();
        assert!(matches!(
            j.command_project_request(&projection(submit(3)), NOW),
            Err(Error::Invalid("command_retained_capacity"))
        ));
    }
    #[test]
    fn client_response_projection_binds_frozen_request_and_conflicts_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([1; 32]),
        )
        .unwrap();
        let owner = Owner::from_context(&AuthenticatedCommandContext::test_fixture(
            [1; 32],
            [2; 32],
            b"command-group".to_vec(),
            id(0),
            CommandDirection::Client,
        ));
        let request = submit(1);
        let bytes = wire::encode_request(&request).unwrap();
        let publication = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            request.id,
            wire::request_digest(&bytes),
            bytes,
            owner.local(),
        )
        .unwrap();
        let record = j
            .command_submit_client(&owner, &request, &publication, NOW)
            .unwrap();
        assert!(
            j.command_submit_client(&owner, &request, &publication, NOW)
                .unwrap()
                .key
                == record.key
        );
        let mut answer = response(&request);
        let status = match answer.result.as_mut() {
            Ok(ResponseResult::Submit { status, .. }) => status,
            _ => unreachable!(),
        };
        status.engine_known = true;
        status.admission = Admission::Engine;
        status.state = State::Running;
        let event = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            request.id,
            answer.request_sha256,
            wire::encode_response(&answer).unwrap(),
            owner.binding.server,
        )
        .unwrap();
        let mut source = CommandSource {
            group: owner.group.clone(),
            native_id: vec![9],
            app_event_id: event.key.app_event_id,
            author: owner.binding.server,
            authority_metadata: b"authenticated".to_vec(),
        };
        let mut wrong = answer.clone();
        wrong.request_sha256 = [5; 32];
        assert!(j.command_project_response(&source, &wrong, NOW).is_err());
        assert_eq!(
            count(
                &j.conn,
                "SELECT count(*) FROM command_incoming_sources",
                None
            )
            .unwrap(),
            0
        );
        assert!(
            !j.command_project_response(&source, &answer, NOW)
                .unwrap()
                .existing
        );
        source.native_id = vec![10];
        assert!(
            j.command_project_response(&source, &answer, NOW + WEEK + 1)
                .unwrap()
                .existing
        );
        assert!(j.command_get(&record.key).unwrap().unwrap().engine_known);
        if let Ok(ResponseResult::Submit { status, .. }) = &mut wrong.result {
            status.cancel_requested = true;
        }
        wrong.request_sha256 = answer.request_sha256;
        assert!(matches!(
            j.command_project_response(&source, &wrong, NOW),
            Err(Error::Invalid("command_response_conflict"))
        ));
        assert_eq!(
            count(
                &j.conn,
                "SELECT count(*) FROM command_incoming_sources",
                None
            )
            .unwrap(),
            2
        );
    }
    #[test]
    fn source_and_evidence_pages_resume_after_retirement_without_cursor_reuse() {
        let (_tmp, mut j) = setup();
        let (first, key1) = admitted(&mut j, 1);
        let (_, key2) = admitted(&mut j, 2);
        let sources = j.command_native_bindings(None, 1).unwrap();
        assert_eq!(sources.rows.len(), 1);
        let cursor = sources.next.unwrap();
        let page = j.command_evidence(None, 1).unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(
            page.rows[0].cursor,
            j.command_get(&key1).unwrap().unwrap().cursor
        );
        let after = page.next.unwrap();
        assert!(page.rows[0].stdout.sha256.is_none());
        assert!(encode(&page).unwrap().len() <= 64000);
        let fence = j.command_allocate_fence().unwrap();
        fact(&mut j, &key2, State::Running);
        let later = j.command_evidence(Some(after), 1).unwrap();
        assert_eq!(later.rows.len(), 1);
        assert!(later.rows[0].observed_seq > fence);
        assert_eq!(later.rows[0].invocation, hex::encode(key2.invocation));
        settle(&mut j, &first, false);
        let (_, key3) = admitted(&mut j, 3);
        let resumed = j.command_native_bindings(Some(cursor), 64).unwrap();
        assert_eq!(resumed.rows.len(), 2);
        assert!(resumed.rows.iter().all(|r| r.cursor.row_id > cursor.row_id));
        assert!(resumed.next.is_none());
        assert!(
            j.command_get(&key3).unwrap().unwrap().cursor
                > j.command_get(&key2).unwrap().unwrap().cursor
        );
        let rest = j.command_evidence(Some(after), 64).unwrap();
        assert_eq!(rest.rows.len(), 2);
        assert!(rest.next.is_none());
        assert!(j.command_native_bindings(None, 0).is_err());
        assert!(j.command_evidence(None, 65).is_err());
    }
    fn client_setup() -> (tempfile::TempDir, Journal, Owner) {
        let tmp = tempfile::tempdir().unwrap();
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let journal = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([1; 32]),
        )
        .unwrap();
        let owner = Owner::from_context(&AuthenticatedCommandContext::test_fixture(
            [1; 32],
            [2; 32],
            b"command-group".to_vec(),
            id(0),
            CommandDirection::Client,
        ));
        (tmp, journal, owner)
    }
    fn client_request(
        j: &mut Journal,
        owner: &Owner,
        request: &Request,
    ) -> PreparedCommandPublication {
        let bytes = wire::encode_request(request).unwrap();
        let outgoing = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            request.id,
            wire::request_digest(&bytes),
            bytes,
            owner.local(),
        )
        .unwrap();
        j.command_submit_client(owner, request, &outgoing, NOW)
            .unwrap();
        outgoing
    }
    fn client_exchange(
        j: &mut Journal,
        owner: &Owner,
        request: &Request,
        answer: &Response,
    ) -> (SourceKey, PreparedCommandPublication) {
        let outgoing = client_request(j, owner, request);
        let incoming = publication(owner, answer);
        let source = CommandSource {
            group: owner.group.clone(),
            native_id: request.id.to_vec(),
            app_event_id: incoming.key.app_event_id,
            author: owner.binding.server,
            authority_metadata: b"auth".to_vec(),
        };
        j.command_project_response(&source, answer, NOW).unwrap();
        (
            SourceKey {
                group: source.group,
                native_id: source.native_id,
            },
            outgoing,
        )
    }
    fn client_settle(j: &mut Journal, source: &SourceKey, outgoing: &PreparedCommandPublication) {
        j.command_claim_publication(&outgoing.key).unwrap();
        let published = j
            .command_apply_native(&[CommandNativeUpdate::Published {
                key: outgoing.key.clone(),
                native_id: outgoing.request_id.to_vec(),
                evidence: evidence(1),
            }])
            .unwrap();
        j.command_record_native_acks(std::slice::from_ref(source), &published)
            .unwrap();
    }
    #[test]
    fn unread_client_response_survives_gc_and_consumption_is_durable_and_idempotent() {
        let (tmp, mut j, owner) = client_setup();
        let request = submit(1);
        let answer = response(&request);
        let (source, outgoing) = client_exchange(&mut j, &owner, &request, &answer);
        client_settle(&mut j, &source, &outgoing);
        j.command_maintain(NOW + 86401, &CommandProtection::default())
            .unwrap();
        assert_eq!(totals(&j), (1, 1, 1));
        let discovered = j.command_client_unconsumed(&owner, id(1), 64).unwrap();
        assert_eq!(discovered.len(), 1);
        let (saved, digest) = j
            .command_client_response(&owner, request.id)
            .unwrap()
            .unwrap();
        assert!(saved == answer);
        assert!(
            j.command_consume_response(&owner, request.id, [0; 32], None)
                .is_err()
        );
        assert!(
            j.command_client_response(&owner, request.id)
                .unwrap()
                .is_some()
        );
        assert!(
            j.command_consume_response(&owner, request.id, digest, None)
                .unwrap()
                == ClientDelivery::default()
        );
        let original_revision = j.command_get(&owner.key(id(1))).unwrap().unwrap().revision;
        let mut replay_source = CommandSource {
            group: source.group.clone(),
            native_id: source.native_id.clone(),
            app_event_id: publication(&owner, &answer).key.app_event_id,
            author: owner.binding.server,
            authority_metadata: b"auth".to_vec(),
        };
        // New authenticated envelope may attach its source, but cannot rewrite
        // response/facts/consumption, even after later status observations.
        replay_source.native_id = vec![99];
        j.command_project_response(&replay_source, &answer, NOW)
            .unwrap();
        assert_eq!(
            j.command_get(&owner.key(id(1))).unwrap().unwrap().revision,
            original_revision
        );
        assert!(
            j.command_client_response(&owner, request.id)
                .unwrap()
                .is_none()
        );
        drop(j);
        let mut j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([1; 32]),
        )
        .unwrap();
        assert!(
            j.command_client_unconsumed(&owner, id(1), 1)
                .unwrap()
                .is_empty()
        );
        assert!(
            j.command_consume_response(&owner, request.id, digest, None)
                .unwrap()
                == ClientDelivery::default()
        );
        j.command_record_native_acks(
            &[SourceKey {
                group: owner.group.clone(),
                native_id: vec![99],
            }],
            &[],
        )
        .unwrap();
        j.command_maintain(NOW + 86401, &CommandProtection::default())
            .unwrap();
        assert_eq!(totals(&j), (1, 0, 0));
        assert!(matches!(
            j.command_consume_response(&owner, request.id, digest, None),
            Err(Error::Invalid("command_response_retired"))
        ));
        assert!(
            j.command_get(&owner.key(id(1)))
                .unwrap()
                .unwrap()
                .submit
                .is_none()
        );
    }
    #[test]
    fn client_locator_metadata_survives_slot_payload_gc_and_encrypted_reopen() {
        use crate::command_client::CommandHandle;
        let (tmp, mut journal, owner) = client_setup();
        let request = submit(1);
        assert!(
            journal
                .command_client_slot_state(&owner, request.id)
                .unwrap()
                == ClientSlotState::Absent
        );
        let bytes = wire::encode_request(&request).unwrap();
        let outgoing = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            request.id,
            wire::request_digest(&bytes),
            bytes.clone(),
            owner.local(),
        )
        .unwrap();
        journal
            .command_submit_client(&owner, &request, &outgoing, NOW)
            .unwrap();
        assert!(
            journal
                .command_client_slot_state(&owner, request.id)
                .unwrap()
                == ClientSlotState::Pending
        );
        let entry = journal.command_client_invocation(&owner, id(1)).unwrap();
        let handle = CommandHandle::from_invocation("default", &entry).unwrap();
        handle
            .validate_entry("default", owner.local(), &entry)
            .unwrap();
        assert!(
            handle
                .validate_entry("other", owner.local(), &entry)
                .is_err()
        );
        assert!(handle.validate_entry("default", [9; 32], &entry).is_err());
        assert_eq!(
            entry.group_submit_sha256,
            journal
                .command_get(&owner.key(id(1)))
                .unwrap()
                .unwrap()
                .digest
        );
        assert_ne!(entry.group_submit_sha256, entry.submit_request_sha256);
        assert_eq!(entry.submit_request_id, request.id);
        assert_eq!(entry.submit_request_sha256, wire::request_digest(&bytes));
        assert_ne!(
            entry.submit_request_sha256,
            journal
                .command_get(&owner.key(id(1)))
                .unwrap()
                .unwrap()
                .digest
        );
        let answer = response(&request);
        let (source, outgoing) = client_exchange(&mut journal, &owner, &request, &answer);
        assert!(
            journal
                .command_client_slot_state(&owner, request.id)
                .unwrap()
                == ClientSlotState::Answered
        );
        let mut wrong_owner = owner.clone();
        wrong_owner.authority[0] ^= 1;
        assert!(
            journal
                .command_client_slot_state(&wrong_owner, request.id)
                .is_err()
        );
        let digest = journal
            .command_client_response(&owner, request.id)
            .unwrap()
            .unwrap()
            .1;
        journal
            .command_consume_response(&owner, request.id, digest, None)
            .unwrap();
        assert!(
            journal
                .command_client_slot_state(&owner, request.id)
                .unwrap()
                == ClientSlotState::Consumed
        );
        client_settle(&mut journal, &source, &outgoing);
        journal
            .command_maintain(NOW + 86401, &CommandProtection::default())
            .unwrap();
        assert!(
            journal
                .command_client_slot_state(&owner, request.id)
                .unwrap()
                == ClientSlotState::Absent
        );
        assert!(
            journal
                .command_get(&owner.key(id(1)))
                .unwrap()
                .unwrap()
                .submit
                .is_none()
        );
        drop(journal);
        let mut journal = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode(owner.local()),
        )
        .unwrap();
        let page = journal.command_client_invocations(None, 64).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert!(page.next.is_none());
        assert_eq!(
            page.entries[0].group_submit_sha256,
            entry.group_submit_sha256
        );
        assert_eq!(page.entries[0].submit_request_id, request.id);
        assert_eq!(
            page.entries[0].submit_request_sha256,
            wire::request_digest(&bytes)
        );
        assert_eq!(page.entries[0].created, NOW);
        assert!(page.entries[0].admission == Admission::Accepted);
        assert!(page.entries[0].state == Some(State::Accepted));
        assert!(!page.entries[0].engine_known);
        assert!(page.entries[0].refusal.is_none());
        handle
            .validate_entry("default", owner.local(), &page.entries[0])
            .unwrap();
        assert!(CommandHandle::from_invocation("default", &page.entries[0]).unwrap() == handle);
        let mut changed = request.clone();
        changed.id = id(500);
        let bytes = wire::encode_request(&changed).unwrap();
        let changed_outgoing = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            changed.id,
            wire::request_digest(&bytes),
            bytes,
            owner.local(),
        )
        .unwrap();
        assert!(matches!(
            journal.command_submit_client(&owner, &changed, &changed_outgoing, NOW + 86401),
            Err(Error::Invalid("command_submit_request_conflict"))
        ));
        assert!(
            journal
                .command_find_request(&owner, &changed)
                .unwrap()
                .is_none()
        );
        journal
            .command_submit_client(&owner, &request, &outgoing, NOW + 86401)
            .unwrap();
        assert_eq!(
            journal
                .command_client_invocations(None, 64)
                .unwrap()
                .entries
                .len(),
            1
        );
        journal.command_revoke_group(owner.group()).unwrap();
        let revoked = journal.command_client_invocations(None, 64).unwrap();
        assert!(!revoked.entries[0].authority_valid);
        assert!(
            handle
                .validate_entry("default", owner.local(), &revoked.entries[0])
                .is_err()
        );
        assert!(journal.command_client_invocation(&owner, id(1)).is_err());
        assert!(
            journal
                .command_client_slot_state(&owner, request.id)
                .is_err()
        );
    }
    #[test]
    fn client_invocation_pages_are_client_only_stable_and_bounded() {
        let (_tmp, mut journal, owner) = client_setup();
        for sequence in 1..=3 {
            let request = submit(sequence);
            let bytes = wire::encode_request(&request).unwrap();
            let publication = crate::mdk_adapter::prepare_command_publication(
                owner.group(),
                owner.binding.clone(),
                request.id,
                wire::request_digest(&bytes),
                bytes,
                owner.local(),
            )
            .unwrap();
            journal
                .command_submit_client(&owner, &request, &publication, NOW)
                .unwrap();
        }
        let server_owner = Owner::from_context(&AuthenticatedCommandContext::test_fixture(
            owner.local(),
            [3; 32],
            b"server-room".to_vec(),
            id(0),
            CommandDirection::Server,
        ));
        let payload = Submit {
            invocation: id(4),
            argv: vec![b"/bin/true".to_vec()],
            stdin: vec![],
        };
        insert_invocation(&journal.conn, &server_owner, &payload, [3; 32], None, NOW).unwrap();
        let mut after = None;
        let mut seen = Vec::new();
        loop {
            let page = journal.command_client_invocations(after, 1).unwrap();
            assert_eq!(page.entries.len(), 1);
            seen.push(page.entries[0].invocation);
            if page.next.is_none() {
                break;
            }
            assert!(after.is_none_or(|cursor| page.next.unwrap() > cursor));
            after = page.next;
        }
        assert!(seen == vec![id(1), id(2), id(3)]);
        assert!(journal.command_client_invocations(None, 0).is_err());
        assert!(journal.command_client_invocations(None, 65).is_err());
        assert!(
            journal
                .command_client_invocations(Some(u64::MAX), 1)
                .is_err()
        );
    }
    #[test]
    fn answered_unretired_server_poll_defers_same_source_until_ack_and_maintenance() {
        let (_tmp, mut journal) = setup();
        let (_, key) = admitted(&mut journal, 1);
        let poll = |sequence| Request {
            binding: owner().binding.clone(),
            id: id(sequence),
            op: RequestOp::Poll {
                invocation: key.invocation,
                stdout_cursor: 0,
                stderr_cursor: 0,
                max_bytes: 100,
            },
        };
        let mut first = projection(poll(200));
        first.decision = AdmissionDecision::Existing;
        journal.command_project_request(&first, NOW).unwrap();
        let mut second = projection(poll(201));
        second.decision = AdmissionDecision::Existing;
        let original_source = second.source.clone();
        assert!(matches!(
            journal.command_project_request(&second, NOW),
            Err(Error::Invalid("command_poll_busy"))
        ));
        // The failed transaction cannot attach or reject the later source. It
        // remains replayable while the first answer owns the sole Poll slot.
        assert!(
            journal
                .command_find_request(&second.owner, &second.request)
                .unwrap()
                .is_none()
        );
        assert!(
            !journal
                .command_native_bindings(None, 64)
                .unwrap()
                .rows
                .iter()
                .any(|binding| binding.source.native_id == original_source.native_id)
        );
        assert!(
            journal
                .command_find_request(&first.owner, &first.request)
                .unwrap()
                .unwrap()
                .response
                .is_some()
        );
        client_settle(
            &mut journal,
            &SourceKey {
                group: first.source.group.clone(),
                native_id: first.source.native_id.clone(),
            },
            first.response.as_ref().unwrap(),
        );
        journal
            .command_maintain(NOW, &CommandProtection::default())
            .unwrap();
        assert!(
            journal
                .command_find_request(&first.owner, &first.request)
                .unwrap()
                .is_none()
        );
        journal.command_project_request(&second, NOW).unwrap();
        let saved = journal
            .command_find_request(&second.owner, &second.request)
            .unwrap()
            .unwrap();
        assert!(saved.response.is_some());
        let sources = journal.command_native_bindings(None, 64).unwrap();
        assert!(
            sources
                .rows
                .iter()
                .any(|source| source.source.native_id == original_source.native_id)
        );
    }
    #[test]
    fn poll_consumption_validates_gaps_both_cursors_and_preserves_current_pair_on_repeat() {
        let (_tmp, mut j, owner) = client_setup();
        let initial = submit(1);
        let (initial_source, initial_outgoing) =
            client_exchange(&mut j, &owner, &initial, &response(&initial));
        let initial_digest = j
            .command_client_response(&owner, initial.id)
            .unwrap()
            .unwrap()
            .1;
        j.command_consume_response(&owner, initial.id, initial_digest, None)
            .unwrap();

        let poll = |n, stdout_cursor, stderr_cursor| Request {
            binding: owner.binding.clone(),
            id: id(n),
            op: RequestOp::Poll {
                invocation: id(1),
                stdout_cursor,
                stderr_cursor,
                max_bytes: 100,
            },
        };
        let first = poll(200, 0, 0);
        let mut answer = response(&first);
        if let Ok(ResponseResult::Poll {
            status,
            stdout,
            stderr,
        }) = &mut answer.result
        {
            status.engine_known = true;
            status.admission = Admission::Engine;
            status.state = State::Running;
            status.stdout.produced = 10;
            status.stderr.produced = 2;
            status.stdout.gaps.push(wire::Gap {
                start: 0,
                end: Some(8),
                reason: wire::GapReason::Expired,
            });
            stdout.bytes = vec![1, 2];
            stdout.next_cursor = 10;
            stderr.bytes = vec![3, 4];
            stderr.next_cursor = 2;
        }
        let (source, outgoing) = client_exchange(&mut j, &owner, &first, &answer);
        let digest = j
            .command_client_response(&owner, first.id)
            .unwrap()
            .unwrap()
            .1;
        assert!(matches!(
            j.command_consume_response(
                &owner,
                first.id,
                digest,
                Some(ClientDelivery {
                    stdout: 1,
                    stderr: 0
                })
            ),
            Err(Error::Invalid("command_cursor_stale"))
        ));
        let pair = j
            .command_consume_response(&owner, first.id, digest, Some(ClientDelivery::default()))
            .unwrap();
        assert!(
            pair == ClientDelivery {
                stdout: 10,
                stderr: 2
            }
        );
        assert!(
            j.command_consume_response(&owner, initial.id, initial_digest, None)
                .unwrap()
                == pair
        );
        client_settle(&mut j, &initial_source, &initial_outgoing);
        // An unretired Poll still owns its slot even after local consumption.
        let second = poll(201, 10, 2);
        let bytes = wire::encode_request(&second).unwrap();
        let next_publication = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            second.id,
            wire::request_digest(&bytes),
            bytes,
            owner.local(),
        )
        .unwrap();
        assert!(
            j.command_submit_client(&owner, &second, &next_publication, NOW)
                .is_err()
        );
        client_settle(&mut j, &source, &outgoing);
        j.command_maintain(NOW, &CommandProtection::default())
            .unwrap();
        let stale = poll(202, 0, 0);
        let bytes = wire::encode_request(&stale).unwrap();
        let pub_stale = crate::mdk_adapter::prepare_command_publication(
            owner.group(),
            owner.binding.clone(),
            stale.id,
            wire::request_digest(&bytes),
            bytes,
            owner.local(),
        )
        .unwrap();
        assert!(matches!(
            j.command_submit_client(&owner, &stale, &pub_stale, NOW),
            Err(Error::Invalid("command_cursor_stale"))
        ));
        let mut bad = response(&second);
        if let Ok(ResponseResult::Poll {
            status,
            stdout,
            stderr,
        }) = &mut bad.result
        {
            status.engine_known = true;
            status.admission = Admission::Engine;
            status.state = State::Running;
            stdout.next_cursor = 9;
            stderr.next_cursor = 3;
            stderr.bytes = vec![1];
            status.stdout.gaps.push(wire::Gap {
                start: 8,
                end: None,
                reason: wire::GapReason::Expired,
            });
        }
        let (_, _) = client_exchange(&mut j, &owner, &second, &bad);
        let bad_digest = j
            .command_client_response(&owner, second.id)
            .unwrap()
            .unwrap()
            .1;
        assert!(matches!(
            j.command_consume_response(&owner, second.id, bad_digest, Some(pair)),
            Err(Error::Invalid("command_output_cursor_invalid"))
        ));
        assert!(j.command_client_cursors(&owner, id(1)).unwrap() == pair);
        assert!(
            j.command_client_response(&owner, second.id)
                .unwrap()
                .is_none()
        );
        assert!(
            j.command_consume_response(&owner, second.id, bad_digest, Some(pair))
                .unwrap()
                == pair
        );
        assert!(
            j.command_consume_response(&owner, initial.id, digest, None)
                .is_err()
        );
    }
    #[test]
    fn payload_expiry_survives_uncertain_observation_reopen_and_duplicate_submit() {
        let (tmp, mut j) = setup();
        let (plan, key) = admitted(&mut j, 1);
        fact(&mut j, &key, State::Exited);
        settle(&mut j, &plan, false);
        j.command_maintain(NOW + 86401, &CommandProtection::default())
            .unwrap();
        let row = j.command_get(&key).unwrap().unwrap();
        assert!(row.submit.is_none());
        assert_eq!(row.stdin_length, 4);
        let op = j
            .command_begin_operation(&key, row.revision, EngineOperation::Observe)
            .unwrap();
        j.command_commit_completion(
            &EngineCompletion {
                operation: op.id,
                key: key.clone(),
                revision: op.revision,
                result: EngineObservation::Failed(OperationalFailure::Timeout),
            },
            &[],
        )
        .unwrap();
        drop(j);
        let mut j = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        let recovered = j.command_recovery().unwrap();
        assert_eq!(recovered.len(), 1);
        let row = j.command_get(&recovered[0].key).unwrap().unwrap();
        assert!(row.engine_known && row.submit.is_none());
        assert!(
            crate::command_engine::next_feeder_operation(&row)
                .unwrap()
                .is_none()
        );
        assert!(j.command_get(&key).unwrap().unwrap().uncertain);
        let mut duplicate = projection(plan.request.clone());
        duplicate.decision = AdmissionDecision::Existing;
        j.command_project_request(&duplicate, NOW + 86401).unwrap();
        assert!(j.command_get(&key).unwrap().unwrap().submit.is_none());
        let mut changed = duplicate;
        if let RequestOp::Submit(s) = &mut changed.request.op {
            s.stdin.push(8);
        }
        assert!(j.command_project_request(&changed, NOW + 86401).is_err());
    }
    #[test]
    fn payload_scan_wraps_past_protected_rows_and_refusals_do_not_require_payload() {
        let (_tmp, mut j) = setup();
        let mut protected = CommandProtection::default();
        protected.groups.insert(owner().group.clone());
        let refusal = WireError {
            code: ErrorCode::Unauthorized,
            detail: None,
        };
        for n in 1..=257 {
            let mut o = owner();
            if n == 257 {
                o.group = b"unprotected".to_vec();
            }
            let request = submit(n);
            let RequestOp::Submit(s) = request.op else {
                unreachable!()
            };
            let digest = wire::submit_digest(o.group(), o.binding(), &s).unwrap();
            insert_invocation(&j.conn, &o, &s, digest, Some(refusal.clone()), NOW).unwrap();
        }
        j.command_maintain(NOW + 86401, &protected).unwrap();
        assert!(
            j.command_get(&owner().key(id(257)))
                .unwrap()
                .unwrap()
                .submit
                .is_some()
        );
        j.command_maintain(NOW + 86401, &protected).unwrap();
        assert!(
            j.command_get(&owner().key(id(257)))
                .unwrap()
                .unwrap()
                .submit
                .is_none()
        );
        assert!(
            j.command_get(&owner().key(id(1)))
                .unwrap()
                .unwrap()
                .submit
                .is_some()
        );
        j.command_maintain(NOW + 86401, &protected).unwrap();
        let cursor = j
            .conn
            .query_row("SELECT payload_cursor FROM command_replay_state", [], |r| {
                sql_u64(r, 0)
            })
            .unwrap();
        assert_eq!(cursor, 256);
    }
    #[test]
    fn pending_invalidation_preserves_only_its_group_until_late_revocation() {
        let (_tmp, mut j, a) = client_setup();
        let mut b = a.clone();
        b.group = b"other-group".to_vec();
        b.binding.session = id(9);
        let first = submit(1);
        let mut second = submit(2);
        second.binding = b.binding.clone();
        let (source_a, out_a) = client_exchange(&mut j, &a, &first, &response(&first));
        let (source_b, out_b) = client_exchange(&mut j, &b, &second, &response(&second));
        for (o, r, s, p) in [
            (&a, &first, &source_a, &out_a),
            (&b, &second, &source_b, &out_b),
        ] {
            let digest = j.command_client_response(o, r.id).unwrap().unwrap().1;
            j.command_consume_response(o, r.id, digest, None).unwrap();
            client_settle(&mut j, s, p);
        }
        let mut protection = CommandProtection::default();
        protection.invalidation_groups.insert(a.group.clone());
        j.command_maintain(NOW + 86401, &protection).unwrap();
        assert!(j.command_find_request(&a, &first).unwrap().is_some());
        assert!(j.command_find_request(&b, &second).unwrap().is_none());
        assert!(
            j.command_get(&a.key(id(1)))
                .unwrap()
                .unwrap()
                .submit
                .is_some()
        );
        assert!(
            j.command_get(&b.key(id(2)))
                .unwrap()
                .unwrap()
                .submit
                .is_none()
        );
        j.command_apply_native(&[CommandNativeUpdate::Invalidated {
            group: source_a.group,
            native_id: source_a.native_id,
            possible_publication: None,
        }])
        .unwrap();
        assert!(
            !j.command_get(&a.key(id(1)))
                .unwrap()
                .unwrap()
                .authority_valid
        );
        j.command_maintain(NOW + 86401, &CommandProtection::default())
            .unwrap();
        assert!(j.command_find_request(&a, &first).unwrap().is_none());
    }
    #[test]
    fn early_invalidation_survives_separate_batches_and_unknown_incoming_allocates_nothing() {
        let (_tmp, mut j) = setup();
        let (first, _) = admitted(&mut j, 1);
        let (second, _) = admitted(&mut j, 2);
        let a = first.response.unwrap().key;
        let b = second.response.unwrap().key;
        j.command_claim_publication(&a).unwrap();
        j.command_claim_publication(&b).unwrap();
        let invalid = |key: &PublicationKey, n| CommandNativeUpdate::Invalidated {
            group: key.group.clone(),
            native_id: vec![n],
            possible_publication: Some(key.clone()),
        };
        let before: Vec<_> = (1..=64).map(|n| invalid(&a, n)).collect();
        j.command_apply_native(&before).unwrap();
        assert_eq!(j.command_pending_invalidations().unwrap().len(), 64);
        j.command_apply_native(&[CommandNativeUpdate::Published {
            key: a.clone(),
            native_id: vec![1],
            evidence: evidence(1),
        }])
        .unwrap();
        assert_eq!(
            publication_state(&j.conn, &a).unwrap().unwrap(),
            "invalidated"
        );
        assert!(j.command_publication_bindings(&a).unwrap()[0].invalidated);
        assert_eq!(j.command_pending_invalidations().unwrap().len(), 63);
        j.command_apply_native(&[CommandNativeUpdate::Published {
            key: b.clone(),
            native_id: vec![100],
            evidence: evidence(1),
        }])
        .unwrap();
        let mut after: Vec<_> = (101..=163)
            .map(|n| CommandNativeUpdate::Invalidated {
                group: a.group.clone(),
                native_id: vec![n],
                possible_publication: None,
            })
            .collect();
        after.push(invalid(&b, 100));
        j.command_apply_native(&after).unwrap();
        assert_eq!(
            publication_state(&j.conn, &b).unwrap().unwrap(),
            "invalidated"
        );
        assert_eq!(j.command_pending_invalidations().unwrap().len(), 63);
        let source = SourceKey {
            group: a.group.clone(),
            native_id: vec![2],
        };
        j.command_retire_invalidation(&source).unwrap();
        assert_eq!(j.command_pending_invalidations().unwrap().len(), 62);
        // A later binding remains retained even if passed to tombstone pruning.
        j.command_retire_invalidation(&SourceKey {
            group: a.group.clone(),
            native_id: vec![1],
        })
        .unwrap();
        assert_eq!(j.command_publication_bindings(&a).unwrap().len(), 1);
        for n in 1000u16..1194 {
            j.command_apply_native(&[CommandNativeUpdate::Invalidated {
                group: a.group.clone(),
                native_id: n.to_be_bytes().to_vec(),
                possible_publication: Some(a.clone()),
            }])
            .unwrap();
        }
        assert_eq!(j.command_pending_invalidations().unwrap().len(), 256);
        assert!(matches!(
            j.command_apply_native(&[CommandNativeUpdate::Invalidated {
                group: a.group.clone(),
                native_id: vec![255, 255],
                possible_publication: Some(a.clone())
            }]),
            Err(Error::Storage("command_invalidation_capacity"))
        ));
        assert_eq!(j.command_pending_invalidations().unwrap().len(), 256);
    }
    #[test]
    fn conflicting_publication_evidence_is_quarantined_without_blocking_other_bindings() {
        let (_tmp, mut j) = setup();
        let (first, _) = admitted(&mut j, 1);
        let (second, _) = admitted(&mut j, 2);
        let a = first.response.unwrap().key;
        let b = second.response.unwrap().key;
        j.command_claim_publication(&a).unwrap();
        j.command_claim_publication(&b).unwrap();
        j.command_apply_native(&[CommandNativeUpdate::Published {
            key: a.clone(),
            native_id: vec![1],
            evidence: evidence(10),
        }])
        .unwrap();
        let results = j
            .command_apply_native(&[
                CommandNativeUpdate::Published {
                    key: a.clone(),
                    native_id: vec![1],
                    evidence: evidence(11),
                },
                CommandNativeUpdate::Published {
                    key: b.clone(),
                    native_id: vec![2],
                    evidence: evidence(12),
                },
            ])
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].key == b);
        let bindings = j.command_publication_bindings(&a).unwrap();
        assert!(bindings[0].quarantined);
        assert!(!bindings[0].acked);
        assert!(bindings[0].evidence.as_ref() == Some(&evidence(10)));
        assert!(
            j.command_record_native_acks(
                &[],
                &[PublishedKey {
                    key: a.clone(),
                    native_id: vec![1]
                }]
            )
            .is_err()
        );
        j.command_record_native_acks(&[], &results).unwrap();
        assert!(j.command_publication_bindings(&b).unwrap()[0].acked);
        assert!(
            j.command_apply_native(&[CommandNativeUpdate::Published {
                key: a.clone(),
                native_id: vec![1],
                evidence: evidence(10)
            }])
            .unwrap()
            .is_empty()
        );
        // The same native ID cannot silently move to another application event.
        assert!(
            j.command_apply_native(&[CommandNativeUpdate::Published {
                key: a.clone(),
                native_id: vec![2],
                evidence: evidence(12)
            }])
            .unwrap()
            .is_empty()
        );
        assert!(j.command_publication_bindings(&b).unwrap()[0].quarantined);
        j.command_maintain(NOW + WEEK + 1, &CommandProtection::default())
            .unwrap();
        assert!(j.command_publication_owned(&a).unwrap());
        assert!(j.command_publication_owned(&b).unwrap());
    }
    #[test]
    fn revoked_client_slots_retire_after_native_acks_without_local_consumption() {
        for (projected, native_invalidation) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let (_tmp, mut j, owner) = client_setup();
            let request = submit(1);
            let (source, outgoing) = if projected {
                let (source, outgoing) =
                    client_exchange(&mut j, &owner, &request, &response(&request));
                (Some(source), outgoing)
            } else {
                (None, client_request(&mut j, &owner, &request))
            };
            j.command_claim_publication(&outgoing.key).unwrap();
            let bindings = j
                .command_apply_native(&[CommandNativeUpdate::Published {
                    key: outgoing.key.clone(),
                    native_id: vec![77],
                    evidence: evidence(1),
                }])
                .unwrap();
            if native_invalidation {
                j.command_apply_native(&[CommandNativeUpdate::Invalidated {
                    group: owner.group().to_vec(),
                    native_id: vec![77],
                    possible_publication: None,
                }])
                .unwrap();
            } else {
                j.command_revoke_group(owner.group()).unwrap();
            }
            assert!(j.command_client_response(&owner, request.id).is_err());
            j.command_maintain(NOW + 86401, &CommandProtection::default())
                .unwrap();
            assert_eq!(totals(&j), (1, 1, 1));
            // Genuine outgoing ACK alone is insufficient for an incoming response.
            j.command_record_native_acks(&[], &bindings).unwrap();
            if let Some(source) = source {
                j.command_maintain(NOW + 86401, &CommandProtection::default())
                    .unwrap();
                assert_eq!(totals(&j), (1, 1, 1));
                j.command_record_native_acks(&[source], &[]).unwrap();
            }
            let mut protected = CommandProtection::default();
            protected.invalidation_groups.insert(owner.group().to_vec());
            j.command_maintain(NOW + 86401, &protected).unwrap();
            assert_eq!(totals(&j), (1, 1, 1));
            j.command_maintain(NOW + 86401, &CommandProtection::default())
                .unwrap();
            assert_eq!(totals(&j), (1, 0, 0));
            assert!(
                j.command_get(&owner.key(id(1)))
                    .unwrap()
                    .unwrap()
                    .submit
                    .is_none()
            );
        }
    }
    #[test]
    fn conflicting_submit_freezes_refusal_without_mutating_original_invocation() {
        let (_tmp, mut j) = setup();
        let (_, key) = admitted(&mut j, 1);
        let original = j.command_get(&key).unwrap().unwrap();
        let original_facts = encode(&Facts::from_record(&original)).unwrap();
        let mut request = submit(2);
        let RequestOp::Submit(submit) = &mut request.op else {
            panic!()
        };
        submit.invocation = key.invocation;
        submit.stdin.push(255);
        let mut plan = projection(request);
        assert!(matches!(
            j.command_project_request(&plan, NOW),
            Err(Error::Invalid("command_invocation_conflict"))
        ));
        let refusal = WireError {
            code: ErrorCode::InvocationConflict,
            detail: None,
        };
        let mut answer = response(&plan.request);
        answer.result = Err(refusal.clone());
        plan.response = Some(publication(&plan.owner, &answer));
        plan.decision = AdmissionDecision::Refused(refusal);
        j.command_project_request(&plan, NOW).unwrap();
        assert_eq!(totals(&j), (1, 2, 2));
        let after = j.command_get(&key).unwrap().unwrap();
        assert_eq!(original_facts, encode(&Facts::from_record(&after)).unwrap());
        assert_eq!(original.digest, after.digest);
        assert!(original.submit == after.submit);
        let frozen = plan.response.clone().unwrap();
        plan.response = None;
        plan.decision = AdmissionDecision::Existing;
        plan.source.native_id = vec![93];
        assert!(j.command_project_request(&plan, NOW).unwrap().existing);
        assert_eq!(totals(&j), (1, 2, 2));
        assert!(
            j.command_claim_publication(&frozen.key)
                .unwrap()
                .unwrap()
                .encoded_wire
                == frozen.encoded_wire
        );
        // A new mismatched request cannot bypass immutability using another refusal.
        plan.request.id = id(3);
        plan.decision = AdmissionDecision::Refused(WireError {
            code: ErrorCode::CommandDenied,
            detail: None,
        });
        plan.response = Some(publication(&plan.owner, &response(&plan.request)));
        assert!(j.command_project_request(&plan, NOW).is_err());
        assert_eq!(totals(&j), (1, 2, 2));
    }
    #[test]
    fn transient_client_read_errors_do_not_change_invocation_admission() {
        let (_tmp, mut j, owner) = client_setup();
        let request = submit(1);
        client_exchange(&mut j, &owner, &request, &response(&request));
        let read = Request {
            binding: request.binding.clone(),
            id: id(2),
            op: RequestOp::Status { invocation: id(1) },
        };
        let mut error = response(&read);
        error.result = Err(WireError {
            code: ErrorCode::RecoveryUncertain,
            detail: None,
        });
        client_exchange(&mut j, &owner, &read, &error);
        let record = j.command_get(&owner.key(id(1))).unwrap().unwrap();
        assert!(record.refusal.is_none());
        assert!(record.active());
        let retry = Request { id: id(3), ..read };
        client_exchange(&mut j, &owner, &retry, &response(&retry));
        assert!(
            j.command_get(&owner.key(id(1)))
                .unwrap()
                .unwrap()
                .refusal
                .is_none()
        );
    }
    #[test]
    fn third_native_binding_is_quarantined_without_aborting_unrelated_publication() {
        let (_tmp, mut j) = setup();
        let (first, _) = admitted(&mut j, 1);
        let (second, _) = admitted(&mut j, 2);
        let a = first.response.unwrap().key;
        let b = second.response.unwrap().key;
        j.command_claim_publication(&a).unwrap();
        j.command_claim_publication(&b).unwrap();
        let results = j
            .command_apply_native(&[
                CommandNativeUpdate::Published {
                    key: a.clone(),
                    native_id: vec![1],
                    evidence: evidence(1),
                },
                CommandNativeUpdate::Published {
                    key: a.clone(),
                    native_id: vec![2],
                    evidence: evidence(2),
                },
                CommandNativeUpdate::Published {
                    key: a.clone(),
                    native_id: vec![3],
                    evidence: evidence(3),
                },
                CommandNativeUpdate::Published {
                    key: b.clone(),
                    native_id: vec![4],
                    evidence: evidence(4),
                },
            ])
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].key == b);
        let bindings = j.command_publication_bindings(&a).unwrap();
        assert_eq!(bindings.len(), 2);
        assert!(
            bindings
                .iter()
                .all(|binding| binding.quarantined && !binding.acked)
        );
        j.command_record_native_acks(&[], &results).unwrap();
        assert!(j.command_publication_bindings(&b).unwrap()[0].acked);
        assert!(
            j.command_apply_native(&[CommandNativeUpdate::Published {
                key: a.clone(),
                native_id: vec![3],
                evidence: evidence(3)
            }])
            .unwrap()
            .is_empty()
        );
        j.command_maintain(NOW + WEEK + 1, &CommandProtection::default())
            .unwrap();
        assert!(j.command_publication_owned(&a).unwrap());
    }
    #[test]
    fn retention_ceiling_and_unknown_incoming_ack_are_rejected_without_mutation() {
        let (_tmp, mut j) = setup();
        let mut settings = limits();
        settings.payload_retention = std::time::Duration::from_secs(WEEK + 1);
        assert!(matches!(
            j.command_configure(&settings),
            Err(Error::Invalid("command_limits"))
        ));
        let (plan, _) = admitted(&mut j, 1);
        let source = SourceKey {
            group: plan.source.group,
            native_id: plan.source.native_id,
        };
        let unknown = SourceKey {
            group: source.group.clone(),
            native_id: vec![99],
        };
        assert!(matches!(
            j.command_record_native_acks(&[source, unknown], &[]),
            Err(Error::Invalid("command_source_unknown"))
        ));
        assert_eq!(
            count(
                &j.conn,
                "SELECT count(*) FROM command_incoming_sources WHERE acked=1",
                None
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn authenticated_revoked_status_synchronizes_slot_authority_and_allows_acked_retirement() {
        let (_tmp, mut j, owner) = client_setup();
        let request = submit(1);
        let outgoing = client_request(&mut j, &owner, &request);
        j.command_claim_publication(&outgoing.key).unwrap();
        let published = j
            .command_apply_native(&[CommandNativeUpdate::Published {
                key: outgoing.key.clone(),
                native_id: vec![51],
                evidence: evidence(1),
            }])
            .unwrap();
        let mut answer = response(&request);
        let Ok(ResponseResult::Submit { status, .. }) = &mut answer.result else {
            panic!()
        };
        status.authority_valid = false;
        let incoming = publication(&owner, &answer);
        let source = CommandSource {
            group: owner.group().to_vec(),
            native_id: vec![52],
            app_event_id: incoming.key.app_event_id,
            author: owner.binding.server,
            authority_metadata: b"auth".to_vec(),
        };
        j.command_project_response(&source, &answer, NOW).unwrap();
        assert!(
            !j.command_get(&owner.key(id(1)))
                .unwrap()
                .unwrap()
                .authority_valid
        );
        assert!(j.command_client_response(&owner, request.id).is_err());
        j.command_maintain(NOW, &CommandProtection::default())
            .unwrap();
        assert_eq!(totals(&j), (1, 1, 1));
        j.command_record_native_acks(
            &[SourceKey {
                group: source.group,
                native_id: source.native_id,
            }],
            &published,
        )
        .unwrap();
        j.command_maintain(NOW, &CommandProtection::default())
            .unwrap();
        assert_eq!(totals(&j), (1, 0, 0));
    }
    #[test]
    fn conflict_refusal_requires_an_existing_invocation() {
        let (_tmp, mut j) = setup();
        let mut plan = projection(submit(1));
        let error = WireError {
            code: ErrorCode::InvocationConflict,
            detail: None,
        };
        let mut answer = response(&plan.request);
        answer.result = Err(error.clone());
        plan.response = Some(publication(&plan.owner, &answer));
        plan.decision = AdmissionDecision::Refused(error);
        assert!(matches!(
            j.command_project_request(&plan, NOW),
            Err(Error::Invalid("command_invocation_conflict"))
        ));
        assert_eq!(totals(&j), (0, 0, 0));
    }
    #[test]
    fn quarantined_binding_blocks_retirement_after_another_publication() {
        let (_tmp, mut j) = setup();
        let (plan, _) = admitted(&mut j, 1);
        let key = plan.response.as_ref().unwrap().key.clone();
        j.command_claim_publication(&key).unwrap();
        for (native, epoch) in [(1, 1), (1, 2), (2, 3)] {
            j.command_apply_native(&[CommandNativeUpdate::Published {
                key: key.clone(),
                native_id: vec![native],
                evidence: evidence(epoch),
            }])
            .unwrap();
        }
        assert_eq!(
            publication_state(&j.conn, &key).unwrap().as_deref(),
            Some("published")
        );
        let bindings = j.command_publication_bindings(&key).unwrap();
        assert_eq!(bindings.len(), 2);
        assert!(bindings.iter().any(|binding| binding.quarantined));
        // Publication status can converge, but a quarantined native binding is
        // still an independent, irreversible ACK/retirement gate.
        j.command_record_native_acks(
            &[SourceKey {
                group: plan.source.group,
                native_id: plan.source.native_id,
            }],
            &[PublishedKey {
                key: key.clone(),
                native_id: vec![2],
            }],
        )
        .unwrap();
        j.command_maintain(NOW + WEEK + 1, &CommandProtection::default())
            .unwrap();
        assert!(j.command_publication_owned(&key).unwrap());
    }
    #[test]
    fn runtime_uncertain_publication_converges_and_retires_after_genuine_binding_ack() {
        let (_tmp, mut j) = setup();
        let (plan, _) = admitted(&mut j, 1);
        let key = plan.response.as_ref().unwrap().key.clone();
        j.command_claim_publication(&key).unwrap();
        j.command_publication_uncertain(&key).unwrap();
        assert_eq!(
            publication_state(&j.conn, &key).unwrap().as_deref(),
            Some("uncertain")
        );
        let published = j
            .command_apply_native(&[CommandNativeUpdate::Published {
                key: key.clone(),
                native_id: vec![88],
                evidence: evidence(1),
            }])
            .unwrap();
        assert_eq!(
            publication_state(&j.conn, &key).unwrap().as_deref(),
            Some("published")
        );
        j.command_maintain(NOW, &CommandProtection::default())
            .unwrap();
        assert_eq!(totals(&j), (1, 1, 1));
        j.command_record_native_acks(
            &[SourceKey {
                group: plan.source.group,
                native_id: plan.source.native_id,
            }],
            &published,
        )
        .unwrap();
        j.command_maintain(NOW, &CommandProtection::default())
            .unwrap();
        assert_eq!(totals(&j), (1, 0, 0));
    }
}
