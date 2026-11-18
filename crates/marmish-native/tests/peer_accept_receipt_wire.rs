#[path = "../src/peer_accept_receipt_wire.rs"]
mod peer_accept_receipt_wire;
#[path = "../src/peer_control_version.rs"]
mod peer_control_version;

use cgka_traits::app_event::MarmotAppEvent;
use nostr::key::PublicKey;
use peer_accept_receipt_wire::{Decoded, InvalidReceipt, Receipt, decode, prepare};

const FOUNDER: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const RECIPIENT: &str = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
const CREATED_AT: u64 = 1_730_000_000;
const CONTENT: &str = concat!(
    r#"{"v":1,"founder":"79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798","#,
    r#""recipient":"c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5","#,
    r#""room_id":"abababababababababababababababab","offer_id":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","#,
    r#""offer_revision":"efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef","#,
    r#""welcome_id":"1212121212121212121212121212121212121212121212121212121212121212","#,
    r#""epoch":18446744073709551615,"epoch_identity":"3434343434343434343434343434343434343434343434343434343434343434"}"#,
);

fn receipt() -> Receipt {
    Receipt {
        founder: PublicKey::from_hex(FOUNDER).unwrap(),
        recipient: PublicKey::from_hex(RECIPIENT).unwrap(),
        room_id: [0xab; 16],
        offer_id: [0xcd; 16],
        offer_revision: [0xef; 32],
        welcome_id: [0x12; 32],
        epoch: u64::MAX,
        epoch_identity: [0x34; 32],
    }
}

#[test]
fn prepare_matches_canonical_event_and_retains_fixed_authored_time() {
    let receipt = receipt();
    let prepared = prepare(&receipt, CREATED_AT).unwrap();
    let app = MarmotAppEvent::decode(prepared.payload()).unwrap();
    assert_eq!(app.kind, 30102);
    assert_eq!(app.pubkey, RECIPIENT);
    assert!(app.tags.is_empty());
    assert_eq!(app.content, CONTENT);
    assert_eq!(
        hex::encode(prepared.event_id()),
        "8fbdbf7d18682099135ac405e3ac799cc0f64fc23191a5ebc2a780447af8e8dc"
    );
    assert_eq!(
        decode(prepared.payload()),
        Ok(Decoded {
            receipt,
            event_id: prepared.event_id(),
            created_at: CREATED_AT,
        })
    );
    assert_eq!(prepare(&receipt, CREATED_AT), Ok(prepared));
    let new_time = prepare(&receipt, CREATED_AT + 1).unwrap();
    let old_time = prepare(&receipt, CREATED_AT).unwrap();
    assert_ne!(new_time.event_id(), old_time.event_id());
    let zero_epoch = prepare(
        &Receipt {
            epoch: 0,
            ..receipt
        },
        0,
    )
    .unwrap();
    assert_eq!(decode(zero_epoch.payload()).unwrap().receipt.epoch, 0);
}

#[test]
fn rejects_noncanonical_content_even_under_valid_inner_ids() {
    let epoch = "18446744073709551615";
    let room = "abababababababababababababababab";
    let invalid_content = [
        ("leading body whitespace", format!(" {CONTENT}")),
        ("internal body whitespace", CONTENT.replacen(':', ": ", 1)),
        (
            "field order",
            CONTENT.replacen(
                &format!(r#""v":1,"founder":"{FOUNDER}""#),
                &format!(r#""founder":"{FOUNDER}","v":1"#),
                1,
            ),
        ),
        (
            "escaped key",
            CONTENT.replacen("founder", "f\\u006funder", 1),
        ),
        ("unknown field", CONTENT.replacen('{', r#"{"extra":2,"#, 1)),
        ("duplicate field", CONTENT.replacen('{', r#"{"v":1,"#, 1)),
        ("missing field", CONTENT.replacen(r#""v":1,"#, "", 1)),
        ("wrong version", CONTENT.replacen(r#""v":1"#, r#""v":2"#, 1)),
        (
            "float version",
            CONTENT.replacen(r#""v":1"#, r#""v":1.0"#, 1),
        ),
        (
            "string version",
            CONTENT.replacen(r#""v":1"#, r#""v":"1""#, 1),
        ),
        ("negative epoch", CONTENT.replacen(epoch, "-1", 1)),
        ("float epoch", CONTENT.replacen(epoch, "12.0", 1)),
        ("string epoch", CONTENT.replacen(epoch, r#""12""#, 1)),
        (
            "overflow epoch",
            CONTENT.replacen(epoch, "18446744073709551616", 1),
        ),
        (
            "uppercase hex",
            CONTENT.replacen(room, &room.to_uppercase(), 1),
        ),
        ("wrong width", CONTENT.replacen(room, &room[..30], 1)),
        (
            "nonhex",
            CONTENT.replacen(room, &format!("z{}", &room[1..]), 1),
        ),
        ("same principals", CONTENT.replacen(FOUNDER, RECIPIENT, 1)),
        (
            "invalid founder key",
            CONTENT.replacen(FOUNDER, &"ff".repeat(32), 1),
        ),
        (
            "invalid recipient key",
            CONTENT.replacen(RECIPIENT, &"ff".repeat(32), 1),
        ),
        ("malformed body", CONTENT.trim_end_matches('}').to_owned()),
        (
            "oversized decoded body",
            format!("{CONTENT}{}", " ".repeat(1_025 - CONTENT.len())),
        ),
    ];
    for (case, content) in invalid_content {
        let app = MarmotAppEvent::new(RECIPIENT, CREATED_AT, 30102, Vec::new(), content);
        assert_eq!(
            decode(&app.encode().unwrap()),
            Err(InvalidReceipt),
            "{case}"
        );
    }
    let receipt = receipt();
    let same = Receipt {
        founder: receipt.recipient,
        ..receipt
    };
    assert_eq!(prepare(&same, CREATED_AT), Err(InvalidReceipt));
    let invalid = Receipt {
        recipient: PublicKey::from_byte_array([0xff; 32]),
        ..receipt
    };
    assert_eq!(prepare(&invalid, CREATED_AT), Err(InvalidReceipt));
}

#[test]
fn rejects_invalid_envelope_and_authorship() {
    for (author, kind, tags) in [
        (RECIPIENT.to_owned(), 30103, Vec::new()),
        (
            RECIPIENT.to_owned(),
            30102,
            vec![vec!["p".to_owned(), FOUNDER.to_owned()]],
        ),
        (FOUNDER.to_owned(), 30102, Vec::new()),
        (RECIPIENT.to_uppercase(), 30102, Vec::new()),
    ] {
        let app = MarmotAppEvent::new(author, CREATED_AT, kind, tags, CONTENT);
        assert_eq!(decode(&app.encode().unwrap()), Err(InvalidReceipt));
    }

    let app = MarmotAppEvent::new(RECIPIENT, CREATED_AT, 30102, Vec::new(), CONTENT);
    let mut altered = app.clone();
    altered.id = "a1".repeat(32);
    assert_eq!(decode(&altered.encode().unwrap()), Err(InvalidReceipt));
    let mut altered = app.clone();
    altered.created_at += 1;
    assert_eq!(decode(&altered.encode().unwrap()), Err(InvalidReceipt));
    let mut altered = app.clone();
    altered.content = CONTENT.replacen(&"34".repeat(32), &"35".repeat(32), 1);
    assert_eq!(decode(&altered.encode().unwrap()), Err(InvalidReceipt));

    let envelope = String::from_utf8(app.encode().unwrap()).unwrap();
    let unknown = envelope.replacen('{', r#"{"extra":1,"#, 1);
    assert_eq!(decode(unknown.as_bytes()), Err(InvalidReceipt));
    let duplicate = envelope.replacen('{', r#"{"kind":30102,"#, 1);
    assert_eq!(decode(duplicate.as_bytes()), Err(InvalidReceipt));
    let oversized = format!("{envelope}{}", " ".repeat(3_000));
    assert_eq!(decode(oversized.as_bytes()), Err(InvalidReceipt));
}

#[test]
fn v2_receipt_binds_every_coordinate_and_cannot_be_decoded_as_v1() {
    use peer_accept_receipt_wire::{decode_v2, prepare_v2};
    use peer_control_version::ReceiptCoordinates;

    let receipt = receipt();
    let prepared = prepare_v2(&receipt, CREATED_AT).unwrap();
    let decoded = decode_v2(prepared.payload()).unwrap();
    let app = MarmotAppEvent::decode(prepared.payload()).unwrap();
    assert_eq!(app.content, CONTENT.replacen(r#""v":1"#, r#""v":2"#, 1));
    assert_eq!(
        hex::encode(prepared.event_id()),
        "abd47b5ad42f9f9f891147caa92923e13bc7f59073218fa1b56ba5950cde99b6"
    );
    assert_eq!(
        *decoded.decoded(),
        Decoded {
            receipt,
            event_id: prepared.event_id(),
            created_at: CREATED_AT
        }
    );
    assert_eq!(decode(prepared.payload()), Err(InvalidReceipt));
    assert_eq!(
        decode_v2(prepare(&receipt, CREATED_AT).unwrap().payload()),
        Err(InvalidReceipt)
    );

    let coordinates = ReceiptCoordinates {
        founder: receipt.founder,
        recipient: receipt.recipient,
        room_id: receipt.room_id,
        offer_id: receipt.offer_id,
        offer_revision: receipt.offer_revision,
        welcome_id: receipt.welcome_id,
    };
    assert!(decoded.matches_coordinates(&coordinates));
    for mismatch in [
        ReceiptCoordinates {
            founder: receipt.recipient,
            ..coordinates
        },
        ReceiptCoordinates {
            recipient: receipt.founder,
            ..coordinates
        },
        ReceiptCoordinates {
            room_id: [0; 16],
            ..coordinates
        },
        ReceiptCoordinates {
            offer_id: [0; 16],
            ..coordinates
        },
        ReceiptCoordinates {
            offer_revision: [0; 32],
            ..coordinates
        },
        ReceiptCoordinates {
            welcome_id: [0; 32],
            ..coordinates
        },
    ] {
        assert!(!decoded.matches_coordinates(&mismatch));
    }

    let noncanonical = MarmotAppEvent::new(
        RECIPIENT,
        CREATED_AT,
        30102,
        Vec::new(),
        format!(" {}", app.content),
    );
    assert_eq!(
        decode_v2(&noncanonical.encode().unwrap()),
        Err(InvalidReceipt)
    );
    let false_author = MarmotAppEvent::new(FOUNDER, CREATED_AT, 30102, Vec::new(), app.content);
    assert_eq!(
        decode_v2(&false_author.encode().unwrap()),
        Err(InvalidReceipt)
    );
}
