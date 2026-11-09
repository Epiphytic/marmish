//! Private signal failure detail. Shared process probes never emit diagnostics.
use crate::model::EngineError;

#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Reaped,
    ChildIdentity,
    #[cfg(target_os = "linux")]
    WorkerLookup,
    WorkerIdentity,
    GroupIdentity,
    LeaderIdentity,
    Kill,
    #[cfg(target_os = "macos")]
    Darwin(#[cfg_attr(not(debug_assertions), allow(dead_code))] DarwinStage),
}
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub(crate) enum DarwinStage {
    InvalidPid,
    BsdRead,
    BsdMetadata,
    StartTime,
    BootId,
    ChildGroup,
    ChildParent,
    ZombieStatus,
    HeldWait,
    HeldStatus,
    MembersRead,
    MembersCount,
    MembersIds,
    ProofGuard,
    ProofIdentity,
    ProofLeaderMissing,
    ProofMembersChanged,
    ProofExitChanged,
    ProofLeaderChanged,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
pub(crate) enum Context {
    #[cfg(debug_assertions)]
    Probe,
    Worker,
    Leader,
    ProofHeld,
    ProofOwner,
    ProofLeader,
    ProofFirstMembers,
    ProofSecondMembers,
    ProofFinalHeld,
    ProofFinalLeader,
}

pub(crate) struct Failure {
    pub(crate) error: EngineError,
    #[cfg(debug_assertions)]
    stage: Stage,
    #[cfg(all(debug_assertions, target_os = "macos"))]
    context: Context,
    #[cfg(debug_assertions)]
    errno: Option<i32>,
}
pub(crate) type Staged<T> = std::result::Result<T, Failure>;
impl Failure {
    pub(crate) fn new(error: EngineError, _stage: Stage, _errno: Option<i32>) -> Self {
        Self {
            error,
            #[cfg(debug_assertions)]
            stage: _stage,
            #[cfg(all(debug_assertions, target_os = "macos"))]
            context: Context::Probe,
            #[cfg(debug_assertions)]
            errno: _errno,
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn context(self, _context: Context) -> Self {
        #[cfg(debug_assertions)]
        {
            Self {
                context: _context,
                ..self
            }
        }
        #[cfg(not(debug_assertions))]
        {
            self
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn darwin(error: EngineError, stage: DarwinStage, errno: Option<i32>) -> Self {
        Self::new(error, Stage::Darwin(stage), errno)
    }
}
#[derive(Clone, Copy)]
pub(super) enum Origin {
    Term,
    Kill,
    DropKill,
    Other,
}

#[cfg(debug_assertions)]
struct Record {
    origin: Origin,
    stage: Stage,
    #[cfg(all(debug_assertions, target_os = "macos"))]
    context: Context,
    errno: Option<i32>,
}
#[cfg(debug_assertions)]
fn record(result: &Staged<()>, origin: Origin) -> Option<Record> {
    result.as_ref().err().map(|failure| Record {
        origin,
        stage: failure.stage,
        #[cfg(target_os = "macos")]
        context: failure.context,
        errno: failure.errno,
    })
}
#[cfg(debug_assertions)]
impl Stage {
    fn label(self) -> &'static str {
        match self {
            Self::Reaped => "reaped",
            Self::ChildIdentity => "child-identity",
            #[cfg(target_os = "linux")]
            Self::WorkerLookup => "worker-lookup",
            Self::WorkerIdentity => "worker-identity",
            Self::GroupIdentity => "group-identity",
            Self::LeaderIdentity => "leader-identity",
            Self::Kill => "kill",
            #[cfg(target_os = "macos")]
            Self::Darwin(stage) => stage.label(),
        }
    }
}
#[cfg(all(debug_assertions, target_os = "macos"))]
impl DarwinStage {
    fn label(self) -> &'static str {
        match self {
            Self::InvalidPid => "darwin-invalid-pid",
            Self::BsdRead => "darwin-bsd-read",
            Self::BsdMetadata => "darwin-bsd-metadata",
            Self::StartTime => "darwin-start-time",
            Self::BootId => "darwin-boot-id",
            Self::ChildGroup => "darwin-child-group",
            Self::ChildParent => "darwin-child-parent",
            Self::ZombieStatus => "darwin-zombie-status",
            Self::HeldWait => "darwin-held-wait",
            Self::HeldStatus => "darwin-held-status",
            Self::MembersRead => "darwin-members-read",
            Self::MembersCount => "darwin-members-count",
            Self::MembersIds => "darwin-members-ids",
            Self::ProofGuard => "darwin-proof-guard",
            Self::ProofIdentity => "darwin-proof-identity",
            Self::ProofLeaderMissing => "darwin-proof-leader-missing",
            Self::ProofMembersChanged => "darwin-proof-members-changed",
            Self::ProofExitChanged => "darwin-proof-exit-changed",
            Self::ProofLeaderChanged => "darwin-proof-leader-changed",
        }
    }
}
#[cfg(debug_assertions)]
impl Record {
    fn format(&self) -> String {
        let origin = match self.origin {
            Origin::Term => "term",
            Origin::Kill => "kill",
            Origin::DropKill => "drop-kill",
            Origin::Other => "other",
        };
        let errno = self
            .errno
            .map_or_else(|| "none".into(), |value| value.to_string());
        #[cfg(target_os = "macos")]
        let context = match self.context {
            Context::Probe => "",
            Context::Worker => "worker/",
            Context::Leader => "leader/",
            Context::ProofHeld => "proof-held/",
            Context::ProofOwner => "proof-owner/",
            Context::ProofLeader => "proof-leader/",
            Context::ProofFirstMembers => "proof-first-members/",
            Context::ProofSecondMembers => "proof-second-members/",
            Context::ProofFinalHeld => "proof-final-held/",
            Context::ProofFinalLeader => "proof-final-leader/",
        };
        #[cfg(not(target_os = "macos"))]
        let context = "";
        format!(
            "marmish-engine-signal-failure origin={origin} stage={context}{} errno={errno}\n",
            self.stage.label()
        )
    }
}
pub(super) fn emit(_result: &Staged<()>, _origin: Origin) {
    #[cfg(debug_assertions)]
    if let Some(record) = record(_result, _origin) {
        use std::io::Write;
        // One complete bounded record; bypass libtest capture without panicking
        // on stderr failure. No diagnostic is emitted by shared probe helpers.
        let _ = std::io::stderr()
            .lock()
            .write_all(record.format().as_bytes());
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    fn silent_staged_probes_preserve_errors_and_context() {
        // No global sink or thread-local state: formatting is a pure decision
        // over returned detail; the shared probe itself has no output path.
        assert!(matches!(
            crate::platform::process_identity(0),
            Err(EngineError::WorkerUnavailable)
        ));
        assert!(matches!(
            crate::platform::owned_child_identity_and_group(0),
            Err(EngineError::WorkerUnavailable)
        ));
        for (context, expected) in [
            (Context::Worker, "worker/"),
            (Context::Leader, "leader/"),
            (Context::ProofFirstMembers, "proof-first-members/"),
            (Context::ProofSecondMembers, "proof-second-members/"),
        ] {
            let failure = match crate::platform::process_identity_staged(0) {
                Ok(_) => panic!("invalid probe unexpectedly succeeded"),
                Err(failure) => failure.context(context),
            };
            let result = Err(failure);
            let text = record(&result, Origin::Term).unwrap().format();
            assert_eq!(text, format!("marmish-engine-signal-failure origin=term stage={expected}darwin-invalid-pid errno=none\n"));
            assert!(matches!(
                result.map_err(|failure| failure.error),
                Err(EngineError::WorkerUnavailable)
            ));
        }
    }

    #[test]
    fn only_failed_outcomes_produce_complete_closed_records() {
        assert!(record(&Ok(()), Origin::Term).is_none());
        for (origin, expected) in [
            (Origin::Term, "term"),
            (Origin::Kill, "kill"),
            (Origin::DropKill, "drop-kill"),
            (Origin::Other, "other"),
        ] {
            for errno in [None, Some(libc::EPERM)] {
                let result = Err(Failure::new(
                    EngineError::RecoveryUncertain,
                    Stage::Kill,
                    errno,
                ));
                let text = record(&result, origin).unwrap().format();
                let number = errno.map_or_else(|| "none".into(), |value| value.to_string());
                assert_eq!(text, format!("marmish-engine-signal-failure origin={expected} stage=kill errno={number}\n"));
                assert!(text.len() < 512);
                assert_eq!(text.lines().count(), 1);
            }
        }
    }
}
