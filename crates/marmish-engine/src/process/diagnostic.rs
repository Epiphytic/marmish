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
    ZombieGroup,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

// This closed classification is part of Darwin proof control flow in BOTH
// profiles. Diagnostic stage/errno fields below remain debug-only.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
enum ObservationClass {
    Hard,
    BsdReadEsrch,
    ExitingNonleader,
}
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProofAttempt {
    One,
    Two,
}

#[cfg(all(debug_assertions, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemberKind {
    Leader,
    Other,
}
#[cfg(all(debug_assertions, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ZombieDetail {
    pub(crate) member: MemberKind,
    pub(crate) group_zero: bool,
    pub(crate) status: u32,
    pub(crate) in_exit: bool,
}

pub(crate) struct Failure {
    pub(crate) error: EngineError,
    #[cfg(target_os = "macos")]
    observation: ObservationClass,
    #[cfg(all(debug_assertions, target_os = "macos"))]
    bsd_member: Option<MemberKind>,
    #[cfg(all(debug_assertions, target_os = "macos"))]
    proof_attempt: Option<ProofAttempt>,
    #[cfg(debug_assertions)]
    stage: Stage,
    #[cfg(all(debug_assertions, target_os = "macos"))]
    context: Context,
    #[cfg(all(debug_assertions, target_os = "macos"))]
    zombie_detail: Option<ZombieDetail>,
    #[cfg(debug_assertions)]
    errno: Option<i32>,
}
pub(crate) type Staged<T> = std::result::Result<T, Failure>;
impl Failure {
    pub(crate) fn new(error: EngineError, _stage: Stage, _errno: Option<i32>) -> Self {
        Self {
            error,
            #[cfg(target_os = "macos")]
            observation: ObservationClass::Hard,
            #[cfg(all(debug_assertions, target_os = "macos"))]
            bsd_member: None,
            #[cfg(all(debug_assertions, target_os = "macos"))]
            proof_attempt: None,
            #[cfg(debug_assertions)]
            stage: _stage,
            #[cfg(all(debug_assertions, target_os = "macos"))]
            context: Context::Probe,
            #[cfg(all(debug_assertions, target_os = "macos"))]
            zombie_detail: None,
            #[cfg(debug_assertions)]
            errno: _errno,
        }
    }
    #[cfg(all(debug_assertions, target_os = "macos"))]
    pub(crate) fn with_zombie_detail(self, detail: ZombieDetail) -> Self {
        Self {
            zombie_detail: Some(detail),
            ..self
        }
    }
    #[cfg(all(test, debug_assertions, target_os = "macos"))]
    pub(crate) fn diagnostic_detail(&self) -> (&'static str, Option<ZombieDetail>) {
        (self.stage.label(), self.zombie_detail)
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
        Self {
            observation: if matches!(stage, DarwinStage::BsdRead) && errno == Some(libc::ESRCH) {
                ObservationClass::BsdReadEsrch
            } else {
                ObservationClass::Hard
            },
            ..Self::new(error, Stage::Darwin(stage), errno)
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn exiting_nonleader(self) -> Self {
        Self {
            observation: ObservationClass::ExitingNonleader,
            ..self
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn is_exiting_nonleader(&self) -> bool {
        matches!(self.observation, ObservationClass::ExitingNonleader)
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn is_bsd_read_esrch(&self) -> bool {
        matches!(self.observation, ObservationClass::BsdReadEsrch)
    }
    #[cfg(all(debug_assertions, target_os = "macos"))]
    pub(crate) fn with_bsd_member(self, leader: bool) -> Self {
        let bsd_member = if matches!(self.stage, Stage::Darwin(DarwinStage::BsdRead)) {
            Some(if leader {
                MemberKind::Leader
            } else {
                MemberKind::Other
            })
        } else {
            None
        };
        Self { bsd_member, ..self }
    }
    #[cfg(all(debug_assertions, target_os = "macos"))]
    pub(crate) fn with_proof_attempt(self, attempt: ProofAttempt) -> Self {
        Self {
            proof_attempt: Some(attempt),
            ..self
        }
    }
    #[cfg(all(test, debug_assertions, target_os = "macos"))]
    pub(crate) fn proof_detail(&self) -> (Context, Option<MemberKind>, Option<ProofAttempt>) {
        (self.context, self.bsd_member, self.proof_attempt)
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
    #[cfg(target_os = "macos")]
    bsd_member: Option<MemberKind>,
    #[cfg(target_os = "macos")]
    proof_attempt: Option<ProofAttempt>,
    stage: Stage,
    #[cfg(all(debug_assertions, target_os = "macos"))]
    context: Context,
    #[cfg(all(debug_assertions, target_os = "macos"))]
    zombie_detail: Option<ZombieDetail>,
    errno: Option<i32>,
}
#[cfg(debug_assertions)]
fn record<T>(result: &Staged<T>, origin: Origin) -> Option<Record> {
    result.as_ref().err().map(|failure| Record {
        origin,
        #[cfg(target_os = "macos")]
        bsd_member: failure.bsd_member,
        #[cfg(target_os = "macos")]
        proof_attempt: failure.proof_attempt,
        stage: failure.stage,
        #[cfg(target_os = "macos")]
        context: failure.context,
        #[cfg(target_os = "macos")]
        zombie_detail: failure.zombie_detail,
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
            Self::ZombieGroup => "darwin-zombie-group",
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
        #[cfg(target_os = "macos")]
        let detail = self.zombie_detail.map_or_else(String::new, |detail| {
            let member = match detail.member {
                MemberKind::Leader => "leader",
                MemberKind::Other => "other",
            };
            format!(
                " member={member} group_zero={} status={} in_exit={}",
                detail.group_zero, detail.status, detail.in_exit
            )
        });
        #[cfg(not(target_os = "macos"))]
        let detail = "";
        #[cfg(target_os = "macos")]
        let member = match self.bsd_member {
            Some(MemberKind::Leader) => " member=leader",
            Some(MemberKind::Other) => " member=other",
            None => "",
        };
        #[cfg(target_os = "macos")]
        let attempt = match self.proof_attempt {
            Some(ProofAttempt::One) => " proof_attempt=1",
            Some(ProofAttempt::Two) => " proof_attempt=2",
            None => "",
        };
        #[cfg(not(target_os = "macos"))]
        let (member, attempt) = ("", "");
        format!(
            "marmish-engine-signal-failure origin={origin} stage={context}{} errno={errno}{detail}{member}{attempt}\n",
            self.stage.label()
        )
    }
}
pub(super) fn emit<T>(_result: &Staged<T>, _origin: Origin) {
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
            let text = record::<()>(&result, Origin::Term).unwrap().format();
            assert_eq!(
                text,
                format!(
                    "marmish-engine-signal-failure origin=term stage={expected}darwin-invalid-pid errno=none\n"
                )
            );
            assert!(matches!(
                result.map_err(|failure| failure.error),
                Err(EngineError::WorkerUnavailable)
            ));
        }
    }

    #[test]
    fn only_failed_outcomes_produce_complete_closed_records() {
        assert!(record::<()>(&Ok(()), Origin::Term).is_none());
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
                let text = record::<()>(&result, origin).unwrap().format();
                let number = errno.map_or_else(|| "none".into(), |value| value.to_string());
                assert_eq!(
                    text,
                    format!(
                        "marmish-engine-signal-failure origin={expected} stage=kill errno={number}\n"
                    )
                );
                assert!(text.len() < 512);
                assert_eq!(text.lines().count(), 1);
            }
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn member_read_records_discriminate_final_attempt_without_identifiers() {
        for (leader, label) in [(true, "leader"), (false, "other")] {
            for (attempt, number) in [(ProofAttempt::One, 1), (ProofAttempt::Two, 2)] {
                let failure = Failure::darwin(
                    EngineError::WorkerUnavailable,
                    DarwinStage::BsdRead,
                    Some(libc::ESRCH),
                )
                .with_bsd_member(leader)
                .context(Context::ProofFirstMembers)
                .with_proof_attempt(attempt);
                let text = record::<()>(&Err(failure), Origin::Kill).unwrap().format();
                assert_eq!(
                    text,
                    format!(
                        "marmish-engine-signal-failure origin=kill stage=proof-first-members/darwin-bsd-read errno={} member={label} proof_attempt={number}\n",
                        libc::ESRCH
                    )
                );
                assert!(text.len() < 512);
                assert_eq!(text.lines().count(), 1);
            }
        }
        let failure = Failure::darwin(
            EngineError::WorkerUnavailable,
            DarwinStage::BsdMetadata,
            None,
        )
        .with_bsd_member(false);
        let text = record::<()>(&Err(failure), Origin::Kill).unwrap().format();
        assert!(!text.contains("member="));
        assert!(!text.contains("proof_attempt="));
    }
    #[cfg(all(debug_assertions, target_os = "macos"))]
    #[test]
    fn zombie_records_preserve_context_errors_and_closed_detail() {
        for (member, label) in [(MemberKind::Leader, "leader"), (MemberKind::Other, "other")] {
            for group_zero in [false, true] {
                for status in [0, libc::SZOMB, u32::MAX] {
                    for in_exit in [false, true] {
                        let detail = ZombieDetail {
                            member,
                            group_zero,
                            status,
                            in_exit,
                        };
                        for (stage, stage_label) in [
                            (DarwinStage::ZombieGroup, "darwin-zombie-group"),
                            (DarwinStage::ZombieStatus, "darwin-zombie-status"),
                        ] {
                            let failure =
                                Failure::darwin(EngineError::RecoveryUncertain, stage, None)
                                    .with_zombie_detail(detail)
                                    .context(Context::ProofFirstMembers);
                            assert_eq!(failure.diagnostic_detail(), (stage_label, Some(detail)));
                            let result = Err(failure);
                            let text = record::<()>(&result, Origin::Kill).unwrap().format();
                            assert_eq!(
                                text,
                                format!(
                                    "marmish-engine-signal-failure origin=kill stage=proof-first-members/{stage_label} errno=none member={label} group_zero={group_zero} status={status} in_exit={in_exit}\n"
                                )
                            );
                            assert!(text.len() < 512);
                            assert_eq!(text.lines().count(), 1);
                            assert!(matches!(
                                result.map_err(|failure| failure.error),
                                Err(EngineError::RecoveryUncertain)
                            ));
                        }
                    }
                }
            }
        }
    }
}
