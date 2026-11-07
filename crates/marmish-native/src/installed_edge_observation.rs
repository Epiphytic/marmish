//! Fixed, bounded breadcrumbs for build-only edge-stream qualification.
use super::streaming_observation::{Role, stderr_sink};
use crate::{Error, Result};
use serde::Serialize;
use std::{cell::Cell, io};
const PREFIX: &[u8] = b"marmish installed edge observation: ";
const LIMIT: usize = 24;
const LINE_LIMIT: usize = 512;
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Family {
    Coordinator,
    Admission,
    Signals,
    Quota,
    Preopen,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Case {
    Family,
    MissingExecutable,
    PolicyDenied,
    RsyncBootstrapWithoutSeclusion,
    StreamSigint,
    StreamSignalClosedOutput,
    StreamEpipeDetach,
    StreamSigkillDetach,
    TwoChildrenQuota,
    InterruptBeforeOpen,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    Begin,
    Prerequisite,
    Baseline,
    Attachment,
    Startup,
    LocalAction,
    AttachmentFinish,
    DetachedDwell,
    Cancel,
    JournalCheck,
    Prefix,
    DiagnosticCheck,
    Validation,
    Idle,
    Cleanup,
    Reconciliation,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Begin,
    Ok,
    Error,
    Abandoned,
    Truncated,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Category {
    Timeout,
    Io,
    Invalid,
    Other,
}
fn category(error: &Error) -> Category {
    match error {
        Error::Timeout => Category::Timeout,
        Error::Io(_) => Category::Io,
        Error::Invalid(_) => Category::Invalid,
        _ => Category::Other,
    }
}
#[derive(Serialize)]
struct Event {
    schema: u8,
    client_role: Role,
    family: Family,
    case: Case,
    stage: Stage,
    outcome: Outcome,
    error_category: Option<Category>,
}
pub(super) struct Trace<'a> {
    role: Option<Role>,
    family: Family,
    case: Case,
    stage: Cell<Stage>,
    count: Cell<usize>,
    truncated: Cell<bool>,
    finished: Cell<bool>,
    sink: &'a dyn Fn(&[u8]) -> io::Result<()>,
}
impl Trace<'static> {
    pub(super) fn new(role: &str, family: Family, case: Case) -> Self {
        Self::with_sink(role, family, case, &stderr_sink)
    }
}
impl<'a> Trace<'a> {
    fn with_sink(
        role: &str,
        family: Family,
        case: Case,
        sink: &'a dyn Fn(&[u8]) -> io::Result<()>,
    ) -> Self {
        let trace = Self {
            role: Role::parse(role),
            family,
            case,
            stage: Cell::new(Stage::Begin),
            count: Cell::new(1),
            truncated: Cell::new(false),
            finished: Cell::new(false),
            sink,
        };
        trace.emit(Outcome::Begin, None);
        trace
    }
    fn emit(&self, outcome: Outcome, error_category: Option<Category>) {
        let Some(client_role) = self.role else {
            return;
        };
        let event = Event {
            schema: 1,
            client_role,
            family: self.family,
            case: self.case,
            stage: self.stage.get(),
            outcome,
            error_category,
        };
        if let Ok(json) = serde_json::to_vec(&event) {
            let mut line = Vec::with_capacity(PREFIX.len() + json.len() + 1);
            line.extend_from_slice(PREFIX);
            line.extend_from_slice(&json);
            line.push(b'\n');
            if line.len() <= LINE_LIMIT {
                let _ = (self.sink)(&line);
            }
        }
    }
    pub(super) fn stage(&self, stage: Stage) {
        if self.finished.get() {
            return;
        }
        self.stage.set(stage);
        if self.count.get() < LIMIT {
            self.count.set(self.count.get() + 1);
            self.emit(Outcome::Begin, None);
        } else if !self.truncated.replace(true) {
            self.emit(Outcome::Truncated, None);
        }
    }
    pub(super) fn finish<T>(&self, result: &Result<T>) {
        if self.finished.replace(true) {
            return;
        }
        match result {
            Ok(_) => self.emit(Outcome::Ok, None),
            Err(error) => self.emit(Outcome::Error, Some(category(error))),
        }
    }
}
impl Drop for Trace<'_> {
    fn drop(&mut self) {
        if !self.finished.replace(true) {
            self.emit(Outcome::Abandoned, None);
        }
    }
}
#[cfg(test)]
#[path = "installed_edge_observation_tests.rs"]
mod tests;
