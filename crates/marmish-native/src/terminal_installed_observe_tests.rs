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
        Workload::Takeover => {
            prove_takeover(socket, &start, writer, owned, &mut sample, turns).await;
        }
        _ => unreachable!("only observer and takeover workloads use this fixture"),
    }
    assert_eq!(owned.sockets().count(), 1, "no attach created another pane");
    sample.after = command(turns).await;
    sample
}
