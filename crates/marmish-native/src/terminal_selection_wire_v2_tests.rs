use super::*;

const AUTHOR: [u8; 32] = [3; 32];
const NOW: u64 = 1_800_000_000;
const SESSION: Session = Session {
    transcript: [1; 32],
    transaction: [2; 16],
};
const SIZE: Size = Size { rows: 24, cols: 80 };
const SLOT: [u8; 16] = [4; 16];
const OTHER: [u8; 16] = [5; 16];
const PRIOR_CLAIM: [u8; 16] = [7; 16];
const WRITER: Binding = Binding {
    role: Role::Writer,
    claim: [6; 16],
    generation: NonZeroU64::MIN,
};
const OBSERVER: Binding = Binding {
    role: Role::Observer,
    ..WRITER
};

fn offer() -> Ticket {
    Ticket::new([0xab; 16], SLOT, PRIOR_CLAIM, WRITER.generation).unwrap()
}

fn request(choice: Choice, target: Target) -> Request {
    Request {
        choice,
        target,
        size: SIZE,
    }
}

fn check(bytes: &[u8], direction: Direction) -> Option<Record> {
    decode(
        bytes,
        &AUTHOR,
        NOW,
        SESSION.transcript,
        Some(SESSION.transaction),
        direction,
    )
    .map(|(_, record)| record)
}

#[test]
fn terminal_selection_v2_core_exact_records() {
    let ticket = offer();
    let ticket_hex = [
        "ab".repeat(16),
        "04".repeat(16),
        "07".repeat(16),
        "0000000000000001".into(),
    ]
    .concat();
    let claim_hex = ["06".repeat(16), "0000000000000001".into()].concat();
    let header = ["02".into(), "01".repeat(32), "02".repeat(16)].concat();
    let cases = [
        (
            Record::Request(request(Choice::Regular, Target::Default)),
            "01010100180050".to_owned(),
        ),
        (
            Record::Request(request(Choice::Regular, Target::Named(SLOT))),
            format!("01010200180050{}", "04".repeat(16)),
        ),
        (
            Record::Request(request(Choice::New, Target::Default)),
            "01020100180050".to_owned(),
        ),
        (
            Record::Request(request(Choice::Observe, Target::Named(SLOT))),
            format!("01030200180050{}", "04".repeat(16)),
        ),
        (
            Record::Request(request(Choice::Takeover, Target::Offer(ticket))),
            format!("01040300180050{ticket_hex}"),
        ),
        (
            Record::Accepted {
                binding: WRITER,
                slot: OTHER,
            },
            format!("0201{claim_hex}{}", "05".repeat(16)),
        ),
        (
            Record::Accepted {
                binding: OBSERVER,
                slot: SLOT,
            },
            format!("0202{claim_hex}{}", "04".repeat(16)),
        ),
        (Record::Busy(ticket), format!("03{ticket_hex}")),
        (
            Record::Refused(Refusal::ReplacementPending),
            "0407".to_owned(),
        ),
        (Record::Confirm(WRITER), format!("0501{claim_hex}")),
    ];
    for (record, body) in cases {
        let bytes = encode(&AUTHOR, NOW, SESSION, record).unwrap();
        assert!(bytes.len() <= MAX_EVENT);
        let event = MarmotAppEvent::decode(&bytes).unwrap();
        assert_eq!(event.content, format!("{header}{body}"));
        assert!(check(&bytes, record.direction()) == Some(record));
        assert!(
            decode(
                &bytes,
                &AUTHOR,
                NOW,
                SESSION.transcript,
                None,
                record.direction()
            ) == Some((SESSION, record))
        );
        let reverse = match record.direction() {
            Direction::ToServer => Direction::FromServer,
            Direction::FromServer => Direction::ToServer,
        };
        assert!(check(&bytes, reverse).is_none());
        assert!(content_record(format!("{}00", event.content), record.direction()).is_none());
        assert!(
            content_record(
                event.content[..event.content.len() - 2].to_owned(),
                record.direction(),
            )
            .is_none()
        );
    }
    let mut encoded = offer().bytes();
    assert_eq!(&encoded[..16], &[0xab; 16]);
    assert_eq!(&encoded[16..32], &SLOT);
    assert_eq!(&encoded[32..48], &PRIOR_CLAIM);
    assert_eq!(&encoded[48..], &1u64.to_be_bytes());
    assert!(Ticket::read(&encoded) == Some(offer()));
    encoded[48..].fill(0);
    assert!(Ticket::read(&encoded).is_none());
}

#[test]
fn terminal_selection_v2_core_request_outcomes() {
    let exact = Target::Named(SLOT);
    let ticket = offer();
    let offered = Target::Offer(ticket);
    let cases = [
        (
            Choice::Regular,
            Target::Default,
            true,
            true,
            false,
            true,
            true,
        ),
        (Choice::Regular, exact, true, true, false, false, true),
        (Choice::Regular, offered, false, false, false, false, false),
        (Choice::New, Target::Default, true, true, false, true, false),
        (Choice::New, exact, false, false, false, false, false),
        (Choice::New, offered, true, false, false, true, false),
        (
            Choice::Observe,
            Target::Default,
            true,
            false,
            true,
            false,
            false,
        ),
        (Choice::Observe, exact, true, false, true, false, false),
        (Choice::Observe, offered, true, false, true, false, false),
        (
            Choice::Takeover,
            Target::Default,
            true,
            false,
            false,
            false,
            true,
        ),
        (Choice::Takeover, exact, true, false, false, false, true),
        (Choice::Takeover, offered, true, true, false, false, false),
    ];
    for (choice, target, valid, writer, observer, other, busy) in cases {
        let record = request(choice, target);
        let encoded = encode(&AUTHOR, NOW, SESSION, Record::Request(record));
        assert_eq!(encoded.is_some(), valid);
        if let Some(bytes) = encoded {
            assert!(check(&bytes, Direction::ToServer) == Some(Record::Request(record)));
        }
        assert_eq!(record.accepts(WRITER, SLOT), writer);
        assert_eq!(record.accepts(OBSERVER, SLOT), observer);
        assert_eq!(record.accepts(WRITER, OTHER), other);
        assert_eq!(record.accepts_busy(ticket), busy);
    }
    assert!(!request(Choice::Regular, Target::Named(OTHER)).accepts_busy(ticket));
    assert!(!request(Choice::Observe, offered).accepts(OBSERVER, OTHER));
    for (choice, mut binding, slot) in [
        (Choice::Observe, OBSERVER, SLOT),
        (Choice::Takeover, WRITER, SLOT),
        (Choice::New, WRITER, OTHER),
    ] {
        binding.claim = PRIOR_CLAIM;
        assert!(!request(choice, offered).accepts(binding, slot));
    }
}

#[test]
fn terminal_selection_v2_core_rejects_mismatched_and_noncanonical_records() {
    let longest = Record::Request(request(Choice::Takeover, Target::Offer(offer())));
    let bytes = encode(&AUTHOR, NOW, SESSION, longest).unwrap();
    let event = MarmotAppEvent::decode(&bytes).unwrap();
    let raw = hex::decode(&event.content).unwrap();
    assert_eq!(event.content.len(), 224);
    for (offset, change) in [
        (0, 0),
        (0, 1),
        (0, 3),
        (1, 0xff),
        (33, 0xff),
        (49, 0xff),
        (50, 1),
        (51, 2),
    ] {
        let mut changed = raw.clone();
        changed[offset] = change;
        assert!(modified(hex::encode(changed), &AUTHOR, NOW, KIND, vec![]).is_none());
    }
    for range in [1..33, 33..49, 52..54, 56..72, 72..88, 88..104, 104..112] {
        let mut changed = raw.clone();
        changed[range].fill(0);
        assert!(modified(hex::encode(changed), &AUTHOR, NOW, KIND, vec![]).is_none());
    }
    let header = &event.content[..98];
    for invalid in [
        format!("{header}0400"),
        format!("{header}04ff"),
        format!("{header}0200{}", "01".repeat(40)),
        format!("{header}0503{}", "01".repeat(24)),
        format!(
            "{header}0201{}0000000000000001{}",
            "00".repeat(16),
            "04".repeat(16)
        ),
        format!(
            "{header}0201{}0000000000000000{}",
            "06".repeat(16),
            "04".repeat(16)
        ),
        format!(
            "{header}0201{}0000000000000001{}",
            "06".repeat(16),
            "00".repeat(16)
        ),
        format!("{header}0501{}0000000000000001", "00".repeat(16)),
        format!("{header}0501{}0000000000000000", "06".repeat(16)),
    ] {
        assert!(content_record(invalid.clone(), Direction::FromServer).is_none());
        assert!(content_record(invalid, Direction::ToServer).is_none());
    }
    let legacy = format!("01{}0100180050", &event.content[2..98]);
    assert!(content_record(legacy, Direction::ToServer).is_none());
    for content in [
        event.content.to_uppercase(),
        event.content[..223].into(),
        format!("{}zz", &event.content[..222]),
        format!("{}00", event.content),
    ] {
        assert!(modified(content, &AUTHOR, NOW, KIND, vec![]).is_none());
    }
    assert!(modified(event.content.clone(), &[99; 32], NOW, KIND, vec![]).is_none());
    assert!(modified(event.content.clone(), &AUTHOR, NOW - 3600, KIND, vec![]).is_none());
    assert!(modified(event.content.clone(), &AUTHOR, NOW, KIND + 1, vec![]).is_none());
    assert!(modified(event.content, &AUTHOR, NOW, KIND, vec![vec!["p".into()]]).is_none());
}

fn modified(
    content: String,
    author: &[u8; 32],
    created: u64,
    kind: u64,
    tags: Vec<Vec<String>>,
) -> Option<Record> {
    let bytes = MarmotAppEvent::new(hex::encode(author), created, kind, tags, content)
        .encode()
        .unwrap();
    check(&bytes, Direction::ToServer)
}

fn content_record(content: String, direction: Direction) -> Option<Record> {
    let bytes = MarmotAppEvent::new(hex::encode(AUTHOR), NOW, KIND, vec![], content)
        .encode()
        .unwrap();
    check(&bytes, direction)
}
