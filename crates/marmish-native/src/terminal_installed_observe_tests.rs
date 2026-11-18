//! Installed observation and takeover across the real local IPC, TLS relay
//! and native tmux. The two sockets share one canonical account and Command
//! binding. Display receipts follow bytes read by the fixture, with no TTY
//! emulator or physical-display latency claim.
use super::*;

const OUTPUT_LIMIT: usize = 32 * 1024;

type Start = handshake::Start;
type Attach = handshake::Attach;

#[test]
#[ignore = "requires native binaries, tmux and a private loopback TLS relay"]
fn terminal_installed_runtime_observer_read_only() {
    run_native(|| async {
        prove_writer(Workload::Observe).await;
    });
}

#[test]
#[ignore = "requires native binaries, tmux and a private loopback TLS relay"]
fn terminal_installed_runtime_takeover_named_resume() {
    run_native(|| async {
        prove_writer(Workload::Takeover).await;
    });
}

const TRIALS: usize = 20;
const PAYLOAD_BYTES: usize = 1024;
const PUBLICATION_CAP: usize = 2048;
const ATTACHED_BENCHMARK: Duration = Duration::from_secs(240);
const QUIET: Duration = Duration::from_millis(100);

#[test]
#[ignore = "report-only workload requiring native binaries, tmux and a private loopback TLS relay"]
fn terminal_installed_runtime_observer_benchmark() {
    run_native(|| async {
        prove_writer(Workload::ObserveBenchmark).await;
    });
}

pub(super) struct Benchmark {
    ready: Duration,
    keys: [[Duration; 2]; TRIALS],
    displayed: [usize; 2],
    output_records: [usize; 2],
    display_time: Duration,
    drain_time: Duration,
}

impl Benchmark {
    pub(super) fn report(&self, publications: usize) {
        assert!(
            (1..=PUBLICATION_CAP).contains(&publications),
            "bounded whole-fixture relay publications: {publications}"
        );
        assert!(!self.display_time.is_zero());
        eprintln!(
            "terminal_observer_benchmark platform={}-{} transport=private_loopback_tls fixture=single_thread debug_assertions={} relay_publications={publications}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(debug_assertions),
        );
        eprintln!(
            "terminal_observer_benchmark workload=first_named_observer_ready n=1 duration_us={}",
            self.ready.as_micros()
        );
        for (reader, role) in ["writer", "observer"].into_iter().enumerate() {
            let mut latencies = self.keys.map(|sample| sample[reader]);
            latencies.sort_unstable();
            let percentile = |rank: usize| latencies[(rank * TRIALS).div_ceil(100) - 1].as_micros();
            eprintln!(
                "terminal_observer_benchmark workload=single_byte_ipc_ack reader={role} n={TRIALS} min_us={} p50_us={} p95_us={} max_us={}",
                latencies[0].as_micros(),
                percentile(50),
                percentile(95),
                latencies[TRIALS - 1].as_micros(),
            );
            let bytes = self.displayed[reader];
            eprintln!(
                "terminal_observer_benchmark workload=serial_shell_display reader={role} turns={TRIALS} requested_bytes={} measured_display_bytes={bytes} elapsed_us={} drain_us={} display_bytes_per_second={} attachment_output_and_ack_records={}",
                TRIALS * PAYLOAD_BYTES,
                self.display_time.as_micros(),
                self.drain_time.as_micros(),
                (bytes as f64 / self.display_time.as_secs_f64()) as u64,
                self.output_records[reader],
            );
        }
    }
}

async fn drain_display(writer: &mut LocalTerminal, observer: &mut LocalTerminal) -> Duration {
    let began = Instant::now();
    timeout(Duration::from_secs(2), async {
        loop {
            let before = (writer.output_records, observer.output_records);
            writer
                .while_attached(observer.while_attached(tokio::time::sleep(QUIET)))
                .await;
            if (writer.output_records, observer.output_records) == before {
                return;
            }
        }
    })
    .await
    .expect("both local displays settle before measurement");
    began.elapsed()
}

async fn measure_turn(
    writer: &mut LocalTerminal,
    observer: &mut LocalTerminal,
    input: &[u8],
    marker: &[u8],
) -> [Duration; 2] {
    writer.data.clear();
    observer.data.clear();
    let accepted = writer.sent.checked_add(input.len() as u64).unwrap();
    assert!(
        writer.credit >= accepted,
        "measured Input must already have cumulative credit"
    );
    writer.sent = accepted;
    let began = Instant::now();
    writer.write_record(Record::Input(input)).await;
    let (writer_time, observer_time) = tokio::join!(
        async {
            writer.until(marker, OUTPUT_LIMIT).await;
            began.elapsed()
        },
        async {
            observer.until(marker, OUTPUT_LIMIT).await;
            began.elapsed()
        },
    );
    [writer_time, observer_time]
}

async fn prove_benchmark(socket: &Path, request: Start, writer: &mut LocalTerminal) -> Benchmark {
    let began = Instant::now();
    let mut observer = observe(socket, request, writer).await;
    let ready = began.elapsed();
    let key_reader = concat!(
        r#"old=$(stty -g) && stty -echo -icanon min 1 time 0 && { printf '\033[2J\033[H<key-%s>' ready; i=0; "#,
        r#"while [ "$i" -lt "$trials" ]; do c=$(dd bs=1 count=1 2>/dev/null) || break; printf '\n<key-%02d-%s>' "$i" "$c"; i=$((i+1)); done; "#,
        r#"stty "$old" && [ "$i" -eq "$trials" ] && printf '\n<key-%s>' restored; }"#,
    );
    let input = format!("trials={}; {key_reader}\r", TRIALS + 1);
    both_receive(writer, &mut observer, input.as_bytes(), b"<key-ready>").await;
    let mut keys = [[Duration::ZERO; 2]; TRIALS];
    for ordinal in 0..=TRIALS {
        drain_display(writer, &mut observer).await;
        let key = b'a' + ordinal as u8;
        let marker = format!("<key-{ordinal:02}-{}>", char::from(key));
        let sample = measure_turn(writer, &mut observer, &[key], marker.as_bytes()).await;
        if ordinal > 0 {
            keys[ordinal - 1] = sample;
        }
    }
    tokio::join!(
        writer.until(b"<key-restored>", OUTPUT_LIMIT),
        observer.until(b"<key-restored>", OUTPUT_LIMIT),
    );

    let mut displayed = [0; 2];
    let mut drain_time = Duration::ZERO;
    let began = Instant::now();
    for turn in 0..TRIALS {
        drain_time += drain_display(writer, &mut observer).await;
        let end = format!("display-end-{turn:02}");
        let input = format!(
            "printf '\\033[2J\\033[H' && awk -v phase={turn} -v size={PAYLOAD_BYTES} 'BEGIN {{for (i=0;i<size;i++) printf \"%c\",33+(17*int(i/32)+29*(i%32)+phase)%90}}' && printf '<%s>' '{end}'\r"
        );
        let marker = format!("<{end}>");
        measure_turn(writer, &mut observer, input.as_bytes(), marker.as_bytes()).await;
        displayed[0] += writer.data.len();
        displayed[1] += observer.data.len();
    }
    let display_time = began.elapsed();
    let output_records = [writer.output_records, observer.output_records];
    assert_eq!(output_records, [writer.output_acks, observer.output_acks]);
    writer.while_attached(observer.close(End::Detach)).await;
    Benchmark {
        ready,
        keys,
        displayed,
        output_records,
        display_time,
        drain_time,
    }
}

fn named(mut start: Start, slot: [u8; 16]) -> Start {
    start.session_slot = Some(hex::encode(slot));
    start
}

fn offered(mut start: Start, offer: handshake::Offer) -> Start {
    start.offer = Some(hex::encode(offer.bytes()));
    start
}

async fn offer(socket: &Path, request: Start, slot: [u8; 16]) -> handshake::Offer {
    match handshake::start(socket, request, Instant::now() + ATTACH).await {
        Ok(handshake::StartOutcome::Busy(ticket)) => {
            assert_eq!(
                ticket.bytes()[16..32],
                slot,
                "offer names the old writer's slot"
            );
            ticket
        }
        Ok(handshake::StartOutcome::Ready { .. }) => panic!("occupied slot returned a stream"),
        Err(error) => panic!("occupied slot did not return an offer: {error:?}"),
    }
}

async fn observe(socket: &Path, mut request: Start, writer: &mut LocalTerminal) -> LocalTerminal {
    request.rows = 11;
    request.columns = 35;
    let observer = writer
        .while_attached(open_terminal(socket, request))
        .await
        .expect("observer Ready");
    assert_eq!(observer.role, Role::Observer);
    assert_eq!(observer.slot, writer.slot);
    observer
}

async fn both_receive(
    writer: &mut LocalTerminal,
    observer: &mut LocalTerminal,
    input: &[u8],
    marker: &[u8],
) -> (Duration, Duration) {
    observer.data.clear();
    tokio::join!(
        writer.echo(input, marker, OUTPUT_LIMIT),
        observer.until(marker, OUTPUT_LIMIT),
    )
}

async fn await_close(terminal: &mut LocalTerminal) -> local::Close {
    terminal.data.clear();
    terminal.input_closed = true; // the server may close before a display receipt
    let deadline = Instant::now() + IO;
    loop {
        match terminal.next(deadline).await {
            Event::Close(reason) => return reason,
            Event::Eof => panic!("terminal reached EOF without Close"),
            Event::Output | Event::Resize | Event::Credit => (),
        }
        assert!(
            terminal.data.len() <= OUTPUT_LIMIT,
            "bounded retirement output"
        );
    }
}

async fn replaced_close(terminal: &mut LocalTerminal) -> local::Close {
    let deadline = Instant::now() + ATTACH + IO;
    loop {
        match terminal.next(deadline).await {
            Event::Close(reason) => return reason,
            Event::Eof => panic!("replaced attachment reached EOF without Close"),
            Event::Output | Event::Resize | Event::Credit => terminal.data.clear(),
        }
    }
}

async fn reject_observer_effect(observer: &mut LocalTerminal, effect: Record<'_>) {
    let forbidden = local::encode(Direction::ToDaemon, Role::Writer, effect).unwrap();
    assert!(local::encode(Direction::ToDaemon, Role::Observer, effect).is_err());
    // Deliberately bypass the foreground encoder. The daemon must independently
    // reject a writer's local record on this observer socket.
    timeout(IO, observer.stream.write_all(&forbidden))
        .await
        .expect("bounded observer test record")
        .expect("send the forbidden record to the daemon");
    assert_eq!(await_close(observer).await, local::Close::Transport);
    assert_eq!(observer.credit, 0, "observer has no input credit");
    assert_eq!(observer.sent, 0, "observer receives no writer credit");
}

async fn prove_read_only(
    socket: &Path,
    start: &impl Fn(Attach) -> Start,
    writer: &mut LocalTerminal,
    sample: &mut Measurements,
    turns: &mpsc::Sender<CommandTurn>,
) {
    let began = Instant::now();
    let mut observer = observe(socket, start(Attach::Observe), writer).await;
    sample.cold = began.elapsed();
    both_receive(
        writer,
        &mut observer,
        b"printf '<%s>' \"same-pane-$(stty size | tr ' ' :)\"\r",
        b"<same-pane-30:90>",
    )
    .await;
    sample.loaded = writer
        .while_attached(observer.while_attached(command(turns)))
        .await;
    writer
        .while_attached(reject_observer_effect(
            &mut observer,
            Record::Input(b"OBS_FORBIDDEN=BAD; printf '<%s>' unauthorized\r"),
        ))
        .await;
    drop(observer);

    let slot = writer.slot;
    let mut observer = observe(socket, named(start(Attach::Observe), slot), writer).await;
    both_receive(
        writer,
        &mut observer,
        b"printf '<%s>' \"${OBS_FORBIDDEN:-CLEAN}\"\r",
        b"<CLEAN>",
    )
    .await;
    writer.resize(31, 101).await;
    both_receive(writer, &mut observer, b"stty size | tr ' ' :\r", b"31:101").await;
    writer
        .while_attached(reject_observer_effect(
            &mut observer,
            Record::Resize {
                rows: 12,
                columns: 36,
            },
        ))
        .await;
    writer
        .echo(
            b"printf '<%s>' \"after-observer-$(stty size | tr ' ' :)\"\r",
            b"<after-observer-31:101>",
            OUTPUT_LIMIT,
        )
        .await;
}

async fn prove_takeover(
    socket: &Path,
    start: &impl Fn(Attach) -> Start,
    mut writer: LocalTerminal,
    owned: &OwnedTmux,
    sample: &mut Measurements,
    turns: &mpsc::Sender<CommandTurn>,
) {
    let slot = writer.slot;
    let mut observer = observe(socket, named(start(Attach::Observe), slot), &mut writer).await;
    both_receive(
        &mut writer,
        &mut observer,
        b"printf '<%s>' takeover-baseline\r",
        b"<takeover-baseline>",
    )
    .await;

    sample.loaded = writer
        .while_attached(observer.while_attached(command(turns)))
        .await;
    let stale = writer
        .while_attached(observer.while_attached(offer(
            socket,
            named(start(Attach::Default), slot),
            slot,
        )))
        .await;
    let successor = writer
        .while_attached(observer.while_attached(offer(
            socket,
            named(start(Attach::Takeover), slot),
            slot,
        )))
        .await;
    assert_ne!(
        stale, successor,
        "a new offer supersedes the previous nonce"
    );
    let error = writer
        .while_attached(observer.while_attached(open_terminal(
            socket,
            offered(start(Attach::Takeover), stale),
        )))
        .await;
    assert!(
        matches!(error, Err(Error::Invalid("terminal_offer_stale"))),
        "a stale offer must leave both existing sockets active"
    );
    both_receive(
        &mut writer,
        &mut observer,
        b"printf '<%s>' still-active\r",
        b"<still-active>",
    )
    .await;

    let began = Instant::now();
    let (successor_ready, old_writer, old_observer) = tokio::join!(
        async {
            let terminal = open_terminal(socket, offered(start(Attach::Takeover), successor))
                .await
                .expect("winning ticket must become the new writer");
            (terminal, began.elapsed())
        },
        replaced_close(&mut writer),
        replaced_close(&mut observer),
    );
    let (mut new_writer, ready) = successor_ready;
    sample.cold = ready;
    for reason in [old_writer, old_observer] {
        assert!(
            matches!(
                reason,
                local::Close::Ended | local::Close::Transport | local::Close::Replaced
            ),
            "unexpected old attachment close: {reason:?}"
        );
    }
    assert_eq!(new_writer.role, Role::Writer);
    assert_eq!(new_writer.slot, slot);
    assert_eq!(
        owned.sockets().count(),
        1,
        "takeover reused the exact tmux slot"
    );
    sample.echo = new_writer
        .echo(
            b"printf '<takeover-%s>' \"$MARMISH_OBSERVE_KEEP\"\r",
            b"<takeover-preserved>",
            OUTPUT_LIMIT,
        )
        .await;
    let late = local::encode(
        Direction::ToDaemon,
        Role::Writer,
        Record::Input(b"MARMISH_OBSERVE_KEEP=OLD_WRITER_LATE\r"),
    )
    .unwrap();
    // A stale Unix socket may already be shut down. If a local write still
    // reaches a pipe buffer, it must never become pane input.
    let _ = timeout(IO, writer.stream.write_all(&late)).await;
    let replay = new_writer
        .while_attached(open_terminal(
            socket,
            offered(start(Attach::Takeover), successor),
        ))
        .await;
    assert!(matches!(
        replay,
        Err(Error::Invalid("terminal_offer_stale"))
    ));
    new_writer
        .echo(
            b"printf '<after-late-%s>' \"$MARMISH_OBSERVE_KEEP\"\r",
            b"<after-late-preserved>",
            OUTPUT_LIMIT,
        )
        .await;
    new_writer.close(End::Detach).await;

    let began = Instant::now();
    let (mut resumed, retries) = resume_default(socket, |attach| named(start(attach), slot))
        .await
        .expect("an idle named slot resumes its persistent pane");
    sample.warm = began.elapsed();
    sample.warm_busy_retries = retries;
    assert_eq!(resumed.role, Role::Writer);
    assert_eq!(resumed.slot, slot);
    resumed
        .echo(
            b"printf '<resume-%s>' \"$MARMISH_OBSERVE_KEEP\"\r",
            b"<resume-preserved>",
            OUTPUT_LIMIT,
        )
        .await;
    resumed.close(End::Detach).await;
}

pub(super) async fn prove(
    socket: &Path,
    start: impl Fn(Attach) -> Start,
    owned: &OwnedTmux,
    turns: &mpsc::Sender<CommandTurn>,
    workload: Workload,
) -> Measurements {
    let mut writer = open_terminal(socket, start(Attach::Default))
        .await
        .expect("first writer Ready");
    assert_eq!(writer.role, Role::Writer);
    assert_ne!(writer.slot, [0; 16]);
    assert_eq!(
        owned.sockets().count(),
        1,
        "default writer allocated one pane"
    );
    predictable_shell(&mut writer).await;
    writer.resize(30, 90).await;
    writer
        .echo(
            b"stty size | tr ' ' :; MARMISH_OBSERVE_KEEP=preserved\r",
            b"30:90",
            OUTPUT_LIMIT,
        )
        .await;

    let mut sample = Measurements::default();
    match workload {
        Workload::Observe => {
            prove_read_only(socket, &start, &mut writer, &mut sample, turns).await;
            writer.close(End::Detach).await;
        }
        Workload::ObserveBenchmark => {
            let request = named(start(Attach::Observe), writer.slot);
            let benchmark = timeout(
                ATTACHED_BENCHMARK,
                prove_benchmark(socket, request, &mut writer),
            )
            .await
            .expect("bounded attached observer benchmark");
            sample.observer = Some(benchmark);
            writer.close(End::Detach).await;
        }
        Workload::Takeover => {
            prove_takeover(socket, &start, writer, owned, &mut sample, turns).await;
        }
        _ => unreachable!("only observer and takeover workloads use this fixture"),
    }
    assert_eq!(owned.sockets().count(), 1, "no attach created another pane");
    sample.after = command(turns).await;
    sample
}
