//! Private trusted-frontend pipe protocol; this bridge is not a network authenticator.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use marmish_engine::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::time::Duration;
use zeroize::Zeroizing;

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
}
impl Bridge {
    fn dispatch(&mut self, operation: Operation) -> Result<Value> {
        if let Operation::Init {
            version,
            state_root,
            worker_executable,
            server,
            allowed_principals,
            cwd,
            allow_shell_command,
            command_policy,
        } = operation
        {
            if self.engine.is_some() || self.cleanup.is_some() || version != 1 || !cwd.is_absolute()
            {
                return Err(EngineError::InvalidRequest);
            }
            let server = ServerId(hex(&server)?);
            let allowed_principals = allowed_principals
                .iter()
                .map(|p| hex(p).map(PrincipalId))
                .collect::<Result<Vec<_>>>()?;
            self.engine = Some(Engine::open(EnginePolicy {
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
            })?);
            self.server = Some(server);
            return Ok(json!({"version":1}));
        }
        if let Operation::CleanupReady = operation {
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
        let mut value: Value =
            serde_json::from_slice(line).map_err(|_| EngineError::InvalidRequest)?;
        let object = value.as_object_mut().ok_or(EngineError::InvalidRequest)?;
        let id = object.remove("id").ok_or(EngineError::InvalidRequest)?;
        hex::<16>(id.as_str().ok_or(EngineError::InvalidRequest)?)?;
        let operation =
            serde_json::from_value::<Operation>(value).map_err(|_| EngineError::InvalidRequest);
        Ok::<_, EngineError>((id, operation))
    })();
    match parsed {
        Ok((id, operation)) => match operation.and_then(|op| bridge.dispatch(op)) {
            Ok(result) => json!({"id":id,"ok":true,"result":result}),
            Err(error) => failure(id, error_code(error)),
        },
        Err(error) => failure(Value::Null, error_code(error)),
    }
}

/// Process one bounded request and flush one bounded reply at a time.
/// EOF drops only the frontend; detached command workers retain their lifecycle.
pub fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut input = input;
    let mut bridge = Bridge::default();
    loop {
        let mut line = Zeroizing::new(Vec::new());
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
