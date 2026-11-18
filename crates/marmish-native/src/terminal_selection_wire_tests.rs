use super::*;

const CLIENT: [u8; 32] = [0x11; 32];
const SERVER: [u8; 32] = [0x22; 32];
const AT: u64 = 100;
const SESSION: Session = Session {
    transcript: [0xab; 32],
    transaction: [0xcd; 16],
};

fn binding() -> Binding {
    Binding {
        claim: [0x45; 16],
        generation: NonZeroU64::new(7).unwrap(),
    }
}

fn request() -> Record {
    Record::Request {
        choice: Choice::Default,
        size: Size { rows: 24, cols: 80 },
    }
}

fn parse(bytes: &[u8], now: u64, direction: Direction) -> Option<(Session, Record)> {
    let author = if direction == Direction::ToServer {
        &CLIENT
    } else {
        &SERVER
    };
    decode(
        bytes,
        author,
        now,
        SESSION.transcript,
        Some(SESSION.transaction),
        direction,
    )
}

fn modify(encoded: &[u8], change: impl FnOnce(&mut MarmotAppEvent)) -> Vec<u8> {
    let mut event = MarmotAppEvent::decode(encoded).unwrap();
    change(&mut event);
    MarmotAppEvent::new(
        event.pubkey,
        event.created_at,
        event.kind,
        event.tags,
        event.content,
    )
    .encode()
    .unwrap()
}

#[test]
fn terminal_selection_core_exact_formats_and_direction() {
    let records = [
        (request(), Direction::ToServer, 55),
        (
            Record::Request {
                choice: Choice::New,
                size: Size {
                    rows: 250,
                    cols: 500,
                },
            },
            Direction::ToServer,
            55,
        ),
        (
            Record::Accepted {
                binding: binding(),
                slot: [0x89; 16],
            },
            Direction::FromServer,
            90,
        ),
        (Record::Busy, Direction::FromServer, 50),
        (
            Record::Refused(Refusal::Unavailable),
            Direction::FromServer,
            51,
        ),
        (Record::Confirm(binding()), Direction::ToServer, 74),
    ];
    for (record, direction, length) in records {
        let author = if direction == Direction::ToServer {
            &CLIENT
        } else {
            &SERVER
        };
        let encoded = encode(author, AT, SESSION, record).unwrap();
        assert_eq!(
            MarmotAppEvent::decode(&encoded).unwrap().content.len(),
            2 * length
        );
        assert!(parse(&encoded, AT, direction) == Some((SESSION, record)));
        let wrong = if direction == Direction::ToServer {
            Direction::FromServer
        } else {
            Direction::ToServer
        };
        assert!(
            decode(&encoded, author, AT, SESSION.transcript, None, wrong).is_none(),
            "same sender cannot reverse the direction"
        );
    }
    let request = encode(&CLIENT, AT, SESSION, request()).unwrap();
    let raw = MarmotAppEvent::decode(&request).unwrap().content;
    assert_eq!(&raw[98..], "010100180050");
}

#[test]
fn terminal_selection_core_rejects_stale_wrong_transcript_and_transaction() {
    let encoded = encode(&SERVER, AT, SESSION, Record::Busy).unwrap();
    assert!(parse(&encoded, 95, Direction::FromServer).is_some());
    assert!(parse(&encoded, 129, Direction::FromServer).is_some());
    assert!(parse(&encoded, 94, Direction::FromServer).is_none());
    assert!(parse(&encoded, 130, Direction::FromServer).is_none());
    for (transcript, transaction) in [
        ([0xab; 32], Some([0xee; 16])),
        ([0; 32], Some(SESSION.transaction)),
        ([0; 32], None),
    ] {
        assert!(
            decode(
                &encoded,
                &SERVER,
                AT,
                transcript,
                transaction,
                Direction::FromServer
            )
            .is_none()
        );
    }
    let zero = modify(&encoded, |event| {
        event.content.replace_range(66..98, &"00".repeat(16))
    });
    assert!(
        decode(
            &zero,
            &SERVER,
            AT,
            SESSION.transcript,
            None,
            Direction::FromServer
        )
        .is_none()
    );
    assert!(parse(&encoded, AT, Direction::ToServer).is_none());
}

#[test]
fn terminal_selection_core_rejects_noncanonical_and_cross_kind() {
    let encoded = encode(&CLIENT, AT, SESSION, request()).unwrap();
    let rejected = [
        modify(&encoded, |e| e.kind = crate::terminal_frames::KIND),
        modify(&encoded, |e| e.tags.push(vec!["p".to_owned()])),
        modify(&encoded, |e| e.content.make_ascii_uppercase()),
        modify(&encoded, |e| e.content.push('a')),
        modify(&encoded, |e| e.content.push_str("00")),
        modify(&encoded, |e| e.content.replace_range(0..2, "02")),
        modify(&encoded, |e| e.content.replace_range(98..100, "ff")),
        modify(&encoded, |e| e.content.replace_range(100..102, "ff")),
        modify(&encoded, |e| e.content.replace_range(102..106, "0000")),
        modify(&encoded, |e| e.content = "a".repeat(182)),
    ];
    for bytes in rejected {
        assert!(parse(&bytes, AT, Direction::ToServer).is_none());
    }
    assert!(parse(&[0; MAX_EVENT + 1], AT, Direction::ToServer).is_none());
    let accepted = encode(
        &SERVER,
        AT,
        SESSION,
        Record::Accepted {
            binding: binding(),
            slot: [0x89; 16],
        },
    )
    .unwrap();
    for (start, end) in [(100, 132), (132, 148), (148, 180)] {
        let invalid = modify(&accepted, |e| {
            e.content
                .replace_range(start..end, &"0".repeat(end - start))
        });
        assert!(parse(&invalid, AT, Direction::FromServer).is_none());
    }
}
