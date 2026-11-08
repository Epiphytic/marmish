use super::*;
use std::cell::RefCell;

#[test]
fn finite_diagnostic_is_emitted_once() {
    let lines = RefCell::new(Vec::new());
    let sink = |line: &[u8]| {
        lines.borrow_mut().push(line.to_vec());
        Ok(())
    };
    let mut tracker = Tracker::new();
    tracker.emit(&Error::Timeout, &sink);
    tracker.emit(&Error::Invalid("cleanup"), &sink);
    assert_eq!(lines.borrow().len(), 1, "missing one finite diagnostic");
}

fn checked_line(line: &[u8]) -> serde_json::Value {
    assert!(line.len() <= LINE_LIMIT);
    assert!(line.starts_with(PREFIX));
    assert_eq!(line.last(), Some(&b'\n'));
    let value: serde_json::Value =
        serde_json::from_slice(&line[PREFIX.len()..line.len() - 1]).unwrap();
    let object = value.as_object().unwrap();
    let keys: std::collections::BTreeSet<_> = object.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "schema",
            "component",
            "event",
            "stage",
            "origin",
            "category",
            "role",
            "peer_role"
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(value["schema"], 1);
    assert_eq!(value["component"], "driver");
    assert_eq!(value["event"], "failed");
    assert!(
        [
            "setup",
            "daemon_startup",
            "daemon_readiness",
            "own_ready_publish",
            "server_record_read",
            "server_record_validate",
            "server_rendezvous_wait",
            "common_budget",
            "edge_execute",
            "interruption_execute",
            "completed_result_validation",
            "client_result_publish",
            "remote_client_barrier",
            "server_confirmation",
            "local_cells",
            "cells_incomplete",
            "server_receipts",
            "post_work_fallback",
            "post_work_completion",
            "server_result_publish"
        ]
        .contains(&value["stage"].as_str().unwrap())
    );
    assert!(
        [
            "setup_error",
            "work_error",
            "work_deadline",
            "external_signal",
            "post_work_error"
        ]
        .contains(&value["origin"].as_str().unwrap())
    );
    assert!(
        [
            "daemon_startup",
            "local_io",
            "invalid",
            "identity",
            "storage",
            "transport",
            "marmot",
            "copy",
            "stream",
            "timeout"
        ]
        .contains(&value["category"].as_str().unwrap())
    );
    for key in ["role", "peer_role"] {
        assert!(
            value[key].is_null()
                || ["linux-amd64", "linux-arm64", "macos-arm64"]
                    .contains(&value[key].as_str().unwrap())
        );
    }
    value
}

#[test]
fn closed_schema_filters_every_error_payload_and_bounds_large_messages() {
    const HOSTILE: &str = "SECRET_TOKEN_https://secret.test/body/argv/private/path/npub ::error::";
    let errors = [
        (
            Error::DaemonStartup(HOSTILE.repeat(10000)),
            "daemon_startup",
        ),
        (Error::Io(io::Error::other(HOSTILE)), "local_io"),
        (Error::Invalid(HOSTILE), "invalid"),
        (Error::Identity(HOSTILE), "identity"),
        (Error::Storage(HOSTILE), "storage"),
        (Error::Transport(HOSTILE), "transport"),
        (Error::Mdk(HOSTILE), "marmot"),
        (
            Error::Copy(marmish_stream::copy_protocol::CopyError::Protocol),
            "copy",
        ),
        (
            Error::Stream(marmish_stream::wire::WireErrorCode::Invalid),
            "stream",
        ),
        (Error::Timeout, "timeout"),
    ];
    for (error, category) in errors {
        let mut tracker = Tracker::new();
        tracker.role(HOSTILE);
        tracker.peer(Stage::ServerRecordRead, HOSTILE);
        let bytes = RefCell::new(Vec::new());
        tracker.emit(&error, &|line| {
            bytes.borrow_mut().extend_from_slice(line);
            Ok(())
        });
        let bytes = bytes.into_inner();
        let value = checked_line(&bytes);
        assert_eq!(value["category"], category);
        assert!(value["role"].is_null());
        assert!(value["peer_role"].is_null());
        assert!(!String::from_utf8(bytes).unwrap().contains("SECRET_TOKEN"));
    }
}

#[test]
fn stage_origin_and_peer_reset_remain_closed() {
    let stages = [
        Stage::Setup,
        Stage::DaemonStartup,
        Stage::DaemonReadiness,
        Stage::OwnReadyPublish,
        Stage::ServerRecordRead,
        Stage::ServerRecordValidate,
        Stage::ServerRendezvousWait,
        Stage::CommonBudget,
        Stage::EdgeExecute,
        Stage::InterruptionExecute,
        Stage::CompletedResultValidation,
        Stage::ClientResultPublish,
        Stage::RemoteClientBarrier,
        Stage::ServerConfirmation,
        Stage::LocalCells,
        Stage::CellsIncomplete,
        Stage::ServerReceipts,
        Stage::PostWorkFallback,
        Stage::PostWorkCompletion,
        Stage::ServerResultPublish,
    ];
    let origins = [
        Origin::SetupError,
        Origin::WorkError,
        Origin::WorkDeadline,
        Origin::ExternalSignal,
        Origin::PostWorkError,
    ];
    for stage in stages {
        for origin in origins {
            let mut tracker = Tracker::new();
            tracker.role("linux-amd64");
            tracker.peer(Stage::ServerRecordValidate, "macos-arm64");
            tracker.enter(stage);
            tracker.origin(origin);
            let bytes = RefCell::new(Vec::new());
            tracker.emit(&Error::Timeout, &|line| {
                bytes.borrow_mut().extend_from_slice(line);
                Ok(())
            });
            let value = checked_line(&bytes.into_inner());
            assert_eq!(value["role"], "linux-amd64");
            assert!(value["peer_role"].is_null());
        }
    }
}

#[test]
fn failing_sink_is_best_effort_and_never_retries_or_replaces_first_failure() {
    let calls = std::cell::Cell::new(0);
    let mut tracker = Tracker::new();
    tracker.enter(Stage::CompletedResultValidation);
    let sink = |line: &[u8]| {
        calls.set(calls.get() + 1);
        assert_eq!(checked_line(line)["stage"], "completed_result_validation");
        Err(io::Error::other("SECRET_SINK"))
    };
    tracker.emit(&Error::Invalid("SECRET_WORK"), &sink);
    tracker.enter(Stage::PostWorkCompletion);
    tracker.emit(&Error::Io(io::Error::other("SECRET_CLEANUP")), &sink);
    assert_eq!(calls.get(), 1);
}
