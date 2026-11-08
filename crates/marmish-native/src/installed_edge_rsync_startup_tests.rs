//! Pure journal-decision coverage; these tests do not claim live IPC evidence.
use super::*;
use crate::stream_service::ChildDisposition;

fn running() -> Entry {
    Entry {
        stream: "a".repeat(32),
        session: "b".repeat(32),
        client: "client".into(),
        server: "server".into(),
        direction: "server".into(),
        state: "running".into(),
        cancel_requested: false,
        terminal: None,
        child_disposition: None,
        open_digest: Some("c".repeat(64)),
        group_sha256: "d".repeat(64),
        active: true,
    }
}
fn classify(e: Option<&Entry>, case: Case) -> Result<Startup> {
    startup(e, "client", "server", case)
}

#[test]
fn startup_and_marker_eligibility_cover_transitional_records_in_both_modes() {
    for case in [Case::Interrupt, Case::Failure] {
        assert_eq!(classify(None, case).unwrap(), Startup::Wait);
        for state in ["hello", "preparing", "launching", "running"] {
            let mut e = running();
            e.state = state.into();
            if state == "hello" {
                e.active = false;
                e.open_digest = None;
            }
            let phase = classify(Some(&e), case).unwrap();
            let expected = if state == "running" {
                Startup::Eligible
            } else {
                Startup::Wait
            };
            assert_eq!(phase, expected, "{state}");
            for marker_exists in [false, true] {
                assert_eq!(
                    phase.marker_eligible() && marker_exists,
                    state == "running" && marker_exists
                );
            }
        }
    }
}

#[test]
fn natural_failure_accepts_both_cancel_flags_and_waits_through_cancelling() {
    let mut e = running();
    e.state = "cancelling".into();
    e.cancel_requested = true;
    let phase = classify(Some(&e), Case::Failure).unwrap();
    assert_eq!(phase, Startup::Wait);
    for marker_exists in [false, true] {
        assert!(!(phase.marker_eligible() && marker_exists));
    }
    assert!(classify(Some(&e), Case::Interrupt).is_err());
    e.state = "terminal".into();
    e.active = false;
    e.terminal = Some(Terminal::Exited { code: 23 });
    for cancel in [false, true] {
        e.cancel_requested = cancel;
        let phase = classify(Some(&e), Case::Failure).unwrap();
        assert_eq!(phase, Startup::Eligible);
        for marker_exists in [false, true] {
            assert_eq!(phase.marker_eligible() && marker_exists, marker_exists);
        }
        assert!(classify(Some(&e), Case::Interrupt).is_err());
    }
    e.terminal = Some(Terminal::Cancelled);
    assert!(classify(Some(&e), Case::Failure).is_err());
}

#[test]
fn wrong_binding_open_terminal_and_state_are_never_startup_proof() {
    for case in [Case::Interrupt, Case::Failure] {
        for mutation in 0..15 {
            let mut e = running();
            match mutation {
                0 => e.client = "other".into(),
                1 => e.server = "other".into(),
                2 => e.direction = "client".into(),
                3 => e.open_digest = None,
                4 => e.open_digest = Some("X".repeat(64)),
                5 => e.open_digest = Some("a".repeat(63)),
                6 => e.active = false,
                7 => e.cancel_requested = true,
                8 => e.terminal = Some(Terminal::Exited { code: 23 }),
                9 => e.state = "lost".into(),
                10 => e.state = "unknown_launch".into(),
                11 => e.state = "server_stopping".into(),
                12 => e.state = "authority_revoked".into(),
                13 => {
                    e.state = "hello".into();
                    e.active = false;
                }
                _ => {
                    e.state = "cancelling".into();
                }
            }
            assert!(classify(Some(&e), case).is_err(), "mutation {mutation}");
        }
        for disposition in [
            ChildDisposition::Absent,
            ChildDisposition::PresentUnowned,
            ChildDisposition::Unknown,
        ] {
            for state in ["running", "terminal", "preparing", "hello", "cancelling"] {
                let mut e = running();
                e.state = state.into();
                e.child_disposition = Some(disposition);
                if state == "terminal" {
                    e.active = false;
                    e.terminal = Some(Terminal::Exited { code: 23 });
                }
                if state == "hello" {
                    e.active = false;
                    e.open_digest = None;
                }
                if state == "cancelling" {
                    e.cancel_requested = true;
                }
                assert!(classify(Some(&e), case).is_err());
            }
        }
    }
    let mut e = running();
    e.state = "terminal".into();
    e.terminal = Some(Terminal::Exited { code: 23 });
    assert!(classify(Some(&e), Case::Failure).is_err()); // active terminal
    e.active = false;
    e.terminal = Some(Terminal::Exited { code: 0 });
    assert!(classify(Some(&e), Case::Failure).is_err());
    e.terminal = Some(Terminal::Exited { code: 23 });
    for state in [
        "lost",
        "server_stopped",
        "running",
        "preparing",
        "launching",
    ] {
        e.state = state.into();
        assert!(classify(Some(&e), Case::Failure).is_err(), "{state}");
    }
}

#[test]
fn entry_decoder_rejects_wrong_types_and_unknown_fields() {
    let encoded = serde_json::to_value(running()).unwrap();
    for (field, value) in [
        ("active", serde_json::json!(1)),
        ("cancel_requested", serde_json::json!("false")),
        ("open_digest", serde_json::json!(false)),
        ("terminal", serde_json::json!(23)),
        ("child_disposition", serde_json::json!("invented")),
        ("private_path", serde_json::json!("unexpected")),
    ] {
        let mut invalid = encoded.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<Entry>(invalid).is_err());
    }
}

#[test]
fn source_contract_keeps_eligible_only_marker_and_unchanged_reconciliation_gates() {
    // Architectural wiring guard paired with the behavioral table above, not
    // runtime IPC or process evidence.
    let source = include_str!("installed_edge_rsync.rs");
    let started = source
        .split("async fn started(")
        .nth(1)
        .unwrap()
        .split("async fn terminal(")
        .next()
        .unwrap();
    assert!(started.contains("startup(observed.as_ref(), client, &c.ready.server_account, case)?"));
    assert!(started.contains("phase.marker_eligible() && marker.try_exists()?"));
    assert!(started.contains("if !matches!(case, Case::Failure)"));
    assert!(started.contains("streaming_cleanup::validate_charged(&status, 1)?"));
    assert!(started.contains("!records::hexadecimal(&m.argv_sha256, 64)"));
    assert!(source.contains("terminal(c, &initial, expected).await?"));
    assert!(source.contains("trace.stage(Stage::Cleanup);\n        child.abort().await?;"));
    for case in [
        "RsyncSigint",
        "RsyncSigterm",
        "RsyncSighup",
        "RsyncGroupSigint",
        "RsyncTimeoutUsr1",
        "RsyncIndependentFailure",
    ] {
        assert!(source.contains(&format!("TraceCase::{case}")));
    }
}
