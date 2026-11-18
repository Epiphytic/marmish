use super::*;
use std::time::Instant;

fn filter(input: &[u8], group: usize) -> Vec<u8> {
    let mut output = Vec::new();
    let mut filter = Filter::default();
    for bytes in input.chunks(group) {
        filter.advance(Duration::ZERO).unwrap();
        for &byte in bytes {
            if let Some(unit) = filter.push(byte).unwrap() {
                assert!(approved_syntax(unit.bytes()), "invalid emitted encoding");
                output.extend_from_slice(unit.bytes());
            }
        }
    }
    output
}

// An independent encoding check. The exact semantic allowlist is tested with
// positive and negative controls below.
fn approved_syntax(bytes: &[u8]) -> bool {
    if let Some(rest) = bytes.strip_prefix(b"\x1b[") {
        let Some((&last, rest)) = rest.split_last() else {
            return false;
        };
        let rest = rest.strip_prefix(b"?").unwrap_or(rest);
        let rest = rest.strip_suffix(b" ").unwrap_or(rest);
        return b"@ABCDEFGH IJKLMPSTXZbdfhlmrsuq".contains(&last)
            && rest.split(|&byte| byte == b';').all(|field| {
                field.iter().all(u8::is_ascii_digit) && (field.len() <= 1 || field[0] != b'0')
            });
    }
    if bytes.starts_with(b"\x1b") {
        return [
            b"\x1b7".as_slice(),
            b"\x1b8",
            b"\x1bD",
            b"\x1bE",
            b"\x1bM",
            b"\x1b=",
            b"\x1b>",
            b"\x1b(B",
            b"\x1b(0",
            b"\x1b)B",
            b"\x1b)0",
        ]
        .contains(&bytes);
    }
    std::str::from_utf8(bytes).is_ok_and(|text| {
        let mut chars = text.chars();
        let valid = chars.next().is_some_and(|ch| {
            !ch.is_control() || matches!(ch, '\x08' | '\t' | '\n' | '\r' | '\x0e' | '\x0f')
        });
        valid && chars.next().is_none()
    })
}

fn assert_filtered(input: &[u8], expected: &[u8]) {
    for group in 1..=input.len().max(1) {
        assert_eq!(filter(input, group), expected, "chunk size {group}");
    }
    assert_eq!(filter(expected, 1), expected);
}

#[test]
fn core_tests_terminal_frontend_rendering_and_blocked_side_effects() {
    assert_filtered(
        b"\x1b[0007;002H\x1b[?01049h\x1b[38;2;0;92;255m\x1b[48;5;214m\x1b[02 q\x1b[;2r\x1b[2J\x1b[4l\x1b(0\x0e\x1b)B\x0f",
        b"\x1b[7;2H\x1b[?1049h\x1b[38;2;0;92;255m\x1b[48;5;214m\x1b[2 q\x1b[;2r\x1b[2J\x1b[4l\x1b(0\x0e\x1b)B\x0f",
    );
    assert_filtered(
        b"a\x1b]52;c;?\x07\x1b]52;c;AAAA\x1b\\\x1b]8;;https://example.invalid\x07\x1b]7;file://host\x07\x1b]1337;File=x\x07\x1b]2;title\x07\x1b[6n\x1b[c\x1b[>1u\x1b[?1004h\x1b[?25;1049h\x1b[8m\x1bH\x1b[3J\x1b[1;2T\x1b[38:5:2m\x1b[48;5;256m\x1b[251rb",
        b"ab",
    );
    assert_filtered(
        b"A\x1bPtmux;\x1b\x1b]52;t;AAAA\x07\x9c\x1b\x1b\\\x1b\\B\x1b_k=z\x1b\\C\x1bktitle\x1b\\D",
        b"ABCD",
    );
}

#[test]
fn core_tests_terminal_frontend_encoding_and_cancellation() {
    assert_filtered(
        b"a\xe2\x80\x9cb\xc2\x9bc\xf0\x9f\x8d\x89d\xc0\xaf\xed\xa0\x80\xf4\x90\x80\x80\x1b\\e",
        "a“bc🍉de".as_bytes(),
    );
    assert_filtered(
        b"a\x9b31m\x1b[12\nb\x1b[10\x18c\x1b[3\x1a\x1b[Dd",
        b"a\nbc\x1b[Dd",
    );
    assert_filtered(b"a\x9dU\xc2\x9cSE\x9cb\x1b]52;c;truncated", b"ab");
    assert_filtered(b"a\xe2\x82", b"a");
    assert_filtered(b"a\x1b[9999Bb", b"ab");
    assert_filtered(b"a\x1b[2;17$b", b"a");
}

#[test]
fn core_tests_terminal_frontend_limits_and_expiry() {
    let mut filter = Filter::default();
    filter.push(0x1b).unwrap();
    filter.push(b']').unwrap();
    for _ in 0..STRING_LIMIT {
        assert!(filter.push(b'x').unwrap().is_none());
    }
    assert!(filter.push(b'x').is_err());

    let mut filter = Filter::default();
    filter.advance(Duration::from_secs(30)).unwrap();
    filter.push(0x1b).unwrap();
    filter
        .advance(TIME_LIMIT - Duration::from_nanos(1))
        .unwrap();
    assert!(filter.advance(Duration::from_nanos(1)).is_err());

    let mut input = b"a\x1b[".to_vec();
    input.extend(std::iter::repeat_n(b'0', UNIT_LIMIT + 2));
    input.extend_from_slice(b"mb");
    assert_filtered(&input, b"ab");
}

#[test]
fn core_tests_terminal_frontend_all_bytes_per_state() {
    let states = [
        b"".as_slice(),
        b"\xe2",
        b"\xf0\x9f",
        b"\x1b",
        b"\x1b(",
        b"\x1b[",
        b"\x1b[2",
        b"\x1b[?",
        b"\x9b",
        b"\x1b]52",
        b"\x1bPtmux;\x1b",
        b"\x1bPtmux;\x1b\x1b",
        b"\x90tmux",
        b"\x98",
        b"\x1b_\xc2",
        b"\x1b^",
    ];
    let suffix = b"\x1b\\x\x1b[0002A\x1b]52;c;secret\x07y";
    for state in states {
        for byte in 0..=u8::MAX {
            let mut input = state.to_vec();
            input.push(byte);
            input.extend_from_slice(suffix);
            let output = filter(&input, input.len());
            assert_eq!(filter(&input, 1), output);
            assert_eq!(filter(&input, 3), output);
            assert_eq!(filter(&output, 1), output);
        }
    }
}

#[test]
#[ignore = "local byte policy throughput and latency"]
fn terminal_output_benchmark() {
    let workloads = [
        ("text", "line status • 投入\r\n".as_bytes()),
        (
            "redraw",
            b"\x1b[1;30H\x1b[38;2;245;8;11mabcdef\x1b[0m\x1b[2L",
        ),
        ("hostile", b"\x1bPtmux;\x1b\x1b]52;c;01234567890\x07\x1b\\"),
    ];
    for (name, seed) in workloads {
        for chunk in [1usize, 128, 4096] {
            let minimum = (chunk * 1000).max(262144);
            let bytes = seed.repeat(minimum.div_ceil(seed.len()));
            let mut filter = Filter::default();
            let mut durations = Vec::new();
            let mut rendered = 0;
            let started = Instant::now();
            for input in bytes.chunks(chunk) {
                let begin = Instant::now();
                for &byte in input {
                    if let Some(unit) = filter.push(byte).unwrap() {
                        rendered += std::hint::black_box(unit.bytes()).len();
                    }
                }
                durations.push(begin.elapsed());
            }
            let elapsed = started.elapsed();
            durations.sort_unstable();
            println!(
                "{name} chunk={chunk} samples={} input={} output={rendered} mib_s={:.2} p50_ns={} p95_ns={} p99_ns={} filter_bytes={}",
                durations.len(),
                bytes.len(),
                bytes.len() as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64(),
                durations[durations.len() / 2].as_nanos(),
                durations[(durations.len() - 1) * 95 / 100].as_nanos(),
                durations[(durations.len() - 1) * 99 / 100].as_nanos(),
                std::mem::size_of::<Filter>()
            );
        }
    }
}
