//! Finite diagnostics for the feature-gated installed qualification driver.
use super::streaming_observation::Role;
use crate::Error;
use serde::Serialize;
use std::io;

const PREFIX: &[u8] = b"marmish installed diagnostic: ";
const LINE_LIMIT: usize = 1024;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    Setup,
    DaemonStartup,
    DaemonReadiness,
    OwnReadyPublish,
    ServerRecordRead,
    ServerRecordValidate,
    ServerRendezvousWait,
    CommonBudget,
    EdgeExecute,
    InterruptionExecute,
    CompletedResultValidation,
    ClientResultPublish,
    RemoteClientBarrier,
    ServerConfirmation,
    LocalCells,
    CellsIncomplete,
    ServerReceipts,
    PostWorkFallback,
    PostWorkCompletion,
    ServerResultPublish,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Origin {
    SetupError,
    WorkError,
    WorkDeadline,
    ExternalSignal,
    PostWorkError,
}
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Category {
    DaemonStartup,
    LocalIo,
    Invalid,
    Identity,
    Storage,
    Transport,
    Marmot,
    Copy,
    Stream,
    Timeout,
}
#[derive(Serialize)]
struct Diagnostic {
    schema: u8,
    component: &'static str,
    event: &'static str,
    stage: Stage,
    origin: Origin,
    category: Category,
    role: Option<Role>,
    peer_role: Option<Role>,
}
fn classify(error: &Error) -> Category {
    match error {
        Error::DaemonStartup(_) => Category::DaemonStartup,
        Error::Io(_) => Category::LocalIo,
        Error::Invalid(_) => Category::Invalid,
        Error::Identity(_) => Category::Identity,
        Error::Storage(_) => Category::Storage,
        Error::Transport(_) => Category::Transport,
        Error::Mdk(_) => Category::Marmot,
        Error::Copy(_) => Category::Copy,
        Error::Stream(_) => Category::Stream,
        Error::Timeout => Category::Timeout,
    }
}
pub(super) struct Tracker {
    stage: Stage,
    origin: Origin,
    role: Option<Role>,
    peer_role: Option<Role>,
    emitted: bool,
}
impl Tracker {
    pub(super) fn new() -> Self {
        Self {
            stage: Stage::Setup,
            origin: Origin::SetupError,
            role: None,
            peer_role: None,
            emitted: false,
        }
    }
    pub(super) fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        self.peer_role = None;
    }
    pub(super) fn peer(&mut self, stage: Stage, role: &str) {
        self.stage = stage;
        self.peer_role = Role::parse(role);
    }
    pub(super) fn role(&mut self, role: &str) {
        self.role = Role::parse(role);
    }
    pub(super) fn origin(&mut self, origin: Origin) {
        self.origin = origin;
    }
    pub(super) fn emit(&mut self, error: &Error, sink: &dyn Fn(&[u8]) -> io::Result<()>) {
        if self.emitted {
            return;
        }
        self.emitted = true;
        let diagnostic = Diagnostic {
            schema: 1,
            component: "driver",
            event: "failed",
            stage: self.stage,
            origin: self.origin,
            category: classify(error),
            role: self.role,
            peer_role: self.peer_role,
        };
        let Ok(encoded) = serde_json::to_vec(&diagnostic) else {
            return;
        };
        if PREFIX.len() + encoded.len() + 1 > LINE_LIMIT {
            return;
        }
        let mut line = PREFIX.to_vec();
        line.extend(encoded);
        line.push(b'\n');
        let _ = sink(&line);
    }
}
#[cfg(test)]
#[path = "installed_streaming_diagnostic_tests.rs"]
mod tests;
