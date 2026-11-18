use super::*;

fn number(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

#[test]
fn terminal_socket_state_core_full_credit_and_coalesced_resize() {
    let mut daemon = DaemonInput::new();
    let mut foreground = ForegroundResize::default();
    assert!(foreground.applied().is_err());
    let data = [255; MAX_DATA];
    assert!(daemon.receive(&[0]).is_err());
    let initial = daemon.credit().unwrap().unwrap();
    assert_eq!(initial.get(), INPUT_WINDOW);
    daemon.credit_sent(initial).unwrap();
    daemon.receive(&data).unwrap();

    foreground.request((24, 80));
    let first = foreground.take_ready().unwrap();
    daemon.queue_resize(first).unwrap();
    for width in 81..=200 {
        foreground.request((25, width));
        assert!(foreground.take_ready().is_none());
    }
    for _ in 0..3 {
        daemon.receive(&data).unwrap();
    }
    assert!(daemon.receive(&[0]).is_err());
    assert!(daemon.queue_resize((24, 90)).is_err());
    assert!(daemon.resize_applied().is_err());

    daemon.wrote(9).unwrap();
    assert!(
        matches!(daemon.next(), Some(InputEffect::Bytes(bytes)) if bytes.len() == MAX_DATA - 9)
    );
    daemon.wrote(MAX_DATA - 9).unwrap();
    assert!(matches!(daemon.next(), Some(InputEffect::Resize((24, 80)))));
    assert!(daemon.wrote(1).is_err());
    daemon.resize_applied().unwrap();
    assert!(daemon.resize_receipt_ready());
    assert!(daemon.queue_resize((24, 90)).is_err());

    let next = daemon.credit().unwrap().unwrap();
    assert_eq!(next.get(), INPUT_WINDOW + MAX_DATA as u64);
    daemon.credit_sent(next).unwrap();
    daemon.receive(&data).unwrap();
    assert!(daemon.receive(&[0]).is_err());

    daemon.resize_receipt_sent().unwrap();
    assert!(daemon.resize_receipt_sent().is_err());
    foreground.applied().unwrap();
    daemon
        .queue_resize(foreground.take_ready().unwrap())
        .unwrap();
    for _ in 0..4 {
        assert!(matches!(daemon.next(), Some(InputEffect::Bytes(bytes)) if bytes == data));
        daemon.wrote(MAX_DATA).unwrap();
    }
    assert!(matches!(
        daemon.next(),
        Some(InputEffect::Resize((25, 200)))
    ));
    daemon.resize_applied().unwrap();
    daemon.resize_receipt_sent().unwrap();
    foreground.applied().unwrap();
    assert!(foreground.take_ready().is_none());
    assert!(daemon.next().is_none());
}

#[test]
fn terminal_socket_state_core_exact_display_ack_gates_native_commit() {
    let mut daemon = DaemonOutput::default();
    assert!(daemon.ack(number(1)).is_err());
    assert_eq!(
        daemon.offer(42, 11).unwrap(),
        OutputOffer::Encode(number(1))
    );
    assert!(daemon.offer(42, 10).is_err());
    assert!(daemon.offer(43, 11).is_err());
    assert!(daemon.ack(number(1)).is_err());
    assert_eq!(daemon.offer(42, 11).unwrap(), OutputOffer::Waiting);
    daemon.fully_written(number(1)).unwrap();
    assert!(daemon.fully_written(number(1)).is_err());
    assert!(daemon.ack(number(2)).is_err());
    assert_eq!(daemon.offer(42, 11).unwrap(), OutputOffer::Waiting);
    daemon.ack(number(1)).unwrap();
    assert!(daemon.ack(number(1)).is_err());
    assert_eq!(daemon.offer(42, 11).unwrap(), OutputOffer::Committed(11));
    daemon.ack(number(1)).unwrap();
    assert!(daemon.offer(42, 11).is_err());
    assert_eq!(
        daemon.offer(44, 4096).unwrap(),
        OutputOffer::Encode(number(2))
    );
    daemon.fully_written(number(2)).unwrap();
    daemon.ack(number(2)).unwrap();
    assert_eq!(
        daemon.offer(44, 4096).unwrap(),
        OutputOffer::Committed(4096)
    );
    assert!(daemon.ack(number(1)).is_err());
    assert_eq!(daemon.offer(45, 1).unwrap(), OutputOffer::Encode(number(3)));
}

#[test]
fn terminal_socket_state_core_credit_and_ids_refuse_overflow() {
    let input = DaemonInput {
        drained: u64::MAX - INPUT_WINDOW + 1,
        ..DaemonInput::new()
    };
    assert!(input.credit().is_err());
    let mut output = DaemonOutput {
        committed: u64::MAX,
        ..DaemonOutput::default()
    };
    assert!(output.offer(1, 1).is_err());
}
