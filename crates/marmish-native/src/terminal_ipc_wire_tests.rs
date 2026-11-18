use super::*;

const DAEMON: Direction = Direction::ToDaemon;
const CLIENT: Direction = Direction::ToClient;
const WRITER: Role = Role::Writer;
const OBSERVER: Role = Role::Observer;

fn number(n: u64) -> NonZeroU64 {
    NonZeroU64::new(n).unwrap()
}

#[test]
fn terminal_ipc_wire_core_exact_records_and_roles() {
    let fixtures: &[(Direction, Record<'_>, &[u8], bool)] = &[
        (
            DAEMON,
            Record::Input(b"\0\xff"),
            &[0, 4, 1, 1, 0, 255],
            false,
        ),
        (
            DAEMON,
            Record::Ack(number(3)),
            &[0, 10, 1, 2, 0, 0, 0, 0, 0, 0, 0, 3],
            true,
        ),
        (
            DAEMON,
            Record::Resize {
                rows: 250,
                columns: 500,
            },
            &[0, 6, 1, 3, 0, 250, 1, 244],
            false,
        ),
        (DAEMON, Record::Detach, &[0, 2, 1, 4], true),
        (
            CLIENT,
            Record::Output {
                id: number(4),
                bytes: b"\xff",
            },
            &[0, 11, 1, 5, 0, 0, 0, 0, 0, 0, 0, 4, 255],
            true,
        ),
        (
            CLIENT,
            Record::Credit(number(5)),
            &[0, 10, 1, 6, 0, 0, 0, 0, 0, 0, 0, 5],
            false,
        ),
        (
            CLIENT,
            Record::Close(Close::Transport),
            &[0, 3, 1, 7, 5],
            true,
        ),
    ];
    for &(direction, record, bytes, observer) in fixtures {
        for input_direction in [DAEMON, CLIENT] {
            for role in [WRITER, OBSERVER] {
                let mut decoder = Decoder::new(input_direction, role);
                if input_direction == direction && (role == WRITER || observer) {
                    assert_eq!(encode(input_direction, role, record).unwrap(), bytes);
                    assert_eq!(decoder.push(bytes), Ok((bytes.len(), Some(record))));
                } else {
                    assert_eq!(
                        encode(input_direction, role, record),
                        Err(WireError::Invalid)
                    );
                    assert_eq!(decoder.push(bytes), Err(WireError::Invalid));
                    assert_eq!(decoder.push(bytes), Err(WireError::Poisoned));
                }
            }
        }
    }
}

#[test]
fn terminal_ipc_wire_core_all_splits_and_following_bytes() {
    let data = [0xff; MAX_DATA];
    let record = Record::Output {
        id: number(u64::MAX),
        bytes: &data,
    };
    let bytes = encode(CLIENT, OBSERVER, record).unwrap();
    assert_eq!(bytes.len(), 4108);
    assert_eq!(&bytes[..4], &[16, 10, 1, 5]);
    let mut concatenated = bytes.clone();
    concatenated.extend_from_slice(&[0, 3, 1, 7, 3]);
    for split in 0..bytes.len() {
        let mut decoder = Decoder::new(CLIENT, OBSERVER);
        assert_eq!(decoder.push(&bytes[..split]), Ok((split, None)));
        assert_eq!(
            decoder.push(&concatenated[split..]),
            Ok((bytes.len() - split, Some(record)))
        );
        assert_eq!(
            decoder.push(&concatenated[bytes.len()..]),
            Ok((5, Some(Record::Close(Close::Replaced))))
        );
        assert_eq!(decoder.end_of_input(), Ok(Eof::Ended));
    }

    let mut decoder = Decoder::new(DAEMON, WRITER);
    let data = [0x13; MAX_DATA];
    let input = encode(DAEMON, WRITER, Record::Input(&data)).unwrap();
    for (idx, byte) in input.iter().enumerate() {
        let got = decoder.push(std::slice::from_ref(byte)).unwrap();
        assert_eq!(got.0, 1);
        assert_eq!(
            got.1,
            (idx + 1 == input.len()).then_some(Record::Input(&data))
        );
    }
}

#[test]
fn terminal_ipc_wire_core_invalid_lengths_values_and_eof() {
    for bytes in [
        &[0, 0][..],
        &[0, 1],
        &[16, 11],
        &[255, 255],
        &[0, 2, 2, 4],
        &[0, 2, 1, 1],
        &[0, 7, 1, 3, 0, 2, 0, 2, 0],
        &[0, 6, 1, 3, 0, 1, 0, 2],
        &[0, 6, 1, 3, 0, 2, 1, 245],
        &[0, 10, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0],
        &[0, 2, 1, 99],
    ] {
        let mut decoder = Decoder::new(DAEMON, WRITER);
        assert_eq!(decoder.push(bytes), Err(WireError::Invalid));
        assert_eq!(decoder.end_of_input(), Err(WireError::Poisoned));
    }
    let mut decoder = Decoder::new(DAEMON, WRITER);
    assert_eq!(decoder.end_of_input(), Ok(Eof::Boundary));
    assert_eq!(decoder.end_of_input(), Ok(Eof::Boundary));
    assert_eq!(decoder.push(&[1]), Err(WireError::AfterEnd));
    for input in [&[0][..], &[0, 3, 1, 1]] {
        let mut decoder = Decoder::new(DAEMON, WRITER);
        assert_eq!(decoder.push(input), Ok((input.len(), None)));
        assert_eq!(decoder.end_of_input(), Err(WireError::Truncated));
        assert_eq!(decoder.push(&[1]), Err(WireError::Poisoned));
    }
    assert_eq!(
        encode(DAEMON, WRITER, Record::Input(&[0; MAX_DATA + 1])),
        Err(WireError::Invalid)
    );
    assert_eq!(
        encode(
            CLIENT,
            WRITER,
            Record::Output {
                id: number(1),
                bytes: &[]
            }
        ),
        Err(WireError::Invalid)
    );
}

#[test]
fn terminal_ipc_wire_core_directional_close_and_detach() {
    for discriminant in 0..=7 {
        let mut decoder = Decoder::new(CLIENT, OBSERVER);
        let bytes = [0, 3, 1, 7, discriminant];
        if (1..=6).contains(&discriminant) {
            let Some(Record::Close(category)) = decoder.push(&bytes).unwrap().1 else {
                panic!("expected a close");
            };
            assert_eq!(
                encode(CLIENT, OBSERVER, Record::Close(category)).unwrap(),
                bytes
            );
            assert_eq!(decoder.push(&[]), Ok((0, None)));
            assert_eq!(decoder.push(&[1]), Err(WireError::AfterEnd));
            assert_eq!(decoder.push(&[1]), Err(WireError::Poisoned));
        } else {
            assert_eq!(decoder.push(&bytes), Err(WireError::Invalid));
        }
    }
    let mut opposite = Decoder::new(DAEMON, OBSERVER);
    let ack = encode(DAEMON, OBSERVER, Record::Ack(number(1))).unwrap();
    assert_eq!(
        opposite.push(&ack),
        Ok((ack.len(), Some(Record::Ack(number(1)))))
    );
    assert_eq!(opposite.push(&[0, 2, 1, 4]), Ok((4, Some(Record::Detach))));
    assert_eq!(opposite.end_of_input(), Ok(Eof::Ended));
    assert_eq!(opposite.push(&ack), Err(WireError::AfterEnd));
}

#[test]
#[ignore = "local microbenchmark"]
fn terminal_ipc_wire_bench() {
    use std::time::Instant;
    for data in [&[0x21; 3][..], &[0x21; MAX_DATA]] {
        let input = Record::Output {
            id: number(1),
            bytes: data,
        };
        let bytes = encode(CLIENT, WRITER, input).unwrap();
        let mut nanos = Vec::with_capacity(20_000);
        let mut decoder = Decoder::new(CLIENT, WRITER);
        for _ in 0..nanos.capacity() {
            let start = Instant::now();
            let out = decoder.push(std::hint::black_box(&bytes)).unwrap();
            assert_eq!(std::hint::black_box(out), (bytes.len(), Some(input)));
            nanos.push(start.elapsed().as_nanos());
        }
        nanos.sort_unstable();
        eprintln!(
            "ipc_decode payload_bytes={} p50_ns={} p95_ns={}",
            data.len(),
            nanos[nanos.len() / 2],
            nanos[nanos.len() * 95 / 100]
        );
    }
}
