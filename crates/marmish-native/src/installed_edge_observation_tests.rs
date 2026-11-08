use super::*;
use std::cell::RefCell;
fn decoded(lines: &RefCell<Vec<Vec<u8>>>) -> Vec<serde_json::Value> {
    lines
        .borrow()
        .iter()
        .map(|line| {
            assert!(line.len() <= LINE_LIMIT);
            serde_json::from_slice(&line[PREFIX.len()..]).unwrap()
        })
        .collect()
}
// Standalone Trace contracts, not runtime copy-wrapper or cleanup parity proof.
#[test]
fn last_stage_error_finish_is_once_only_and_closed() {
    for (role, family, case, stage, stage_name, error, category) in [
        (
            "macos-arm64",
            Family::Signals,
            Case::StreamEpipeDetach,
            Stage::DetachedDwell,
            "detached_dwell",
            Error::Invalid("private path must not appear"),
            "invalid",
        ),
        (
            "linux-amd64",
            Family::CopyPut,
            Case::CopyEmpty,
            Stage::Prerequisite,
            "prerequisite",
            Error::Io(io::Error::other("private fixture path")),
            "io",
        ),
        (
            "linux-arm64",
            Family::CopyGet,
            Case::CopyEmpty,
            Stage::Attachment,
            "attachment",
            Error::Timeout,
            "timeout",
        ),
        (
            "macos-arm64",
            Family::CopyPut,
            Case::CopyOverwrite,
            Stage::Validation,
            "validation",
            Error::Invalid("private postcheck path"),
            "invalid",
        ),
    ] {
        let lines = RefCell::new(Vec::new());
        let sink = |line: &[u8]| {
            lines.borrow_mut().push(line.to_vec());
            Ok(())
        };
        {
            let trace = Trace::with_sink(role, family, case, &sink);
            trace.stage(stage);
            trace.finish::<()>(&Err(error));
            trace.finish(&Ok(()));
            trace.stage(Stage::Cleanup);
        }
        let events = decoded(&lines);
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[2],
            serde_json::json!({"schema":1,"client_role":role,
            "family":family,"case":case,"stage":stage_name,
            "outcome":"error","error_category":category})
        );
        assert!(
            !String::from_utf8(lines.borrow().concat())
                .unwrap()
                .contains("private")
        );
        if stage_name == "prerequisite" {
            assert!(!events.iter().any(|event| event["stage"] == "attachment"));
        }
    }
}
#[test]
fn abandonment_and_truncation_reserve_final_line() {
    for (family, case) in [
        (Family::Quota, Case::TwoChildrenQuota),
        (Family::CopyPut, Case::CopyEmpty),
        (Family::CopyGet, Case::CopyResumeCorruptCheckpoint),
    ] {
        let lines = RefCell::new(Vec::new());
        let sink = |line: &[u8]| {
            lines.borrow_mut().push(line.to_vec());
            Ok(())
        };
        {
            let trace = Trace::with_sink("linux-amd64", family, case, &sink);
            for _ in 0..100 {
                trace.stage(Stage::Startup);
            }
            trace.stage(Stage::Cleanup);
        }
        let events = decoded(&lines);
        assert_eq!(events.len(), LIMIT + 2); // ordinary begin/stages, truncation, final
        assert_eq!(
            events
                .iter()
                .filter(|v| v["outcome"] == "truncated")
                .count(),
            1
        );
        assert_eq!(events.last().unwrap()["outcome"], "abandoned");
        assert_eq!(events.last().unwrap()["stage"], "cleanup");
    }
}
#[test]
fn failing_sink_preserves_original_result_and_cleanup_precedence() {
    for (family, case) in [
        (Family::Signals, Case::StreamSigint),
        (Family::CopyPut, Case::CopyEmpty),
        (Family::CopyGet, Case::CopyEmpty),
    ] {
        let sink = |_: &[u8]| Err(io::Error::other("sink private error"));
        let run = |operation: Result<()>, cleanup: Result<()>| {
            let trace = Trace::with_sink("linux-arm64", family, case, &sink);
            trace.stage(Stage::AttachmentFinish);
            let result = if operation.is_err() {
                cleanup.and(operation)
            } else {
                operation
            };
            trace.finish(&result);
            result
        };
        assert!(run(Ok(()), Ok(())).is_ok());
        assert!(matches!(
            run(Err(Error::Timeout), Ok(())),
            Err(Error::Timeout)
        ));
        assert!(matches!(
            run(Err(Error::Timeout), Err(Error::Invalid("cleanup"))),
            Err(Error::Invalid("cleanup"))
        ));
    }
}
#[test]
fn earlier_success_is_retained_and_later_case_is_not_entered() {
    let lines = RefCell::new(Vec::new());
    let sink = |line: &[u8]| {
        lines.borrow_mut().push(line.to_vec());
        Ok(())
    };
    let run = || -> Result<()> {
        let first = Trace::with_sink(
            "linux-amd64",
            Family::Admission,
            Case::MissingExecutable,
            &sink,
        );
        first.finish(&Ok(()));
        let second = Trace::with_sink("linux-amd64", Family::Admission, Case::PolicyDenied, &sink);
        let result: Result<()> = Err(Error::Timeout);
        second.finish(&result);
        result?;
        let later = Trace::with_sink(
            "linux-amd64",
            Family::Preopen,
            Case::InterruptBeforeOpen,
            &sink,
        );
        later.finish(&Ok(()));
        Ok(())
    };
    assert!(matches!(run(), Err(Error::Timeout)));
    let events = decoded(&lines);
    assert_eq!(events.len(), 4);
    assert_eq!(events[1]["outcome"], "ok");
    assert_eq!(events[3]["outcome"], "error");
    assert!(!events.iter().any(|v| v["family"] == "preopen"));
}
#[test]
fn every_closed_enum_combination_fits_line_limit() {
    let roles = ["linux-amd64", "linux-arm64", "macos-arm64"];
    let families = [
        Family::Coordinator,
        Family::Admission,
        Family::Signals,
        Family::Quota,
        Family::Preopen,
        Family::Rsync,
        Family::CopyPut,
        Family::CopyGet,
    ];
    let cases = [
        Case::Family,
        Case::MissingExecutable,
        Case::PolicyDenied,
        Case::RsyncBootstrapWithoutSeclusion,
        Case::StreamSigint,
        Case::StreamSignalClosedOutput,
        Case::StreamEpipeDetach,
        Case::StreamSigkillDetach,
        Case::TwoChildrenQuota,
        Case::InterruptBeforeOpen,
        Case::RsyncSigint,
        Case::RsyncSigterm,
        Case::RsyncSighup,
        Case::RsyncGroupSigint,
        Case::RsyncTimeoutUsr1,
        Case::RsyncIndependentFailure,
        Case::CopyEmpty,
        Case::CopyExistingRefusal,
        Case::CopyOverwrite,
        Case::CopyReceiptSeed,
        Case::CopySymlinkSource,
        Case::CopyDirectorySource,
        Case::CopyDestinationSymlink,
        Case::CopyDestinationDirectory,
        Case::CopyResumeChangedSource,
        Case::CopyResumeChangedBinding,
        Case::CopyResumeCorruptCheckpoint,
    ];
    let stages = [
        Stage::Begin,
        Stage::Prerequisite,
        Stage::Baseline,
        Stage::Attachment,
        Stage::Startup,
        Stage::LocalAction,
        Stage::AttachmentFinish,
        Stage::DetachedDwell,
        Stage::Cancel,
        Stage::JournalCheck,
        Stage::Prefix,
        Stage::DiagnosticCheck,
        Stage::Validation,
        Stage::Idle,
        Stage::Cleanup,
        Stage::Reconciliation,
    ];
    for role in roles {
        for family in families {
            for case in cases {
                for stage in stages {
                    let calls = Cell::new(0);
                    let sink = |line: &[u8]| {
                        assert!(line.len() <= LINE_LIMIT);
                        calls.set(calls.get() + 1);
                        Ok(())
                    };
                    let trace = Trace::with_sink(role, family, case, &sink);
                    trace.stage(stage);
                    trace.finish::<()>(&Err(Error::Timeout));
                    assert_eq!(calls.get(), 3);
                }
            }
        }
    }
}

#[test]
fn rsync_cases_have_distinct_closed_names_and_bounded_startup() {
    for (case, name) in [
        (Case::RsyncSigint, "rsync_sigint"),
        (Case::RsyncSigterm, "rsync_sigterm"),
        (Case::RsyncSighup, "rsync_sighup"),
        (Case::RsyncGroupSigint, "rsync_group_sigint"),
        (Case::RsyncTimeoutUsr1, "rsync_timeout_usr1"),
        (Case::RsyncIndependentFailure, "rsync_independent_failure"),
    ] {
        let lines = RefCell::new(Vec::new());
        let sink = |line: &[u8]| {
            lines.borrow_mut().push(line.to_vec());
            Ok(())
        };
        let trace = Trace::with_sink("macos-arm64", Family::Rsync, case, &sink);
        for _ in 0..100 {
            trace.stage(Stage::Startup);
        }
        trace.finish::<()>(&Err(Error::Timeout));
        let events = decoded(&lines);
        assert_eq!(events.len(), LIMIT + 2);
        assert!(
            events
                .iter()
                .all(|event| event["family"] == "rsync" && event["case"] == name)
        );
        assert_eq!(events.last().unwrap()["stage"], "startup");
        assert_eq!(events.last().unwrap()["error_category"], "timeout");
    }
}
#[test]
fn rsync_cleanup_records_operation_stage_before_final_error_precedence() {
    for cleanup_fails in [false, true] {
        let lines = RefCell::new(Vec::new());
        let sink = |line: &[u8]| {
            lines.borrow_mut().push(line.to_vec());
            Ok(())
        };
        let trace = Trace::with_sink("linux-arm64", Family::Rsync, Case::RsyncTimeoutUsr1, &sink);
        trace.stage(Stage::Startup);
        let operation: Result<()> = Err(Error::Timeout);
        trace.stage(Stage::Cleanup);
        let cleanup = if cleanup_fails {
            Err(Error::Invalid("private cleanup detail"))
        } else {
            Ok(())
        };
        let result = cleanup.and(operation);
        trace.finish(&result);
        drop(trace);
        let events = decoded(&lines);
        assert_eq!(events.len(), 4);
        assert_eq!(events[1]["stage"], "startup");
        assert_eq!(events[1]["outcome"], "begin");
        assert_eq!(events[2]["stage"], "cleanup");
        assert_eq!(events[2]["outcome"], "begin");
        assert_eq!(events[3]["stage"], "cleanup");
        assert_eq!(events[3]["outcome"], "error");
        assert_eq!(
            events[3]["error_category"],
            if cleanup_fails { "invalid" } else { "timeout" }
        );
    }
}
#[test]
fn rsync_setup_failure_finishes_without_child_or_cleanup() {
    let lines = RefCell::new(Vec::new());
    let sink = |line: &[u8]| {
        lines.borrow_mut().push(line.to_vec());
        Ok(())
    };
    {
        let trace = Trace::with_sink(
            "linux-amd64",
            Family::Rsync,
            Case::RsyncIndependentFailure,
            &sink,
        );
        trace.stage(Stage::Prerequisite);
        trace.finish::<()>(&Err(Error::Invalid("private setup detail")));
    }
    let events = decoded(&lines);
    assert_eq!(events.len(), 3);
    assert_eq!(events[2]["stage"], "prerequisite");
    assert_eq!(events[2]["outcome"], "error");
    assert_eq!(events[2]["error_category"], "invalid");
    assert!(
        !events
            .iter()
            .any(|event| event["stage"] == "cleanup" || event["outcome"] == "abandoned")
    );
}

#[test]
fn rsync_startup_and_journal_failures_keep_distinct_decisive_stages() {
    for (stage, stage_name, error, category) in [
        (
            Stage::Startup,
            "startup",
            Error::Invalid("private startup"),
            "invalid",
        ),
        (Stage::Startup, "startup", Error::Timeout, "timeout"),
        (
            Stage::JournalCheck,
            "journal_check",
            Error::Invalid("private journal"),
            "invalid",
        ),
    ] {
        let lines = RefCell::new(Vec::new());
        let sink = |line: &[u8]| {
            lines.borrow_mut().push(line.to_vec());
            Ok(())
        };
        let trace = Trace::with_sink("macos-arm64", Family::Rsync, Case::RsyncSigint, &sink);
        trace.stage(stage);
        trace.finish::<()>(&Err(error));
        let events = decoded(&lines);
        assert_eq!(events.len(), 3);
        assert_eq!(events[1]["stage"], stage_name);
        assert_eq!(events[2]["stage"], stage_name);
        assert_eq!(events[2]["outcome"], "error");
        assert_eq!(events[2]["error_category"], category);
    }
}

#[test]
fn copy_trace_has_exactly_twenty_two_distinct_closed_direction_case_labels() {
    let cases = [
        (Case::CopyEmpty, "copy_empty"),
        (Case::CopyExistingRefusal, "copy_existing_refusal"),
        (Case::CopyOverwrite, "copy_overwrite"),
        (Case::CopyReceiptSeed, "copy_receipt_seed"),
        (Case::CopySymlinkSource, "copy_symlink_source"),
        (Case::CopyDirectorySource, "copy_directory_source"),
        (Case::CopyDestinationSymlink, "copy_destination_symlink"),
        (Case::CopyDestinationDirectory, "copy_destination_directory"),
        (Case::CopyResumeChangedSource, "copy_resume_changed_source"),
        (
            Case::CopyResumeChangedBinding,
            "copy_resume_changed_binding",
        ),
        (
            Case::CopyResumeCorruptCheckpoint,
            "copy_resume_corrupt_checkpoint",
        ),
    ];
    let mut labels = std::collections::BTreeSet::new();
    for (family, name) in [(Family::CopyPut, "copy_put"), (Family::CopyGet, "copy_get")] {
        for (case, case_name) in cases {
            let lines = RefCell::new(Vec::new());
            let sink = |line: &[u8]| {
                lines.borrow_mut().push(line.to_vec());
                Ok(())
            };
            {
                let trace = Trace::with_sink("macos-arm64", family, case, &sink);
                trace.stage(Stage::Validation);
                trace.finish(&Ok(()));
                trace.finish::<()>(&Err(Error::Timeout));
            }
            let events = decoded(&lines);
            assert_eq!(events.len(), 3);
            assert_eq!(events[2]["family"], name);
            assert_eq!(events[2]["case"], case_name);
            assert_eq!(events[2]["stage"], "validation");
            assert_eq!(events[2]["outcome"], "ok");
            assert!(labels.insert((name, case_name)));
        }
    }
    assert_eq!(labels.len(), 22);
}

#[test]
fn copy_coordinator_source_wiring_covers_run_and_both_reconciliation_callers() {
    // Architectural wiring guard, not runtime IPC or wrapper parity evidence.
    let source = include_str!("installed_edges.rs");
    assert_eq!(
        source
            .matches("\"edge-stream\" | \"edge-rsync\" | \"edge-copy\"")
            .count(),
        3
    );
    let arm = source.split("\"edge-copy\" => {").nth(1).unwrap();
    let prerequisite = arm.find("trace.stage(Stage::Prerequisite)").unwrap();
    let run = arm.find("edge_copy::run(again(&c)).await?").unwrap();
    assert!(prerequisite < run);
    assert!(source.contains("confirm_server(&evidence, &c).await?"));
    let reconciliation = source
        .split("pub(super) async fn confirm_server")
        .nth(1)
        .unwrap();
    assert!(
        reconciliation
            .find("trace.stage(Stage::Reconciliation)")
            .unwrap()
            < reconciliation
                .find("confirm_server_inner(e, c).await")
                .unwrap()
    );
}
