//! File-copy endpoint protocol. The stream and blocking filesystem worker stay bounded.
use crate::{
    Error, Result,
    copy_io::{CopyIo, fail},
    copy_worker::{Event, Job, Worker, now},
};
use marmish_stream::copy_protocol::{
    self as protocol, CopyError, Direction, Frame, Launch, Manifest, Phase,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::time::{Duration, Instant};

#[cfg(test)]
pub(crate) async fn run(
    io: &mut CopyIo,
    launch: Launch,
    stream: [u8; 16],
    retention: u64,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let worker = Worker::start(crate::copy_worker::Cancellation::Owned(cancel.clone()));
    let result = run_with_worker(io, launch, stream, retention, cancel, &worker).await;
    // Fixtures and non-indexed Put callers still prove physical completion.
    let joined = worker.shutdown(Duration::from_secs(15)).await;
    if result.is_ok() {
        joined?;
    }
    result
}
pub(crate) async fn run_with_worker(
    io: &mut CopyIo,
    launch: Launch,
    stream: [u8; 16],
    retention: u64,
    cancel: Arc<AtomicBool>,
    worker: &Worker,
) -> Result<()> {
    let mut commit_possible = false;
    let result = async {
        launch.validate().map_err(|_| fail(CopyError::Protocol))?;
        if !(protocol::MIN_RETENTION_SECS as u64..=protocol::MAX_RETENTION_SECS)
            .contains(&retention)
        {
            return Err(fail(CopyError::Protocol));
        }
        io.send(Frame::Start(launch.clone(), io.codec())).await?;
        match launch.direction {
            Direction::Put => transmit(io, worker, launch, &mut commit_possible).await,
            Direction::Get => {
                receive(
                    io,
                    worker,
                    launch,
                    stream,
                    retention,
                    &cancel,
                    &mut commit_possible,
                )
                .await
            }
        }
    }
    .await;
    if let Err(Error::Copy(code)) = &result {
        // A finite local refusal terminates the peer protocol. A transport loss
        // or explicit user cancellation follows its separate asynchronous path.
        if io.abort_failure(*code).await.is_err() {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr().lock(),
                "marmish copy: remote_cleanup_unconfirmed"
            );
        }
    }
    classify_commit_result(result, commit_possible, io.peer_error_received())
}
fn classify_commit_result(result: Result<()>, possible: bool, peer_error: bool) -> Result<()> {
    if possible
        && !peer_error
        && matches!(
            &result,
            Err(Error::Timeout
                | Error::Io(_)
                | Error::Transport(_)
                | Error::Stream(_)
                | Error::Copy(CopyError::Protocol))
        )
    {
        return Err(fail(CopyError::CommitUncertain));
    }
    result
}
fn invalid<T>() -> Result<T> {
    Err(fail(CopyError::Protocol))
}
fn clock() -> Result<i64> {
    now().map_err(|failure| fail(failure.code))
}
fn matches_resume(phase: Phase, offset: u64, size: u64) -> bool {
    offset <= size
        && (phase != Phase::Created || offset == 0)
        && (!matches!(
            phase,
            Phase::Verifying | Phase::CommitPrepared | Phase::Committed
        ) || offset == size)
}
fn matches_commit(frame: Frame, launch: &Launch, size: u64, hash: [u8; 32]) -> bool {
    matches!(frame, Frame::Commit {transfer, size: n, sha256}
        if transfer == launch.transfer && n == size && sha256 == hash)
}
fn matches_finish(frame: Frame, transfer: [u8; 32], hash: [u8; 32]) -> bool {
    matches!(frame, Frame::Finish { transfer: id, sha256 }
        if id == transfer && sha256 == hash)
}
async fn transmit(
    io: &mut CopyIo,
    worker: &Worker,
    launch: Launch,
    commit_possible: &mut bool,
) -> Result<()> {
    let Event::Source { size, mode, mtime } = io.work(worker, Job::Source(launch.clone())).await?
    else {
        return invalid();
    };
    let Event::Hash(hash) = io.work(worker, Job::Hash(size, false)).await? else {
        return invalid();
    };
    let manifest = Manifest {
        version: launch.version,
        transfer: launch.transfer,
        client: launch.client,
        server: launch.server,
        direction: launch.direction,
        source: launch.source.clone(),
        destination: launch.destination.clone(),
        overwrite: launch.overwrite,
        preserve_mode: launch.preserve_mode,
        preserve_mtime: launch.preserve_mtime,
        size,
        sha256: hash,
        mode: launch.preserve_mode.then_some(mode),
        mtime: launch.preserve_mtime.then_some(mtime),
    };
    manifest.validate().map_err(|_| fail(CopyError::Protocol))?;
    io.send(Frame::Manifest(manifest)).await?;
    let Frame::Resume {
        phase,
        offset,
        sha256,
    } = io.next().await?
    else {
        return invalid();
    };
    if !matches_resume(phase, offset, size) {
        return invalid();
    }
    let Event::Hash(prefix) = io.work(worker, Job::Hash(offset, true)).await? else {
        return invalid();
    };
    if prefix != sha256 {
        return Err(fail(CopyError::HashMismatch));
    }
    io.configure_credit(offset, prefix)?;
    io.send(Frame::Prefix {
        offset,
        sha256: prefix,
    })
    .await?;
    while io.credit().0 < size {
        io.wait_credit().await?;
        let (offset, durable) = io.credit();
        let remaining = marmish_stream::copy_protocol::DURABLE_WINDOW - (offset - durable);
        let limit = usize::try_from(remaining.min(marmish_stream::MAX_BLOCK as u64))
            .map_err(|_| fail(CopyError::Protocol))?;
        let Event::Data(bytes, prefix) = io.work(worker, Job::ReadBounded(offset, limit)).await?
        else {
            return invalid();
        };
        if bytes.is_empty() {
            return Err(fail(CopyError::SourceChanged));
        }
        if bytes.len() as u64 > size - offset {
            return Err(fail(CopyError::SourceChanged));
        }
        io.offer(offset, bytes.len(), prefix)?;
        io.send(Frame::Data { offset, bytes }).await?;
    }
    if !matches!(io.work(worker, Job::Unchanged).await?, Event::Unit) {
        return invalid();
    }
    *commit_possible = true;
    io.send(Frame::Done { size, sha256: hash }).await?;
    if !matches_commit(io.next().await?, &launch, size, hash) || io.credit() != (size, size) {
        return invalid();
    }
    io.clear_credit()?;
    io.send(Frame::Finish {
        transfer: launch.transfer,
        sha256: hash,
    })
    .await?;
    io.finish().await
}
async fn checkpoint(io: &mut CopyIo, worker: &Worker) -> Result<crate::copy_files::DurablePrefix> {
    let Event::Prefix(prefix) = io.work(worker, Job::Checkpoint(clock()?)).await? else {
        return invalid();
    };
    io.send(Frame::Checkpoint {
        offset: prefix.offset,
        sha256: prefix.sha256,
    })
    .await?;
    Ok(prefix)
}
async fn receive(
    io: &mut CopyIo,
    worker: &Worker,
    launch: Launch,
    stream: [u8; 16],
    retention: u64,
    cancel: &AtomicBool,
    commit_possible: &mut bool,
) -> Result<()> {
    let Frame::Manifest(manifest) = io.next().await? else {
        return invalid();
    };
    if !launch.matches_manifest(&manifest) {
        return Err(fail(CopyError::Conflict));
    }
    let Event::Receiver(phase, mut prefix) = io
        .work(
            worker,
            Job::Receiver(
                launch.clone(),
                Box::new(manifest.clone()),
                stream,
                retention,
            ),
        )
        .await?
    else {
        return invalid();
    };
    if !matches_resume(phase, prefix.offset, manifest.size) {
        return invalid();
    }
    io.send(Frame::Resume {
        phase,
        offset: prefix.offset,
        sha256: prefix.sha256,
    })
    .await?;
    match io.next().await? {
        Frame::Prefix { offset, sha256 } if offset == prefix.offset && sha256 == prefix.sha256 => {}
        _ => return invalid(),
    }
    let mut accepted = prefix.offset;
    let mut last_checkpoint = Instant::now();
    loop {
        let deadline =
            (accepted > prefix.offset).then_some(last_checkpoint + Duration::from_secs(5));
        match io.next_until(deadline).await? {
            None => {
                prefix = checkpoint(io, worker).await?;
                last_checkpoint = Instant::now();
            }
            Some(Frame::Data { offset, bytes })
                if matches!(phase, Phase::Created | Phase::Receiving)
                    && offset == accepted
                    && offset
                        .checked_add(bytes.len() as u64)
                        .is_some_and(|end| end <= manifest.size) =>
            {
                accepted += bytes.len() as u64;
                if !matches!(
                    io.work(worker, Job::Data(offset, bytes)).await?,
                    Event::Unit
                ) {
                    return invalid();
                }
                if accepted - prefix.offset >= protocol::CHECKPOINT_INTERVAL {
                    prefix = checkpoint(io, worker).await?;
                    last_checkpoint = Instant::now();
                }
            }
            Some(Frame::Done { size, sha256 })
                if accepted == manifest.size
                    && size == manifest.size
                    && sha256 == manifest.sha256 =>
            {
                break;
            }
            _ => return invalid(),
        }
    }
    if matches!(phase, Phase::Created | Phase::Receiving) {
        checkpoint(io, worker).await?;
    } else {
        io.send(Frame::Checkpoint {
            offset: prefix.offset,
            sha256: prefix.sha256,
        })
        .await?;
    }
    if !matches!(io.work(worker, Job::Prepare(clock()?)).await?, Event::Unit) {
        return invalid();
    }
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Invalid("copy_cancelled"));
    }
    *commit_possible = true;
    let Event::Receipt(receipt) = io.work(worker, Job::Commit).await? else {
        return invalid();
    };
    let receipt = receipt.ok_or(Error::Invalid("copy_cancelled"))?;
    if receipt.transfer != launch.transfer
        || receipt.size != manifest.size
        || receipt.sha256 != manifest.sha256
    {
        return invalid();
    }
    io.send(Frame::Commit {
        transfer: receipt.transfer,
        size: receipt.size,
        sha256: receipt.sha256,
    })
    .await?;
    if !matches_finish(io.next().await?, receipt.transfer, receipt.sha256) {
        return invalid();
    }
    io.finish().await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commit_possible_disconnect_is_uncertain_but_finite_peer_errors_survive() {
        for error in [
            Error::Timeout,
            Error::Copy(CopyError::Protocol),
            Error::Stream(marmish_stream::wire::WireErrorCode::Closed),
        ] {
            assert!(matches!(
                classify_commit_result(Err(error), true, false),
                Err(Error::Copy(CopyError::CommitUncertain))
            ));
        }
        assert!(matches!(
            classify_commit_result(Err(Error::Copy(CopyError::Protocol)), true, true),
            Err(Error::Copy(CopyError::Protocol))
        ));
        assert!(matches!(
            classify_commit_result(Err(Error::Copy(CopyError::DiskFull)), true, false),
            Err(Error::Copy(CopyError::DiskFull))
        ));
        assert!(matches!(
            classify_commit_result(Err(Error::Timeout), false, false),
            Err(Error::Timeout)
        ));
    }
    #[test]
    fn resume_requires_valid_size_and_final_phase_offsets() {
        for phase in [
            Phase::Created,
            Phase::Receiving,
            Phase::Verifying,
            Phase::CommitPrepared,
            Phase::Committed,
        ] {
            assert!(!matches_resume(phase, 11, 10));
            assert!(matches_resume(phase, 0, 0));
        }
        assert!(!matches_resume(Phase::Created, 1, 10));
        assert!(matches_resume(Phase::Receiving, 4, 10));
        for phase in [Phase::Verifying, Phase::CommitPrepared, Phase::Committed] {
            assert!(!matches_resume(phase, 4, 10));
            assert!(matches_resume(phase, 10, 10));
        }
    }
    #[test]
    fn commit_requires_exact_transfer_size_hash_and_frame_type() {
        let launch = Launch {
            version: protocol::VERSION,
            mode: protocol::AttemptMode::Create,
            transfer: [1; 32],
            client: [2; 32],
            server: [3; 32],
            direction: Direction::Put,
            source: b"source".to_vec(),
            destination: b"destination".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
        };
        assert!(matches_commit(
            Frame::Commit {
                transfer: [1; 32],
                size: 9,
                sha256: [4; 32]
            },
            &launch,
            9,
            [4; 32]
        ));
        for (transfer, size, sha256) in [
            ([5; 32], 9, [4; 32]),
            ([1; 32], 8, [4; 32]),
            ([1; 32], 9, [5; 32]),
        ] {
            assert!(!matches_commit(
                Frame::Commit {
                    transfer,
                    size,
                    sha256
                },
                &launch,
                9,
                [4; 32]
            ));
        }
        assert!(!matches_commit(
            Frame::Done {
                size: 9,
                sha256: [4; 32]
            },
            &launch,
            9,
            [4; 32]
        ));
    }
    #[test]
    fn finish_requires_exact_transfer_and_hash_and_frame_type() {
        assert!(matches_finish(
            Frame::Finish {
                transfer: [1; 32],
                sha256: [2; 32]
            },
            [1; 32],
            [2; 32]
        ));
        assert!(!matches_finish(
            Frame::Finish {
                transfer: [9; 32],
                sha256: [2; 32]
            },
            [1; 32],
            [2; 32]
        ));
        assert!(!matches_finish(
            Frame::Finish {
                transfer: [1; 32],
                sha256: [9; 32]
            },
            [1; 32],
            [2; 32]
        ));
        assert!(!matches_finish(
            Frame::Done {
                size: 0,
                sha256: [2; 32]
            },
            [1; 32],
            [2; 32]
        ));
    }
}
