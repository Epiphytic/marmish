//! Closed, independently reconciled evidence for isolated installed edge waves.
use super::edge_observation::{Case, Family, Stage, Trace};
use super::{
    copy_interrupt::Binding,
    edge_admission, edge_copy, edge_preopen, edge_quota, edge_rsync,
    edge_signals::{self, Cohort},
    records, streaming_cleanup,
};
use crate::{Error, Result};
use marmish_stream::wire::Terminal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
fn bad() -> Error {
    Error::Invalid("installed_edges")
}
pub(super) fn is_edge(cohort: &str) -> bool {
    matches!(cohort, "edge-stream" | "edge-rsync" | "edge-copy")
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "family", deny_unknown_fields)]
pub(super) enum Evidence {
    Stream {
        admission: Vec<edge_admission::Observation>,
        signals: Vec<edge_signals::Observation>,
        quota: Box<edge_quota::Observation>,
        preopen: edge_preopen::Observation,
    },
    Rsync {
        cases: Vec<edge_rsync::Observation>,
    },
    Copy {
        cases: Vec<edge_copy::Observation>,
    },
}
impl Evidence {
    pub(super) fn validate(&self, cohort: &str) -> Result<()> {
        match (self, cohort) {
            (
                Self::Stream {
                    admission,
                    signals,
                    quota,
                    preopen,
                },
                "edge-stream",
            ) => {
                if admission.len() != 3 || signals.len() != 4 {
                    return Err(bad());
                }
                for (o, case) in admission.iter().zip([
                    "missing-executable",
                    "policy-denied",
                    "rsync-bootstrap-without-seclusion",
                ]) {
                    o.validate()?;
                    if o.case != case {
                        return Err(bad());
                    }
                }
                for (o, case) in signals.iter().zip([
                    "stream-sigint",
                    "stream-signal-closed-output",
                    "stream-epipe-detach",
                    "stream-sigkill-detach",
                ]) {
                    o.validate()?;
                    if o.case != case {
                        return Err(bad());
                    }
                }
                quota.validate()?;
                preopen.validate()?;
            }
            (Self::Rsync { cases }, "edge-rsync") => edge_rsync::validate(cases)?,
            (Self::Copy { cases }, "edge-copy") => {
                let mut keys = BTreeSet::new();
                for o in cases {
                    o.validate()?;
                    if !keys.insert(o.key()) {
                        return Err(bad());
                    }
                }
                if keys != edge_copy::expected_keys().into_iter().collect() {
                    return Err(bad());
                }
            }
            _ => return Err(bad()),
        }
        let rows = self.metrics();
        if rows
            .iter()
            .try_fold(0u64, |n, (_, w)| n.checked_add(*w))
            .ok_or_else(bad)?
            > 1_140_000_000_000
        {
            return Err(bad());
        }
        let mut repeatable_seeds = BTreeSet::new();
        if let Self::Copy { cases } = self {
            let mut current = BTreeSet::new();
            for o in cases {
                if o.remote_terminal().is_some() {
                    let bindings = o.bindings();
                    let first = bindings.first().ok_or_else(bad)?;
                    if !current.insert(first.stream_sha256.clone()) {
                        return Err(bad());
                    }
                    if o.key().ends_with("-receipt-seed") {
                        repeatable_seeds.insert(first.stream_sha256.clone());
                    }
                }
            }
        }
        validate_binding_reuse(self.bindings(), &repeatable_seeds)
    }
    pub(super) fn metrics(&self) -> Vec<(String, u64)> {
        match self {
            Self::Stream {
                admission,
                signals,
                quota,
                preopen,
            } => admission
                .iter()
                .map(|o| (o.case.clone(), o.wall_ns))
                .chain(signals.iter().map(|o| (o.case.clone(), o.wall_ns)))
                .chain([
                    ("stream-peer-quota".into(), quota.wall_ns),
                    ("stream-preopen-interrupt".into(), preopen.wall_ns),
                ])
                .collect(),
            Self::Rsync { cases } => cases.iter().map(|o| (o.case.clone(), o.wall_ns)).collect(),
            Self::Copy { cases } => cases.iter().map(|o| (o.key(), o.wall_ns())).collect(),
        }
    }
    fn bindings(&self) -> Vec<(&Binding, Option<Terminal>)> {
        match self {
            Self::Stream {
                admission,
                signals,
                quota,
                ..
            } => admission
                .iter()
                .filter_map(|o| o.binding.as_ref().map(|b| (b, o.terminal)))
                .chain(
                    signals
                        .iter()
                        .map(|o| (&o.binding, Some(o.remote_terminal))),
                )
                .chain(quota.held.iter().map(|b| (b, Some(Terminal::Cancelled))))
                .collect(),
            Self::Rsync { cases } => cases
                .iter()
                .map(|o| (&o.binding, Some(o.remote_terminal)))
                .collect(),
            // The first binding is the current attempt, except pre-Open refusal,
            // where the only binding is the previously committed seed.
            Self::Copy { cases } => cases
                .iter()
                .flat_map(|o| {
                    o.bindings().into_iter().enumerate().map(move |(index, b)| {
                        let terminal = if index == 0 {
                            o.remote_terminal().unwrap_or(Terminal::Exited { code: 0 })
                        } else {
                            Terminal::Exited { code: 0 }
                        };
                        (b, Some(terminal))
                    })
                })
                .collect(),
        }
    }
}
fn validate_binding_reuse(
    rows: Vec<(&Binding, Option<Terminal>)>,
    repeatable: &BTreeSet<String>,
) -> Result<()> {
    let mut streams = BTreeMap::new();
    for (binding, terminal) in rows {
        let observation = records::canonical(&(binding, terminal))?;
        if let Some(previous) = streams.insert(&binding.stream_sha256, observation.clone())
            && (!repeatable.contains(&binding.stream_sha256) || previous != observation)
        {
            return Err(bad());
        }
    }
    Ok(())
}
fn again<'a, 'b>(c: &Cohort<'a, 'b>) -> Cohort<'a, 'b> {
    Cohort {
        context: c.context,
        identities: c.identities,
        role: c.role,
        ready: c.ready,
        client: c.client,
        server: c.server,
        deadline: c.deadline,
    }
}
pub(super) async fn run(c: Cohort<'_, '_>) -> Result<Evidence> {
    let trace = matches!(
        c.context.cohort.as_str(),
        "edge-stream" | "edge-rsync" | "edge-copy"
    )
    .then(|| Trace::new(c.role, Family::Coordinator, Case::Family));
    let result = run_inner(c, trace.as_ref()).await;
    if let Some(trace) = &trace {
        trace.finish(&result);
    }
    result
}
async fn run_inner(c: Cohort<'_, '_>, trace: Option<&Trace<'_>>) -> Result<Evidence> {
    let evidence = match c.context.cohort.as_str() {
        "edge-stream" => {
            if let Some(trace) = trace {
                trace.stage(Stage::Prerequisite);
            }
            super::edge_fixture::prepare_gates(c.server.workspace)?;
            let family_trace = Trace::new(c.role, Family::Admission, Case::Family);
            let admission = edge_admission::run(edge_admission::Cohort {
                context: c.context,
                identities: c.identities,
                role: c.role,
                ready: c.ready,
                client: c.client,
                server: c.server,
                deadline: c.deadline,
            })
            .await;
            family_trace.finish(&admission);
            let admission = admission?;
            let family_trace = Trace::new(c.role, Family::Signals, Case::Family);
            let signals = edge_signals::run(again(&c)).await;
            family_trace.finish(&signals);
            let signals = signals?;
            let family_trace = Trace::new(c.role, Family::Quota, Case::Family);
            let quota = edge_quota::run(again(&c)).await;
            family_trace.finish(&quota);
            let quota = quota?;
            let family_trace = Trace::new(c.role, Family::Preopen, Case::Family);
            let preopen = edge_preopen::run(again(&c)).await;
            family_trace.finish(&preopen);
            let preopen = preopen?;
            Evidence::Stream {
                admission,
                signals,
                quota: Box::new(quota),
                preopen,
            }
        }
        "edge-rsync" => Evidence::Rsync {
            cases: edge_rsync::run(edge_rsync::Cohort {
                context: c.context,
                identities: c.identities,
                role: c.role,
                ready: c.ready,
                client: c.client,
                server: c.server,
                deadline: c.deadline,
            })
            .await?,
        },
        "edge-copy" => {
            if let Some(trace) = trace {
                trace.stage(Stage::Prerequisite);
            }
            Evidence::Copy {
                cases: edge_copy::run(again(&c)).await?,
            }
        }
        _ => return Err(bad()),
    };
    if let Some(trace) = trace {
        trace.stage(Stage::Validation);
    }
    evidence.validate(&c.context.cohort)?;
    confirm_server(&evidence, &c).await?;
    Ok(evidence)
}
/// Uses private same-UID observations, never public artifact hashes as authority.
pub(super) async fn confirm_server(e: &Evidence, c: &Cohort<'_, '_>) -> Result<()> {
    let trace = matches!(
        c.context.cohort.as_str(),
        "edge-stream" | "edge-rsync" | "edge-copy"
    )
    .then(|| Trace::new(c.role, Family::Coordinator, Case::Family));
    if let Some(trace) = &trace {
        trace.stage(Stage::Reconciliation);
    }
    let result = confirm_server_inner(e, c).await;
    if let Some(trace) = &trace {
        trace.finish(&result);
    }
    result
}
async fn confirm_server_inner(e: &Evidence, c: &Cohort<'_, '_>) -> Result<()> {
    e.validate(&c.context.cohort)?;
    let entries =
        streaming_cleanup::entries(*c.server.daemon.lock().await, c.server.socket, c.deadline)
            .await?;
    let principal = |x: &&streaming_cleanup::Entry| {
        x.direction == "server"
            && x.client == c.identities[c.role].client_account
            && x.server == c.ready.server_account
    };
    for (b, terminal) in e.bindings() {
        let found: Vec<_> = entries
            .iter()
            .filter(principal)
            .filter(|x| records::digest(x.stream.as_bytes()) == b.stream_sha256)
            .collect();
        if found.len() != 1 {
            return Err(bad());
        }
        let x = found[0];
        if records::digest(x.session.as_bytes()) != b.session_sha256
            || x.open_digest.as_deref() != Some(b.open_digest.as_str())
            || x.group_sha256 != b.group_sha256
            || x.active
            || x.terminal.is_none()
            || terminal.is_some_and(|t| x.terminal != Some(t))
        {
            return Err(bad());
        }
    }
    if let Evidence::Stream { preopen, quota, .. } = e {
        for (stream, session, group) in [
            (
                &preopen.stream_sha256,
                &preopen.session_sha256,
                &preopen.group_sha256,
            ),
            (
                &quota.refusal.binding.stream_sha256,
                &quota.refusal.binding.session_sha256,
                &quota.refusal.binding.group_sha256,
            ),
        ] {
            let found: Vec<_> = entries
                .iter()
                .filter(principal)
                .filter(|x| records::digest(x.stream.as_bytes()) == *stream)
                .collect();
            if found.len() != 1
                || found[0].state != "hello"
                || found[0].open_digest.is_some()
                || found[0].cancel_requested
                || found[0].active
                || found[0].terminal.is_some()
                || found[0].child_disposition.is_some()
                || records::digest(found[0].session.as_bytes()) != *session
                || found[0].group_sha256 != *group
            {
                return Err(bad());
            }
        }
    }
    streaming_cleanup::status(*c.server.daemon.lock().await, c.server.socket, c.deadline)
        .await?
        .validate_idle()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rsync() -> Evidence {
        let names = [
            "rsync-sigint",
            "rsync-sigterm",
            "rsync-sighup",
            "rsync-group-sigint",
            "rsync-timeout-usr1",
            "rsync-independent-failure",
        ];
        Evidence::Rsync {
            cases: names
                .into_iter()
                .enumerate()
                .map(|(i, name)| edge_rsync::Observation {
                    case: name.into(),
                    wall_ns: 1_000_000,
                    binding: Binding {
                        stream_sha256: format!("{i:064x}"),
                        session_sha256: "b".repeat(64),
                        open_digest: "c".repeat(64),
                        group_sha256: "d".repeat(64),
                    },
                    marker_sha256: "e".repeat(64),
                    fixture_argv_sha256: "f".repeat(64),
                    local_rsync_sha256: "1".repeat(64),
                    remote_rsync_sha256: "2".repeat(64),
                    local_exit: if i == 4 { 30 } else { 23 },
                    stderr_sha256: "3".repeat(64),
                    cancellation_confirmed: i != 5,
                    remote_terminal: if i == 5 {
                        Terminal::Exited { code: 23 }
                    } else {
                        Terminal::Cancelled
                    },
                    cleanup: streaming_cleanup::StreamStatus {
                        active: 0,
                        cancelling: 0,
                        uncertain: 0,
                        owned_runners: 0,
                        unfinished_runners: 0,
                        observation_pending: false,
                        cleanup_ready: true,
                        stop_complete: true,
                    },
                })
                .collect(),
        }
    }
    #[test]
    fn integrated_evidence_requires_complete_unique_original_family() {
        let mut e = rsync();
        e.validate("edge-rsync").unwrap();
        assert!(e.validate("edge-stream").is_err());
        let Evidence::Rsync { cases } = &mut e else {
            unreachable!()
        };
        cases.pop();
        assert!(e.validate("edge-rsync").is_err());
        let mut e = rsync();
        let Evidence::Rsync { cases } = &mut e else {
            unreachable!()
        };
        cases[1].binding.stream_sha256 = cases[0].binding.stream_sha256.clone();
        assert!(e.validate("edge-rsync").is_err());
    }
    #[test]
    fn only_known_committed_seed_bindings_may_repeat_exactly() {
        let binding = Binding {
            stream_sha256: "a".repeat(64),
            session_sha256: "b".repeat(64),
            open_digest: "c".repeat(64),
            group_sha256: "d".repeat(64),
        };
        let terminal = Some(Terminal::Exited { code: 0 });
        assert!(
            validate_binding_reuse(
                vec![(&binding, terminal), (&binding, terminal)],
                &BTreeSet::new()
            )
            .is_err()
        );
        let seeds = BTreeSet::from([binding.stream_sha256.clone()]);
        validate_binding_reuse(vec![(&binding, terminal), (&binding, terminal)], &seeds).unwrap();
        assert!(
            validate_binding_reuse(
                vec![
                    (&binding, terminal),
                    (&binding, Some(Terminal::Exited { code: 1 }))
                ],
                &seeds
            )
            .is_err()
        );
        let mut forged = binding.clone();
        forged.open_digest = "e".repeat(64);
        assert!(
            validate_binding_reuse(vec![(&binding, terminal), (&forged, terminal)], &seeds)
                .is_err()
        );
    }
    #[test]
    fn serialized_family_is_closed_and_cannot_hide_extra_evidence() {
        let e = rsync();
        let bytes = records::canonical(&e).unwrap();
        let round: Evidence = serde_json::from_slice(&bytes).unwrap();
        round.validate("edge-rsync").unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["unobserved"] = true.into();
        assert!(serde_json::from_value::<Evidence>(value).is_err());
    }
}
