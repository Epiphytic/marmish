//! Revocation for one registered candidate daemon lifetime.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

#[derive(Clone)]
pub(crate) struct Fatal(CancellationToken);

/// Retained by the exact registration owner. Other owners only observe.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Trip(CancellationToken);

#[cfg_attr(not(test), allow(dead_code))]
impl Trip {
    #[cfg(test)]
    pub(crate) fn new() -> (Self, Fatal) {
        let signal = CancellationToken::new();
        (Self(signal.clone()), Fatal(signal))
    }

    pub(crate) fn trip(&self) {
        self.0.cancel();
    }
}

impl Fatal {
    pub(crate) fn is_tripped(&self) -> bool {
        self.0.is_cancelled()
    }

    pub(crate) async fn tripped(&self) {
        self.0.cancelled().await;
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
            read: Box::pin(fatal.0.clone().cancelled_owned()),
            write: Box::pin(fatal.0.clone().cancelled_owned()),
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
