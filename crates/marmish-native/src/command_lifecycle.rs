//! Joined engine stop with the existing profile lease held across frontend close.
//! Finite workers survive ordinary stop; live streams receive bounded grace.
use crate::command_engine::CommandEngine;
use crate::identity::ProfileLease;
use crate::mdk_adapter::MdkOwner;
use crate::stream_runner::RunnerHandle;
use serde::Serialize;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::Instant;

pub(crate) struct OwnedIdentity {
    // Rust drops fields in declaration order. Implicit drop remains degraded;
    // normal/error paths transfer both resources to stop_owned instead.
    pub owner: MdkOwner,
    pub profile: ProfileLease,
}

#[derive(Clone, Copy, Serialize)]
pub(crate) struct LifecycleStopReport {
    pub schema: u8,
    pub engine_quiesced: bool,
    pub engine_stop_complete: bool,
    pub mdk_stop_complete: bool,
    #[serde(skip)]
    pub stream_stop_complete: bool,
    #[serde(
        rename = "stream_stop_complete",
        skip_serializing_if = "Option::is_none"
    )]
    pub stream_stop_observation: Option<bool>,
    pub runtime_cleanup_complete: bool,
}
impl Default for LifecycleStopReport {
    fn default() -> Self {
        Self {
            schema: 1,
            engine_quiesced: false,
            engine_stop_complete: false,
            mdk_stop_complete: false,
            stream_stop_complete: false,
            stream_stop_observation: None,
            runtime_cleanup_complete: false,
        }
    }
}
type Progress = Arc<Mutex<LifecycleStopReport>>;
fn update(progress: &Progress, change: impl FnOnce(&mut LifecycleStopReport)) {
    change(&mut progress.lock().unwrap_or_else(|poison| poison.into_inner()));
}
fn snapshot(progress: &Progress) -> LifecycleStopReport {
    *progress.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[must_use = "Retain and finish the keeper before shutting down the Tokio runtime"]
pub(crate) struct PendingStop {
    task: JoinHandle<LifecycleStopReport>,
    progress: Progress,
}
pub(crate) struct StopObservation {
    pub report: LifecycleStopReport,
    pub pending: Option<PendingStop>,
}
impl PendingStop {
    pub(crate) async fn observe(mut self, deadline: Instant) -> StopObservation {
        match tokio::time::timeout_at(deadline, &mut self.task).await {
            Ok(Ok(report)) => StopObservation {
                report,
                pending: None,
            },
            Ok(Err(_)) => {
                // A keeper panic/cancellation is never cleanup evidence. The
                // caller must preserve state even if another flag was observed.
                let mut report = snapshot(&self.progress);
                report.runtime_cleanup_complete = false;
                report.engine_quiesced = false;
                StopObservation {
                    report,
                    pending: None,
                }
            }
            Err(_) => StopObservation {
                report: snapshot(&self.progress),
                pending: Some(self),
            },
        }
    }
    pub(crate) async fn finish(self) -> LifecycleStopReport {
        let Self { task, progress } = self;
        match task.await {
            Ok(report) => report,
            Err(_) => {
                let mut report = snapshot(&progress);
                report.engine_quiesced = false;
                report.runtime_cleanup_complete = false;
                report
            }
        }
    }
}

// The sender is closed only after owner destruction, including cancellation of
// the async network-shutdown future. The keeper cannot unlock the profile first.
struct Frontend {
    owner: Option<MdkOwner>,
    closed: Option<mpsc::Sender<()>>,
}
impl Drop for Frontend {
    fn drop(&mut self) {
        drop(self.owner.take());
        drop(self.closed.take());
    }
}
impl Frontend {
    async fn stop(mut self, begun: bool, progress: Progress) {
        let Some(owner) = self.owner.as_mut() else {
            return;
        };
        let grace = Instant::now() + Duration::from_secs(2);
        while !owner.stream_runners_finished() && Instant::now() < grace {
            let turn = grace.min(Instant::now() + Duration::from_millis(100));
            let _ = tokio::time::timeout_at(turn, owner.stream_stop_progress(turn)).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let cancelled = owner.stream_cancel_stop().is_ok();
        // Journal failure must not prevent physical cleanup; its uncertainty is
        // retained in the report instead of keeping an uncancelled child alive.
        for runner in owner.stream_runner_barriers() {
            runner.cancel();
        }
        loop {
            let _ = owner.progress_streams();
            if owner.stream_runners_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Physical completion alone does not prove the terminal journal commit.
        let confirmed = begun && cancelled && owner.stream_stop_confirmed().unwrap_or(false);
        update(&progress, |report| {
            report.stream_stop_complete = confirmed;
            if report.stream_stop_observation.is_some() {
                report.stream_stop_observation = Some(confirmed);
            }
        });
        let publication = Instant::now() + Duration::from_secs(2);
        let _ = tokio::time::timeout_at(publication, owner.stream_stop_publish(publication)).await;
        let deadline = Instant::now() + Duration::from_secs(2);
        let success = matches!(
            tokio::time::timeout_at(deadline, owner.shutdown(deadline)).await,
            Ok(Ok(()))
        );
        update(&progress, |report| report.mdk_stop_complete = success);
        // Drop runs before the keeper can finish its frontend handoff.
    }
}

fn start_keeper(
    profile: ProfileLease,
    mut engine: Option<CommandEngine>,
    frontend: mpsc::Receiver<()>,
    barriers: Vec<RunnerHandle>,
    progress: Progress,
) -> PendingStop {
    let shared = progress.clone();
    let task = tokio::task::spawn_blocking(move || {
        let mut failed = false;
        if let Some(engine) = engine.as_mut() {
            loop {
                match engine.shutdown(std::time::Instant::now() + Duration::from_secs(1)) {
                    Ok(stopped) => {
                        update(&shared, |report| {
                            report.engine_stop_complete = stopped.complete;
                            report.engine_quiesced = stopped.quiesced && !failed;
                        });
                        if stopped.complete {
                            break;
                        }
                    }
                    Err(_) => {
                        // A panicked engine join has already consumed the OS
                        // handle. A second turn confirms physical termination;
                        // the original error still forbids clean cleanup.
                        failed = true;
                        update(&shared, |report| report.engine_quiesced = false);
                    }
                }
            }
        } else {
            update(&shared, |report| report.engine_stop_complete = true);
        }
        // recv returning Disconnected is the normal ownership handoff: the
        // Frontend guard closed its sender after dropping MDK/journal.
        let _ = frontend.recv();
        // A frontend panic or runtime cancellation cannot release the lease while
        // detached runner threads still own children. Its channel closes only
        // after MDK drops; these independent handles preserve the physical fence.
        for runner in &barriers {
            runner.cancel();
        }
        while barriers.iter().any(|runner| !runner.finished()) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let streams_clean = snapshot(&shared).stream_stop_complete
            && barriers.iter().all(|runner| !runner.snapshot().uncertain);
        drop(engine);
        let runtime_clean = !failed && streams_clean && profile.cleanup_runtime().is_ok();
        update(&shared, |report| {
            report.runtime_cleanup_complete = runtime_clean
        });
        let report = snapshot(&shared);
        drop(profile);
        report
    });
    PendingStop { task, progress }
}

/// Consumes the existing identity. Observer expiry returns the same keeper;
/// callers must retain it and the runtime until actual engine join completes.
pub(crate) async fn stop_owned(identity: OwnedIdentity, deadline: Instant) -> StopObservation {
    let OwnedIdentity { mut owner, profile } = identity;
    // Close admissions and invalidate all preparation results before capturing
    // independent child-ownership barriers. No network wait precedes the keeper.
    let stream_qualified = owner.stream_has_server_state().unwrap_or(true);
    let begun = owner.stream_begin_stop().is_ok();
    let barriers = owner.stream_runner_barriers();
    let engine = owner.take_engine_for_stop();
    let progress = Arc::new(Mutex::new(LifecycleStopReport {
        stream_stop_observation: stream_qualified.then_some(false),
        ..LifecycleStopReport::default()
    }));
    let (closed, frontend) = mpsc::channel();
    let keeper = start_keeper(profile, engine, frontend, barriers, progress.clone());
    let frontend = Frontend {
        owner: Some(owner),
        closed: Some(closed),
    };
    // Detached deliberately: observer expiry returns the keeper immediately,
    // while this guard continues bounded network grace and physical cleanup.
    tokio::spawn(frontend.stop(begun, progress));
    keeper.observe(deadline).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn stream_stop_observation_is_optional_and_never_defaults_to_success() {
        let mut report = LifecycleStopReport::default();
        assert!(
            serde_json::to_value(report)
                .unwrap()
                .get("stream_stop_complete")
                .is_none()
        );
        report.stream_stop_complete = true;
        // Internal readiness cannot manufacture public proof for a finite-only
        // identity; public proof appears only after a qualified stream stop.
        assert!(
            serde_json::to_value(report)
                .unwrap()
                .get("stream_stop_complete")
                .is_none()
        );
        for confirmed in [false, true] {
            report.stream_stop_observation = Some(confirmed);
            assert_eq!(
                serde_json::to_value(report).unwrap()["stream_stop_complete"],
                confirmed
            );
        }
    }
    #[tokio::test]
    async fn expired_observer_retains_lease_until_real_thread_join() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = ProfileLease::acquire(root.path(), "account").unwrap();
        let (release, gate) = mpsc::channel();
        let engine = CommandEngine::gated_stop_for_test(gate);
        let progress = Arc::new(Mutex::new(LifecycleStopReport::default()));
        let (closed, frontend) = mpsc::channel();
        update(&progress, |report| report.stream_stop_complete = true);
        let keeper = start_keeper(profile, Some(engine), frontend, vec![], progress);
        drop(closed); // frontend is already closed; only the OS thread is gated.
        let observed = keeper
            .observe(Instant::now() + Duration::from_millis(20))
            .await;
        assert!(!observed.report.engine_stop_complete);
        assert!(ProfileLease::acquire(root.path(), "account").is_err());
        release.send(()).unwrap();
        let finished = observed.pending.unwrap().finish().await;
        assert!(finished.engine_stop_complete);
        assert!(!finished.engine_quiesced);
        assert!(ProfileLease::acquire(root.path(), "account").is_ok());
    }
    #[tokio::test]
    async fn joined_engine_still_holds_lease_until_frontend_drop() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = ProfileLease::acquire(root.path(), "account").unwrap();
        let progress = Arc::new(Mutex::new(LifecycleStopReport::default()));
        let (closed, frontend) = mpsc::channel();
        update(&progress, |report| report.stream_stop_complete = true);
        let keeper = start_keeper(profile, None, frontend, vec![], progress);
        let observed = keeper
            .observe(Instant::now() + Duration::from_millis(20))
            .await;
        assert!(observed.report.engine_stop_complete);
        assert!(!observed.report.runtime_cleanup_complete);
        assert!(ProfileLease::acquire(root.path(), "account").is_err());
        drop(closed);
        let finished = observed.pending.unwrap().finish().await;
        assert!(finished.engine_stop_complete);
        assert!(!finished.mdk_stop_complete); // no successful network stop claimed.
        assert!(ProfileLease::acquire(root.path(), "account").is_ok());
    }
    #[tokio::test]
    async fn closed_frontend_cannot_release_lease_before_live_runner_exits() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = ProfileLease::acquire(root.path(), "account").unwrap();
        let (_state, prepared) = crate::stream_runner::tests::prepare(
            "trap '' TERM; printf ready; while :; do sleep 1; done",
        );
        let runner = RunnerHandle::start(
            prepared,
            crate::stream_runner::RunnerLimits {
                cancel_grace: Duration::from_millis(250),
                drain_grace: Duration::from_millis(80),
                poll_interval: Duration::from_millis(2),
            },
        )
        .unwrap();
        let ready = Instant::now() + Duration::from_secs(5);
        while runner.snapshot().stdout.is_none() {
            assert!(Instant::now() < ready);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let progress = Arc::new(Mutex::new(LifecycleStopReport::default()));
        let (closed, frontend) = mpsc::channel();
        let keeper = start_keeper(profile, None, frontend, vec![runner.clone()], progress);
        drop(closed); // Models frontend abort/drop; the keeper must cancel safely.
        let observed = keeper
            .observe(Instant::now() + Duration::from_millis(20))
            .await;
        assert!(observed.pending.is_some());
        assert!(!runner.finished());
        assert!(ProfileLease::acquire(root.path(), "account").is_err());
        let report =
            tokio::time::timeout(Duration::from_secs(5), observed.pending.unwrap().finish())
                .await
                .unwrap();
        assert!(runner.finished());
        assert!(!report.stream_stop_complete);
        assert!(!report.runtime_cleanup_complete);
        assert!(ProfileLease::acquire(root.path(), "account").is_ok());
    }
    #[tokio::test]
    async fn unresolved_stream_stop_never_claims_runtime_cleanup() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = ProfileLease::acquire(root.path(), "account").unwrap();
        let progress = Arc::new(Mutex::new(LifecycleStopReport::default()));
        let (closed, frontend) = mpsc::channel();
        let keeper = start_keeper(profile, None, frontend, vec![], progress);
        drop(closed); // Models frontend failure before confirming its stream journal.
        let report = keeper.finish().await;
        assert!(report.engine_stop_complete);
        assert!(!report.stream_stop_complete);
        assert!(!report.runtime_cleanup_complete);
        assert!(ProfileLease::acquire(root.path(), "account").is_ok());
    }
    #[tokio::test]
    async fn actual_unactivated_owner_closes_before_runtime_cleanup_and_unlock() {
        use crate::identity::BackendChoice;
        use crate::transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits};
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = ProfileLease::acquire(root.path(), "account").unwrap();
        let material = profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let connector = RelayConnector::new(
            RelayPolicy::parse(
                &["wss://127.0.0.1:9".into()],
                RouteMode::Off,
                TransportLimits::default(),
            )
            .unwrap(),
            ArtiDirs {
                state: profile.arti_state().into(),
                cache: profile.arti_cache().into(),
            },
        )
        .unwrap();
        let owner = MdkOwner::open(&profile, &material, connector, vec![]).unwrap();
        profile.mark_ready(owner.opened_binding()).unwrap();
        let stopped = stop_owned(OwnedIdentity { owner, profile }, Instant::now()).await;
        let report = match stopped.pending {
            Some(pending) => pending.finish().await,
            None => stopped.report,
        };
        assert!(
            report.engine_stop_complete
                && report.stream_stop_complete
                && report.mdk_stop_complete
                && report.runtime_cleanup_complete
        );
        assert!(!report.engine_quiesced);
        assert!(ProfileLease::acquire(root.path(), "account").is_ok());
    }
}
