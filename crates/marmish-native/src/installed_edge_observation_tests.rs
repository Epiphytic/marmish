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
#[test]
fn last_stage_error_finish_is_once_only_and_closed() {
    let lines = RefCell::new(Vec::new());
    let sink = |line: &[u8]| {
        lines.borrow_mut().push(line.to_vec());
        Ok(())
    };
    {
        let trace = Trace::with_sink(
            "macos-arm64",
            Family::Signals,
            Case::StreamEpipeDetach,
            &sink,
        );
        trace.stage(Stage::DetachedDwell);
        trace.finish::<()>(&Err(Error::Invalid("private path must not appear")));
        trace.finish(&Ok(()));
        trace.stage(Stage::Cleanup);
    }
    let events = decoded(&lines);
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[2],
        serde_json::json!({"schema":1,"client_role":"macos-arm64",
        "family":"signals","case":"stream_epipe_detach","stage":"detached_dwell",
        "outcome":"error","error_category":"invalid"})
    );
}
#[test]
fn abandonment_and_truncation_reserve_final_line() {
    let lines = RefCell::new(Vec::new());
    let sink = |line: &[u8]| {
        lines.borrow_mut().push(line.to_vec());
        Ok(())
    };
    {
        let trace = Trace::with_sink("linux-amd64", Family::Quota, Case::TwoChildrenQuota, &sink);
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
#[test]
fn failing_sink_preserves_original_result_and_cleanup_precedence() {
    let sink = |_: &[u8]| Err(io::Error::other("sink private error"));
    let run = |operation: Result<()>, cleanup: Result<()>| {
        let trace = Trace::with_sink("linux-arm64", Family::Signals, Case::StreamSigint, &sink);
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
