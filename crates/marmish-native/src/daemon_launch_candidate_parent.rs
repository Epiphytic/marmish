//! Private parent protocol and child ownership for registered qualification.
use super::*;

#[must_use]
pub(crate) struct RegisteredLaunch {
    pub(crate) child: Child,
    pub(crate) report: Result<()>,
}

#[allow(dead_code, reason = "registered qualification mounts independently")]
pub(crate) fn launch(executable: &Path, argv: &[OsString], log: File) -> Result<RegisteredLaunch> {
    let (mut guard, channel) =
        spawn_child_with_marker(executable, argv, log, CLEANUP_GRACE, None, PRIVATE_MARKER)?;
    let child = guard
        .child
        .take()
        .ok_or_else(|| failure("daemon child unavailable"))?;
    Ok(RegisteredLaunch {
        child,
        report: read_report(channel, Instant::now() + STARTUP_TIMEOUT),
    })
}

pub(crate) fn spawn_fault_fixture(
    executable: &Path,
    argv: &[OsString],
    log: File,
    control: OwnedFd,
) -> Result<(Child, UnixStream)> {
    let (mut guard, channel) = spawn_child_with_stdio(
        executable,
        argv,
        log,
        CLEANUP_GRACE,
        None,
        None,
        Stdio::from(control),
    )?;
    let child = guard
        .child
        .take()
        .ok_or_else(|| failure("daemon child unavailable"))?;
    Ok((child, channel))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReportEdge {
    Ready,
    Ack,
    Commit,
    PostAckClosed,
}

pub(crate) fn observe_fault_report(
    channel: UnixStream,
    end: Instant,
    observe: impl FnMut(ReportEdge),
) -> Result<()> {
    read_report_observed(channel, end, observe)
}

fn read_report(channel: UnixStream, end: Instant) -> Result<()> {
    read_report_observed(channel, end, |_| {})
}

fn read_report_observed(
    mut channel: UnixStream,
    end: Instant,
    mut observe: impl FnMut(ReportEdge),
) -> Result<()> {
    match read_byte(&mut channel, end)? {
        b'R' => {
            observe(ReportEdge::Ready);
            let remaining = end
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| failure("daemon startup timed out; see daemon.log"))?;
            channel.set_write_timeout(Some(remaining.min(ACK_TIMEOUT)))?;
            channel.write_all(b"A")?;
            observe(ReportEdge::Ack);
            match read_byte_io(&mut channel, end) {
                Ok(b'C') => {
                    observe(ReportEdge::Commit);
                    Ok(())
                }
                Ok(_) => Err(failure("daemon startup report invalid")),
                Err(error) => {
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                    ) {
                        observe(ReportEdge::PostAckClosed);
                    }
                    Err(unavailable())
                }
            }
        }
        b'F' => {
            let mut length = [0; 2];
            read_before(&mut channel, &mut length, end)?;
            let length = usize::from(u16::from_be_bytes(length));
            if length == 0 || length > MESSAGE_LIMIT {
                return Err(failure("daemon startup report invalid"));
            }
            let mut text = vec![0; length];
            read_before(&mut channel, &mut text, end)?;
            if !valid_message(&text) {
                return Err(failure("daemon startup report invalid"));
            }
            let text =
                String::from_utf8(text).map_err(|_| failure("daemon startup report invalid"))?;
            Err(Error::DaemonStartup(text))
        }
        _ => Err(failure("daemon startup report invalid")),
    }
}

fn read_byte(channel: &mut UnixStream, end: Instant) -> Result<u8> {
    read_byte_io(channel, end).map_err(|error| {
        if error.kind() == std::io::ErrorKind::TimedOut {
            failure("daemon startup timed out; see daemon.log")
        } else {
            unavailable()
        }
    })
}

fn unavailable() -> Error {
    failure("daemon startup unavailable or timed out; see daemon.log")
}

fn read_byte_io(channel: &mut UnixStream, end: Instant) -> std::io::Result<u8> {
    let mut byte = [0];
    read_before(channel, &mut byte, end)?;
    if Instant::now() >= end {
        return Err(std::io::ErrorKind::TimedOut.into());
    }
    Ok(byte[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn provisional_ready_must_be_followed_by_commit() {
        for final_byte in [Some(b'C'), Some(b'F'), Some(b'Z'), None] {
            let (parent, mut child) = UnixStream::pair().unwrap();
            let writer = std::thread::spawn(move || {
                child.write_all(b"R").unwrap();
                let mut ack = [0];
                child.read_exact(&mut ack).unwrap();
                assert_eq!(ack, *b"A");
                if let Some(byte) = final_byte {
                    child.write_all(&[byte]).unwrap();
                }
            });
            let report = read_report(parent, Instant::now() + Duration::from_secs(1));
            assert_eq!(report.is_ok(), final_byte == Some(b'C'));
            writer.join().unwrap();
        }
    }

    #[test]
    fn accepts_bounded_failure_only_before_ready() {
        let (parent, mut child) = UnixStream::pair().unwrap();
        child.write_all(b"F\x00\x04busy").unwrap();
        assert!(matches!(
            read_report(parent, Instant::now() + Duration::from_secs(1)),
            Err(Error::DaemonStartup(text)) if text == "busy"
        ));
    }

    #[test]
    fn expired_parent_drops_the_channel_with_commit_buffered() {
        let (mut parent, mut child) = UnixStream::pair().unwrap();
        let begin = Instant::now();
        child.write_all(b"C").unwrap();
        assert!(read_byte(&mut parent, begin).is_err());
        drop(parent);
        let mut bytes = [0];
        match child.read(&mut bytes) {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            other => panic!("unexpected report channel result: {other:?}"),
        }
    }

    #[test]
    fn failure_report_returns_the_still_running_child_to_the_harness() {
        let args = [
            OsString::from("-c"),
            OsString::from("printf 'F\\000\\004busy' >&3; exec /bin/sleep 30"),
        ];
        let mut launched =
            launch(Path::new("/bin/sh"), &args, tempfile::tempfile().unwrap()).unwrap();
        let running = launched.child.try_wait().unwrap().is_none();
        if running {
            launched.child.kill().unwrap();
            launched.child.wait().unwrap();
        }
        assert!(running);
        assert!(matches!(
            launched.report,
            Err(Error::DaemonStartup(text)) if text == "busy"
        ));
    }
}
