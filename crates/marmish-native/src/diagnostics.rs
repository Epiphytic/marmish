//! Opt-in finite diagnostics. Never accepts provider text, keys or payloads.
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
    let _ = writeln!(
        std::io::stderr().lock(),
        "{}",
        serde_json::json!({"schema":1,"event":"context","platform":std::env::consts::OS,"architecture":std::env::consts::ARCH,"version":env!("CARGO_PKG_VERSION")})
    );
}

pub fn event(stage: &'static str, category: &'static str) {
    if ENABLED.load(Ordering::Relaxed) {
        let _ = writeln!(
            std::io::stderr().lock(),
            "{}",
            serde_json::json!({"schema":1,"event":"diagnostic","stage":stage,"category":category})
        );
    }
}

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(crate) fn manual_approval_timing(elapsed: std::time::Duration) {
    if ENABLED.load(Ordering::Relaxed) {
        let _ = writeln!(
            std::io::stderr().lock(),
            "{}",
            serde_json::json!({
                "schema": 1,
                "event": "manual_approval_timing",
                "decision_ns": pending_refresh::nanos(elapsed),
            })
        );
    }
}

pub fn stage(current: &mut &'static str, value: &'static str) {
    *current = value;
    event(value, "begin");
}

#[cfg(marmish_peer_receiver_candidate)]
pub(crate) mod pending_refresh {
    use super::*;
    use serde::Serialize;
    use std::time::Duration;

    #[derive(Debug, Eq, PartialEq, Serialize)]
    pub(crate) struct Clocks {
        pub(crate) monotonic_ns: i64,
        pub(crate) continuous_ns: i64,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    pub(crate) enum Outcome {
        Published,
        TicketUnavailable,
        NoPending,
        ProofRefused,
        SelectionFailed,
    }

    #[derive(Serialize)]
    pub(crate) struct Timing {
        pub(crate) schedule_ns: i64,
        pub(crate) start_lag_ns: i64,
        pub(crate) missed_ticks: u64,
        pub(crate) outcome: Outcome,
        pub(crate) proof_age: Option<Clocks>,
        pub(crate) publish_gap: Option<Clocks>,
        pub(crate) prior_slack: Option<Clocks>,
    }

    pub(crate) fn nanos(duration: Duration) -> i64 {
        i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX)
    }

    pub(crate) fn enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    fn json(timing: Timing) -> serde_json::Result<String> {
        #[derive(Serialize)]
        struct Event {
            schema: u8,
            event: &'static str,
            #[serde(flatten)]
            timing: Timing,
        }

        serde_json::to_string(&Event {
            schema: 1,
            event: "pending_refresh",
            timing,
        })
    }

    pub(crate) fn emit(timing: Timing) {
        if let Ok(line) = json(timing) {
            let _ = writeln!(std::io::stderr().lock(), "{line}");
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn pending_refresh_is_one_fixed_safe_json_record() {
            let line = json(Timing {
                schedule_ns: i64::MAX,
                start_lag_ns: 123,
                missed_ticks: u64::MAX,
                outcome: Outcome::ProofRefused,
                proof_age: None,
                publish_gap: None,
                prior_slack: None,
            })
            .unwrap();
            assert!(!line.contains('\n'));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&line).unwrap(),
                serde_json::json!({
                    "schema": 1, "event": "pending_refresh",
                    "schedule_ns": i64::MAX, "start_lag_ns": 123, "missed_ticks": u64::MAX,
                    "outcome": "proof_refused", "proof_age": null,
                    "publish_gap": null, "prior_slack": null
                })
            );
            let clocks = || Clocks {
                monotonic_ns: i64::MAX,
                continuous_ns: i64::MIN,
            };
            let longest = json(Timing {
                schedule_ns: i64::MAX,
                start_lag_ns: i64::MAX,
                missed_ticks: u64::MAX,
                outcome: Outcome::Published,
                proof_age: Some(clocks()),
                publish_gap: Some(clocks()),
                prior_slack: Some(clocks()),
            })
            .unwrap();
            assert!(longest.len() < 512);
        }
    }
}
