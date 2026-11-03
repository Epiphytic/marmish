//! Joined engine stop with the existing profile lease held across frontend close.
//! Ordinary stop never cancels workers or authorizes engine-state deletion.
use crate::command_engine::CommandEngine;
use crate::identity::ProfileLease;
use crate::mdk_adapter::MdkOwner;
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
    pub runtime_cleanup_complete: bool,
}
impl Default for LifecycleStopReport {
    fn default() -> Self {
        Self {
            schema: 1,
            engine_quiesced: false,
            engine_stop_complete: false,
            mdk_stop_complete: false,
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
    async fn stop(mut self, deadline: Instant, progress: Progress) {
        let success = match self.owner.as_mut() {
            Some(owner) => matches!(
                tokio::time::timeout_at(deadline, owner.shutdown(deadline)).await,
                Ok(Ok(()))
            ),
            None => true,
        };
        update(&progress, |report| report.mdk_stop_complete = success);
        // Drop runs before the keeper can finish its frontend handoff.
    }
}

fn start_keeper(
    profile: ProfileLease,
    mut engine: Option<CommandEngine>,
    frontend: mpsc::Receiver<()>,
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
        drop(engine);
        let runtime_clean = !failed && profile.cleanup_runtime().is_ok();
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
    let engine = owner.take_engine_for_stop();
    let progress = Arc::new(Mutex::new(LifecycleStopReport::default()));
    let (closed, frontend) = mpsc::channel();
    let keeper = start_keeper(profile, engine, frontend, progress.clone());
    let frontend = Frontend {
        owner: Some(owner),
        closed: Some(closed),
    };
    let (_, mut observation) = tokio::join!(
        frontend.stop(deadline, progress.clone()),
        keeper.observe(deadline)
    );
    // Concurrent network shutdown may have completed after the keeper observer
    // expired; return the latest actual flags with the same pending handle.
    if observation.pending.is_some() {
        observation.report = snapshot(&progress);
    }
    observation
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn expired_observer_retains_lease_until_real_thread_join() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let profile = ProfileLease::acquire(root.path(), "account").unwrap();
        let (release, gate) = mpsc::channel();
        let engine = CommandEngine::gated_stop_for_test(gate);
        let progress = Arc::new(Mutex::new(LifecycleStopReport::default()));
        let (closed, frontend) = mpsc::channel();
        let keeper = start_keeper(profile, Some(engine), frontend, progress);
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
        let keeper = start_keeper(profile, None, frontend, progress);
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
        let stopped = stop_owned(
            OwnedIdentity { owner, profile },
            Instant::now() + Duration::from_secs(3),
        )
        .await;
        let report = match stopped.pending {
            Some(pending) => pending.finish().await,
            None => stopped.report,
        };
        assert!(
            report.engine_stop_complete
                && report.mdk_stop_complete
                && report.runtime_cleanup_complete
        );
        assert!(!report.engine_quiesced);
        assert!(ProfileLease::acquire(root.path(), "account").is_ok());
    }
}
