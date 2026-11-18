use super::*;
use crate::terminal_ipc_wire::Close;
use std::os::unix::net::UnixStream;

#[test]
fn terminal_socket_io_core_suffix_order_and_typed_eof() -> Result<()> {
    let (mut owner, mut peer) = UnixStream::pair()?;
    owner.set_nonblocking(true)?;
    let now = Instant::now();
    let mut reader = Reader::new(Direction::ToClient, Role::Writer);
    assert_eq!(
        reader.read(&mut owner, now, |_| panic!("nothing queued"))?,
        ReadEvent::Pending
    );

    let flow = [0, 2, 1, 8, 0, 3, 1, 7, Close::Ended as u8];
    peer.write_all(&flow)?;
    peer.shutdown(std::net::Shutdown::Write)?;
    assert_eq!(
        reader.read(&mut owner, now, |record| {
            assert_eq!(record, Record::ResizeApplied);
            Ok(())
        })?,
        ReadEvent::Record
    );
    assert!(reader.has_buffered());
    assert_eq!(
        reader.read(&mut owner, now, |record| {
            assert_eq!(record, Record::Close(Close::Ended));
            Ok(())
        })?,
        ReadEvent::Record
    );
    assert!(!reader.has_buffered());
    assert_eq!(
        reader.read(&mut owner, now, |_| panic!("expected EOF"))?,
        ReadEvent::Eof(Eof::Ended)
    );

    Ok(())
}

#[test]
fn terminal_socket_io_core_truncated_timeout_and_surplus() -> Result<()> {
    let t = Instant::now();
    for expiry in [false, true] {
        let (mut owner, mut peer) = UnixStream::pair()?;
        owner.set_nonblocking(true)?;
        peer.write_all(&[0, 3, 1])?;
        let mut reader = Reader::new(Direction::ToClient, Role::Writer);
        assert_eq!(
            reader.read(&mut owner, t, |_| panic!("incomplete"))?,
            ReadEvent::Pending
        );
        assert_eq!(reader.deadline(), Some(t + STALL));
        let time = if expiry {
            t + STALL
        } else {
            peer.shutdown(std::net::Shutdown::Write)?;
            t
        };
        let actual = reader.read(&mut owner, time, |_| panic!("incomplete"));
        assert!(matches!(
            (expiry, actual),
            (true, Err(Error::Timeout)) | (false, Err(Error::Invalid("terminal_local_record")))
        ));
    }

    let mut reader = Reader::new(Direction::ToClient, Role::Writer);
    let mut after_close = [0, 3, 1, 7, Close::Ended as u8, 0, 2, 1, 8].as_slice();
    assert_eq!(
        reader.read(&mut after_close, t, |_| Ok(()))?,
        ReadEvent::Record
    );
    assert!(matches!(
        reader.read(&mut after_close, t, |_| panic!("after Close")),
        Err(Error::Invalid("terminal_local_record"))
    ));
    Ok(())
}

struct ShortWrites {
    calls: usize,
    all: Vec<u8>,
}

impl Write for ShortWrites {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.calls += 1;
        if self.calls == 2 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.all.push(bytes[0]);
        Ok(1)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn terminal_socket_io_core_immutable_frame_and_completion() -> Result<()> {
    let t = Instant::now();
    let mut writer = Writer::new(Direction::ToDaemon, Role::Writer);
    let mut stream = ShortWrites {
        calls: 0,
        all: Vec::new(),
    };
    writer.queue(Record::Input(b"\x00Q"), "consumed", t, t + STALL)?;
    assert_eq!(writer.flush(&mut stream, t)?, None);
    assert_eq!(writer.flush(&mut stream, t)?, None);
    assert!(
        writer
            .queue(Record::Detach, "detached", t, t + STALL)
            .is_err()
    );
    for _ in 1..5 {
        assert_eq!(writer.flush(&mut stream, t)?, None);
    }
    assert_eq!(writer.flush(&mut stream, t)?, Some("consumed"));
    assert_eq!(stream.all, [0, 4, 1, 1, 0, b'Q']);
    assert!(!writer.has_frame());

    writer.queue(Record::Detach, "detached", t, t + STALL)?;
    for _ in 0..3 {
        assert_eq!(writer.flush(&mut stream, t)?, None);
    }
    assert_eq!(writer.flush(&mut stream, t)?, Some("detached"));
    assert_eq!(&stream.all[6..], [0, 2, 1, 4]);
    assert!(
        writer
            .queue(Record::Detach, "duplicate", t, t + STALL)
            .is_err()
    );
    Ok(())
}

#[test]
fn terminal_socket_io_core_write_deadline_never_restarts() -> Result<()> {
    let t = Instant::now();
    let end = t + Duration::from_millis(20);
    let mut writer = Writer::new(Direction::ToDaemon, Role::Writer);
    let mut stream = ShortWrites {
        calls: 0,
        all: Vec::new(),
    };
    writer.queue(Record::Input(b"abc"), (), t, end)?;
    assert_eq!(writer.deadline(), Some(end));
    assert_eq!(writer.flush(&mut stream, t)?, None);
    assert_eq!(
        writer.flush(&mut stream, t + Duration::from_millis(19))?,
        None
    );
    assert!(matches!(
        writer.flush(&mut stream, end),
        Err(Error::Timeout)
    ));
    assert_eq!(stream.all, [0]);
    Ok(())
}
