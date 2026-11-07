//! Bounded, closed observations for the original build-only duplex cell.
use super::{records, streaming_records::DuplexChecks};
use serde::Serialize;
use std::{
    cell::Cell,
    io::{self, Write},
    time::Instant,
};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Role {
    LinuxAmd64,
    LinuxArm64,
    MacosArm64,
}
impl Role {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "linux-amd64" => Some(Self::LinuxAmd64),
            "linux-arm64" => Some(Self::LinuxArm64),
            "macos-arm64" => Some(Self::MacosArm64),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    Cell,
    Preflight,
    Spawn,
    InputWrite,
    InputShutdown,
    Wait,
    StdoutJoin,
    StderrJoin,
    Terminate,
    Overflow,
    Capture,
    Prefix,
    RemoteDiagnostic,
    ListLock,
    ListRequest,
    Entry,
    Binding,
    Construction,
    PairValidation,
    Truncated,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Begin,
    Ok,
    Error,
    Timeout,
    Truncated,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PrefixFailure {
    MissingPrefix,
    Short,
    Newline,
    Utf8,
    Hex,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum IoKind {
    BrokenPipe,
    ConnectionReset,
    PermissionDenied,
    NotFound,
    Interrupted,
    WouldBlock,
    TimedOut,
    UnexpectedEof,
    InvalidData,
    InvalidInput,
    Other,
}
pub(super) fn io_kind(kind: io::ErrorKind) -> IoKind {
    match kind {
        io::ErrorKind::BrokenPipe => IoKind::BrokenPipe,
        io::ErrorKind::ConnectionReset => IoKind::ConnectionReset,
        io::ErrorKind::PermissionDenied => IoKind::PermissionDenied,
        io::ErrorKind::NotFound => IoKind::NotFound,
        io::ErrorKind::Interrupted => IoKind::Interrupted,
        io::ErrorKind::WouldBlock => IoKind::WouldBlock,
        io::ErrorKind::TimedOut => IoKind::TimedOut,
        io::ErrorKind::UnexpectedEof => IoKind::UnexpectedEof,
        io::ErrorKind::InvalidData => IoKind::InvalidData,
        io::ErrorKind::InvalidInput => IoKind::InvalidInput,
        _ => IoKind::Other,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "code")]
pub(super) enum RemoteDiagnostic {
    None,
    Code(marmish_stream::wire::WireErrorCode),
    Ambiguous,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Evidence {
    Io {
        io: IoKind,
    },
    JoinFailure,
    Written {
        local_pipe_written: usize,
    },
    Capture {
        stdout_bytes: usize,
        stdout_sha256: String,
        stderr_bytes: usize,
        stderr_sha256: String,
        exit: Option<i32>,
    },
    Payload {
        stderr_bytes: usize,
        stderr_sha256: String,
    },
    Prefix {
        failure: PrefixFailure,
    },
    Remote {
        diagnostic: RemoteDiagnostic,
    },
    Binding {
        direction_ok: bool,
        client_ok: bool,
        server_ok: bool,
        active: bool,
        terminal: Option<marmish_stream::wire::Terminal>,
    },
    Construction {
        open_digest_present: Option<bool>,
        exit_representable: Option<bool>,
        wall_ns_representable: Option<bool>,
    },
    Checks {
        checks: DuplexChecks,
    },
}
#[derive(Serialize)]
struct Event<'a> {
    schema: u8,
    case: &'static str,
    client: Role,
    server: Role,
    elapsed_ms: u64,
    stage: Stage,
    outcome: Outcome,
    evidence: Option<&'a Evidence>,
}
pub(super) struct Trace<'a> {
    client: Role,
    server: Role,
    start: Instant,
    count: Cell<u8>,
    truncated: Cell<bool>,
    finished: Cell<bool>,
    sink: &'a dyn Fn(&[u8]) -> io::Result<()>,
}
pub(super) fn stderr_sink(bytes: &[u8]) -> io::Result<()> {
    io::stderr().lock().write_all(bytes)
}
pub(super) fn elapsed_ms(value: u128) -> u64 {
    value.min(u64::MAX as u128) as u64
}
impl<'a> Trace<'a> {
    pub(super) fn new(
        client: &str,
        server: &str,
        sink: &'a dyn Fn(&[u8]) -> io::Result<()>,
    ) -> Option<Self> {
        Some(Self {
            client: Role::parse(client)?,
            server: Role::parse(server)?,
            start: Instant::now(),
            count: Cell::new(0),
            truncated: Cell::new(false),
            finished: Cell::new(false),
            sink,
        })
    }
    fn write(&self, stage: Stage, outcome: Outcome, evidence: Option<&Evidence>) {
        let event = Event {
            schema: 1,
            case: "duplex",
            client: self.client,
            server: self.server,
            elapsed_ms: elapsed_ms(self.start.elapsed().as_millis()),
            stage,
            outcome,
            evidence,
        };
        if let Ok(json) = serde_json::to_vec(&event) {
            let mut line = b"marmish installed stream observation: ".to_vec();
            line.extend(json);
            line.push(b'\n');
            if line.len() <= 1536 {
                let _ = (self.sink)(&line);
            }
        }
    }
    pub(super) fn event(&self, stage: Stage, outcome: Outcome, evidence: Option<Evidence>) {
        if self.finished.get() {
            return;
        }
        if self.count.get() < 46 {
            self.count.set(self.count.get() + 1);
            self.write(stage, outcome, evidence.as_ref());
        } else if !self.truncated.replace(true) {
            self.write(Stage::Truncated, Outcome::Truncated, None);
        }
    }
    pub(super) fn finish(&self, success: bool) {
        if !self.finished.replace(true) {
            self.write(
                Stage::Cell,
                if success { Outcome::Ok } else { Outcome::Error },
                None,
            );
        }
    }
}
pub(super) fn note(
    trace: Option<&Trace<'_>>,
    stage: Stage,
    outcome: Outcome,
    evidence: Option<Evidence>,
) {
    if let Some(trace) = trace {
        trace.event(stage, outcome, evidence);
    }
}
pub(super) fn capture(output: &super::process::Output) -> Evidence {
    Evidence::Capture {
        stdout_bytes: output.stdout.len(),
        stdout_sha256: records::digest(&output.stdout),
        stderr_bytes: output.stderr.len(),
        stderr_sha256: records::digest(&output.stderr),
        exit: output.status.code(),
    }
}

#[cfg(test)]
#[path = "installed_streaming_observation_tests.rs"]
mod tests;
