use super::*;
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn topology() -> Topology {
    Topology {
        role: Role::LinuxAmd64,
        pairs: [
            (Role::LinuxAmd64, Role::LinuxAmd64, [10; 32], [20; 32]),
            (Role::LinuxAmd64, Role::LinuxArm64, [10; 32], [21; 32]),
            (Role::LinuxAmd64, Role::MacosArm64, [10; 32], [22; 32]),
        ],
    }
}
fn entry(server: u8) -> StreamInspectionEntry {
    StreamInspectionEntry {
        binding: Some(marmish_stream::wire::Binding {
            stream: [0xab; 16],
            session: [0xcd; 16],
            client: [10; 32],
            server: [server; 32],
        }),
        direction: Some(InspectionDirection::Client),
        integrity: InspectionIntegrity::Valid,
        phase: Some(InspectionPhase::Running),
        server_hello_validated: None,
        open_present: Some(true),
        active: Some(true),
        cancel_requested: Some(false),
        projection_invalidated: Some(false),
        record_sequence: Some(17),
        record_stdin_accepted: Some(98304),
        record_stdin_delivered: Some(65536),
        stdout_eof: None,
        stderr_eof: None,
        terminal: None,
        slots: vec![InspectionSlot {
            integrity: InspectionIntegrity::Valid,
            kind: InspectionKind::Exchange,
            sequence: Some(17),
            response_cached: false,
            exchange: Some(InspectionExchange {
                stdin_offset: Some(65536),
                stdin_end: Some(98304),
                stdin_eof: None,
                stdout_ack: 32768,
                stderr_ack: 0,
            }),
            response: None,
        }],
        slots_truncated: false,
        outbox: vec![InspectionOutbox {
            bucket: InspectionPublicationState::Queued,
            count: 1,
            native_id_present: 1,
            intent_present: 1,
            evidence_present: 0,
        }],
    }
}
fn snapshot(entries: Vec<StreamInspectionEntry>) -> StreamInspection {
    StreamInspection {
        schema: 1,
        entries,
        records_truncated: false,
        outbox_truncated: false,
    }
}
fn json(result: &Fetch) -> serde_json::Value {
    serde_json::to_value(topology().summarize(InspectionDirection::Client, result, 42)).unwrap()
}
#[test]
fn exact_association_keeps_absence_ambiguity_and_direction_unknown() {
    let mut wrong_direction = entry(22);
    wrong_direction.direction = Some(InspectionDirection::Server);
    let mut wrong_account = entry(22);
    wrong_account.binding.as_mut().unwrap().client = [99; 32];
    let result = Ok(snapshot(vec![
        entry(20),
        entry(21),
        entry(21),
        wrong_direction,
        wrong_account,
    ]));
    let value = json(&result);
    assert_eq!(value["pairs"][0]["match_count"], "one");
    assert_eq!(
        value["pairs"][0]["progress"]["record_stdin_delivered"],
        65536
    );
    assert_eq!(value["pairs"][1]["match_count"], "multiple");
    assert!(value["pairs"][1]["progress"].is_null());
    assert_eq!(value["pairs"][2]["match_count"], "zero");
    assert!(value["pairs"][2]["progress"].is_null());
    let unavailable = json(&Err(Outcome::Unavailable));
    assert!(unavailable["records_truncated"].is_null());
    assert!(unavailable["pairs"][0]["match_count"].is_null());
}
#[test]
fn sanitizer_has_no_binding_identifiers_and_preserves_nullable_invalid_fields() {
    let mut invalid = entry(20);
    invalid.integrity = InspectionIntegrity::Invalid;
    invalid.phase = None;
    invalid.record_sequence = None;
    invalid.record_stdin_accepted = None;
    let mut s = snapshot(vec![invalid]);
    s.records_truncated = true;
    s.outbox_truncated = true;
    let bytes = RefCell::new(Vec::new());
    write_summary(
        &topology(),
        InspectionDirection::Client,
        &Ok(s),
        u64::MAX,
        &|line| {
            bytes.borrow_mut().extend(line);
            Ok(())
        },
    );
    let bytes = bytes.into_inner();
    let line = bytes.strip_prefix(PREFIX).unwrap();
    let value: serde_json::Value = serde_json::from_slice(line).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for secret in [
        "abababababababab",
        "cdcdcdcdcdcdcdcd",
        "0a0a0a0a0a0a0a0a",
        "1414141414141414",
        "binding",
        "session",
        "argv",
        "native_id\"",
        "intent\"",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert_eq!(value["records_truncated"], true);
    assert_eq!(value["outbox_truncated"], true);
    assert_eq!(value["pairs"][0]["progress"]["integrity"], "invalid");
    assert!(value["pairs"][0]["progress"]["record_sequence"].is_null());
    assert_eq!(value["elapsed_ms"], u64::MAX);
}
#[test]
fn max_shape_stays_bounded_and_sink_failure_cannot_escape() {
    let mut entries = Vec::new();
    for server in 20..23 {
        let mut e = entry(server);
        let slot = e.slots[0].clone();
        e.slots = vec![slot; 5];
        for slot in &mut e.slots {
            slot.response = Some(InspectionExchangeResponse {
                stdin_accepted: u64::MAX,
                stdin_delivered: u64::MAX,
                stdout_end: Some(u64::MAX),
                stderr_end: Some(u64::MAX),
                stdout_eof: Some(u64::MAX),
                stderr_eof: Some(u64::MAX),
            });
        }
        let outbox = e.outbox[0].clone();
        e.outbox = vec![outbox; 9];
        entries.push(e);
    }
    let result = checked(snapshot(entries));
    assert!(result.is_ok());
    let calls = RefCell::new(0);
    write_summary(
        &topology(),
        InspectionDirection::Client,
        &result,
        42,
        &|line| {
            *calls.borrow_mut() += 1;
            assert!(line.len() <= LINE_LIMIT);
            let value: serde_json::Value =
                serde_json::from_slice(line.strip_prefix(PREFIX).unwrap()).unwrap();
            assert_eq!(value["outcome"], "ok");
            Err(io::Error::other("SECRET/path/account"))
        },
    );
    assert_eq!(calls.into_inner(), 1);
    assert_eq!(
        checked(snapshot(vec![entry(20); 9])).err(),
        Some(Outcome::Capacity)
    );
    let mut bad = snapshot(vec![]);
    bad.schema = 2;
    assert_eq!(checked(bad).err(), Some(Outcome::Invalid));
}
#[test]
fn exact_failure_gate_and_cleanup_reserve() {
    assert!(eligible(
        "stream-matrix",
        &Err(Error::Invalid("original_failure"))
    ));
    assert!(!eligible("stream-matrix", &Ok(())));
    assert!(!eligible(
        "stream-matrix",
        &Err(Error::Invalid("installed_interrupted"))
    ));
    assert!(!eligible("copy-matrix", &Err(Error::Timeout)));
    let now = Instant::now();
    assert_eq!(
        cutoff(now, now + Duration::from_secs(100)),
        Some(now + Duration::from_secs(20))
    );
    assert_eq!(
        cutoff(now, now + Duration::from_secs(35)),
        Some(now + Duration::from_secs(5))
    );
    assert!(cutoff(now, now + Duration::from_secs(30)).is_none());
    assert!(cutoff(now, now).is_none());
}
#[tokio::test]
async fn both_requests_start_concurrently_once_under_one_cutoff() {
    let starts = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let request = || {
        let starts = starts.clone();
        let barrier = barrier.clone();
        async move {
            starts.fetch_add(1, Ordering::SeqCst);
            barrier.wait().await;
            Ok(snapshot(vec![]))
        }
    };
    let result = concurrent(
        request(),
        request(),
        std::future::pending(),
        Some(Instant::now() + Duration::from_secs(10)),
    )
    .await;
    assert!(matches!(result, Some((Ok(_), Ok(_)))));
    assert_eq!(starts.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn stalled_requests_share_cutoff_and_failing_sink_is_best_effort() {
    let deadline = Instant::now() + Duration::from_millis(30);
    let results = concurrent(
        std::future::pending(),
        std::future::pending(),
        std::future::pending(),
        Some(deadline),
    )
    .await
    .unwrap();
    assert_eq!(results.0.as_ref().err(), Some(&Outcome::Timeout));
    assert_eq!(results.1.as_ref().err(), Some(&Outcome::Timeout));
    for result in [&results.0, &results.1] {
        write_summary(
            &topology(),
            InspectionDirection::Client,
            result,
            30,
            &|_| Err(io::Error::other("sink failed")),
        );
    }
}
#[tokio::test]
async fn pending_signal_and_no_budget_never_poll_requests() {
    let starts = AtomicUsize::new(0);
    let request = || async {
        starts.fetch_add(1, Ordering::SeqCst);
        Ok(snapshot(vec![]))
    };
    assert!(
        concurrent(
            request(),
            request(),
            std::future::ready(()),
            Some(Instant::now() + Duration::from_secs(10))
        )
        .await
        .is_none()
    );
    assert!(
        concurrent(request(), request(), std::future::ready(()), None)
            .await
            .is_none()
    );
    let result = concurrent(request(), request(), std::future::pending(), None)
        .await
        .unwrap();
    assert_eq!(result.0.err(), Some(Outcome::SkippedNoBudget));
    assert_eq!(result.1.err(), Some(Outcome::SkippedNoBudget));
    assert_eq!(starts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn signal_during_inspection_drops_both_waits_before_cleanup() {
    struct PendingDrop(Arc<AtomicUsize>);
    impl Drop for PendingDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let request = || {
        let drops = drops.clone();
        let barrier = barrier.clone();
        async move {
            let _drop = PendingDrop(drops);
            barrier.wait().await;
            std::future::pending::<Fetch>().await
        }
    };
    let signal = async {
        barrier.wait().await;
    };
    assert!(
        concurrent(
            request(),
            request(),
            signal,
            Some(Instant::now() + Duration::from_secs(10))
        )
        .await
        .is_none()
    );
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}

#[test]
fn result_classifier_keeps_transport_capacity_and_validation_distinct() {
    let Ok(valid) = classify(Ok(StreamLocalResult::Inspection {
        snapshot: Box::new(snapshot(vec![entry(20)])),
    })) else {
        panic!("valid inspection refused")
    };
    assert_eq!(valid.entries.len(), 1);
    for (result, expected) in [
        (
            Ok(StreamLocalResult::Failure {
                category: "inspection_capacity".into(),
            }),
            Outcome::Capacity,
        ),
        (Err(Error::Timeout), Outcome::Timeout),
        (
            Err(Error::Io(io::Error::other("SECRET path/account"))),
            Outcome::Unavailable,
        ),
        (
            Ok(StreamLocalResult::Failure {
                category: "SECRET unexpected failure".into(),
            }),
            Outcome::Invalid,
        ),
        (
            Err(Error::Invalid("installed_cleanup_proof")),
            Outcome::Invalid,
        ),
        (Err(Error::Invalid("other_invalid")), Outcome::Invalid),
        (
            Ok(StreamLocalResult::Entries { entries: vec![] }),
            Outcome::Invalid,
        ),
    ] {
        assert_eq!(classify(result).err(), Some(expected));
    }
    let mut invalid = snapshot(vec![]);
    invalid.schema = 2;
    assert_eq!(
        classify(Ok(StreamLocalResult::Inspection {
            snapshot: Box::new(invalid)
        }))
        .err(),
        Some(Outcome::Invalid)
    );
}
#[tokio::test]
async fn actual_attempt_handles_missing_dead_and_unreachable_owned_children() {
    use std::process::Stdio;
    let deadline = Instant::now() + Duration::from_secs(10);
    assert_eq!(
        attempt(None, deadline).await.err(),
        Some(Outcome::Unavailable)
    );
    let mut exited = tokio::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout_at(deadline, exited.wait())
        .await
        .unwrap()
        .unwrap();
    let absent = tempfile::tempdir().unwrap().path().join("absent-socket");
    assert_eq!(
        attempt_child(&mut exited, &absent, deadline).await.err(),
        Some(Outcome::Unavailable)
    );
    let mut live = tokio::process::Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let result = attempt_child(&mut live, &absent, deadline).await;
    let kill = live.start_kill();
    let reaped = tokio::time::timeout_at(deadline, live.wait()).await;
    kill.unwrap();
    reaped.unwrap().unwrap();
    assert_eq!(result.err(), Some(Outcome::Unavailable));
}
