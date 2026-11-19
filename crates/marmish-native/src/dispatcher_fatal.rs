//! Revocation for one registered candidate daemon lifetime.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

struct Signal {
    cut: AtomicBool,
    wake: CancellationToken,
}

#[derive(Clone)]
pub(crate) struct Fatal(Arc<Signal>);

/// Retained by the exact registration owner. Other owners only observe.
pub(crate) struct Trip(Arc<Signal>);

impl Trip {
    fn fresh() -> Self {
        Self(Arc::new(Signal {
            cut: AtomicBool::new(false),
            wake: CancellationToken::new(),
        }))
    }

    pub(crate) fn registered(
        _: crate::peer_quota_vfs::dispatch::candidate_registration::RegistryMint,
    ) -> Self {
        Self::fresh()
    }

    #[cfg(test)]
    pub(crate) fn new() -> (Self, Fatal) {
        let trip = Self::fresh();
        let fatal = trip.observe();
        (trip, fatal)
    }

    pub(crate) fn observe(&self) -> Fatal {
        Fatal(Arc::clone(&self.0))
    }

    pub(crate) fn mark_cut(&self) {
        self.0.cut.store(true, Ordering::Release);
    }

    pub(crate) fn notify(&self) {
        self.mark_cut();
        self.0.wake.cancel();
    }

    #[cfg(test)]
    pub(crate) fn trip(&self) {
        self.notify();
    }
}

impl Fatal {
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(crate) fn same_lifetime(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub(crate) fn is_tripped(&self) -> bool {
        self.0.cut.load(Ordering::Acquire)
    }

    pub(crate) async fn tripped(&self) {
        if !self.is_tripped() {
            self.0.wake.cancelled().await;
            assert!(self.is_tripped(), "fatal wake requires a cut");
        }
    }
}

/// Wrap the raw duplex stream, before TLS. This cuts off writes attempted
/// by reads inside the WebSocket or TLS implementation as well as the sink.
pub(crate) struct FencedStream<S> {
    io: S,
    // Field order keeps the actual route slot held until I/O destruction.
    _registered_slot: Option<Arc<OwnedSemaphorePermit>>,
    fatal: Fatal,
    read: Pin<Box<WaitForCancellationFutureOwned>>,
    write: Pin<Box<WaitForCancellationFutureOwned>>,
}

impl<S> FencedStream<S> {
    pub(crate) fn new(io: S, fatal: &Fatal) -> Self {
        Self {
            io,
            _registered_slot: None,
            fatal: fatal.clone(),
            read: Box::pin(fatal.0.wake.clone().cancelled_owned()),
            write: Box::pin(fatal.0.wake.clone().cancelled_owned()),
        }
    }

    pub(crate) fn with_slot(io: S, fatal: &Fatal, slot: &Arc<OwnedSemaphorePermit>) -> Self {
        Self {
            _registered_slot: Some(slot.clone()),
            ..Self::new(io, fatal)
        }
    }
}

fn open(
    fatal: &Fatal,
    waiting: Pin<&mut WaitForCancellationFutureOwned>,
    cx: &mut Context<'_>,
) -> io::Result<()> {
    // Register before polling the I/O. A permanently blocked socket must
    // still wake on fatal. The first check avoids repolling a completed future.
    if fatal.is_tripped() || waiting.poll(cx).is_ready() {
        return Err(io::ErrorKind::ConnectionAborted.into());
    }
    Ok(())
}

impl<S: AsyncRead + Unpin> AsyncRead for FencedStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        open(&this.fatal, this.read.as_mut(), cx)?;
        Pin::new(&mut this.io).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for FencedStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        open(&this.fatal, this.write.as_mut(), cx)?;
        Pin::new(&mut this.io).poll_write(cx, buf)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        open(&this.fatal, this.write.as_mut(), cx)?;
        Pin::new(&mut this.io).poll_write_vectored(cx, bufs)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        open(&this.fatal, this.write.as_mut(), cx)?;
        Pin::new(&mut this.io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        open(&this.fatal, this.write.as_mut(), cx)?;
        Pin::new(&mut this.io).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::poll_fn;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn registered_slot_outlives_raw_io_destructor() {
        struct ObserveDrop {
            slot: Arc<tokio::sync::Semaphore>,
            observed: Arc<std::sync::atomic::AtomicBool>,
        }

        impl Drop for ObserveDrop {
            fn drop(&mut self) {
                assert_eq!(self.slot.available_permits(), 0);
                self.observed
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }

        let (_, fatal) = Trip::new();
        let slot = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = Arc::new(slot.clone().try_acquire_owned().unwrap());
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let io = ObserveDrop {
            slot: slot.clone(),
            observed: observed.clone(),
        };
        let stream = FencedStream::with_slot(io, &fatal, &permit);
        drop(permit);
        drop(stream);
        assert!(observed.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(slot.available_permits(), 1);
    }

    #[tokio::test]
    async fn cut_is_visible_before_notify_and_during_waiter_registration() {
        let (trip, fatal) = Trip::new();
        let (raw, mut remote) = tokio::io::duplex(1);
        let mut fenced = FencedStream::new(raw, &fatal);
        assert!(!fatal.is_tripped());
        trip.mark_cut();
        assert!(fatal.is_tripped());
        tokio::time::timeout(Duration::from_millis(100), fatal.tripped())
            .await
            .unwrap();
        assert_eq!(
            fenced.write(b"a").await.unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert!(
            poll_fn(|cx| {
                let mut bytes = [0];
                let pending = Pin::new(&mut remote)
                    .poll_read(cx, &mut ReadBuf::new(&mut bytes))
                    .is_pending();
                Poll::Ready(pending)
            })
            .await
        );
        trip.notify();
        trip.notify();
        assert!(fatal.is_tripped());
    }

    #[tokio::test]
    async fn blocked_duplex_polls_wake_without_forwarding_after_fatal() {
        let (trip, fatal) = Trip::new();
        let (raw, mut remote) = tokio::io::duplex(1);
        let mut fenced = FencedStream::new(raw, &fatal);
        fenced.write_all(b"a").await.unwrap();

        let (mut read, mut write) = tokio::io::split(fenced);
        let (read_pending, read_ready) = tokio::sync::oneshot::channel();
        let (write_pending, write_ready) = tokio::sync::oneshot::channel();
        let reader = tokio::spawn(async move {
            let mut signal = Some(read_pending);
            let mut data = [0];
            let result = poll_fn(|cx| {
                let mut buf = ReadBuf::new(&mut data);
                let result = Pin::new(&mut read).poll_read(cx, &mut buf);
                if let Some(signal) = signal.take() {
                    assert!(result.is_pending());
                    signal.send(()).unwrap();
                }
                result
            })
            .await;
            (read, result)
        });
        let writer = tokio::spawn(async move {
            let mut signal = Some(write_pending);
            let result = poll_fn(|cx| {
                let result = Pin::new(&mut write).poll_write(cx, b"b");
                if let Some(signal) = signal.take() {
                    assert!(result.is_pending());
                    signal.send(()).unwrap();
                }
                result
            })
            .await;
            (write, result)
        });
        let displace = async {
            read_ready.await.unwrap();
            write_ready.await.unwrap();
            trip.trip();
        };
        let (reader, writer, ()) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(reader, writer, displace)
        })
        .await
        .expect("fatal must wake both pending operations");
        let (read, read_error) = reader.unwrap();
        let (write, write_error) = writer.unwrap();
        assert_eq!(
            read_error.unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert_eq!(
            write_error.unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );

        let mut fenced = read.unsplit(write);
        let data = [io::IoSlice::new(b"c"), io::IoSlice::new(b"d")];
        assert_eq!(
            fenced.write_vectored(&data).await.unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert_eq!(
            fenced.flush().await.unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        assert_eq!(
            fenced.shutdown().await.unwrap_err().kind(),
            io::ErrorKind::ConnectionAborted
        );
        drop(fenced);
        let mut bytes = Vec::new();
        remote.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"a");
    }
}
