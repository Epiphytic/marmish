//! The sole child-side readiness channel for registered qualification startup.

use super::{ACK_TIMEOUT, StartupReporter, failure};
use crate::{
    Result,
    command_engine::Admission,
    dispatcher_fatal::{Fatal, FencedStream},
};
use std::{
    future::{Future, poll_fn},
    io::{self, Write},
    os::unix::net::UnixStream as StdUnixStream,
    pin::{Pin, pin},
    task::{Context, Poll, ready},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::UnixStream,
    time::sleep,
};

pub(crate) struct ChildChannel(StdUnixStream);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Interrupt {
    Shutdown,
    Expired,
    ListenerExited,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    Fatal,
    Interrupted(Interrupt),
    TimedOut,
    Admission,
    Acknowledgement,
    ParentEof,
    Socket(io::ErrorKind),
}

#[derive(Debug)]
#[must_use]
pub(crate) struct Committed {
    _written: (),
}

#[derive(Clone, Copy)]
enum Stage {
    Offer,
    Acknowledge,
    Commit,
}

impl ChildChannel {
    /// Takes the stream before the registered owner can perform startup I/O.
    /// Every retained reporter clone observes the same empty slot.
    pub(crate) fn take(reporter: &StartupReporter) -> Result<Self> {
        let stream = reporter
            .take()?
            .ok_or_else(|| failure("daemon startup already reported"))?;
        stream.set_nonblocking(true)?;
        Ok(Self(stream))
    }

    /// May emit a fixed diagnostic before entering the exchange. The caller
    /// gives up the channel even when the single nonblocking write fails.
    pub(crate) fn fail_before_offer(mut self, admission: &Admission) {
        if admission.admit() {
            let _ = self.0.write(b"F\x00\x15daemon startup failed");
        }
    }

    /// The caller provides Admission and Fatal from the same registered
    /// custody. The interrupt gate polls and retains external stop evidence
    /// in shutdown, lifetime, then listener order, registering this waker.
    /// It runs again after A and before every resumed write attempt.
    pub(crate) async fn exchange(
        self,
        admission: &Admission,
        fatal: &Fatal,
        mut interrupt: impl FnMut(&mut Context<'_>) -> Option<Interrupt>,
    ) -> std::result::Result<Committed, Failure> {
        let mut timeout = pin!(sleep(ACK_TIMEOUT));
        let mut stream =
            UnixStream::from_std(self.0).map_err(|error| socket_error(fatal, error))?;
        let mut io = FencedStream::new(&mut stream, fatal);
        let mut stage = Stage::Offer;

        poll_fn(|cx| {
            loop {
                if fatal.is_tripped() {
                    return Poll::Ready(Err(Failure::Fatal));
                }
                if let Some(cut) = interrupt(cx) {
                    return Poll::Ready(Err(Failure::Interrupted(cut)));
                }
                if timeout.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(Err(Failure::TimedOut));
                }

                match stage {
                    Stage::Offer => {
                        ready!(poll_byte(&mut io, cx, admission, fatal, b"R"))?;
                        stage = Stage::Acknowledge;
                    }
                    Stage::Acknowledge => {
                        let mut byte = [0];
                        let mut read = ReadBuf::new(&mut byte);
                        ready!(Pin::new(&mut io).poll_read(cx, &mut read))
                            .map_err(|error| socket_error(fatal, error))?;
                        match read.filled() {
                            b"A" => {
                                #[cfg(all(
                                    test,
                                    marmish_peer_receiver_candidate,
                                    target_os = "linux",
                                    target_arch = "x86_64"
                                ))]
                                crate::daemon::registered::registered_loss::before_commit(
                                    admission,
                                    timeout.as_ref().deadline(),
                                );
                                stage = Stage::Commit;
                            }
                            [] => return Poll::Ready(Err(Failure::ParentEof)),
                            _ => return Poll::Ready(Err(Failure::Acknowledgement)),
                        }
                    }
                    Stage::Commit => {
                        ready!(poll_byte(&mut io, cx, admission, fatal, b"C"))?;
                        return Poll::Ready(Ok(Committed { _written: () }));
                    }
                }
            }
        })
        .await
    }
}

fn poll_byte(
    io: &mut FencedStream<&mut UnixStream>,
    cx: &mut Context<'_>,
    admission: &Admission,
    fatal: &Fatal,
    byte: &[u8; 1],
) -> Poll<std::result::Result<(), Failure>> {
    if !admission.admit() {
        return Poll::Ready(Err(if fatal.is_tripped() {
            Failure::Fatal
        } else {
            Failure::Admission
        }));
    }

    Poll::Ready(match ready!(Pin::new(io).poll_write(cx, byte)) {
        Ok(1) => Ok(()),
        Ok(_) => Err(Failure::Socket(io::ErrorKind::WriteZero)),
        Err(error) => Err(socket_error(fatal, error)),
    })
}

fn socket_error(fatal: &Fatal, error: io::Error) -> Failure {
    if fatal.is_tripped() {
        Failure::Fatal
    } else {
        Failure::Socket(error.kind())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher_fatal::Trip;
    use std::{
        os::fd::AsRawFd,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_util::sync::CancellationToken;

    const TEST_DEADLINE: Duration = Duration::from_secs(2);

    async fn expect_byte(parent: &mut UnixStream, expected: u8) {
        assert_eq!(
            tokio::time::timeout(TEST_DEADLINE, parent.read_u8())
                .await
                .unwrap()
                .unwrap(),
            expected
        );
    }

    async fn expect_eof(parent: &mut UnixStream) {
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(TEST_DEADLINE, parent.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }

    fn admission() -> (Arc<Trip>, Fatal, Admission) {
        let (trip, fatal) = Trip::new();
        let trip = Arc::new(trip);
        let admission = Admission::new(&trip, || true);
        (trip, fatal, admission)
    }

    #[tokio::test]
    async fn delayed_ack_commits_once_after_clones_lose_the_channel() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let retained = reporter.clone();
        let channel = ChildChannel::take(&reporter).unwrap();
        assert!(ChildChannel::take(&retained).is_err());
        retained.failed(&crate::Error::Invalid("candidate_test"));
        assert!(retained.ready().await.is_err());

        let (trip, fatal, admission) = admission();
        let child =
            tokio::spawn(async move { channel.exchange(&admission, &fatal, |_| None).await });
        expect_byte(&mut parent, b'R').await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), parent.read_u8())
                .await
                .is_err()
        );
        parent.write_all(b"A").await.unwrap();
        expect_byte(&mut parent, b'C').await;
        trip.trip();
        let _committed = child.await.unwrap().unwrap();
        expect_eof(&mut parent).await;
    }

    #[tokio::test]
    async fn fatal_has_priority_before_offer() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let channel = ChildChannel::take(&reporter).unwrap();
        let (trip, fatal, admission) = admission();
        trip.trip();
        let result = channel
            .exchange(&admission, &fatal, |_| Some(Interrupt::Shutdown))
            .await;
        assert!(matches!(result, Err(Failure::Fatal)));
        expect_eof(&mut parent).await;
    }

    #[tokio::test]
    async fn fatal_during_commit_admission_revokes_the_channel() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let channel = ChildChannel::take(&reporter).unwrap();
        let (trip, fatal, _) = admission();
        let ack_sent = Arc::new(AtomicBool::new(false));
        let commit_attempts = Arc::new(AtomicUsize::new(0));
        let admission = Admission::new(&trip, {
            let trip = trip.clone();
            let ack_sent = ack_sent.clone();
            let commit_attempts = commit_attempts.clone();
            move || {
                if ack_sent.load(Ordering::Relaxed) {
                    commit_attempts.fetch_add(1, Ordering::Relaxed);
                    trip.trip();
                }
                true
            }
        });

        let child = channel.exchange(&admission, &fatal, |_| None);
        let parent_exchange = async {
            expect_byte(&mut parent, b'R').await;
            parent.write_all(b"A").await.unwrap();
            // This branch and the exchange share a task. No commit attempt
            // can intervene between the successful send and this flag.
            ack_sent.store(true, Ordering::Relaxed);
            expect_eof(&mut parent).await;
        };
        let (result, ()) = tokio::join!(child, parent_exchange);
        assert!(matches!(result, Err(Failure::Fatal)));
        assert_eq!(commit_attempts.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn external_interrupt_wakes_while_waiting_for_ack() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let channel = ChildChannel::take(&reporter).unwrap();
        let (_, fatal, admission) = admission();
        let listener = CancellationToken::new();
        let mut cancelled = pin!(listener.cancelled());
        let child = channel.exchange(&admission, &fatal, |cx| {
            cancelled
                .as_mut()
                .poll(cx)
                .is_ready()
                .then_some(Interrupt::ListenerExited)
        });
        let parent_exchange = async {
            expect_byte(&mut parent, b'R').await;
            listener.cancel();
            expect_eof(&mut parent).await;
        };
        let (result, ()) = tokio::join!(child, parent_exchange);
        assert!(matches!(
            result,
            Err(Failure::Interrupted(Interrupt::ListenerExited))
        ));
    }

    #[tokio::test]
    async fn gate_expiry_after_ack_before_commit_admission() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let channel = ChildChannel::take(&reporter).unwrap();
        let child_fd = channel.0.as_raw_fd();
        let (trip, fatal, _) = admission();
        let ack_sent = AtomicBool::new(false);
        let attempts = Arc::new(AtomicUsize::new(0));
        let admission = Admission::new(&trip, {
            let attempts = attempts.clone();
            move || {
                attempts.fetch_add(1, Ordering::Relaxed);
                true
            }
        });

        let child = channel.exchange(&admission, &fatal, |_| {
            if !ack_sent.load(Ordering::Relaxed) {
                return None;
            }

            // SAFETY: the exchange still owns this fd while invoking its gate.
            // Peek at one socket byte without consuming it or blocking.
            let mut byte = 0u8;
            let count = unsafe {
                libc::recv(
                    child_fd,
                    (&mut byte as *mut u8).cast(),
                    1,
                    libc::MSG_DONTWAIT | libc::MSG_PEEK,
                )
            };
            if count == 1 {
                assert_eq!(byte, b'A');
                return None;
            }
            assert_eq!(count, -1);
            assert_eq!(io::Error::last_os_error().kind(), io::ErrorKind::WouldBlock);
            Some(Interrupt::Expired)
        });
        let parent_exchange = async {
            expect_byte(&mut parent, b'R').await;
            let offer_attempts = attempts.load(Ordering::Relaxed);
            parent.write_all(b"A").await.unwrap();
            ack_sent.store(true, Ordering::Relaxed);
            expect_eof(&mut parent).await;
            offer_attempts
        };
        let (result, offer_attempts) = tokio::join!(child, parent_exchange);
        assert!(matches!(
            result,
            Err(Failure::Interrupted(Interrupt::Expired))
        ));
        assert_ne!(offer_attempts, 0);
        assert_eq!(attempts.load(Ordering::Relaxed), offer_attempts);
    }

    #[tokio::test]
    async fn nonfatal_denial_blocks_offer_commit_and_failure_frame() {
        let (trip, fatal, _) = admission();
        let denied = Admission::new(&trip, || false);
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let channel = ChildChannel::take(&reporter).unwrap();
        let result = channel.exchange(&denied, &fatal, |_| None).await;
        assert!(matches!(result, Err(Failure::Admission)));
        expect_eof(&mut parent).await;

        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        ChildChannel::take(&reporter)
            .unwrap()
            .fail_before_offer(&denied);
        expect_eof(&mut parent).await;

        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let channel = ChildChannel::take(&reporter).unwrap();
        let ack_sent = Arc::new(AtomicBool::new(false));
        let switch = ack_sent.clone();
        let admission = Admission::new(&trip, move || !switch.load(Ordering::Relaxed));
        let child = channel.exchange(&admission, &fatal, |_| None);
        let parent_exchange = async {
            expect_byte(&mut parent, b'R').await;
            parent.write_all(b"A").await.unwrap();
            ack_sent.store(true, Ordering::Relaxed);
            expect_eof(&mut parent).await;
        };
        let (result, ()) = tokio::join!(child, parent_exchange);
        assert!(matches!(result, Err(Failure::Admission)));
    }

    #[tokio::test]
    async fn malformed_ack_and_closed_parent_cannot_commit() {
        for (ack, expected) in [
            (Some(b'C'), Failure::Acknowledgement),
            (None, Failure::ParentEof),
        ] {
            let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
            let channel = ChildChannel::take(&reporter).unwrap();
            let (_, fatal, admission) = admission();
            let child = channel.exchange(&admission, &fatal, |_| None);
            let parent_exchange = async {
                expect_byte(&mut parent, b'R').await;
                if let Some(ack) = ack {
                    parent.write_u8(ack).await.unwrap();
                } else {
                    parent.shutdown().await.unwrap();
                }
                expect_eof(&mut parent).await;
            };
            let (result, ()) = tokio::join!(child, parent_exchange);
            assert_eq!(result.unwrap_err(), expected);
        }
    }

    #[tokio::test]
    async fn before_offer_failure_is_fixed_and_consumes_the_channel() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let channel = ChildChannel::take(&reporter).unwrap();
        let (_, _, admission) = admission();
        channel.fail_before_offer(&admission);
        let mut frame = [0; 24];
        tokio::time::timeout(TEST_DEADLINE, parent.read_exact(&mut frame))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&frame, b"F\x00\x15daemon startup failed");
        expect_eof(&mut parent).await;
    }

    fn backpressure(channel: &mut ChildChannel) -> usize {
        let mut sent = 0;
        loop {
            match channel.0.write(&[b'x'; 8192]) {
                Ok(written) => {
                    assert_ne!(written, 0);
                    sent += written;
                    assert!(sent < 4 * 1024 * 1024);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("could not fill fixture socket: {error}"),
            }
        }
        // Prove the exact one-byte write is also backpressured.
        loop {
            match channel.0.write(b"x") {
                Ok(1) => {
                    sent += 1;
                    assert!(sent < 4 * 1024 * 1024);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return sent,
                result => panic!("unexpected one-byte fixture write: {result:?}"),
            }
        }
    }

    async fn expect_only_backlog(parent: &mut UnixStream, sent: usize) {
        let mut received = Vec::new();
        tokio::time::timeout(TEST_DEADLINE, parent.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received.len(), sent);
        assert!(received.iter().all(|byte| *byte == b'x'));
    }

    #[tokio::test]
    async fn pending_offer_write_wakes_on_fatal_without_parent_reading() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let mut channel = ChildChannel::take(&reporter).unwrap();
        let sent = backpressure(&mut channel);

        let (trip, fatal, _) = admission();
        let attempted = CancellationToken::new();
        let receipt = attempted.clone();
        let admission = Admission::new(&trip, move || {
            receipt.cancel();
            true
        });
        let child =
            tokio::spawn(async move { channel.exchange(&admission, &fatal, |_| None).await });
        tokio::time::timeout(TEST_DEADLINE, attempted.cancelled())
            .await
            .unwrap();
        assert!(!child.is_finished());
        trip.trip();
        let result = tokio::time::timeout(TEST_DEADLINE, child)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(Failure::Fatal)));

        expect_only_backlog(&mut parent, sent).await;
    }

    #[tokio::test]
    async fn resumed_pending_offer_needs_fresh_admission() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let mut channel = ChildChannel::take(&reporter).unwrap();
        let sent = backpressure(&mut channel);
        let (trip, fatal, _) = admission();
        let attempts = Arc::new(AtomicUsize::new(0));
        let admission = Admission::new(&trip, {
            let attempts = attempts.clone();
            move || attempts.fetch_add(1, Ordering::Relaxed) == 0
        });

        let mut child = pin!(channel.exchange(&admission, &fatal, |_| None));
        let result = poll_fn(|cx| {
            assert!(child.as_mut().poll(cx).is_pending());
            assert_eq!(attempts.load(Ordering::Relaxed), 1);
            // Force a resume while the actual socket is still backpressured.
            // The resumed poll must consult Admission before retrying I/O.
            Poll::Ready(child.as_mut().poll(cx))
        })
        .await;
        assert!(matches!(result, Poll::Ready(Err(Failure::Admission))));
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        expect_only_backlog(&mut parent, sent).await;
    }

    #[tokio::test]
    async fn missing_ack_expires_one_fixed_exchange_budget() {
        let (reporter, mut parent) = StartupReporter::test_channel().unwrap();
        let channel = ChildChannel::take(&reporter).unwrap();
        let (_, fatal, admission) = admission();
        let child =
            tokio::spawn(async move { channel.exchange(&admission, &fatal, |_| None).await });
        expect_byte(&mut parent, b'R').await;
        let result = tokio::time::timeout(ACK_TIMEOUT + TEST_DEADLINE, child)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(Failure::TimedOut)));
        expect_eof(&mut parent).await;
    }
}
