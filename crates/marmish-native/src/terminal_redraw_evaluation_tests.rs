//! Generated offline replay. Only the typed fixture can supply display coordinates.

use super::{Batches, FLUSH_AFTER, Filter, MAX_DATA};
use crate::terminal_frames::{self, Binding, Data, Direction, Record, Role};
use crate::{Error, Result};
use std::io::Write as _;
use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "terminal_redraw_tmux_oracle.rs"]
mod tmux_oracle;

const WIDTH: usize = 80;
const HEIGHT: usize = 24;
const PROBE_ROW: usize = HEIGHT - 2;
const MAX_CHECKPOINTS: usize = 128;
const MAX_WORKLOAD: usize = 512 * 1024;
const SYNC_START: &[u8] = b"\x1b[?2026h";
const SYNC_END: &[u8] = b"\x1b[?2026l";
const REPLAY_END: Duration = Duration::from_secs(9);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Screen {
    cells: [[u8; WIDTH]; HEIGHT],
    cursor: (usize, usize),
}

impl Screen {
    fn initial() -> Self {
        let mut cells = [[b' '; WIDTH]; HEIGHT];
        cells[0][..4].copy_from_slice(b"BASE");
        cells[6][3..7].copy_from_slice(b"pane");
        Self {
            cells,
            cursor: (1, 2),
        }
    }

    fn full_paint(&self) -> Vec<u8> {
        let mut bytes = b"\x1b[0m\x1b(B\x0f\x1b[?6l\x1b[4l\x1b[r\x1b[?7h\x1b[?25h\x1b[2J".to_vec();
        for (row, line) in self.cells.iter().enumerate() {
            if let Some(last) = line.iter().rposition(|byte| *byte != b' ') {
                position(&mut bytes, (row, 0));
                bytes.extend_from_slice(&line[..=last]);
            }
        }
        position(&mut bytes, self.cursor);
        bytes
    }

    fn diff(&self, target: &Self) -> Vec<u8> {
        if self == target {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        for (row, (before, after)) in self.cells.iter().zip(&target.cells).enumerate() {
            let mut col = 0;
            while col < WIDTH - 1 {
                if before[col] == after[col] {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < WIDTH - 1 && before[col] != after[col] {
                    col += 1;
                }
                position(&mut bytes, (row, start));
                bytes.extend_from_slice(&after[start..col]);
            }
        }
        position(&mut bytes, target.cursor);
        bytes
    }
}

fn position(output: &mut Vec<u8>, (row, col): (usize, usize)) {
    write!(output, "\x1b[{};{}H", row + 1, col + 1).unwrap();
}

struct Paint {
    row: usize,
    col: usize,
    cells: Vec<u8>,
}

struct Redraw {
    paints: Vec<Paint>,
    cursor: (usize, usize),
    synchronized: bool,
}

impl Redraw {
    fn baseline(&self, before: &Screen) -> (Vec<u8>, Vec<u8>, Screen) {
        assert!(self.cursor.0 < PROBE_ROW && self.cursor.1 < WIDTH - 2);
        let mut after = before.clone();
        let mut plain = Vec::new();
        for paint in &self.paints {
            let end = paint.col.checked_add(paint.cells.len()).unwrap();
            assert!(paint.row < HEIGHT && paint.row != PROBE_ROW && end < WIDTH);
            assert!(paint.cells.iter().all(|byte| matches!(byte, b' '..=b'~')));
            after.cells[paint.row][paint.col..end].copy_from_slice(&paint.cells);
            position(&mut plain, (paint.row, paint.col));
            plain.extend_from_slice(&paint.cells);
        }
        after.cursor = self.cursor;
        position(&mut plain, self.cursor);
        let raw = if self.synchronized {
            [SYNC_START, &plain, SYNC_END].concat()
        } else {
            plain.clone()
        };
        (raw, plain, after)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    at: Duration,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
enum Partitions {
    One,
    Bytes,
    NearCap,
}

impl Partitions {
    fn width(self, len: usize) -> usize {
        match self {
            Self::One => len.max(1),
            Self::Bytes => 1,
            Self::NearCap => MAX_DATA - 1,
        }
    }
}

#[derive(Default)]
struct Replay {
    batches: Batches,
    at: Duration,
    frames: Vec<Frame>,
}

impl Replay {
    fn collect(&mut self) -> Result<()> {
        while let Some(bytes) = self.batches.take(self.at) {
            if bytes.is_empty() {
                return Err(Error::Invalid("redraw_empty_batch"));
            }
            Filter::validate_batch(&bytes)?;
            self.frames.push(Frame { at: self.at, bytes });
        }
        Ok(())
    }

    fn advance(&mut self, until: Duration) -> Result<()> {
        assert!(until >= self.at);
        self.collect()?;
        while self.at < until {
            let wait = self.batches.deadline(self.at).unwrap_or(until - self.at);
            let elapsed = wait.min(until - self.at);
            assert!(!elapsed.is_zero());
            self.batches.advance(elapsed)?;
            self.at += elapsed;
            self.collect()?;
        }
        Ok(())
    }

    fn push(&mut self, bytes: &[u8], at: Duration, partitions: Partitions) -> Result<()> {
        self.advance(at)?;
        for chunk in bytes.chunks(partitions.width(bytes.len())) {
            let mut rest = chunk;
            while !rest.is_empty() {
                let consumed = self.batches.push(rest, at)?;
                assert!(consumed > 0);
                rest = &rest[consumed..];
                self.collect()?;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<Frame>> {
        self.advance(REPLAY_END)?;
        assert!(self.batches.deadline(self.at).is_none());
        Ok(self.frames)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Substitute,
    Elide,
    SourceLimit,
    BatchBoundary,
    Deadline,
    BaselineOnly,
}

struct Comparison {
    baseline: Vec<Frame>,
    timed: Vec<Frame>,
    ideal: Vec<Frame>,
    choice: Choice,
}

fn replay(bytes: &[u8], at: Duration, partitions: Partitions) -> Result<Vec<Frame>> {
    let mut replay = Replay::default();
    replay.push(bytes, at, partitions)?;
    replay.finish()
}

fn redraw(
    before: &Screen,
    update: &Redraw,
    lag: Duration,
    partitions: Partitions,
) -> Result<(Screen, Comparison)> {
    let (raw, plain, after) = update.baseline(before);
    let mut source = Replay::default();
    let split = if lag.is_zero() {
        raw.len()
    } else {
        raw.len() - 1
    };
    source.push(&raw[..split], Duration::ZERO, partitions)?;
    source.push(&raw[split..], lag, partitions)?;
    let baseline = source.finish()?;
    assert_eq!(
        baseline
            .iter()
            .flat_map(|f| &f.bytes)
            .copied()
            .collect::<Vec<_>>(),
        plain
    );

    let mut result = Comparison::baseline_only(baseline);
    result.choose(&before.diff(&after), raw.len(), lag, partitions)?;
    Ok((after, result))
}

impl Comparison {
    fn baseline_only(baseline: Vec<Frame>) -> Self {
        Self {
            timed: baseline.clone(),
            ideal: baseline.clone(),
            baseline,
            choice: Choice::BaselineOnly,
        }
    }

    fn choose(
        &mut self,
        diff: &[u8],
        source_len: usize,
        lag: Duration,
        partitions: Partitions,
    ) -> Result<()> {
        if source_len > MAX_DATA || lag > FLUSH_AFTER {
            self.choice = Choice::SourceLimit;
            return Ok(());
        }
        let [original] = self.baseline.as_slice() else {
            self.choice = Choice::BatchBoundary;
            return Ok(());
        };

        let candidate = replay(diff, lag, partitions)?;
        if candidate.len() > 1 {
            self.choice = Choice::BatchBoundary;
            return Ok(());
        }
        self.ideal = candidate;
        let ready_at = self.ideal.first().map_or(lag, |frame| frame.at);
        if ready_at > original.at {
            self.choice = Choice::Deadline;
            return Ok(());
        }
        self.timed = self.ideal.clone();
        self.choice = match self.timed.first_mut() {
            Some(frame) => {
                frame.at = original.at;
                Choice::Substitute
            }
            None => Choice::Elide,
        };
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct Traffic {
    frames: usize,
    payload: usize,
    app: usize,
}

impl Traffic {
    fn charge(&mut self, frames: &[Frame]) {
        for frame in frames {
            self.frames += 1;
            self.payload += frame.bytes.len();
            self.app += terminal_frames::encode_app(
                &[1; 32],
                100,
                Binding {
                    transcript: [2; 32],
                    attachment: [3; 16],
                    generation: 1,
                    role: Role::Writer,
                },
                Direction::FromServer,
                &Record::Ordered(self.frames as u64, Data::Output(frame.bytes.clone())),
            )
            .expect("redraw_app_frame")
            .len();
        }
    }
}

#[derive(Default)]
struct Stats {
    arms: [Traffic; 3],
    choices: [usize; 6],
}

impl Stats {
    fn charge(&mut self, turn: &Comparison) {
        for (traffic, frames) in
            self.arms
                .iter_mut()
                .zip([&turn.baseline, &turn.timed, &turn.ideal])
        {
            traffic.charge(frames);
        }
        self.choices[turn.choice as usize] += 1;
    }

    fn report(&self, name: &str) {
        for (arm, traffic) in ["baseline", "timed", "ideal"].iter().zip(self.arms) {
            println!(
                "redraw workload={name} arm={arm} frames={} payload_bytes={} application_bytes={}",
                traffic.frames, traffic.payload, traffic.app
            );
        }
        let [
            substitute,
            elide,
            source_limit,
            batch_boundary,
            deadline,
            baseline_only,
        ] = self.choices;
        println!(
            "redraw workload={name} substitute={substitute} elide={elide} source_limit={source_limit} batch_boundary={batch_boundary} deadline={deadline} baseline_only={baseline_only}"
        );
    }
}

const WORKLOAD_VERSION: &str = "redraw-80x24-v1";
const WORKLOAD_ROUNDS: usize = 12;

fn paint(row: usize, col: usize, cells: &[u8]) -> Paint {
    Paint {
        row,
        col,
        cells: cells.to_vec(),
    }
}

fn workload(name: &str, turn: usize) -> Redraw {
    let current = b'a' + (turn % 4) as u8;
    let paints = match name {
        "sparse" => vec![
            paint(6, 3, b"pane"),
            paint(10, 3, &[current, b' ', b'x']),
            paint(0, 0, b"BASE"),
        ],
        "dense" => (0..HEIGHT)
            .filter(|row| *row != PROBE_ROW)
            .map(|row| paint(row, 0, &[if row < 3 { current } else { b'd' }; WIDTH - 1]))
            .collect(),
        "synchronized" => vec![
            paint(4, 7, &[b'?'; 55]),
            paint(4, 7, &[current; 55]),
            paint(4, 7, &[b' '; 10]),
        ],
        _ => panic!("redraw_unknown_workload"),
    };
    Redraw {
        paints,
        cursor: (2, turn % 4 + 3),
        synchronized: name == "synchronized",
    }
}

#[test]
fn terminal_redraw_evaluation_core() -> Result<()> {
    let start = Screen::initial();
    assert_eq!(
        replay(
            &[SYNC_START, SYNC_END].concat(),
            Duration::ZERO,
            Partitions::Bytes
        )?,
        vec![]
    );
    let (updated, timely) = redraw(
        &start,
        &workload("sparse", 0),
        Duration::ZERO,
        Partitions::Bytes,
    )?;
    assert_eq!(timely.choice, Choice::Substitute);
    assert_eq!(timely.timed[0].at, FLUSH_AFTER);
    assert!(timely.timed[0].bytes.len() < timely.baseline[0].bytes.len());

    let (_, late) = redraw(
        &start,
        &workload("synchronized", 0),
        Duration::from_millis(2),
        Partitions::Bytes,
    )?;
    assert_eq!(late.choice, Choice::Deadline);
    assert_eq!(late.timed, late.baseline);
    assert!(late.ideal[0].bytes.len() < late.baseline[0].bytes.len());

    let (_, across_release) = redraw(
        &start,
        &workload("sparse", 0),
        FLUSH_AFTER,
        Partitions::Bytes,
    )?;
    assert_eq!(across_release.choice, Choice::BatchBoundary);
    assert_eq!(across_release.baseline.len(), 2);
    assert_eq!(across_release.baseline, across_release.timed);

    let cursor_only = Redraw {
        paints: Vec::new(),
        cursor: (3, 5),
        synchronized: true,
    };
    let (moved, cursor_comparison) =
        redraw(&start, &cursor_only, Duration::ZERO, Partitions::NearCap)?;
    assert_eq!(moved.cells, start.cells);
    assert_eq!(moved.cursor, (3, 5));
    assert_eq!(cursor_comparison.choice, Choice::Substitute);
    assert_eq!(cursor_comparison.timed[0].bytes, b"\x1b[4;6H");

    let restore = Redraw {
        paints: vec![paint(10, 3, b"   ")],
        cursor: start.cursor,
        synchronized: false,
    };
    let (restored, comparison) = redraw(&updated, &restore, Duration::ZERO, Partitions::One)?;
    assert_eq!(restored, start);
    assert_eq!(
        comparison.timed[0].bytes,
        b"\x1b[11;4H \x1b[11;6H \x1b[2;3H"
    );
    let (_, no_op) = redraw(&restored, &restore, Duration::ZERO, Partitions::One)?;
    assert_eq!(no_op.choice, Choice::Elide);
    assert!(no_op.timed.is_empty());
    let at_limit = vec![b'x'; MAX_DATA];
    for partition in [Partitions::One, Partitions::Bytes, Partitions::NearCap] {
        let captured = replay(&at_limit, Duration::ZERO, partition)?;
        assert_eq!(
            captured,
            vec![Frame {
                at: Duration::ZERO,
                bytes: at_limit.clone()
            }]
        );
    }
    let too_large = Redraw {
        paints: (0..52).map(|_| paint(4, 0, &[b'a'; WIDTH - 1])).collect(),
        cursor: start.cursor,
        synchronized: false,
    };
    let (_, refused) = redraw(&start, &too_large, Duration::ZERO, Partitions::NearCap)?;
    assert_eq!(refused.choice, Choice::SourceLimit);
    assert_eq!(refused.baseline, refused.timed);
    assert_eq!(refused.baseline, refused.ideal);

    let opaque: &[u8] = b"\x1b]8;;https://example.invalid\x07\x1b]52;c;eA==\x07\x1b]0;title\x07\x1b[8;200;200t\x1b[6n\x1bPtmux;\x1b\x1b[?2026h\x1b\\\xff\xe2x\x1b[?1049h\x1b[31m\x1b[?2004h\xe2\x98\x83\r\n";
    let ordinary = replay(opaque, Duration::ZERO, Partitions::One)?;
    assert_eq!(ordinary, replay(opaque, Duration::ZERO, Partitions::Bytes)?);
    assert_eq!(
        ordinary[0].bytes,
        "x\x1b[?1049h\x1b[31m\x1b[?2004h☃\r\n".as_bytes()
    );
    let selected = Comparison::baseline_only(replay(opaque, Duration::ZERO, Partitions::NearCap)?);
    assert_eq!(ordinary, selected.timed);
    assert_eq!(ordinary, selected.ideal);

    let wrap = b"\x1b[24;79Habc";
    let whole_wrap = replay(wrap, Duration::ZERO, Partitions::One)?;
    let fragmented_wrap =
        Comparison::baseline_only(replay(wrap, Duration::ZERO, Partitions::Bytes)?);
    assert_eq!(whole_wrap[0].bytes, wrap);
    assert_eq!(fragmented_wrap.choice, Choice::BaselineOnly);
    assert_eq!(whole_wrap, fragmented_wrap.timed);
    assert_eq!(whole_wrap, fragmented_wrap.ideal);
    for partition in [Partitions::One, Partitions::Bytes, Partitions::NearCap] {
        assert!(matches!(
            replay(b"x\x1b[?202", Duration::ZERO, partition),
            Err(Error::Invalid("terminal_output_policy"))
        ));
    }

    let mut mixed = Replay::default();
    mixed.push(b"first", Duration::ZERO, Partitions::One)?;
    mixed.push(b"second", Duration::from_millis(1), Partitions::One)?;
    let ineligible = Comparison::baseline_only(mixed.finish()?);
    assert_eq!(ineligible.baseline, ineligible.timed);
    assert_eq!(ineligible.timed[0].bytes, b"firstsecond");
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
#[ignore = "requires an explicitly selected tmux and private Unix sockets"]
fn terminal_redraw_evaluation_tmux() -> Result<()> {
    let started = std::time::Instant::now();
    let tmux = tmux_oracle::Oracle::start();
    println!(
        "redraw fixture={WORKLOAD_VERSION} geometry={WIDTH}x{HEIGHT} rounds={WORKLOAD_ROUNDS}"
    );
    let initial = Screen::initial();
    let initialization = Comparison::baseline_only(replay(
        &initial.full_paint(),
        Duration::ZERO,
        Partitions::One,
    )?);
    tmux.check_negative(&initial, &initialization.baseline);

    let mut mixed = Stats::default();
    let mut initial_paints = Stats::default();
    let mut raw_bytes = 0;
    for name in ["sparse", "dense", "synchronized"] {
        let mut stats = Stats::default();
        let mut panes = tmux.three_panes();
        let mut expected = initial.clone();
        let mut check = |turn: &Comparison, expected: &Screen, index| {
            for (pane, frames) in panes
                .iter_mut()
                .zip([&turn.baseline, &turn.timed, &turn.ideal])
            {
                tmux.check_turn(pane, frames, expected, index);
            }
            stats.charge(turn);
            mixed.charge(turn);
        };
        raw_bytes += initial.full_paint().len();
        initial_paints.charge(&initialization);
        check(&initialization, &expected, 0);
        for index in 0..WORKLOAD_ROUNDS {
            let input = workload(name, index);
            raw_bytes += input.baseline(&expected).0.len();
            let lag = if index % 4 == 3 {
                Duration::from_millis(2)
            } else {
                Duration::ZERO
            };
            let partitions = if index % 2 == 0 {
                Partitions::One
            } else {
                Partitions::Bytes
            };
            let (next, comparison) = redraw(&expected, &input, lag, partitions)?;
            check(&comparison, &next, index + 1);
            expected = next;
        }
        // Rewriting identical cells to the same values with the same cursor
        // exercises the only record-elision case under this one-record model.
        let no_op = workload(name, WORKLOAD_ROUNDS - 1);
        raw_bytes += no_op.baseline(&expected).0.len();
        let (next, comparison) = redraw(&expected, &no_op, Duration::ZERO, Partitions::One)?;
        assert_eq!(next, expected);
        assert_eq!(comparison.choice, Choice::Elide);
        check(&comparison, &next, WORKLOAD_ROUNDS + 1);
        stats.report(name);
        tmux.close_panes(panes);
    }
    initial_paints.report("initial_paint");

    let echo = b"fixture echo\r\n";
    let scroll = b"fixed scroll row\r\n".repeat(256);
    for (name, bytes) in [("echo", echo.as_slice()), ("scroll", scroll.as_slice())] {
        let mut stats = Stats::default();
        for _ in 0..WORKLOAD_ROUNDS {
            raw_bytes += bytes.len();
            let turn =
                Comparison::baseline_only(replay(bytes, Duration::ZERO, Partitions::NearCap)?);
            stats.charge(&turn);
            mixed.charge(&turn);
        }
        stats.report(name);
    }
    assert!(mixed.choices.iter().sum::<usize>() <= MAX_CHECKPOINTS);
    assert!(raw_bytes <= MAX_WORKLOAD);
    mixed.report("mixed");
    println!(
        "redraw elapsed_ms={} raw_source_bytes={raw_bytes}",
        started.elapsed().as_millis()
    );
    Ok(())
}
