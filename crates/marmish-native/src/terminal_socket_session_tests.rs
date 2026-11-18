use super::*;
use crate::terminal_ipc_wire::encode;
use std::io::{self, Read, Write};

fn pair(role: Role) -> Result<(ForegroundSocket, DaemonSocket<UnixStream>)> {
    let (front, daemon) = UnixStream::pair()?;
    daemon.set_nonblocking(true)?;
    Ok((
        ForegroundSocket::new(front, role)?,
        DaemonSocket::new(daemon, role),
    ))
}

#[test]
fn terminal_runtime_core_capacity_and_slow_reader() -> Result<()> {
    let (_blocked, mut slow) = pair(Role::Writer)?;
    let (mut display, mut healthy) = pair(Role::Writer)?;
    let now = Instant::now();
    let deadline = now + STALL;
    let native_deadline = deadline + STALL;
    let mut output = [0; MAX_DATA];

    slow.flush(now, native_deadline)?;
    assert_eq!(slow.offer(1, b"held", now, native_deadline)?, None);
    slow.flush(now, native_deadline)?;
    assert_eq!(slow.offer(1, b"held", now, native_deadline)?, None);
    assert!(slow.offer(2, b"next", now, native_deadline).is_err());

    healthy.flush(now, native_deadline)?;
    assert_eq!(display.read(&mut output, now)?, FrontendOutput::Pending);
    assert_eq!(healthy.offer(1, b"ready", now, native_deadline)?, None);
    healthy.flush(now, native_deadline)?;
    assert_eq!(display.read(&mut output, now)?, FrontendOutput::Bytes(5));
    assert_eq!(&output[..5], b"ready");
    display.display_committed()?;
    assert_eq!(display.write(&[], now)?, 0);
    assert_eq!(healthy.read(now)?, ServerRead::Received);
    assert_eq!(healthy.offer(1, b"ready", now, native_deadline)?, Some(5));

    assert_eq!(slow.deadline(), Some(deadline));
    assert_eq!(
        slow.offer(
            1,
            b"held",
            deadline - Duration::from_millis(1),
            native_deadline
        )?,
        None
    );
    assert!(matches!(slow.read(deadline), Err(Error::Timeout)));
    Ok(())
}

#[test]
fn terminal_socket_session_credit_resize_display_and_detach() -> Result<()> {
    let (mut front, mut daemon) = pair(Role::Writer)?;
    let now = Instant::now();
    let limit = now + STALL;
    let mut output = [0; MAX_DATA];
    let input = [0x7e; MAX_DATA];
    assert_ne!(front.fd(), daemon.fd());
    assert!(!front.wants_write(true));
    front.wait_for_input_progress(true, now);
    assert_eq!(front.deadline(), Some(limit));
    assert!(daemon.wants_write()?);
    daemon.flush(now, limit)?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Pending);
    assert_eq!(front.deadline(), None);

    for _ in 0..4 {
        assert_eq!(front.write(&input, now)?, MAX_DATA);
        assert_eq!(daemon.read(now)?, ServerRead::Received);
    }
    assert!(!front.wants_write(true));
    front.submit_resize((24, 80))?;
    assert_eq!(front.write(&input, now)?, 0);
    assert_eq!(daemon.read(now)?, ServerRead::Received);
    front.submit_resize((35, 90))?;
    front.submit_resize((40, 100))?;

    assert_eq!(daemon.offer(5, b"abc", now, limit)?, None);
    assert!(daemon.wants_write()?);
    daemon.flush(now, limit)?;
    assert!(!front.buffered());
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Bytes(3));
    assert_eq!(&output[..3], b"abc");
    assert!(!front.wants_read());
    assert!(!front.wants_write(true));
    assert_eq!(daemon.offer(5, b"abc", now, limit)?, None);

    front.display_committed()?;
    front.begin_detach(now);
    assert!(!front.detach_complete());
    assert!(front.wants_write(true));
    assert_eq!(front.write(&input, now)?, 0);
    assert_eq!(daemon.read(now)?, ServerRead::Received);
    assert_eq!(daemon.offer(5, b"abc", now, limit)?, Some(3));
    assert_eq!(front.write(&input, now)?, 0);
    assert!(front.detach_complete());
    assert_eq!(daemon.read(now)?, ServerRead::Detached);
    assert!(!daemon.buffered());

    for _ in 0..4 {
        assert!(matches!(daemon.next_input(), Some(InputEffect::Bytes(bytes)) if bytes == input));
        daemon.input_written(MAX_DATA)?;
    }
    assert!(matches!(
        daemon.next_input(),
        Some(InputEffect::Resize((24, 80)))
    ));
    daemon.resize_applied()?;
    daemon.begin_close(Close::Ended);
    assert_eq!(daemon.deadline(), None);
    assert!(daemon.next_input().is_none());
    daemon.flush(now, limit)?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Pending);
    assert_eq!(
        front.read(&mut output, now)?,
        FrontendOutput::Closed(Close::Ended)
    );
    assert!(!daemon.wants_write()?);
    Ok(())
}

#[test]
fn terminal_socket_session_resize_receipt_releases_latest_geometry() -> Result<()> {
    let (mut front, mut daemon) = pair(Role::Writer)?;
    let now = Instant::now();
    let limit = now + STALL;
    let mut output = [0; MAX_DATA];
    front.submit_resize((24, 80))?;
    assert_eq!(front.write(&[], now)?, 0);
    assert_eq!(daemon.read(now)?, ServerRead::Received);
    front.submit_resize((30, 90))?;
    front.submit_resize((40, 100))?;
    assert!(!front.wants_write(false));
    daemon.resize_applied()?;
    daemon.flush(now, limit)?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Pending);
    assert!(front.wants_write(false));
    assert_eq!(front.write(&[], now)?, 0);
    assert_eq!(daemon.read(now)?, ServerRead::Received);
    assert!(matches!(
        daemon.next_input(),
        Some(InputEffect::Resize((40, 100)))
    ));
    Ok(())
}

#[test]
fn terminal_socket_session_observer_and_absolute_display_deadline() -> Result<()> {
    let (mut front, mut daemon) = pair(Role::Observer)?;
    let now = Instant::now();
    let mut output = [0; MAX_DATA];
    assert!(front.submit_resize((24, 80)).is_err());
    assert!(!front.wants_write(true));
    assert!(!daemon.wants_write()?);
    assert_eq!(daemon.offer(1, b"a", now, now + STALL)?, None);
    daemon.flush(now, now + STALL)?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Bytes(1));
    assert_eq!(front.deadline(), Some(now + STALL));
    let earlier = now + Duration::from_secs(1);
    assert_eq!(daemon.offer(1, b"a", now, earlier)?, None);
    assert_eq!(daemon.deadline(), Some(earlier));
    assert!(matches!(
        daemon.offer(1, b"a", earlier, now + STALL),
        Err(Error::Timeout)
    ));
    assert!(matches!(
        front.check_deadline(now + STALL),
        Err(Error::Timeout)
    ));
    Ok(())
}

#[test]
fn terminal_socket_session_typed_eof_waits_for_the_last_display_ack() -> Result<()> {
    let (mut front, mut daemon) = pair(Role::Observer)?;
    let now = Instant::now();
    let limit = now + STALL;
    let mut output = [0; MAX_DATA];
    assert_eq!(daemon.offer(1, b"last", now, limit)?, None);
    daemon.flush(now, limit)?;
    daemon.begin_close(Close::Ended);
    daemon.flush(now, limit)?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Bytes(4));
    assert_eq!(&output[..4], b"last");
    front.display_committed()?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Pending);
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Pending);
    assert!(!front.wants_read());
    assert!(front.wants_write(false));
    assert_eq!(front.write(&[], now)?, 0);
    assert_eq!(daemon.read(now)?, ServerRead::Received);
    assert!(front.wants_read());
    assert!(front.buffered());
    assert_eq!(
        front.read(&mut output, now)?,
        FrontendOutput::Closed(Close::Ended)
    );
    Ok(())
}

#[test]
fn terminal_socket_session_detach_ignores_late_display_completion() -> Result<()> {
    let (mut front, mut daemon) = pair(Role::Writer)?;
    let now = Instant::now();
    let limit = now + STALL;
    let mut output = [0; MAX_DATA];
    daemon.flush(now, limit)?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Pending);
    assert_eq!(daemon.offer(1, b"last", now, limit)?, None);
    daemon.flush(now, limit)?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Bytes(4));
    front.begin_detach(now);
    front.display_committed()?;
    front.submit_resize((28, 80))?;
    assert_eq!(front.write(&[], now)?, 0);
    assert!(front.detach_complete());
    assert_eq!(daemon.read(now)?, ServerRead::Detached);
    assert_eq!(daemon.offer(1, b"last", now, limit)?, None);
    Ok(())
}

#[test]
fn terminal_socket_session_resize_during_orderly_close_is_ignored() -> Result<()> {
    let (mut front, mut daemon) = pair(Role::Writer)?;
    let now = Instant::now();
    let limit = now + STALL;
    let mut output = [0; MAX_DATA];
    daemon.begin_close(Close::Ended);
    daemon.flush(now, limit)?;
    assert_eq!(front.read(&mut output, now)?, FrontendOutput::Pending);
    front.submit_resize((35, 80))?;
    assert!(!front.wants_write(false));
    assert_eq!(
        front.read(&mut output, now)?,
        FrontendOutput::Closed(Close::Ended)
    );
    Ok(())
}

#[test]
fn terminal_socket_session_rejects_input_and_resize_after_close() -> Result<()> {
    for record in [
        Record::Input(b"too late"),
        Record::Resize {
            rows: 25,
            columns: 80,
        },
    ] {
        let (mut peer, socket) = UnixStream::pair()?;
        socket.set_nonblocking(true)?;
        let mut daemon = DaemonSocket::new(socket, Role::Writer);
        let now = Instant::now();
        let limit = now + STALL;
        daemon.flush(now, limit)?;
        daemon.begin_close(Close::Ended);
        daemon.flush(now, limit)?;
        assert!(!daemon.wants_write()?);

        let mut input = encode(Direction::ToDaemon, Role::Writer, record).unwrap();
        input.extend(encode(Direction::ToDaemon, Role::Writer, Record::Detach).unwrap());
        peer.write_all(&input)?;
        assert!(matches!(
            daemon.read(now),
            Err(Error::Invalid("terminal_local_channel"))
        ));
        assert!(daemon.input.as_ref().unwrap().next().is_none());
        assert!(matches!(
            daemon.read(now),
            Err(Error::Invalid("terminal_local_channel"))
        ));
    }
    Ok(())
}

#[test]
fn terminal_socket_session_accepts_in_flight_records_until_close_is_sent() -> Result<()> {
    let (mut peer, socket) = UnixStream::pair()?;
    socket.set_nonblocking(true)?;
    let mut daemon = DaemonSocket::new(socket, Role::Writer);
    let now = Instant::now();
    let limit = now + STALL;
    daemon.flush(now, limit)?;
    daemon.begin_close(Close::Ended);
    for record in [
        Record::Input(b"in flight"),
        Record::Resize {
            rows: 25,
            columns: 80,
        },
    ] {
        peer.write_all(&encode(Direction::ToDaemon, Role::Writer, record).unwrap())?;
        assert_eq!(daemon.read(now)?, ServerRead::Received);
        assert!(daemon.next_input().is_none());
    }
    daemon.flush(now, limit)?;
    peer.write_all(&encode(Direction::ToDaemon, Role::Writer, Record::Detach).unwrap())?;
    assert_eq!(daemon.read(now)?, ServerRead::Detached);
    Ok(())
}

struct ShortSocket(UnixStream);

impl Read for ShortSocket {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}

impl Write for ShortSocket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(&bytes[..bytes.len().min(1)])
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl AsRawFd for ShortSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl ReadWrite for ShortSocket {
    fn finish_write(&mut self) -> Result<()> {
        self.0.finish_write()
    }
}

#[test]
fn terminal_socket_session_detach_preserves_only_partly_written_output() -> Result<()> {
    for offered in [None, Some(false), Some(true)] {
        let (mut peer, socket) = UnixStream::pair()?;
        socket.set_nonblocking(true)?;
        peer.set_read_timeout(Some(STALL))?;
        let mut daemon = DaemonSocket::new(ShortSocket(socket), Role::Observer);
        let now = Instant::now();
        let limit = now + STALL;
        let mut expected = Vec::new();
        if let Some(partial) = offered {
            assert_eq!(daemon.offer(1, b"head", now, limit)?, None);
            if partial {
                daemon.flush(now, limit)?;
                expected.extend(
                    encode(
                        Direction::ToClient,
                        Role::Observer,
                        Record::Output {
                            id: NonZeroU64::MIN,
                            bytes: b"head",
                        },
                    )
                    .unwrap(),
                );
            }
        }

        peer.write_all(&encode(Direction::ToDaemon, Role::Observer, Record::Detach).unwrap())?;
        assert_eq!(daemon.read(now)?, ServerRead::Detached);
        assert_eq!(daemon.offer(2, b"unsolicited", now, limit)?, None);
        assert_eq!(daemon.wants_write()?, offered == Some(true));
        daemon.begin_close(Close::Ended);
        expected.extend(
            encode(
                Direction::ToClient,
                Role::Observer,
                Record::Close(Close::Ended),
            )
            .unwrap(),
        );
        for _ in 0..expected.len() {
            daemon.flush(now, limit)?;
        }
        assert!(!daemon.wants_write()?);
        let mut actual = Vec::new();
        peer.read_to_end(&mut actual)?;
        assert_eq!(actual, expected);
    }
    Ok(())
}

#[test]
#[ignore = "local framed socket turn timing"]
fn terminal_socket_session_bench() -> Result<()> {
    let (mut front, mut daemon) = pair(Role::Observer)?;
    let mut output = [0; MAX_DATA];
    let bytes = [b'a'; MAX_DATA];
    let mut samples = Vec::with_capacity(1000);
    for sequence in 1..=1100 {
        let started = Instant::now();
        let limit = started + STALL;
        assert_eq!(daemon.offer(sequence, &bytes, started, limit)?, None);
        daemon.flush(Instant::now(), limit)?;
        assert_eq!(
            front.read(&mut output, Instant::now())?,
            FrontendOutput::Bytes(MAX_DATA)
        );
        assert_eq!(output, bytes);
        front.display_committed()?;
        assert_eq!(front.write(&[], Instant::now())?, 0);
        assert_eq!(daemon.read(Instant::now())?, ServerRead::Received);
        assert_eq!(
            daemon.offer(sequence, &bytes, Instant::now(), limit)?,
            Some(MAX_DATA)
        );
        if sequence > 100 {
            samples.push(started.elapsed().as_nanos());
        }
    }
    samples.sort_unstable();
    eprintln!(
        "4096 byte socket display receipt turn n=1000 p50={}ns p95={}ns",
        samples[499], samples[949]
    );
    Ok(())
}
