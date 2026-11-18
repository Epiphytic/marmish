use super::delivery::{Attachment, Control, Effect, Fault, Offer, Progress, Receipt};
use super::*;
use std::hint::black_box;
use std::time::Instant;

const AUTHOR: [u8; 32] = [3; 32];
const WRITER: Binding = Binding {
    transcript: [0x11; 32],
    attachment: [0x22; 16],
    generation: 0x0102030405060708,
    role: Role::Writer,
};
const OBSERVER: Binding = Binding {
    role: Role::Observer,
    ..WRITER
};
const TO: Direction = Direction::ToServer;
const FROM: Direction = Direction::FromServer;
const CREATED: u64 = 1_700_000_000;

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn arrival(created: u64, wall: u64, monotonic: Duration) -> Arrival {
    Arrival {
        retention: Some(AppMessageRetentionDecision::new(created, RETENTION)),
        wall: Some(wall),
        monotonic: Some(monotonic),
    }
}

fn input(sequence: u64, data: &[u8]) -> Record {
    Record::Ordered(sequence, Data::Input(data.into()))
}

fn app(kind: u64, tags: Vec<Vec<String>>, content: String) -> Vec<u8> {
    MarmotAppEvent::new(hex::encode(AUTHOR), CREATED, kind, tags, content)
        .encode()
        .unwrap()
}

fn record(binding: Binding, direction: Direction, record: &Record, time: u64) -> Admitted {
    let bytes = encode_app(&AUTHOR, CREATED, binding, direction, record).unwrap();
    decode_app(
        &bytes,
        &AUTHOR,
        binding,
        direction,
        arrival(CREATED, CREATED, secs(time)),
    )
    .unwrap()
}

fn deliver(receiver: &mut Attachment, record: Record, time: u64) -> Result<Receipt, Fault> {
    receiver.admit(self::record(WRITER, TO, &record, time), secs(time))
}

fn expect_bytes(
    receiver: &mut Attachment,
    now: Duration,
    expected_sequence: u64,
    expected_bytes: &[u8],
) {
    let Some(Offer::Bytes(sequence, bytes)) = receiver.begin_sink(now).unwrap() else {
        panic!("missing byte offer")
    };
    assert_eq!(sequence, expected_sequence);
    assert!(bytes == expected_bytes);
}

#[test]
fn fixed_vector_and_exact_application_domain() {
    let bytes = encode_app(&AUTHOR, CREATED, WRITER, TO, &input(9, b"\0\x80\xff")).unwrap();
    let app = MarmotAppEvent::decode(&bytes).unwrap();
    let expected = concat!(
        "4d540101",
        "1111111111111111111111111111111111111111111111111111111111111111",
        "22222222222222222222222222222222",
        "0102030405060708",
        "0000000000000009",
        "0080ff"
    );
    assert!(STANDARD.decode(&app.content).unwrap() == hex::decode(expected).unwrap());
    assert_eq!(app.kind, KIND);
    assert!(app.tags.is_empty());
    assert!(crate::command_wire::decode_request(app.content.as_bytes()).is_err());
    assert!(crate::command_wire::decode_response(app.content.as_bytes()).is_err());
    assert!(marmish_stream::wire::decode_request(app.content.as_bytes()).is_err());
    assert!(marmish_stream::wire::decode_response(app.content.as_bytes()).is_err());
    let ordinary = self::app(
        crate::command_wire::KIND,
        vec![],
        STANDARD.encode(hex::decode(expected).unwrap()),
    );
    assert!(
        decode_app(
            &ordinary,
            &AUTHOR,
            WRITER,
            TO,
            arrival(CREATED, CREATED, secs(0))
        )
        .is_none()
    );

    let raw = hex::decode(expected).unwrap();
    let time = arrival(CREATED, CREATED, secs(1));
    for binding in [
        Binding {
            transcript: [0; 32],
            ..WRITER
        },
        Binding {
            attachment: [0; 16],
            ..WRITER
        },
        Binding {
            generation: 0,
            ..WRITER
        },
        Binding {
            generation: WRITER.generation + 1,
            ..WRITER
        },
        OBSERVER,
    ] {
        assert!(decode_app(&bytes, &AUTHOR, binding, TO, time).is_none());
    }
    assert!(decode_app(&bytes, &AUTHOR, WRITER, FROM, time).is_none());
    assert!(decode_app(&bytes, &[4; 32], WRITER, TO, time).is_none());
    let mut bad_id = MarmotAppEvent::decode(&bytes).unwrap();
    bad_id.id = "0".repeat(64);
    assert!(decode_app(&bad_id.encode().unwrap(), &AUTHOR, WRITER, TO, time).is_none());
    let tagged = self::app(KIND, vec![vec!["extra".into()]], STANDARD.encode(&raw));
    assert!(decode_app(&tagged, &AUTHOR, WRITER, TO, time).is_none());

    // Padding and pad bits must both be canonical.
    let one_byte = &raw[..69];
    let valid = STANDARD.encode(one_byte);
    let padded = STANDARD.encode(&raw);
    assert!(
        decode_app(
            &self::app(KIND, vec![], valid.clone()),
            &AUTHOR,
            WRITER,
            TO,
            time
        )
        .is_some()
    );
    for invalid in [
        padded.trim_end_matches('=').to_owned(),
        padded.replace("gP8=", "gP9="),
        format!("{valid}\\n"),
    ] {
        assert!(decode_app(&self::app(KIND, vec![], invalid), &AUTHOR, WRITER, TO, time).is_none());
    }
}

#[test]
fn record_sizes_roles_and_direction_matrix() {
    let sample: Vec<_> = (0..=u8::MAX).cycle().take(MAX_DATA).collect();
    let time = arrival(CREATED, CREATED, secs(0));
    let max = Binding {
        generation: u64::MAX,
        ..WRITER
    };
    for (data, direction) in [
        (Data::Input(sample.clone()), TO),
        (Data::Output(sample.clone()), FROM),
    ] {
        let record = Record::Ordered(u64::MAX, data);
        let bytes = encode_app(&AUTHOR, u64::MAX - RETENTION, max, direction, &record).unwrap();
        assert!(bytes.len() <= MAX_APP);
        let started = u64::MAX - RETENTION;
        let decoded = decode_app(
            &bytes,
            &AUTHOR,
            max,
            direction,
            arrival(started, started, secs(0)),
        )
        .unwrap();
        assert!(decoded.record == record);
        let app = MarmotAppEvent::decode(&bytes).unwrap();
        assert_eq!(app.content.len(), MAX_BASE64);
        assert_eq!(
            STANDARD.decode(app.content).unwrap().len(),
            HEADER + 8 + MAX_DATA
        );
    }
    let cases = [
        (input(1, b"a"), [true, false, false, false]),
        (
            Record::Ordered(1, Data::Output(vec![1])),
            [false, true, false, true],
        ),
        (
            Record::Ordered(
                1,
                Data::Resize(Size {
                    rows: 250,
                    cols: 500,
                }),
            ),
            [true, false, false, false],
        ),
        (Record::Ack(0), [true; 4]),
        (Record::Ack(u64::MAX), [true; 4]),
        (Record::Pulse(u64::MAX), [true, false, true, false]),
        (Record::Close, [true; 4]),
    ];
    for (record, permits) in cases {
        let allowed = [
            (WRITER, TO),
            (WRITER, FROM),
            (OBSERVER, TO),
            (OBSERVER, FROM),
        ];
        let (binding, direction) = allowed[permits.iter().position(|permit| *permit).unwrap()];
        let bytes = encode_app(&AUTHOR, CREATED, binding, direction, &record).unwrap();
        for ((binding, direction), permits) in allowed.into_iter().zip(permits) {
            assert_eq!(
                permits,
                encode_app(&AUTHOR, CREATED, binding, direction, &record).is_some()
            );
            assert_eq!(
                permits,
                decode_app(&bytes, &AUTHOR, binding, direction, time).is_some()
            );
        }
    }

    for mutating in [
        input(0, b"a"),
        input(1, b""),
        input(1, &[0; MAX_DATA + 1]),
        Record::Pulse(0),
        Record::Ordered(1, Data::Resize(Size { rows: 1, cols: 2 })),
        Record::Ordered(1, Data::Resize(Size { rows: 2, cols: 501 })),
        Record::Ordered(
            1,
            Data::Resize(Size {
                rows: 251,
                cols: 500,
            }),
        ),
    ] {
        assert!(encode_app(&AUTHOR, CREATED, WRITER, TO, &mutating).is_none());
    }

    let close = encode_app(&AUTHOR, CREATED, WRITER, TO, &Record::Close).unwrap();
    let raw = STANDARD
        .decode(MarmotAppEvent::decode(&close).unwrap().content)
        .unwrap();
    for invalid in [
        [vec![0], raw[1..].to_vec()].concat(),
        [raw[..2].to_vec(), vec![2], raw[3..].to_vec()].concat(),
        [raw[..3].to_vec(), vec![7], raw[4..].to_vec()].concat(),
        raw[..59].to_vec(),
        [raw.clone(), vec![0]].concat(),
    ] {
        assert!(
            decode_app(
                &app(KIND, vec![], STANDARD.encode(invalid)),
                &AUTHOR,
                WRITER,
                TO,
                time
            )
            .is_none()
        );
    }

    // Short raw data within the wrapper can be padded by valid JSON whitespace.
    let mut wrapper = close;
    wrapper.resize(MAX_APP, b' ');
    assert!(decode_app(&wrapper, &AUTHOR, WRITER, TO, time).is_some());
    wrapper.push(b' ');
    assert!(decode_app(&wrapper, &AUTHOR, WRITER, TO, time).is_none());
    let input = encode_app(&AUTHOR, CREATED, WRITER, TO, &input(1, &sample)).unwrap();
    let mut raw = STANDARD
        .decode(MarmotAppEvent::decode(&input).unwrap().content)
        .unwrap();
    raw.push(4);
    assert!(
        decode_app(
            &app(KIND, vec![], STANDARD.encode(raw)),
            &AUTHOR,
            WRITER,
            TO,
            time
        )
        .is_none()
    );
}

#[test]
fn independent_clocks_and_strict_freshness_for_every_variant() {
    for record in [
        input(1, b"a"),
        Record::Ack(0),
        Record::Pulse(1),
        Record::Close,
    ] {
        let bytes = encode_app(&AUTHOR, CREATED, WRITER, TO, &record).unwrap();
        let check = |time| decode_app(&bytes, &AUTHOR, WRITER, TO, time);
        assert_eq!(
            check(arrival(CREATED, CREATED - 5, secs(2)))
                .unwrap()
                .deadline,
            secs(17)
        );
        assert_eq!(
            check(arrival(CREATED, CREATED + 14, secs(2)))
                .unwrap()
                .deadline,
            secs(3)
        );
        for refused in [
            arrival(CREATED, CREATED - 6, secs(2)),
            arrival(CREATED, CREATED + 15, secs(2)),
            arrival(CREATED, CREATED + 60, secs(2)),
            arrival(CREATED, CREATED, Duration::MAX),
            arrival(CREATED + 1, CREATED, secs(2)),
            Arrival {
                wall: None,
                ..arrival(CREATED, CREATED, secs(2))
            },
            Arrival {
                monotonic: None,
                ..arrival(CREATED, CREATED, secs(2))
            },
            Arrival {
                retention: None,
                ..arrival(CREATED, CREATED, secs(2))
            },
            Arrival {
                retention: Some(AppMessageRetentionDecision::new(CREATED, 61)),
                ..arrival(CREATED, CREATED, secs(2))
            },
        ] {
            assert!(check(refused).is_none());
        }
    }
    assert!(encode_app(&AUTHOR, u64::MAX, WRITER, TO, &Record::Close).is_none());

    let bytes = encode_app(&AUTHOR, CREATED, WRITER, TO, &input(1, b"ab")).unwrap();
    let first = decode_app(
        &bytes,
        &AUTHOR,
        WRITER,
        TO,
        arrival(CREATED, CREATED + 14, secs(2)),
    )
    .unwrap();
    let backwards = decode_app(
        &bytes,
        &AUTHOR,
        WRITER,
        TO,
        arrival(CREATED, CREATED - 1, secs(2)),
    )
    .unwrap();
    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    assert_eq!(lane.admit(first, secs(2)), Ok(Receipt::Ordered));
    assert_eq!(lane.admit(backwards, secs(2)), Ok(Receipt::Ignored));
    expect_bytes(&mut lane, secs(2), 1, b"ab");
    assert_eq!(
        lane.sink_progress(1, Effect::Wrote(1), secs(2)),
        Ok(Progress::Pending)
    );
    assert_eq!(lane.control(secs(2)), Ok(None));
    assert_eq!(
        lane.sink_progress(1, Effect::Wrote(1), secs(3)),
        Err(Fault::Expired)
    );
    assert_eq!(lane.control(secs(3)), Err(Fault::Closed));
}

#[test]
fn ordered_receive_partial_sink_reordering_resize_and_duplicates() {
    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    let resize = Size { rows: 24, cols: 80 };
    assert_eq!(
        deliver(&mut lane, input(3, b"third"), 0),
        Ok(Receipt::Ordered)
    );
    assert_eq!(
        deliver(&mut lane, Record::Ordered(2, Data::Resize(resize)), 1),
        Ok(Receipt::Ordered)
    );
    assert!(lane.begin_sink(secs(2)).unwrap().is_none());
    assert_eq!(
        deliver(&mut lane, input(1, b"first"), 2),
        Ok(Receipt::Ordered)
    );
    assert_eq!(
        deliver(&mut lane, input(1, b"first"), 2),
        Ok(Receipt::Ignored)
    );
    assert_eq!(lane.control(secs(2)), Ok(None));
    expect_bytes(&mut lane, secs(2), 1, b"first");
    assert_eq!(
        lane.sink_progress(1, Effect::Wrote(2), secs(2)),
        Ok(Progress::Pending)
    );
    expect_bytes(&mut lane, secs(2), 1, b"rst");
    assert_eq!(
        lane.sink_progress(1, Effect::WouldBlock, secs(2)),
        Ok(Progress::Pending)
    );
    expect_bytes(&mut lane, secs(3), 1, b"rst");
    assert_eq!(
        lane.sink_progress(1, Effect::Wrote(3), secs(3)),
        Ok(Progress::Committed)
    );
    assert_eq!(lane.control(secs(3)), Ok(Some(Control::Ack(1))));
    assert_eq!(
        deliver(&mut lane, input(1, b"different old record"), 3),
        Ok(Receipt::Ignored)
    );
    let Some(Offer::Resize(2, actual)) = lane.begin_sink(secs(3)).unwrap() else {
        panic!("expected second resize")
    };
    assert!(resize == actual);
    assert_eq!(
        lane.sink_progress(2, Effect::Resized, secs(3)),
        Ok(Progress::Committed)
    );
    expect_bytes(&mut lane, secs(3), 3, b"third");
    assert_eq!(
        lane.sink_progress(3, Effect::Wrote(5), secs(3)),
        Ok(Progress::Committed)
    );
    assert_eq!(lane.control(secs(3)), Ok(Some(Control::Ack(3))));
    lane.control_begun(Control::Ack(2), secs(3));
    assert_eq!(lane.control(secs(3)), Ok(Some(Control::Ack(3))));
    lane.control_begun(Control::Ack(3), secs(3));
    assert_eq!(lane.control(secs(3)), Ok(None));
    assert!(lane.begin_sink(secs(3)).unwrap().is_none());
}

#[test]
fn receive_window_conflict_fence_and_gap_are_fail_closed() {
    for (records, fault) in [
        (vec![input(9, b"outside")], Fault::Window),
        (vec![input(2, b"one"), input(2, b"two")], Fault::Conflict),
    ] {
        let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
        let results = records
            .into_iter()
            .map(|record| deliver(&mut lane, record, 0));
        assert_eq!(results.last(), Some(Err(fault)));
        assert!(matches!(lane.begin_sink(secs(1)), Err(Fault::Closed)));
    }
    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    for sequence in (1..=8).rev() {
        assert_eq!(
            deliver(&mut lane, input(sequence, &[0; MAX_DATA]), 0),
            Ok(Receipt::Ordered)
        );
    }
    expect_bytes(&mut lane, secs(0), 1, &[0; MAX_DATA]);
    assert_eq!(
        lane.sink_progress(1, Effect::Wrote(1), secs(0)),
        Ok(Progress::Pending)
    );
    lane.fence();
    assert!(matches!(lane.begin_sink(secs(0)), Err(Fault::Closed)));

    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    assert_eq!(
        deliver(&mut lane, input(2, b"gap"), 0),
        Ok(Receipt::Ordered)
    );
    assert_eq!(
        deliver(&mut lane, input(2, b"gap"), 9),
        Ok(Receipt::Ignored)
    );
    assert_eq!(lane.poll(secs(9)), Ok(()));
    assert_eq!(lane.poll(secs(10)), Err(Fault::Expired));

    for effect in [
        Effect::Failed,
        Effect::Wrote(0),
        Effect::Wrote(2),
        Effect::Resized,
    ] {
        let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
        assert_eq!(deliver(&mut lane, input(1, b"a"), 0), Ok(Receipt::Ordered));
        expect_bytes(&mut lane, secs(1), 1, b"a");
        assert_eq!(lane.sink_progress(1, effect, secs(1)), Err(Fault::Sink));
        assert_eq!(lane.control(secs(1)), Err(Fault::Closed));
    }
}

#[test]
fn next_expected_record_gets_its_own_window_with_original_signed_age() {
    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    assert_eq!(
        deliver(&mut lane, input(3, b"third"), 0),
        Ok(Receipt::Ordered)
    );
    for (sequence, bytes) in [(1, b"first"), (2, b"other")] {
        assert_eq!(
            deliver(&mut lane, input(sequence, bytes), 9),
            Ok(Receipt::Ordered)
        );
        expect_bytes(&mut lane, secs(9), sequence, bytes);
        assert_eq!(
            lane.sink_progress(sequence, Effect::Wrote(bytes.len()), secs(9)),
            Ok(Progress::Committed)
        );
    }
    expect_bytes(&mut lane, secs(10), 3, b"third");
    assert_eq!(
        lane.sink_progress(3, Effect::WouldBlock, secs(10)),
        Ok(Progress::Pending)
    );
    assert_eq!(lane.poll(secs(14)), Ok(()));
    // The original signed age can expire before the newer presentation window.
    assert_eq!(lane.poll(secs(15)), Err(Fault::Expired));

    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    for (sequence, bytes) in [(1, b"first"), (2, b"other")] {
        assert_eq!(
            deliver(&mut lane, input(sequence, bytes), 9),
            Ok(Receipt::Ordered)
        );
    }
    expect_bytes(&mut lane, secs(10), 1, b"first");
    assert_eq!(
        lane.sink_progress(1, Effect::Wrote(5), secs(11)),
        Ok(Progress::Committed)
    );
    expect_bytes(&mut lane, secs(12), 2, b"other");
    assert_eq!(
        lane.sink_progress(2, Effect::Wrote(1), secs(21)),
        Ok(Progress::Pending)
    );
    assert_eq!(lane.poll(secs(22)), Err(Fault::Expired));
}

#[test]
fn state_counters_never_roll_over() {
    let mut receiver = Attachment::new(WRITER, TO, secs(0)).unwrap();
    receiver.seed_counters(u64::MAX - 1, 0, 0);
    assert_eq!(
        deliver(&mut receiver, input(u64::MAX, b"a"), 0),
        Ok(Receipt::Ordered)
    );
    expect_bytes(&mut receiver, secs(0), u64::MAX, b"a");
    assert_eq!(
        receiver.sink_progress(u64::MAX, Effect::Wrote(1), secs(0)),
        Ok(Progress::Committed)
    );
    assert_eq!(receiver.control(secs(0)), Ok(Some(Control::Ack(u64::MAX))));
    assert!(matches!(receiver.begin_sink(secs(0)), Err(Fault::Counter)));
    assert_eq!(receiver.poll(secs(0)), Err(Fault::Closed));

    let mut sender = Attachment::new(WRITER, TO, secs(0)).unwrap();
    sender.seed_counters(0, u64::MAX - 1, 0);
    assert_eq!(sender.enqueue(Data::Output(vec![1]), secs(0)), Ok(u64::MAX));
    assert_eq!(
        sender.enqueue(Data::Output(vec![1]), secs(0)),
        Err(Fault::Counter)
    );
    assert_eq!(sender.poll(secs(0)), Err(Fault::Closed));

    let mut observer = Attachment::new(OBSERVER, FROM, secs(0)).unwrap();
    observer.seed_counters(0, 0, u64::MAX);
    assert_eq!(observer.control(secs(300)), Err(Fault::Counter));
    assert_eq!(observer.poll(secs(300)), Err(Fault::Closed));
}

#[test]
fn stale_admission_elides_and_only_new_progress_renews_lease() {
    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    let bytes = encode_app(&AUTHOR, CREATED, WRITER, TO, &input(1, b"a")).unwrap();
    let time = arrival(CREATED, CREATED + 14, secs(2));
    for delivered in [1, 3] {
        let aged = decode_app(&bytes, &AUTHOR, WRITER, TO, time).unwrap();
        assert_eq!(lane.admit(aged, secs(delivered)), Ok(Receipt::Ignored));
    }
    assert!(lane.begin_sink(secs(4)).unwrap().is_none());
    assert_eq!(
        deliver(&mut lane, input(1, b"a"), 200),
        Ok(Receipt::Ordered)
    );
    expect_bytes(&mut lane, secs(201), 1, b"a");
    assert_eq!(
        lane.sink_progress(1, Effect::Wrote(1), secs(201)),
        Ok(Progress::Committed)
    );
    assert_eq!(lane.poll(secs(660)), Ok(()));

    lane.enqueue(Data::Output(vec![1]), secs(700)).unwrap();
    lane.begin_handoff(&AUTHOR, CREATED, secs(700))
        .unwrap()
        .unwrap();
    lane.handed_off(1, secs(701)).unwrap();
    assert_eq!(deliver(&mut lane, Record::Ack(1), 701), Ok(Receipt::Ack));
    assert_eq!(
        deliver(&mut lane, Record::Ack(1), 1200),
        Ok(Receipt::Ignored)
    );
    assert_eq!(lane.poll(secs(1360)), Ok(()));
    assert_eq!(lane.poll(secs(1361)), Err(Fault::Lease));

    let mut unresolved = Attachment::new(WRITER, TO, secs(0)).unwrap();
    assert_eq!(
        deliver(&mut unresolved, input(2, b"gap"), 655),
        Ok(Receipt::Ordered)
    );
    assert_eq!(unresolved.poll(secs(660)), Err(Fault::Lease));
}

#[test]
fn outbound_windows_handoff_immutability_and_cumulative_acks() {
    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    for sequence in 1..=8 {
        assert_eq!(
            lane.enqueue(Data::Output(vec![1; MAX_DATA]), secs(0)),
            Ok(sequence)
        );
    }
    assert_eq!(
        lane.enqueue(Data::Output(vec![2]), secs(0)),
        Err(Fault::Window)
    );
    let (sequence, bytes) = lane
        .begin_handoff(&AUTHOR, CREATED, secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(sequence, 1);
    let frozen = bytes.to_vec();
    assert_eq!(
        lane.begin_handoff(&AUTHOR, CREATED + 1, secs(2)).unwrap(),
        Some((1, frozen.as_slice()))
    );
    lane.handed_off(1, secs(2)).unwrap();
    assert_eq!(
        lane.begin_handoff(&AUTHOR, CREATED + 2, secs(3))
            .unwrap()
            .unwrap()
            .0,
        2
    );
    lane.handed_off(2, secs(3)).unwrap();
    assert_eq!(deliver(&mut lane, Record::Ack(0), 3), Ok(Receipt::Ignored));
    assert_eq!(deliver(&mut lane, Record::Ack(2), 4), Ok(Receipt::Ack));
    assert_eq!(deliver(&mut lane, Record::Ack(1), 4), Ok(Receipt::Ignored));
    assert_eq!(lane.enqueue(Data::Output(vec![3]), secs(4)), Ok(9));
    assert_eq!(deliver(&mut lane, Record::Ack(3), 4), Err(Fault::Window));
    assert!(matches!(
        lane.begin_handoff(&AUTHOR, CREATED, secs(4)),
        Err(Fault::Closed)
    ));

    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    lane.enqueue(Data::Output(vec![1]), secs(0)).unwrap();
    lane.begin_handoff(&AUTHOR, CREATED, secs(1))
        .unwrap()
        .unwrap();
    lane.begin_handoff(&AUTHOR, CREATED + 9, secs(10))
        .unwrap()
        .unwrap();
    assert_eq!(lane.poll(secs(11)), Err(Fault::Expired));
}

#[test]
fn lease_and_controls_remain_independent_from_ordered_work() {
    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    let wrong = Binding {
        generation: WRITER.generation + 1,
        ..WRITER
    };
    assert_eq!(
        lane.admit(record(wrong, TO, &Record::Close, 2), secs(2)),
        Ok(Receipt::Ignored)
    );
    assert_eq!(
        lane.admit(record(WRITER, FROM, &Record::Close, 2), secs(2)),
        Ok(Receipt::Ignored)
    );
    assert_eq!(
        deliver(&mut lane, Record::Pulse(2), 300),
        Ok(Receipt::Pulse)
    );
    assert_eq!(
        deliver(&mut lane, Record::Pulse(1), 400),
        Ok(Receipt::Ignored)
    );
    assert_eq!(
        deliver(&mut lane, Record::Pulse(2), 500),
        Ok(Receipt::Ignored)
    );
    assert_eq!(
        deliver(&mut lane, Record::Ack(0), 659),
        Ok(Receipt::Ignored)
    );
    assert_eq!(lane.poll(secs(959)), Ok(()));
    assert_eq!(lane.poll(secs(960)), Err(Fault::Lease));

    let mut lane = Attachment::new(OBSERVER, FROM, secs(0)).unwrap();
    assert_eq!(
        lane.enqueue(Data::Input(vec![0]), secs(1)),
        Err(Fault::Handoff)
    );
    assert_eq!(lane.control(secs(299)), Ok(None));
    assert_eq!(lane.control(secs(300)), Ok(Some(Control::Pulse(1))));
    assert_eq!(lane.control(secs(301)), Ok(Some(Control::Pulse(1))));
    lane.control_begun(Control::Pulse(1), secs(301));
    assert_eq!(lane.control(secs(600)), Ok(None));
    assert_eq!(lane.control(secs(601)), Ok(Some(Control::Pulse(2))));

    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    for sequence in 1..=8 {
        assert_eq!(
            deliver(&mut lane, input(sequence, b"a"), 0),
            Ok(Receipt::Ordered)
        );
        lane.enqueue(Data::Output(vec![0]), secs(0)).unwrap();
    }
    assert_eq!(deliver(&mut lane, Record::Close, 1), Ok(Receipt::Closed));
    assert_eq!(deliver(&mut lane, Record::Close, 1), Ok(Receipt::Ignored));
    assert_eq!(lane.control(secs(1)), Err(Fault::Closed));
    assert!(matches!(lane.begin_sink(secs(1)), Err(Fault::Closed)));

    let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
    for _ in 0..8 {
        lane.enqueue(Data::Output(vec![0]), secs(0)).unwrap();
    }
    lane.close();
    lane.close();
    assert_eq!(lane.control(secs(0)), Ok(Some(Control::Close)));
    lane.control_begun(Control::Close, secs(0));
    lane.close();
    assert_eq!(lane.control(secs(0)), Err(Fault::Closed));
}

#[test]
#[ignore = "opt-in offline Terminal codec and lane latency benchmark"]
fn codec_and_admission_benchmark() {
    const SAMPLES: usize = 2_000;
    fn percentiles(samples: &mut [u128]) -> (u128, u128) {
        samples.sort_unstable();
        (
            samples[samples.len() / 2],
            samples[samples.len() * 95 / 100],
        )
    }
    for length in [3, MAX_DATA] {
        let payload: Vec<_> = (0..=u8::MAX).cycle().take(length).collect();
        let mut codec = Vec::with_capacity(SAMPLES);
        let mut admission = Vec::with_capacity(SAMPLES);
        for index in 0..SAMPLES + 100 {
            let record = input(1, &payload);
            let start = Instant::now();
            let bytes = encode_app(&AUTHOR, CREATED, WRITER, TO, black_box(&record)).unwrap();
            let admitted = decode_app(
                &bytes,
                &AUTHOR,
                WRITER,
                TO,
                arrival(CREATED, CREATED, secs(0)),
            )
            .unwrap();
            let codec_ns = start.elapsed().as_nanos();

            let mut lane = Attachment::new(WRITER, TO, secs(0)).unwrap();
            let start = Instant::now();
            assert_eq!(
                lane.admit(black_box(admitted), secs(0)),
                Ok(Receipt::Ordered)
            );
            expect_bytes(&mut lane, secs(0), 1, &payload);
            let admission_ns = start.elapsed().as_nanos();
            if index >= 100 {
                codec.push(codec_ns);
                admission.push(admission_ns);
            }
        }
        let codec_total = codec.iter().sum::<u128>();
        let throughput = (SAMPLES * length) as f64 * 1e9 / codec_total as f64 / (1024.0 * 1024.0);
        let (codec_p50, codec_p95) = percentiles(&mut codec);
        let (admit_p50, admit_p95) = percentiles(&mut admission);
        println!(
            "terminal_frame_bench samples={SAMPLES} bytes={length} encode_decode_p50_ns={codec_p50} encode_decode_p95_ns={codec_p95} MiB/s={throughput:.3} admit_ready_p50_ns={admit_p50} admit_ready_p95_ns={admit_p95}"
        );
    }
}
