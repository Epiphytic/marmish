use super::*;
use crate::peer_status_wire::{Capability, DESCRIPTION_LIMIT, HostStatus, PageBytes};
use marmish_engine::{CommandPolicyDescriptionV1, Restrictions};
use sha2::{Digest, Sha256};

const START: u64 = 100_000;

fn binding() -> ClientBinding {
    ClientBinding {
        room: Hex32([1; 32]),
        local_client: Hex32([2; 32]),
        pinned_server: Hex32([3; 32]),
        local_generation: nz(1),
    }
}

fn nz(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn at(offset: u64) -> Clock {
    Clock {
        wall: START + offset,
        monotonic: Duration::from_secs(offset),
    }
}

fn heartbeat(sequence: u64, offset: u64, permission: Option<Generation>) -> Record {
    Record::new(
        binding().room,
        Payload::Heartbeat(Heartbeat {
            sequence: nz(sequence),
            sampled_at: at(offset).wall,
            interval: 300,
            host: HostStatus::default(),
            permission,
        }),
    )
}

fn permission(generation: Generation, offset: u64, content: PageContent) -> Record {
    Record::new(
        binding().room,
        Payload::Permission(PermissionRecord {
            issued_at: at(offset).wall,
            generation,
            content,
        }),
    )
}

fn description(patterns: usize) -> Vec<u8> {
    Description::from_engine(
        CommandPolicyDescriptionV1 {
            version: 1,
            layers: vec![Restrictions {
                allow_exec: None,
                deny_exec: (0..patterns)
                    .map(|i| format!("/bin/{i}/{}*", "a".repeat(450)))
                    .collect(),
            }],
        },
        Capability::Granted,
        Capability::Denied,
    )
    .unwrap()
    .canonical_bytes()
    .unwrap()
}

fn generation(ordinal: u64, bytes: &[u8]) -> Generation {
    Generation {
        ordinal: nz(ordinal),
        digest: super::super::description_digest(bytes),
    }
}

fn page(generation: Generation, offset: u64, bytes: &[u8], index: usize) -> Record {
    let total = bytes.len().div_ceil(PAGE_BYTES);
    let data = bytes.chunks(PAGE_BYTES).nth(index).unwrap().to_vec();
    permission(
        generation,
        offset,
        PageContent::Chunk {
            page: index.try_into().unwrap(),
            total: total.try_into().unwrap(),
            encoded: PageBytes(data),
        },
    )
}

fn oversize(generation: Generation, offset: u64, overflow: u32) -> Record {
    permission(
        generation,
        offset,
        PageContent::TooLarge {
            description_len: u32::try_from(DESCRIPTION_LIMIT).unwrap() + overflow,
        },
    )
}

fn snapshot(receiver: &Receiver) -> String {
    let stage = receiver.stage.as_ref().map(|stage| {
        (
            stage.issued_at,
            stage.received_at,
            stage.total,
            stage.present,
            stage.terminal_length,
            Sha256::digest(&stage.bytes),
        )
    });
    let state = (
        receiver.binding,
        receiver.seen,
        &receiver.sample,
        &receiver.current,
        stage,
        &receiver.completed,
    );
    hex::encode(Sha256::digest(format!("{state:?}")))
}

fn step(receiver: &mut Receiver, label: &str, input: Record, now: u64, allow: bool) {
    step_at(receiver, label, input, at(now), allow);
}

fn step_at(receiver: &mut Receiver, label: &str, input: Record, now: Clock, allow: bool) {
    let signed = match &input.payload {
        Payload::Heartbeat(value) => value.sampled_at,
        Payload::Permission(value) => value.issued_at,
    };
    let encoded = input.encode().unwrap();
    let expected = include_str!("peer_status_plan_golden.txt")
        .lines()
        .find_map(|line| {
            let (name, values) = line.split_once(' ')?;
            (name == label).then_some(values)
        })
        .unwrap();
    let (expected_outcome, expected_state) = expected.split_once(' ').unwrap();

    let before = snapshot(receiver);
    let reserved = receiver.reserved_bytes();
    let prepared = receiver
        .plan_receive(binding(), signed, &encoded, now, allow)
        .unwrap();
    assert_eq!(prepared.is_noop(), before == expected_state, "{label}");
    let outcome = prepared.outcome();
    let reserved_after = prepared.reserved_after();
    assert_eq!(
        format!("{:?}", Ok::<_, Refused>(outcome)),
        expected_outcome,
        "{label}"
    );

    fn simulated_failed_commit() -> Result<(), ()> {
        Err(())
    }
    if simulated_failed_commit().is_ok() {
        prepared.apply();
    } else {
        drop(prepared);
    }
    assert_eq!(snapshot(receiver), before, "{label}");
    assert_eq!(receiver.reserved_bytes(), reserved, "{label}");

    let retried = receiver
        .plan_receive(binding(), signed, &encoded, now, allow)
        .unwrap();
    assert_eq!(retried.outcome(), outcome, "{label}");
    assert_eq!(retried.reserved_after(), reserved_after, "{label}");
    assert_eq!(retried.apply(), outcome, "{label}");
    assert_eq!(receiver.reserved_bytes(), reserved_after, "{label}");
    assert_eq!(snapshot(receiver), expected_state, "{label}");
}

#[test]
fn prepared_receive_transcript() {
    let large = description(28);
    let small = description(1);
    assert_eq!(large.len().div_ceil(PAGE_BYTES), 3);
    assert!(small.len() < PAGE_BYTES);
    let first = generation(1, &large);
    let second = generation(2, &small);
    let other_first = generation(1, &small);
    let other_second = generation(2, &large);

    fn replay(
        receiver: &mut Receiver,
        steps: impl IntoIterator<Item = (&'static str, Record, u64, bool)>,
    ) {
        for (label, record, now, allow) in steps {
            step(receiver, label, record, now, allow);
        }
    }

    let mut receiver = Receiver::new(binding());
    replay(
        &mut receiver,
        [
            ("start-middle", page(first, 0, &large, 1), 0, true),
            ("duplicate", page(first, 0, &large, 1), 1, true),
            ("append-first", page(first, 0, &large, 0), 2, false),
            ("terminal-completion", page(first, 0, &large, 2), 3, false),
            ("refresh", heartbeat(1, 600, Some(first)), 603, true),
            ("replay", heartbeat(1, 600, Some(first)), 604, true),
            ("watermark", heartbeat(2, 604, Some(second)), 1090, true),
            ("old-sequence", heartbeat(1, 1100, None), 1100, true),
            ("advance", heartbeat(3, 1101, Some(second)), 1101, true),
            ("defer-same", page(second, 1102, &small, 0), 1102, false),
            (
                "poison-cached",
                heartbeat(4, 1103, Some(other_second)),
                1103,
                true,
            ),
            ("old-ordinal", page(first, 1104, &large, 1), 1104, true),
        ],
    );

    let mut receiver = Receiver::new(binding());
    step(
        &mut receiver,
        "start-terminal",
        page(first, 100, &large, 2),
        200,
        true,
    );
    step_at(
        &mut receiver,
        "expire-ignore",
        page(first, 100, &large, 2),
        Clock {
            monotonic: at(1100).monotonic,
            ..at(999)
        },
        true,
    );
    replay(
        &mut receiver,
        [
            ("restart-terminal", page(first, 1101, &large, 2), 1101, true),
            ("defer-advance", page(second, 1102, &small, 0), 1102, false),
            ("allow-single", page(second, 1102, &small, 0), 1103, true),
            ("oversize-completed", oversize(second, 1104, 1), 1104, true),
        ],
    );

    replay(
        &mut Receiver::new(binding()),
        [
            ("oversize", oversize(first, 0, 1), 0, true),
            ("oversize-length", oversize(first, 1, 2), 1, true),
            ("oversize-new", oversize(second, 2, 1), 2, true),
            (
                "chunk-against-oversize",
                page(second, 3, &small, 0),
                3,
                true,
            ),
        ],
    );
    replay(
        &mut Receiver::new(binding()),
        [
            (
                "oversize-with-stage-start",
                page(first, 0, &large, 0),
                0,
                true,
            ),
            ("oversize-with-stage", oversize(first, 1, 1), 1, true),
            ("observe-stage-start", page(second, 2, &small, 0), 2, false),
            (
                "observe-conflict",
                page(other_second, 3, &large, 0),
                3,
                true,
            ),
        ],
    );

    let mut changed = large.clone();
    changed[200] ^= 1;
    replay(
        &mut Receiver::new(binding()),
        [
            ("conflict-stage-start", page(first, 0, &large, 0), 0, true),
            ("page-conflict", page(first, 0, &changed, 0), 1, true),
        ],
    );

    // Keep the valid digest for the altered-byte case; match the digest for
    // the padded case to isolate rejection due to its noncanonical encoding.
    let mut padded = large.clone();
    padded.push(b' ');
    for (labels, value, ordinal) in [
        (
            ["reverse-terminal", "reverse-middle", "reverse-first"],
            &large,
            first,
        ),
        (
            ["digest-terminal", "digest-middle", "digest-first"],
            &changed,
            first,
        ),
        (
            ["format-terminal", "format-middle", "format-first"],
            &padded,
            generation(1, &padded),
        ),
    ] {
        let mut receiver = Receiver::new(binding());
        for (label, index) in labels.into_iter().zip([2, 1, 0]) {
            step(
                &mut receiver,
                label,
                page(ordinal, 0, value, index),
                0,
                true,
            );
        }
    }
    replay(
        &mut Receiver::new(binding()),
        [
            ("middle-terminal", page(first, 0, &large, 2), 0, true),
            ("middle-first", page(first, 0, &large, 0), 1, true),
            ("middle-finish", page(first, 0, &large, 1), 2, true),
            ("matching-refresh", heartbeat(1, 3, Some(first)), 3, true),
            ("reissue-first", page(first, 4, &large, 0), 4, true),
            ("reissue-middle", page(first, 4, &large, 1), 5, true),
            ("reissue-finish", page(first, 4, &large, 2), 6, true),
            (
                "poison-current",
                heartbeat(2, 7, Some(other_first)),
                7,
                true,
            ),
        ],
    );
}

#[test]
fn prepared_append_keeps_the_same_stage_and_discards_one_owned_page() {
    let bytes = description(28);
    let generation = generation(1, &bytes);
    let first = page(generation, 0, &bytes, 0).encode().unwrap();
    let middle = page(generation, 0, &bytes, 1).encode().unwrap();

    let mut receiver = Receiver::new(binding());
    assert_eq!(
        receiver.receive(binding(), START, &first, at(0), true),
        Ok(Applied::Pending)
    );
    let buffer = receiver.stage.as_ref().unwrap().bytes.as_ptr();
    let prepared = receiver
        .plan_receive(binding(), START, &middle, at(1), true)
        .unwrap();
    let PreparedStage::Append(stage, page) = &prepared.stage else {
        panic!("an ordinary append prepared a different mutation");
    };
    assert_eq!(stage.bytes.as_ptr(), buffer);
    assert_eq!(stage.present, 1);
    assert_eq!(page.bytes.len(), PAGE_BYTES);
    drop(prepared);
    assert_eq!(receiver.stage.as_ref().unwrap().present, 1);

    receiver.sweep(at(2));
    let prepared = receiver
        .plan_receive(binding(), START, &middle, at(3), true)
        .unwrap();
    assert_eq!(prepared.apply(), Applied::Pending);
    let stage = receiver.stage.as_ref().unwrap();
    assert_eq!(stage.bytes.as_ptr(), buffer);
    assert_eq!(stage.present, 3);
}

#[test]
fn prepared_receive_has_no_initial_receipt_until_applied_and_stays_on_its_receiver() {
    // If a trait is implemented, the explicit and blanket candidates
    // become ambiguous and this declaration stops compiling.
    trait AmbiguousIfClone<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfClone<()> for T {}
    impl<T: Clone> AmbiguousIfClone<u8> for T {}
    trait AmbiguousIfCopy<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfCopy<()> for T {}
    impl<T: Copy> AmbiguousIfCopy<u8> for T {}
    let _ = <PreparedReceive<'static> as AmbiguousIfClone<_>>::check;
    let _ = <PreparedReceive<'static> as AmbiguousIfCopy<_>>::check;

    let mut receiver = Receiver::new(binding());
    let other = Receiver::new(binding());
    let input = heartbeat(1, 1, None).encode().unwrap();

    let plan = receiver
        .plan_receive(binding(), START + 1, &input, at(1), true)
        .unwrap();
    assert_eq!(plan.outcome(), Applied::Heartbeat);
    drop(plan);
    assert!(receiver.seen.is_none());

    let plan = receiver
        .plan_receive(binding(), START + 1, &input, at(2), true)
        .unwrap();
    assert_eq!(plan.apply(), Applied::Heartbeat);
    let HeartbeatView::Fresh(sample) = receiver.heartbeat(at(2)) else {
        panic!("heartbeat did not become fresh after apply");
    };
    assert_eq!(sample.received_at, START + 2);
    assert_eq!(other.heartbeat(at(2)), HeartbeatView::Unknown);
}

#[test]
fn completed_permission_freshness_changes_only_after_refresh_apply() {
    let bytes = description(1);
    let generation = generation(1, &bytes);
    let complete = page(generation, 0, &bytes, 0).encode().unwrap();
    let mut receiver = Receiver::new(binding());
    assert_eq!(
        receiver.receive(binding(), START, &complete, at(0), true),
        Ok(Applied::Permission)
    );
    let now = at(4000);
    assert!(matches!(
        receiver.permission(now),
        PermissionView::Cached(..)
    ));

    let input = heartbeat(1, 4000, Some(generation)).encode().unwrap();
    let plan = receiver
        .plan_receive(binding(), now.wall, &input, now, true)
        .unwrap();
    drop(plan);
    assert!(matches!(
        receiver.permission(now),
        PermissionView::Cached(..)
    ));

    let plan = receiver
        .plan_receive(binding(), now.wall, &input, now, true)
        .unwrap();
    assert_eq!(plan.apply(), Applied::Heartbeat);
    assert!(matches!(
        receiver.permission(now),
        PermissionView::Fresh(..)
    ));
}
