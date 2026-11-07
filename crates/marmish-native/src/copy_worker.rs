//! One bounded filesystem job and response, shared by the hidden helper and CLI.
use crate::copy_files::{self, CopyReceiver, CopySource, DurablePrefix, PinnedPath, Receipt};
use crate::copy_local_gc::{Disposition, Identity, Location, ReceiverBase};
use marmish_stream::copy_protocol::{CopyError, Launch, Manifest, Mtime, Phase};
use sha2::{Digest, Sha256};
use std::{
    os::unix::{ffi::OsStringExt, fs::MetadataExt},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) enum Cancellation {
    Static(&'static AtomicBool),
    Owned(Arc<AtomicBool>),
}
impl Cancellation {
    fn flag(&self) -> &AtomicBool {
        match self {
            Self::Static(flag) => flag,
            Self::Owned(flag) => flag,
        }
    }
}
pub(crate) fn now() -> copy_files::Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_secs()).ok())
        .ok_or_else(file_error)
}
// Data record framing is one tag byte, a four-byte length, and eight-byte offset.
// Keeping the complete record within one transport block avoids a second
// asynchronous exchange for every chunk. The encoder-bound test guards this.
const SEND_CHUNK: usize = marmish_stream::MAX_BLOCK - 13;

pub(crate) enum Job {
    Source(Launch),
    Hash(u64, bool),
    ReadBounded(u64, usize),
    Unchanged,
    Receiver(Launch, Box<Manifest>, [u8; 16], u64),
    Data(u64, Vec<u8>),
    Checkpoint(i64),
    Prepare(i64),
    Commit,
}
pub(crate) enum Event {
    Source { size: u64, mode: u32, mtime: Mtime },
    Hash([u8; 32]),
    Data(Vec<u8>, [u8; 32]),
    Receiver(Phase, DurablePrefix),
    Prefix(DurablePrefix),
    Receipt(Option<Receipt>),
    Unit,
}
enum Files {
    Source {
        file: CopySource,
        rolling: Sha256,
        offset: u64,
    },
    Receiver(Box<CopyReceiver>),
}
pub(crate) struct Worker {
    pub(crate) jobs: SyncSender<Job>,
    pub(crate) events: Receiver<copy_files::Result<Event>>,
    thread: JoinHandle<()>,
    observation: Arc<Mutex<Disposition>>,
}
impl Worker {
    pub(crate) fn start(cancel: Cancellation) -> Self {
        Self::start_at(cancel, None)
    }
    #[cfg(test)]
    pub(crate) fn start_with_commit_hook(
        cancel: Cancellation,
        hook: crate::copy_commit_hook::Hook,
    ) -> Self {
        crate::copy_commit_hook::pending(hook);
        Self::start(cancel)
    }
    pub(crate) fn start_at(cancel: Cancellation, base: Option<ReceiverBase>) -> Self {
        #[cfg(test)]
        let commit_hook = crate::copy_commit_hook::take_pending();
        let (jobs, receive) = mpsc::sync_channel(1);
        let (send, events) = mpsc::sync_channel(1);
        let observation = Arc::new(Mutex::new(Disposition::NeverSubmitted));
        let observed = observation.clone();
        let thread = std::thread::spawn(move || {
            #[cfg(test)]
            crate::copy_commit_hook::install(commit_hook);
            let mut files = None;
            while let Ok(job) = receive.recv() {
                let receiver_stream = match &job {
                    Job::Receiver(_, _, stream, _) => Some(*stream),
                    _ => None,
                };
                if receiver_stream.is_some()
                    && let Ok(mut state) = observed.lock()
                {
                    *state = Disposition::Attempted;
                }
                let result = file_job(&mut files, job, cancel.flag(), base.as_ref());
                if let (
                    Some(stream),
                    Ok(Event::Receiver(..)),
                    Some(Files::Receiver(receiver)),
                    Some(base),
                ) = (receiver_stream, &result, &files, &base)
                    && let Ok(parent) = receiver.parent_identity()
                    && let Ok(mut state) = observed.lock()
                {
                    *state = Disposition::Established {
                        location: Location {
                            base: base.clone(),
                            parent: Identity {
                                device: parent.device,
                                inode: parent.inode,
                            },
                            stream,
                        },
                    };
                }
                if send.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            jobs,
            events,
            thread,
            observation,
        }
    }
    /// Drop both channels before waiting. A worker blocked sending its final
    /// event can then exit, dropping the physical receiver and its flock.
    pub(crate) async fn shutdown(self, timeout: std::time::Duration) -> crate::Result<Disposition> {
        let Self {
            jobs,
            events,
            thread,
            observation,
        } = self;
        drop(jobs);
        drop(events);
        let deadline = tokio::time::Instant::now() + timeout;
        while !thread.is_finished() {
            if tokio::time::Instant::now() >= deadline {
                return Err(crate::Error::Copy(CopyError::CommitUncertain));
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        thread
            .join()
            .map_err(|_| crate::Error::Copy(CopyError::CommitUncertain))?;
        let value = observation
            .lock()
            .map_err(|_| crate::Error::Copy(CopyError::CommitUncertain))?
            .clone();
        Ok(value)
    }
}
fn file_error() -> copy_files::Failure {
    copy_files::Failure {
        code: CopyError::Protocol,
        source: None,
    }
}
fn file_job(
    files: &mut Option<Files>,
    job: Job,
    cancel: &AtomicBool,
    receiver_base: Option<&ReceiverBase>,
) -> copy_files::Result<Event> {
    let read_limit = match &job {
        Job::ReadBounded(_, limit) if *limit > 0 && *limit <= marmish_stream::MAX_BLOCK => *limit,
        Job::ReadBounded(..) => return Err(file_error()),
        _ => marmish_stream::MAX_BLOCK,
    };
    match job {
        Job::Source(launch) => {
            if files.is_some() {
                return Err(file_error());
            }
            let base = copy_files::trusted_base(Path::new("."))?;
            let file = CopySource::open(PinnedPath::open(&base, &launch.source)?)?;
            let metadata = file.metadata()?;
            let event = Event::Source {
                size: file.size(),
                mode: metadata.mode() & 0o777,
                mtime: Mtime {
                    seconds: metadata.mtime(),
                    nanos: metadata.mtime_nsec().try_into().map_err(|_| file_error())?,
                },
            };
            *files = Some(Files::Source {
                file,
                rolling: Sha256::new(),
                offset: 0,
            });
            Ok(event)
        }
        Job::Receiver(launch, manifest, stream, retention_seconds) => {
            if files.is_some() {
                return Err(file_error());
            }
            let base_path = receiver_base
                .map(|base| {
                    std::path::PathBuf::from(std::ffi::OsString::from_vec(base.path.clone()))
                })
                .unwrap_or_else(|| Path::new(".").to_owned());
            let base = copy_files::trusted_base(&base_path)?;
            if let Some(expected) = receiver_base {
                let metadata = base.metadata().map_err(|source| copy_files::Failure {
                    code: CopyError::LocalIo,
                    source: Some(source),
                })?;
                if metadata.dev() != expected.identity.device
                    || metadata.ino() != expected.identity.inode
                {
                    return Err(copy_files::Failure {
                        code: CopyError::IdentityChanged,
                        source: None,
                    });
                }
            }
            let receiver = CopyReceiver::open_or_recover(
                &base,
                &launch.destination,
                *manifest,
                launch.mode,
                stream,
                now()?,
                i64::try_from(retention_seconds).map_err(|_| file_error())?,
            )?;
            let event = Event::Receiver(receiver.phase(), receiver.durable_prefix());
            *files = Some(Files::Receiver(Box::new(receiver)));
            Ok(event)
        }
        Job::Hash(length, track) => match files.as_mut() {
            Some(Files::Source {
                file,
                rolling,
                offset,
            }) => {
                let mut hash = Sha256::new();
                let mut position = 0;
                while position < length {
                    if cancel.load(Ordering::Acquire) {
                        return Err(file_error());
                    }
                    let bytes = file.read_chunk(position)?;
                    let n = bytes.len().min((length - position) as usize);
                    if n == 0 {
                        return Err(file_error());
                    }
                    hash.update(&bytes[..n]);
                    position += n as u64;
                }
                file.unchanged()?;
                let digest = hash.clone().finalize().into();
                if track {
                    *rolling = hash;
                    *offset = position;
                }
                Ok(Event::Hash(digest))
            }
            _ => Err(file_error()),
        },
        Job::ReadBounded(position, _) => match files.as_mut() {
            Some(Files::Source {
                file,
                rolling,
                offset,
            }) if *offset == position => {
                let mut bytes = file.read_chunk(position)?;
                bytes.truncate(SEND_CHUNK.min(read_limit));
                rolling.update(&bytes);
                *offset += bytes.len() as u64;
                Ok(Event::Data(bytes, rolling.clone().finalize().into()))
            }
            _ => Err(file_error()),
        },
        Job::Unchanged => match files.as_ref() {
            Some(Files::Source { file, .. }) => file.unchanged().map(|_| Event::Unit),
            _ => Err(file_error()),
        },
        job => match files.as_mut() {
            Some(Files::Receiver(receiver)) => match job {
                Job::Data(offset, bytes) => {
                    receiver.receive_data(offset, &bytes).map(|_| Event::Unit)
                }
                Job::Checkpoint(now) => receiver.checkpoint(now).map(Event::Prefix),
                Job::Prepare(now) => receiver.prepare_commit(now).map(|_| Event::Unit),
                Job::Commit => receiver.commit_cancellable(cancel).map(Event::Receipt),
                _ => Err(file_error()),
            },
            _ => Err(file_error()),
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sender_data_record_fits_one_transport_block() {
        use marmish_stream::copy_protocol::Frame;
        for offset in [0, 1, marmish_stream::MAX_OFFSET - SEND_CHUNK as u64] {
            let encoded = Frame::Data {
                offset,
                bytes: vec![0xff; SEND_CHUNK],
            }
            .encode()
            .unwrap();
            assert_eq!(encoded.len(), marmish_stream::MAX_BLOCK);
        }
    }

    use super::*;
    #[test]
    fn bounded_sender_handles_nondivisible_credit_window_and_exact_hash() {
        use marmish_stream::copy_protocol::{self as cp, AttemptMode, Direction, Frame};
        use std::{io::Write, os::unix::ffi::OsStrExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        let mut file = std::fs::File::create(&path).unwrap();
        let block: Vec<u8> = (0..8192).map(|n| n as u8).collect();
        let mut expected = Sha256::new();
        for _ in 0..257 {
            file.write_all(&block).unwrap();
            expected.update(&block);
        }
        drop(file);
        let launch = Launch {
            version: cp::VERSION,
            mode: AttemptMode::Create,
            transfer: [3; 32],
            client: [1; 32],
            server: [2; 32],
            direction: Direction::Put,
            source: path.as_os_str().as_bytes().to_vec(),
            destination: b"unused".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
        };
        let mut state = None;
        let cancel = AtomicBool::new(false);
        file_job(&mut state, Job::Source(launch), &cancel, None).unwrap();
        file_job(&mut state, Job::Hash(0, true), &cancel, None).unwrap();
        let size = 257 * 8192u64;
        let (mut sent, mut durable) = (0u64, 0u64);
        let mut saw_residual = false;
        let mut final_hash = [0; 32];
        while sent < size {
            let remaining = cp::DURABLE_WINDOW - (sent - durable);
            if remaining == 0 {
                durable = sent;
                continue;
            }
            if remaining < SEND_CHUNK as u64 {
                saw_residual = true;
            }
            let limit = remaining.min(marmish_stream::MAX_BLOCK as u64) as usize;
            let Event::Data(bytes, hash) =
                file_job(&mut state, Job::ReadBounded(sent, limit), &cancel, None).unwrap()
            else {
                panic!("data")
            };
            assert_eq!(
                bytes.len(),
                limit.min(SEND_CHUNK).min((size - sent) as usize)
            );
            assert!(
                Frame::Data {
                    offset: sent,
                    bytes: bytes.clone()
                }
                .encode()
                .unwrap()
                .len()
                    <= marmish_stream::MAX_BLOCK
            );
            sent += bytes.len() as u64;
            assert!(sent - durable <= cp::DURABLE_WINDOW);
            final_hash = hash;
        }
        assert!(saw_residual);
        assert_eq!(sent, size);
        assert_eq!(final_hash, <[u8; 32]>::from(expected.finalize()));
    }
    #[tokio::test]
    async fn shutdown_timeout_never_confirms_a_physically_owned_lock() {
        use fs2::FileExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("held");
        let held = std::fs::File::create(&path).unwrap();
        held.lock_exclusive().unwrap();
        let release = Arc::new(AtomicBool::new(false));
        let gate = release.clone();
        let (done, ended) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            while !gate.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            drop(held);
            let _ = done.send(());
        });
        let (jobs, _) = mpsc::sync_channel(1);
        let (_, events) = mpsc::sync_channel(1);
        let worker = Worker {
            jobs,
            events,
            thread,
            observation: Arc::new(Mutex::new(Disposition::NeverSubmitted)),
        };
        let result = worker.shutdown(std::time::Duration::ZERO).await;
        // Release the owned test thread even if the assertion below fails.
        let probe = std::fs::File::open(&path).unwrap();
        let still_held = probe.try_lock_exclusive().is_err();
        release.store(true, Ordering::Release);
        ended
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(matches!(
            result,
            Err(crate::Error::Copy(CopyError::CommitUncertain))
        ));
        assert!(still_held);
        probe.try_lock_exclusive().unwrap();
    }
    #[tokio::test]
    async fn shutdown_waits_for_receiver_flock_and_returns_confirmed_location() {
        use marmish_stream::copy_protocol::{self as cp, AttemptMode, Direction};
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        let base = copy_files::trusted_base(&path).unwrap();
        let anchor = ReceiverBase {
            path: path.as_os_str().as_bytes().to_vec(),
            identity: Identity::read(&base).unwrap(),
        };
        let launch = Launch {
            version: cp::VERSION,
            mode: AttemptMode::Create,
            transfer: [3; 32],
            client: [1; 32],
            server: [2; 32],
            direction: Direction::Get,
            source: b"source".to_vec(),
            destination: b"destination".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
        };
        let manifest = Manifest {
            version: launch.version,
            transfer: launch.transfer,
            client: launch.client,
            server: launch.server,
            direction: launch.direction,
            source: launch.source.clone(),
            destination: launch.destination.clone(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
            size: 0,
            sha256: Sha256::digest([]).into(),
            mode: None,
            mtime: None,
        };
        let worker = Worker::start_at(
            Cancellation::Owned(Arc::new(AtomicBool::new(false))),
            Some(anchor),
        );
        worker
            .jobs
            .send(Job::Receiver(
                launch.clone(),
                Box::new(manifest.clone()),
                [4; 16],
                cp::MIN_RETENTION_SECS as u64,
            ))
            .unwrap();
        let event = worker
            .events
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(matches!(event, Ok(Event::Receiver(..))));
        assert!(matches!(
            CopyReceiver::open_or_recover(
                &base,
                &launch.destination,
                manifest.clone(),
                AttemptMode::Resume,
                [5; 16],
                now().unwrap(),
                cp::MIN_RETENTION_SECS
            ),
            Err(copy_files::Failure {
                code: CopyError::Busy,
                ..
            })
        ));
        assert!(matches!(
            worker
                .shutdown(std::time::Duration::from_secs(5))
                .await
                .unwrap(),
            Disposition::Established { .. }
        ));
        assert!(
            CopyReceiver::open_or_recover(
                &base,
                &launch.destination,
                manifest,
                AttemptMode::Resume,
                [5; 16],
                now().unwrap(),
                cp::MIN_RETENTION_SECS
            )
            .is_ok()
        );
    }
    #[tokio::test]
    async fn unsubmitted_worker_shutdown_proves_no_receiver_attempt() {
        let worker = Worker::start(Cancellation::Owned(Arc::new(AtomicBool::new(false))));
        assert!(matches!(
            worker
                .shutdown(std::time::Duration::from_secs(5))
                .await
                .unwrap(),
            Disposition::NeverSubmitted
        ));
    }
}
