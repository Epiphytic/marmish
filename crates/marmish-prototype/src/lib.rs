//! Private trusted-frontend pipe protocol; this bridge is not a network authenticator.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use marmish_engine::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

pub const MAX_LINE: usize = 128 * 1024;
const STREAM_BYTES: usize = 4096;
const COMMAND_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Init {
        version: u32,
        state_root: PathBuf,
        worker_executable: PathBuf,
        server: String,
        allowed_principals: Vec<String>,
        cwd: PathBuf,
        #[serde(default)]
        allow_shell_command: bool,
        #[serde(default)]
        command_policy: Option<WirePolicy>,
        #[serde(default)]
        storage: Option<WireStorage>,
    },
    Token {
        principal: String,
        endpoint: String,
    },
    Submit {
        principal: String,
        endpoint: String,
        invocation: String,
        token: String,
        command: Command,
    },
    Status {
        principal: String,
        endpoint: String,
        invocation: String,
    },
    BeginInput {
        principal: String,
        endpoint: String,
        invocation: String,
    },
    WriteStdin {
        principal: String,
        endpoint: String,
        invocation: String,
        generation: String,
        offset: u64,
        bytes: String,
        fresh_ms: u64,
    },
    StdinEof {
        principal: String,
        endpoint: String,
        invocation: String,
        generation: String,
        final_offset: u64,
    },
    ReadOutput {
        principal: String,
        endpoint: String,
        invocation: String,
        stream: String,
        cursor: u64,
        limit: usize,
    },
    Cancel {
        principal: String,
        endpoint: String,
        invocation: String,
    },
    Maintain {
        limit: u32,
    },
    CleanupReady,
    MigrationCommit,
}
#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "id")]
    _id: String,
    #[serde(flatten)]
    operation: Operation,
}
struct Master(Zeroizing<[u8; 32]>);
impl<'de> Deserialize<'de> for Master {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        let bytes = Zeroizing::new(
            STANDARD
                .decode(value.as_bytes())
                .map_err(|_| serde::de::Error::custom("invalid master"))?,
        );
        if bytes.len() != 32
            || Zeroizing::new(STANDARD.encode(bytes.as_slice())).as_str() != value.as_str()
        {
            return Err(serde::de::Error::custom("invalid master"));
        }
        let mut master = Zeroizing::new([0; 32]);
        master.copy_from_slice(&bytes);
        Ok(Self(master))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    version: u8,
    purpose: String,
    id: String,
}
enum WireStorage {
    Files,
    Native {
        root_id: String,
        reference: Reference,
        master: Master,
    },
    MigrateNative {
        root_id: String,
        reference: Reference,
        master: Master,
    },
}
impl<'de> Deserialize<'de> for WireStorage {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct StorageVisitor;
        impl<'de> serde::de::Visitor<'de> for StorageVisitor {
            type Value = WireStorage;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("storage configuration")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let (mut mode, mut root_id, mut reference, mut master) = (None, None, None, None);
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "mode" if mode.is_none() => mode = Some(map.next_value::<String>()?),
                        "root_id" if root_id.is_none() => {
                            root_id = Some(map.next_value::<String>()?)
                        }
                        "reference" if reference.is_none() => {
                            reference = Some(map.next_value::<Reference>()?)
                        }
                        "master" if master.is_none() => master = Some(map.next_value::<Master>()?),
                        _ => return Err(serde::de::Error::custom("invalid storage field")),
                    }
                }
                if mode.as_deref() == Some("files") {
                    if root_id.is_some() || reference.is_some() || master.is_some() {
                        return Err(serde::de::Error::custom("invalid files storage"));
                    }
                    return Ok(WireStorage::Files);
                }
                let root_id = root_id.ok_or_else(|| serde::de::Error::missing_field("root_id"))?;
                let reference =
                    reference.ok_or_else(|| serde::de::Error::missing_field("reference"))?;
                let master = master.ok_or_else(|| serde::de::Error::missing_field("master"))?;
                match mode.as_deref() {
                    Some("native") => Ok(WireStorage::Native {
                        root_id,
                        reference,
                        master,
                    }),
                    Some("migrate_native") => Ok(WireStorage::MigrateNative {
                        root_id,
                        reference,
                        master,
                    }),
                    _ => Err(serde::de::Error::custom("invalid storage mode")),
                }
            }
        }
        deserializer.deserialize_map(StorageVisitor)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitEnvelope {
    id: String,
    op: String,
    version: u32,
    state_root: PathBuf,
    worker_executable: PathBuf,
    server: String,
    allowed_principals: Vec<String>,
    cwd: PathBuf,
    #[serde(default)]
    allow_shell_command: bool,
    #[serde(default)]
    command_policy: Option<WirePolicy>,
    #[serde(default)]
    storage: Option<WireStorage>,
}
impl InitEnvelope {
    fn operation(self) -> Result<Operation> {
        hex::<16>(&self.id)?;
        if self.op != "init" {
            return Err(EngineError::InvalidRequest);
        }
        Ok(Operation::Init {
            version: self.version,
            state_root: self.state_root,
            worker_executable: self.worker_executable,
            server: self.server,
            allowed_principals: self.allowed_principals,
            cwd: self.cwd,
            allow_shell_command: self.allow_shell_command,
            command_policy: self.command_policy,
            storage: self.storage,
        })
    }
}
fn native_binding(root_id: String, reference: Reference) -> Result<NativeBinding> {
    if reference.version != 1 || reference.purpose != "sqlcipher" {
        return Err(EngineError::InvalidRequest);
    }
    Ok(NativeBinding::new(hex(&root_id)?, hex(&reference.id)?))
}
#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Exec { argv: Vec<String> },
    Shell { command: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePolicy {
    version: u32,
    #[serde(default)]
    defaults: Restrictions,
    #[serde(default)]
    keys: Vec<WireKeyRestrictions>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireKeyRestrictions {
    principal: String,
    restrictions: Restrictions,
}

impl WirePolicy {
    fn policy(self) -> Result<CommandPolicy> {
        Ok(CommandPolicy {
            version: self.version,
            defaults: self.defaults,
            keys: self
                .keys
                .into_iter()
                .map(|entry| {
                    Ok(KeyRestrictions {
                        principal: PrincipalId(hex(&entry.principal)?),
                        restrictions: entry.restrictions,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

fn hex<const N: usize>(value: &str) -> Result<[u8; N]> {
    if value.len() != N * 2 {
        return Err(EngineError::InvalidRequest);
    }
    let digit = |b| match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        _ => Err(EngineError::InvalidRequest),
    };
    let mut output = [0; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Ok(output)
}
fn encoded(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}
fn decoded(value: &str, max: usize) -> Result<Vec<u8>> {
    if value.len() > max.div_ceil(3) * 4 {
        return Err(EngineError::InvalidRequest);
    }
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| EngineError::InvalidRequest)?;
    if bytes.len() > max || STANDARD.encode(&bytes) != value {
        return Err(EngineError::InvalidRequest);
    }
    Ok(bytes)
}
impl Command {
    fn request(self) -> Result<CommandRequest> {
        match self {
            Self::Exec { argv } => {
                if argv.is_empty() || argv.len() > 128 {
                    return Err(EngineError::InvalidRequest);
                }
                let mut total = 0;
                let mut args = Vec::with_capacity(argv.len());
                for arg in argv {
                    let bytes = decoded(&arg, COMMAND_BYTES)?;
                    total += bytes.len() + 1;
                    if total > COMMAND_BYTES || bytes.contains(&0) {
                        return Err(EngineError::InvalidRequest);
                    }
                    args.push(bytes);
                }
                Ok(CommandRequest::Exec { argv: args })
            }
            Self::Shell { command } => {
                let bytes = decoded(&command, COMMAND_BYTES)?;
                if bytes.contains(&0) {
                    return Err(EngineError::InvalidRequest);
                }
                Ok(CommandRequest::Shell { command: bytes })
            }
        }
    }
}

#[derive(Default)]
struct Bridge {
    engine: Option<Engine>,
    server: Option<ServerId>,
    cleanup: Option<CleanupGuard>,
    migration: bool,
}
impl Bridge {
    fn initialize(&mut self, operation: Operation) -> Result<Value> {
        if let Operation::Init {
            version,
            state_root,
            worker_executable,
            server,
            allowed_principals,
            cwd,
            allow_shell_command,
            command_policy,
            storage,
        } = operation
        {
            if self.engine.is_some()
                || self.cleanup.is_some()
                || !matches!(version, 1 | 2)
                || !cwd.is_absolute()
            {
                return Err(EngineError::InvalidRequest);
            }
            let server = ServerId(hex(&server)?);
            let allowed_principals = allowed_principals
                .iter()
                .map(|p| hex(p).map(PrincipalId))
                .collect::<Result<Vec<_>>>()?;
            if (version == 1 && storage.is_some()) || (version == 2 && storage.is_none()) {
                return Err(EngineError::InvalidRequest);
            }
            let policy = EnginePolicy {
                state_root,
                worker_executable,
                server,
                allowed_principals,
                allow_shell_command,
                command_policy: command_policy.map(WirePolicy::policy).transpose()?,
                cwd: Some(cwd),
                limits: EngineLimits {
                    token_lifetime: Duration::from_secs(120),
                    ..EngineLimits::default()
                },
            };
            match storage.unwrap_or(WireStorage::Files) {
                WireStorage::Files => self.engine = Some(Engine::open(policy)?),
                WireStorage::Native {
                    root_id,
                    reference,
                    master,
                } => {
                    self.engine = Some(Engine::open_with_master(
                        policy,
                        native_binding(root_id, reference)?,
                        ProvidedMaster::new(*master.0),
                    )?)
                }
                WireStorage::MigrateNative {
                    root_id,
                    reference,
                    master,
                } => {
                    self.cleanup = Some(Engine::prepare_native_migration(
                        policy,
                        native_binding(root_id, reference)?,
                        ProvidedMaster::new(*master.0),
                    )?);
                    self.migration = true;
                }
            }
            self.server = Some(server);
            return Ok(json!({"version":version}));
        }
        Err(EngineError::InvalidRequest)
    }

    fn dispatch(&mut self, operation: Operation) -> Result<Value> {
        if matches!(operation, Operation::Init { .. }) {
            return self.initialize(operation);
        }
        if let Operation::MigrationCommit = operation {
            self.cleanup
                .as_mut()
                .ok_or(EngineError::InvalidRequest)?
                .commit_native()?;
            return Ok(json!({"committed":true}));
        }
        if let Operation::CleanupReady = operation {
            if self.migration {
                return Err(EngineError::InvalidRequest);
            }
            if self.cleanup.is_none() {
                let engine = self.engine.take().ok_or(EngineError::InvalidRequest)?;
                self.cleanup = Some(engine.into_cleanup()?);
            }
            return Ok(json!({"ready":true}));
        }
        let engine = self.engine.as_mut().ok_or(EngineError::InvalidRequest)?;
        if let Operation::Maintain { limit } = operation {
            if !(1..=128).contains(&limit) {
                return Err(EngineError::InvalidRequest);
            }
            let report = engine.maintain(limit)?;
            return Ok(
                json!({"records_removed":report.records_removed,"spool_bytes_removed":report.spool_bytes_removed}),
            );
        }
        let (principal, endpoint) = match &operation {
            Operation::Token {
                principal,
                endpoint,
            }
            | Operation::Submit {
                principal,
                endpoint,
                ..
            }
            | Operation::Status {
                principal,
                endpoint,
                ..
            }
            | Operation::BeginInput {
                principal,
                endpoint,
                ..
            }
            | Operation::WriteStdin {
                principal,
                endpoint,
                ..
            }
            | Operation::StdinEof {
                principal,
                endpoint,
                ..
            }
            | Operation::ReadOutput {
                principal,
                endpoint,
                ..
            }
            | Operation::Cancel {
                principal,
                endpoint,
                ..
            } => (principal, endpoint),
            _ => return Err(EngineError::InvalidRequest),
        };
        // Only the trusted coordinator may supply these authenticated peer labels.
        // The target account and server cannot be selected by a remote request.
        let ctx = engine.authorize(
            PrincipalId(hex(principal)?),
            EndpointId(hex(endpoint)?),
            self.server.ok_or(EngineError::InvalidRequest)?,
            unsafe { libc::geteuid() },
        )?;
        match operation {
            Operation::Token { .. } => {
                let token = engine.issue_execute_token(&ctx)?;
                let bytes = Zeroizing::new(token.expose_bytes());
                Ok(json!({"token":encoded(bytes.as_ref())}))
            }
            Operation::Submit {
                invocation,
                token,
                command,
                ..
            } => {
                let token = Zeroizing::new(token);
                let token = ExecuteToken::from_bytes(hex(&token)?);
                let submission = engine.submit(
                    &ctx,
                    &token,
                    InvocationId(hex(&invocation)?),
                    &command.request()?,
                )?;
                Ok(
                    json!({"disposition":match submission.disposition { SubmitDisposition::Accepted=>"accepted",SubmitDisposition::Existing=>"existing" },"status":status(submission.status)}),
                )
            }
            Operation::Status { invocation, .. } => Ok(status(
                engine.status(&ctx, InvocationId(hex(&invocation)?))?,
            )),
            Operation::Cancel { invocation, .. } => Ok(status(
                engine.cancel(&ctx, InvocationId(hex(&invocation)?))?,
            )),
            Operation::BeginInput { invocation, .. } => Ok(input(
                engine.begin_input(&ctx, InvocationId(hex(&invocation)?))?,
            )),
            Operation::WriteStdin {
                invocation,
                generation,
                offset,
                bytes,
                fresh_ms,
                ..
            } => {
                let bytes = decoded(&bytes, STREAM_BYTES)?;
                if !(1..=60000).contains(&fresh_ms) {
                    return Err(EngineError::InvalidRequest);
                }
                let receipt = engine.write_stdin(
                    &ctx,
                    InvocationId(hex(&invocation)?),
                    InputGeneration(hex(&generation)?),
                    offset,
                    &bytes,
                    Duration::from_millis(fresh_ms),
                )?;
                Ok(
                    json!({"accepted":receipt.accepted,"available_credit":receipt.available_credit,"duplicate":receipt.duplicate}),
                )
            }
            Operation::StdinEof {
                invocation,
                generation,
                final_offset,
                ..
            } => Ok(input(engine.stdin_eof(
                &ctx,
                InvocationId(hex(&invocation)?),
                InputGeneration(hex(&generation)?),
                final_offset,
            )?)),
            Operation::ReadOutput {
                invocation,
                stream,
                cursor,
                limit,
                ..
            } => {
                if !(1..=STREAM_BYTES).contains(&limit) {
                    return Err(EngineError::InvalidRequest);
                }
                let stream = match stream.as_str() {
                    "stdout" => Stream::Stdout,
                    "stderr" => Stream::Stderr,
                    _ => return Err(EngineError::InvalidRequest),
                };
                let chunk = engine.read_output(
                    &ctx,
                    InvocationId(hex(&invocation)?),
                    stream,
                    cursor,
                    limit,
                )?;
                Ok(
                    json!({"bytes":STANDARD.encode(chunk.bytes),"next_cursor":chunk.next_cursor,"status":output(chunk.status)}),
                )
            }
            _ => Err(EngineError::InvalidRequest),
        }
    }
}

fn input(value: InputStatus) -> Value {
    json!({"generation":value.generation.map(|g|encoded(&g.0)),"accepted":value.accepted,"delivered":value.delivered,"available_credit":value.available_credit,"health":match value.health {InputHealth::Open=>"open",InputHealth::EofPending=>"eof_pending",InputHealth::Closed=>"closed",InputHealth::Gap=>"gap",InputHealth::Uncertain=>"uncertain"},"gap_count":value.gap_count})
}
fn output(value: OutputStatus) -> Value {
    json!({"retained":value.retained,"produced":value.produced,"closed":value.closed,"gaps":value.gaps.into_iter().map(|g|json!({"start":g.start,"end":g.end,"reason":match g.reason {MissingReason::Limit=>"limit",MissingReason::StorageFailure=>"storage_failure",MissingReason::Expired=>"expired",MissingReason::UncertainTail=>"uncertain_tail"}})).collect::<Vec<_>>()})
}
fn status(value: InvocationStatus) -> Value {
    json!({"invocation":encoded(&value.invocation.0),"state":match value.state {InvocationState::Accepted=>"accepted",InvocationState::Launching=>"launching",InvocationState::Running=>"running",InvocationState::Exited=>"exited",InvocationState::UnknownLaunch=>"unknown_launch",InvocationState::Lost=>"lost"},"outcome":value.outcome.map(|o|match o {ExitOutcome::Code(value)=>json!({"kind":"code","value":value}),ExitOutcome::Signal(value)=>json!({"kind":"signal","value":value}),ExitOutcome::NotStarted=>json!({"kind":"not_started"})}),"cancel_requested":value.cancel_requested,"input":input(value.input),"stdout":output(value.stdout),"stderr":output(value.stderr)})
}

fn error_code(error: EngineError) -> &'static str {
    match error {
        EngineError::Unauthorized => "unauthorized",
        EngineError::RootForbidden => "root_forbidden",
        EngineError::InvalidPolicy => "invalid_policy",
        EngineError::CommandDenied => "command_denied",
        EngineError::UnsafePath => "unsafe_path",
        EngineError::AlreadyOpen => "already_open",
        EngineError::InvalidRequest => "invalid_request",
        EngineError::NotFound => "not_found",
        EngineError::InvocationConflict => "invocation_conflict",
        EngineError::InvalidToken => "invalid_token",
        EngineError::ExpiredToken => "expired_token",
        EngineError::QuotaExceeded => "quota_exceeded",
        EngineError::StaleGeneration => "stale_generation",
        EngineError::InputExpired => "input_expired",
        EngineError::InputConflict => "input_conflict",
        EngineError::InputClosed => "input_closed",
        EngineError::InputGap => "input_gap",
        EngineError::WorkerUnavailable => "worker_unavailable",
        EngineError::RecoveryUncertain => "recovery_uncertain",
        EngineError::StorageFailure => "storage_failure",
        EngineError::UnsupportedStorage => "unsupported_storage",
        EngineError::Busy => "busy",
        EngineError::InputBackpressure { .. } => "input_backpressure",
    }
}
fn failure(id: Value, code: &str) -> Value {
    json!({"id":id,"ok":false,"error":{"code":code}})
}
fn handle(bridge: &mut Bridge, line: &[u8]) -> Value {
    let parsed = (|| {
        #[derive(Deserialize)]
        struct Correlation {
            id: String,
            op: String,
        }
        let correlation: Correlation =
            serde_json::from_slice(line).map_err(|_| EngineError::InvalidRequest)?;
        hex::<16>(&correlation.id)?;
        let operation = if correlation.op == "init" {
            serde_json::from_slice::<InitEnvelope>(line)
                .map_err(|_| EngineError::InvalidRequest)
                .and_then(InitEnvelope::operation)
        } else {
            serde_json::from_slice::<Envelope>(line)
                .map(|envelope| envelope.operation)
                .map_err(|_| EngineError::InvalidRequest)
        };
        Ok::<_, EngineError>((Value::String(correlation.id), operation))
    })();
    match parsed {
        Ok((id, operation)) => match operation.and_then(|op| bridge.dispatch(op)) {
            Ok(result) => json!({"id":id,"ok":true,"result":result}),
            Err(error) => failure(id, error_code(error)),
        },
        Err(error) => failure(Value::Null, error_code(error)),
    }
}

// BufRead's ordinary backing allocation can retain consumed master bytes.
// Consume wipes each copied span; Drop wipes unread data as well.
struct PrivateReader<R> {
    inner: R,
    bytes: Zeroizing<[u8; 8192]>,
    start: usize,
    end: usize,
}
impl<R: Read> Read for PrivateReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(output.len());
        output[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}
impl<R: Read> BufRead for PrivateReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.start == self.end {
            self.start = 0;
            self.end = self.inner.read(self.bytes.as_mut())?;
        }
        Ok(&self.bytes[self.start..self.end])
    }
    fn consume(&mut self, count: usize) {
        let end = self.start + count.min(self.end - self.start);
        self.bytes[self.start..end].zeroize();
        self.start = end;
    }
}
/// Process one bounded request and flush one bounded reply at a time.
/// EOF drops only the frontend; detached command workers retain their lifecycle.
pub fn serve(input: impl Read, mut output: impl Write) -> io::Result<()> {
    let mut input = PrivateReader {
        inner: input,
        bytes: Zeroizing::new([0; 8192]),
        start: 0,
        end: 0,
    };
    let mut bridge = Bridge::default();
    loop {
        let mut line = Zeroizing::new(Vec::with_capacity(MAX_LINE + 1));
        let count = input
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        let fatal = count > MAX_LINE || !line.ends_with(b"\n");
        let reply = if fatal {
            failure(Value::Null, "invalid_framing")
        } else {
            handle(&mut bridge, &line)
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&reply)?);
        if bytes.len() + 1 > MAX_LINE {
            return Err(io::Error::other("bridge response limit"));
        }
        output.write_all(&bytes)?;
        output.write_all(b"\n")?;
        output.flush()?;
        if fatal {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod secret_request_tests {
    use super::*;
    #[test]
    fn private_reader_wipes_consumed_spans() {
        let mut input = PrivateReader {
            inner: io::Cursor::new(b"private\nnext"),
            bytes: Zeroizing::new([0; 8192]),
            start: 0,
            end: 0,
        };
        let mut line = Zeroizing::new(Vec::new());
        input.read_until(b'\n', &mut line).unwrap();
        assert_eq!(line.as_slice(), b"private\n");
        assert!(input.bytes[..8].iter().all(|byte| *byte == 0));
        assert_eq!(input.fill_buf().unwrap(), b"next");
        input.consume(4);
        assert!(input.bytes.iter().all(|byte| *byte == 0));
    }
    #[test]
    fn strict_secret_envelope_rejects_duplicate_unknown_and_noncanonical_fields() {
        let id = "1234567890abcdef1234567890abcdef";
        let master = STANDARD.encode([7; 32]);
        let init = format!(
            r#"{{"id":"{id}","op":"init","version":2,"state_root":"/invalid","worker_executable":"/invalid","server":"{}","allowed_principals":[],"cwd":"/","storage":{{"mode":"native","root_id":"{}","reference":{{"version":1,"purpose":"sqlcipher","id":"{}"}},"master":"{master}"}}}}"#,
            "11".repeat(32),
            "22".repeat(32),
            "33".repeat(32)
        );
        assert!(serde_json::from_str::<InitEnvelope>(&init).is_ok());
        for invalid in [
            init.replace("\"version\":2", "\"version\":2,\"version\":2"),
            init.replace(
                "\"master\":",
                &format!("\"master\":\"{master}\",\"master\":"),
            ),
            init.replace(
                "\"purpose\":\"sqlcipher\"",
                "\"purpose\":\"sqlcipher\",\"purpose\":\"sqlcipher\"",
            ),
            init.replace(
                "\"mode\":\"native\"",
                "\"mode\":\"native\",\"unknown\":true",
            ),
            init.replace("\"op\":\"init\"", "\"op\":\"init\",\"unknown\":true"),
            init.replace(&master, &STANDARD.encode([7; 31])),
            init.replace(&master, master.trim_end_matches('=')),
        ] {
            let response = handle(&mut Bridge::default(), invalid.as_bytes());
            assert_eq!(response["ok"], false);
            assert_eq!(response["error"]["code"], "invalid_request");
            assert_eq!(response["id"], id);
        }
    }
}
