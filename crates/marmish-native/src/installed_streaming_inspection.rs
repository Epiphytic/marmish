//! Best-effort, redacted local snapshots after original stream-matrix failure.
use super::{
    cleanup,
    matrix::Daemon,
    records::{IdentityReady, ROLES},
    streaming_observation::Role,
};
use crate::{Error, stream_service::*};
use serde::Serialize;
use std::{collections::BTreeMap, future::Future, io, time::Duration};
use tokio::time::Instant;

const LINE_LIMIT: usize = 24 * 1024;
const PREFIX: &[u8] = b"marmish installed stream inspection: ";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Ok,
    Unavailable,
    Timeout,
    Invalid,
    Capacity,
    SkippedNoBudget,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum MatchCount {
    Zero,
    One,
    Multiple,
}
type Fetch = Result<StreamInspection, Outcome>;
#[derive(Serialize)]
struct Progress<'a> {
    integrity: InspectionIntegrity,
    phase: Option<InspectionPhase>,
    server_hello_validated: Option<bool>,
    open_present: Option<bool>,
    active: Option<bool>,
    cancel_requested: Option<bool>,
    projection_invalidated: Option<bool>,
    record_sequence: Option<u64>,
    record_stdin_accepted: Option<u64>,
    record_stdin_delivered: Option<u64>,
    stdout_eof: Option<u64>,
    stderr_eof: Option<u64>,
    terminal: Option<marmish_stream::wire::Terminal>,
    slots: &'a [InspectionSlot],
    slots_truncated: bool,
    outbox: &'a [InspectionOutbox],
}
impl<'a> From<&'a StreamInspectionEntry> for Progress<'a> {
    fn from(e: &'a StreamInspectionEntry) -> Self {
        Self {
            integrity: e.integrity,
            phase: e.phase,
            server_hello_validated: e.server_hello_validated,
            open_present: e.open_present,
            active: e.active,
            cancel_requested: e.cancel_requested,
            projection_invalidated: e.projection_invalidated,
            record_sequence: e.record_sequence,
            record_stdin_accepted: e.record_stdin_accepted,
            record_stdin_delivered: e.record_stdin_delivered,
            stdout_eof: e.stdout_eof,
            stderr_eof: e.stderr_eof,
            terminal: e.terminal,
            slots: &e.slots,
            slots_truncated: e.slots_truncated,
            outbox: &e.outbox,
        }
    }
}
#[derive(Serialize)]
struct Pair<'a> {
    client: Role,
    server: Role,
    match_count: Option<MatchCount>,
    progress: Option<Progress<'a>>,
}
#[derive(Serialize)]
struct Summary<'a> {
    schema: u8,
    role: Role,
    endpoint: InspectionDirection,
    elapsed_ms: u64,
    outcome: Outcome,
    records_truncated: Option<bool>,
    outbox_truncated: Option<bool>,
    pairs: Vec<Pair<'a>>,
}
struct Topology {
    role: Role,
    pairs: [(Role, Role, [u8; 32], [u8; 32]); 3],
}
impl Topology {
    fn new(
        role: &str,
        ids: &BTreeMap<String, IdentityReady>,
        direction: InspectionDirection,
    ) -> Option<Self> {
        let own = ids.get(role)?;
        let mut pairs = Vec::new();
        for other in ROLES {
            let remote = ids.get(other)?;
            let (client_role, server_role, client, server) = match direction {
                InspectionDirection::Client => {
                    (role, other, &own.client_account, &remote.server_account)
                }
                InspectionDirection::Server => {
                    (other, role, &remote.client_account, &own.server_account)
                }
            };
            pairs.push((
                Role::parse(client_role)?,
                Role::parse(server_role)?,
                hex::decode(client).ok()?.try_into().ok()?,
                hex::decode(server).ok()?.try_into().ok()?,
            ));
        }
        Some(Self {
            role: Role::parse(role)?,
            pairs: pairs.try_into().ok()?,
        })
    }
    fn summarize<'a>(
        &self,
        direction: InspectionDirection,
        result: &'a Fetch,
        elapsed_ms: u64,
    ) -> Summary<'a> {
        let (snapshot, outcome) = match result {
            Ok(s) => (Some(s), Outcome::Ok),
            Err(e) => (None, *e),
        };
        let pairs = self
            .pairs
            .iter()
            .map(|(client, server, c, s)| {
                let matches = snapshot.map(|snapshot| {
                    snapshot
                        .entries
                        .iter()
                        .filter(|entry| {
                            entry.direction == Some(direction)
                                && entry
                                    .binding
                                    .as_ref()
                                    .is_some_and(|b| b.client == *c && b.server == *s)
                        })
                        .collect::<Vec<_>>()
                });
                let (match_count, progress) = match matches.as_deref() {
                    None => (None, None),
                    Some([]) => (Some(MatchCount::Zero), None),
                    Some([entry]) => (Some(MatchCount::One), Some(Progress::from(*entry))),
                    Some(_) => (Some(MatchCount::Multiple), None),
                };
                Pair {
                    client: *client,
                    server: *server,
                    match_count,
                    progress,
                }
            })
            .collect();
        Summary {
            schema: 1,
            role: self.role,
            endpoint: direction,
            elapsed_ms,
            outcome,
            records_truncated: snapshot.map(|s| s.records_truncated),
            outbox_truncated: snapshot.map(|s| s.outbox_truncated),
            pairs,
        }
    }
}
fn checked(snapshot: StreamInspection) -> Fetch {
    if snapshot.schema != 1 {
        return Err(Outcome::Invalid);
    }
    if snapshot.entries.len() > 8
        || snapshot
            .entries
            .iter()
            .any(|e| e.slots.len() > 5 || e.outbox.len() > 9)
    {
        return Err(Outcome::Capacity);
    }
    Ok(snapshot)
}
fn write_summary(
    topology: &Topology,
    direction: InspectionDirection,
    result: &Fetch,
    elapsed_ms: u64,
    sink: &dyn Fn(&[u8]) -> io::Result<()>,
) {
    let summary = topology.summarize(direction, result, elapsed_ms);
    let encoded = serde_json::to_vec(&summary);
    let encoded = match encoded {
        Ok(bytes) if PREFIX.len() + bytes.len() < LINE_LIMIT => bytes,
        _ => match serde_json::to_vec(&topology.summarize(
            direction,
            &Err(Outcome::Capacity),
            elapsed_ms,
        )) {
            Ok(bytes) => bytes,
            Err(_) => return,
        },
    };
    let mut line = PREFIX.to_vec();
    line.extend(encoded);
    line.push(b'\n');
    if line.len() <= LINE_LIMIT {
        let _ = sink(&line);
    }
}
fn cutoff(now: Instant, stop_by: Instant) -> Option<Instant> {
    let until = (now + Duration::from_secs(20)).min(stop_by.checked_sub(Duration::from_secs(30))?);
    (until > now).then_some(until)
}
fn classify(result: crate::Result<StreamLocalResult>) -> Fetch {
    match result {
        Ok(StreamLocalResult::Inspection { snapshot }) => checked(*snapshot),
        Ok(StreamLocalResult::Failure { category }) if category == "inspection_capacity" => {
            Err(Outcome::Capacity)
        }
        Err(Error::Timeout) => Err(Outcome::Timeout),
        Err(Error::Io(_)) => Err(Outcome::Unavailable),
        _ => Err(Outcome::Invalid),
    }
}
async fn attempt_child(
    child: &mut tokio::process::Child,
    socket: &std::path::Path,
    until: Instant,
) -> Fetch {
    // The cleanup helper deliberately uses the same generic refusal for a dead
    // owned child and an invalid response. Keep those observationally distinct
    // without treating arbitrary validation failures as daemon unavailability.
    match child.try_wait() {
        Ok(None) if child.id().is_some() => {}
        _ => return Err(Outcome::Unavailable),
    }
    let result =
        cleanup::stream_request(child, socket, StreamLocalRequest::Inspect {}, until).await;
    if matches!(&result, Err(Error::Invalid("installed_cleanup_proof")))
        && matches!(child.try_wait(), Ok(Some(_)))
    {
        return Err(Outcome::Unavailable);
    }
    classify(result)
}
async fn attempt(daemon: Option<&mut Daemon>, until: Instant) -> Fetch {
    let Some(d) = daemon else {
        return Err(Outcome::Unavailable);
    };
    attempt_child(&mut d.child, &d.socket, until).await
}
async fn concurrent<A, B, S>(
    client: A,
    server: B,
    signal: S,
    until: Option<Instant>,
) -> Option<(Fetch, Fetch)>
where
    A: Future<Output = Fetch>,
    B: Future<Output = Fetch>,
    S: Future<Output = ()>,
{
    let bounded = |result: Result<Fetch, tokio::time::error::Elapsed>| {
        result.unwrap_or(Err(Outcome::Timeout))
    };
    tokio::select! {biased;
        _=signal=>None,
        results=async {
            match until {
                Some(until)=> {
                    let results=tokio::join!(tokio::time::timeout_at(until,client),tokio::time::timeout_at(until,server));
                    (bounded(results.0),bounded(results.1))
                },
                None=>(Err(Outcome::SkippedNoBudget),Err(Outcome::SkippedNoBudget)),
            }
        }=>Some(results),
    }
}
pub(super) fn eligible(cohort: &str, result: &crate::Result<()>) -> bool {
    cohort == "stream-matrix"
        && result.is_err()
        && !matches!(result, Err(Error::Invalid("installed_interrupted")))
}
/// Read-only best effort. The caller always preserves its work result and then cleans up.
pub(super) async fn failure(
    client: Option<&mut Daemon>,
    server: Option<&mut Daemon>,
    ids: &BTreeMap<String, IdentityReady>,
    role: &str,
    stop_by: Instant,
    origin: Instant,
    termination: &mut tokio::signal::unix::Signal,
) {
    let (Some(client_topology), Some(server_topology)) = (
        Topology::new(role, ids, InspectionDirection::Client),
        Topology::new(role, ids, InspectionDirection::Server),
    ) else {
        return;
    };
    let now = Instant::now();
    let until = cutoff(now, stop_by);
    let results = concurrent(
        attempt(client, until.unwrap_or(now)),
        attempt(server, until.unwrap_or(now)),
        async {
            termination.recv().await;
        },
        until,
    )
    .await;
    if let Some((client, server)) = results {
        let elapsed = super::streaming_observation::elapsed_ms(origin.elapsed().as_millis());
        let sink = super::streaming_observation::stderr_sink;
        write_summary(
            &client_topology,
            InspectionDirection::Client,
            &client,
            elapsed,
            &sink,
        );
        write_summary(
            &server_topology,
            InspectionDirection::Server,
            &server,
            elapsed,
            &sink,
        );
    }
}
#[cfg(test)]
#[path = "installed_streaming_inspection_tests.rs"]
mod tests;
