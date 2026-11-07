//! Hidden copy endpoint. This process owns bounded pipe I/O; one filesystem
//! worker owns pinned files/checkpoints. Disconnect is never an implicit cancel.
use crate::{
    copy_files::{self, DurablePrefix},
    copy_worker::{Cancellation, Event, Job, Worker},
};
use marmish_stream::{
    MAX_BLOCK,
    copy_protocol::{
        self as protocol, CopyError, Direction, Frame, FrameDecoder, HelperContext, Launch,
        Manifest, Phase, Stage,
    },
};
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Write},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::TryRecvError,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Failure>;
enum Failure {
    File(CopyError),
    Cancelled,
    Peer,
    Build,
    Launch,
}
// A durable receipt already exists; a local protocol/pipe failure can no
// longer establish that the destination was not committed.
fn after_receipt<T>(result: Result<T>) -> Result<T> {
    result.map_err(|failure| match failure {
        Failure::File(_) => Failure::File(CopyError::CommitUncertain),
        other => other,
    })
}
impl From<copy_files::Failure> for Failure {
    fn from(value: copy_files::Failure) -> Self {
        Self::File(value.code)
    }
}
impl From<marmish_stream::Error> for Failure {
    fn from(_: marmish_stream::Error) -> Self {
        Self::File(CopyError::Protocol)
    }
}
impl From<io::Error> for Failure {
    fn from(value: io::Error) -> Self {
        Self::File(copy_files::Failure::from(value).code)
    }
}
static CANCEL: AtomicBool = AtomicBool::new(false);
extern "C" fn terminate(_: libc::c_int) {
    CANCEL.store(true, Ordering::Release);
}
fn check_cancel() -> Result<()> {
    if CANCEL.load(Ordering::Acquire) {
        Err(Failure::Cancelled)
    } else {
        Ok(())
    }
}
fn now() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|n| i64::try_from(n.as_secs()).ok())
        .ok_or(Failure::File(CopyError::LocalIo))
}
fn invalid<T>() -> Result<T> {
    Err(Failure::File(CopyError::Protocol))
}

#[allow(clippy::unnecessary_cast)] // Darwin mode_t is u16; Linux mode_t is u32.
fn fifo(file: &File) -> Result<()> {
    if file.metadata()?.mode() & libc::S_IFMT as u32 != libc::S_IFIFO as u32 {
        return invalid();
    }
    Ok(())
}
fn nonblocking(file: &File) -> Result<()> {
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}
const CONTEXT_READ_TIMEOUT: Duration = Duration::from_secs(1);

fn context_remaining(deadline: Instant) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(io::Error::from(io::ErrorKind::TimedOut).into());
    }
    Ok(remaining)
}

fn context_readable(file: &File, deadline: Instant) -> Result<()> {
    loop {
        let remaining = context_remaining(deadline)?;
        let mut descriptor = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let milliseconds = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
        let ready = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(io::Error::other("copy context poll failed").into());
        }
        if ready > 0 && descriptor.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            return Ok(());
        }
    }
}

fn context(file: File) -> Result<HelperContext> {
    context_with_wait(file, context_readable)
}

// The private wait boundary also permits deterministic real-pipe tests. It
// runs only after read actually returned WouldBlock, never on a decoded prefix.
fn context_with_wait(
    mut file: File,
    mut wait: impl FnMut(&File, Instant) -> Result<()>,
) -> Result<HelperContext> {
    if file.as_raw_fd() <= 3 {
        return invalid();
    }
    fifo(&file)?;
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 || flags & libc::O_ACCMODE != libc::O_RDONLY {
        return invalid();
    }
    // Our parent prewrites and closes its writer before this helper's spawn.
    // A concurrent fork may retain another CLOEXEC writer until its exec. Wait
    // briefly for actual EOF, with no deadline renewal on progress or EINTR.
    nonblocking(&file)?;
    let deadline = Instant::now() + CONTEXT_READ_TIMEOUT;
    let mut bytes = [0u8; protocol::MAX_CONTEXT + 1];
    let mut length = 0;
    loop {
        context_remaining(deadline)?;
        match file.read(&mut bytes[length..]) {
            Ok(0) => break,
            Ok(count) => {
                length += count;
                if length > protocol::MAX_CONTEXT {
                    return invalid();
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => wait(&file, deadline)?,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    drop(file);
    let context = HelperContext::decode(&bytes[..length])?;
    let build = crate::native_manifest::compiled_release_metadata();
    let mut commit = [0; 20];
    hex::decode_to_slice(build.commit, &mut commit).map_err(|_| Failure::Build)?;
    context
        .validate_build(build.version, build.target, commit)
        .map_err(|_| Failure::Build)?;
    Ok(context)
}

/// Called only by early hidden dispatch, before layout/profile/provider work.
/// Descriptor claims are consistency checks, not same-UID authentication.
pub(crate) fn run(context_fd: File, input: File, mut output: File) -> u8 {
    let setup: Result<HelperContext> = (|| {
        let context = context(context_fd)?;
        fifo(&input)?;
        fifo(&output)?;
        nonblocking(&input)?;
        nonblocking(&output)?;
        CANCEL.store(false, Ordering::Release);
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = terminate as *const () as usize;
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        if unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) } < 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(context)
    })();
    let context = match setup {
        Ok(value) => value,
        Err(failure) => {
            let (code, diagnostic) = if matches!(failure, Failure::Build) {
                (
                    CopyError::Unsupported,
                    "helper_server_build_mismatch_restart_daemon",
                )
            } else {
                (CopyError::Protocol, "helper_context_invalid")
            };
            startup_error(&mut output, code);
            let _ = writeln!(io::stderr().lock(), "marmish copy: {diagnostic}");
            return 1;
        }
    };
    let mut driver = Pipes::new(input, output);
    let result = serve(&mut driver, &context);
    match result {
        Ok(()) => 0,
        Err(Failure::File(code)) => {
            if driver.started {
                let _ = driver.fail(code);
            }
            let _ = writeln!(io::stderr().lock(), "marmish copy: {code:?}");
            1
        }
        Err(Failure::Launch | Failure::Build) => {
            let _ = driver.fail(CopyError::Protocol);
            let _ = writeln!(
                io::stderr().lock(),
                "marmish copy: helper_context_launch_invalid"
            );
            1
        }
        Err(Failure::Cancelled | Failure::Peer) => 1,
    }
}

// This bounded startup record is emitted only to an independently validated
// writable pipe. An invalid locator/context cannot select an arbitrary file.
fn startup_error(output: &mut File, code: CopyError) {
    let flags = unsafe { libc::fcntl(output.as_raw_fd(), libc::F_GETFL) };
    if fifo(output).is_err()
        || flags < 0
        || flags & libc::O_ACCMODE != libc::O_WRONLY
        || nonblocking(output).is_err()
    {
        return;
    }
    if let Ok(bytes) = Frame::Error(code).encode() {
        // A full pipe is not usable at this startup boundary; never block here.
        let _ = output.write(&bytes);
    }
}

struct Credit {
    sent: u64,
    durable: u64,
    hash: [u8; 32],
    prefixes: VecDeque<(u64, [u8; 32])>,
}
impl Credit {
    fn acknowledge(&mut self, offset: u64, hash: [u8; 32]) -> Result<()> {
        if offset < self.durable || offset > self.sent {
            return invalid();
        }
        let expected = if offset == self.durable {
            Some(self.hash)
        } else {
            self.prefixes
                .iter()
                .find(|(n, _)| *n == offset)
                .map(|(_, h)| *h)
        };
        if expected != Some(hash) {
            return Err(Failure::File(CopyError::HashMismatch));
        }
        self.durable = offset;
        self.hash = hash;
        while self.prefixes.front().is_some_and(|(n, _)| *n <= offset) {
            self.prefixes.pop_front();
        }
        Ok(())
    }
    fn offer(&mut self, offset: u64, length: usize, hash: [u8; 32]) -> Result<()> {
        let end = offset
            .checked_add(length as u64)
            .ok_or(Failure::File(CopyError::Protocol))?;
        if length == 0
            || length > MAX_BLOCK
            || offset != self.sent
            || end - self.durable > protocol::DURABLE_WINDOW
            || self.prefixes.len() > (protocol::DURABLE_WINDOW / MAX_BLOCK as u64) as usize
        {
            return invalid();
        }
        self.sent = end;
        self.prefixes.push_back((end, hash));
        Ok(())
    }
}
struct Pipes {
    input: File,
    output: File,
    decoder: FrameDecoder,
    input_bytes: [u8; MAX_BLOCK],
    begin: usize,
    end: usize,
    incoming: Option<Frame>,
    eof: bool,
    started: bool,
    finished: bool,
    credit: Option<Credit>,
    last_progress: Instant,
}
impl Pipes {
    fn new(input: File, output: File) -> Self {
        Self {
            input,
            output,
            decoder: FrameDecoder::default(),
            input_bytes: [0; MAX_BLOCK],
            begin: 0,
            end: 0,
            incoming: None,
            eof: false,
            started: false,
            finished: false,
            credit: None,
            last_progress: Instant::now() - Duration::from_secs(1),
        }
    }
    fn ingest(&mut self) -> Result<()> {
        while self.incoming.is_none() && !self.eof {
            if self.begin == self.end {
                match self.input.read(&mut self.input_bytes) {
                    Ok(0) => {
                        self.eof = true;
                        self.decoder.finish()?;
                        return Ok(());
                    }
                    Ok(n) => {
                        self.begin = 0;
                        self.end = n;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        check_cancel()?;
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            let (used, frame) = self.decoder.feed(&self.input_bytes[self.begin..self.end])?;
            self.begin += used;
            if let Some(frame) = frame {
                match frame {
                    Frame::Progress { .. } if self.started && !self.finished => (),
                    Frame::Checkpoint { offset, sha256 }
                        if !self.finished && self.credit.is_some() =>
                    {
                        self.credit.as_mut().unwrap().acknowledge(offset, sha256)?;
                    }
                    Frame::Error(_) if self.started && !self.finished => return Err(Failure::Peer),
                    frame => self.incoming = Some(frame),
                }
            }
        }
        Ok(())
    }
    fn poll(&mut self, writable: bool) -> Result<()> {
        check_cancel()?;
        let mut descriptors = [
            libc::pollfd {
                fd: self.input.as_raw_fd(),
                events: if self.incoming.is_none() && !self.eof {
                    libc::POLLIN
                } else {
                    0
                },
                revents: 0,
            },
            libc::pollfd {
                fd: self.output.as_raw_fd(),
                events: if writable { libc::POLLOUT } else { 0 },
                revents: 0,
            },
        ];
        if unsafe { libc::poll(descriptors.as_mut_ptr(), 2, 100) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
        check_cancel()?;
        self.ingest()
    }
    fn send(&mut self, frame: Frame) -> Result<()> {
        let bytes = frame.encode()?;
        let mut offset = 0;
        while offset < bytes.len() {
            check_cancel()?;
            self.ingest()?;
            match self.output.write(&bytes[offset..]) {
                Ok(0) => return Err(Failure::File(CopyError::LocalIo)),
                Ok(n) => offset += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => self.poll(true)?,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    fn fail(&mut self, code: CopyError) -> Result<()> {
        // Parsing is terminal. A malformed/truncated input must not prevent the
        // finite Error response by repeatedly failing decoder.feed/finish.
        self.finished = true;
        self.eof = true;
        self.incoming = None;
        self.begin = self.end;
        self.send(Frame::Error(code))
    }
    fn next(&mut self, deadline: Option<Instant>) -> Result<Option<Frame>> {
        loop {
            check_cancel()?;
            self.ingest()?;
            if let Some(frame) = self.incoming.take() {
                return Ok(Some(frame));
            }
            if self.eof {
                return invalid();
            }
            if deadline.is_some_and(|until| Instant::now() >= until) {
                return Ok(None);
            }
            self.poll(false)?;
        }
    }
    fn finish(&mut self) -> Result<()> {
        self.finished = true;
        loop {
            self.ingest()?;
            if self.incoming.is_some() {
                return invalid();
            }
            if self.eof {
                return Ok(());
            }
            self.poll(false)?;
        }
    }
    fn work(&mut self, worker: &Worker, job: Job, progress: Option<(Stage, u64)>) -> Result<Event> {
        worker
            .jobs
            .send(job)
            .map_err(|_| Failure::File(CopyError::LocalIo))?;
        loop {
            match worker.events.try_recv() {
                Ok(result) => return result.map_err(Failure::from),
                Err(TryRecvError::Disconnected) => return Err(Failure::File(CopyError::LocalIo)),
                Err(TryRecvError::Empty) => (),
            }
            if self.eof {
                return invalid();
            }
            if let Some((stage, total)) = progress
                && self.last_progress.elapsed() >= Duration::from_secs(1)
            {
                self.send(Frame::Progress {
                    stage,
                    examined: 0,
                    total,
                })?;
                self.last_progress = Instant::now();
            }
            self.poll(false)?;
        }
    }
}

fn serve(io: &mut Pipes, context: &HelperContext) -> Result<()> {
    let Some(Frame::Start(launch, codec)) = io.next(None).map_err(|failure| match failure {
        Failure::Cancelled => Failure::Cancelled,
        _ => Failure::Launch,
    })?
    else {
        return Err(Failure::Launch);
    };
    io.started = true;
    context
        .validate_start(&launch, codec)
        .map_err(|_| Failure::Launch)?;
    let worker = Worker::start(Cancellation::Static(&CANCEL));
    match launch.direction {
        Direction::Put => receive(
            io,
            &worker,
            launch,
            context.stream,
            context.retention_seconds,
        ),
        Direction::Get => transmit(io, &worker, launch),
    }
}
fn checkpoint(io: &mut Pipes, worker: &Worker, total: u64) -> Result<DurablePrefix> {
    let Event::Prefix(prefix) = io.work(
        worker,
        Job::Checkpoint(now()?),
        Some((Stage::Syncing, total)),
    )?
    else {
        return invalid();
    };
    io.send(Frame::Checkpoint {
        offset: prefix.offset,
        sha256: prefix.sha256,
    })?;
    Ok(prefix)
}
fn receive(
    io: &mut Pipes,
    worker: &Worker,
    launch: Launch,
    stream: [u8; 16],
    retention_seconds: u64,
) -> Result<()> {
    let Some(Frame::Manifest(manifest)) = io.next(None)? else {
        return invalid();
    };
    if !launch.matches_manifest(&manifest) {
        return Err(Failure::File(CopyError::Conflict));
    }
    let Event::Receiver(phase, mut prefix) = io.work(
        worker,
        Job::Receiver(
            launch.clone(),
            Box::new(manifest.clone()),
            stream,
            retention_seconds,
        ),
        Some((Stage::PrefixHash, manifest.size)),
    )?
    else {
        return invalid();
    };
    io.send(Frame::Resume {
        phase,
        offset: prefix.offset,
        sha256: prefix.sha256,
    })?;
    match io.next(None)? {
        Some(Frame::Prefix { offset, sha256 })
            if offset == prefix.offset && sha256 == prefix.sha256 => {}
        _ => return invalid(),
    }
    let mut accepted = prefix.offset;
    let mut last_checkpoint = Instant::now();
    loop {
        let deadline =
            (accepted > prefix.offset).then_some(last_checkpoint + Duration::from_secs(5));
        match io.next(deadline)? {
            None => {
                prefix = checkpoint(io, worker, manifest.size)?;
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
                io.work(worker, Job::Data(offset, bytes), None)?;
                if accepted - prefix.offset >= protocol::CHECKPOINT_INTERVAL {
                    prefix = checkpoint(io, worker, manifest.size)?;
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
        checkpoint(io, worker, manifest.size)?;
    } else {
        io.send(Frame::Checkpoint {
            offset: prefix.offset,
            sha256: prefix.sha256,
        })?;
    }
    io.work(
        worker,
        Job::Prepare(now()?),
        Some((Stage::Verifying, manifest.size)),
    )?;
    check_cancel()?;
    let Event::Receipt(receipt) =
        io.work(worker, Job::Commit, Some((Stage::Syncing, manifest.size)))?
    else {
        return invalid();
    };
    let receipt = receipt.ok_or(Failure::Cancelled)?;
    after_receipt((|| {
        io.send(Frame::Commit {
            transfer: receipt.transfer,
            size: receipt.size,
            sha256: receipt.sha256,
        })?;
        match io.next(None)? {
            Some(Frame::Finish { transfer, sha256 })
                if transfer == receipt.transfer && sha256 == receipt.sha256 => {}
            _ => return invalid(),
        }
        io.finish()
    })())
}
fn transmit(io: &mut Pipes, worker: &Worker, launch: Launch) -> Result<()> {
    let Event::Source { size, mode, mtime } = io.work(worker, Job::Source(launch.clone()), None)?
    else {
        return invalid();
    };
    let Event::Hash(hash) =
        io.work(worker, Job::Hash(size, false), Some((Stage::Hashing, size)))?
    else {
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
    manifest.validate()?;
    io.send(Frame::Manifest(manifest))?;
    let Some(Frame::Resume {
        phase,
        offset,
        sha256,
    }) = io.next(None)?
    else {
        return invalid();
    };
    if offset > size
        || (phase == Phase::Created && offset != 0)
        || (matches!(
            phase,
            Phase::Verifying | Phase::CommitPrepared | Phase::Committed
        ) && offset != size)
    {
        return invalid();
    }
    let Event::Hash(prefix) = io.work(
        worker,
        Job::Hash(offset, true),
        Some((Stage::PrefixHash, offset)),
    )?
    else {
        return invalid();
    };
    if prefix != sha256 {
        return Err(Failure::File(CopyError::HashMismatch));
    }
    io.credit = Some(Credit {
        sent: offset,
        durable: offset,
        hash: prefix,
        prefixes: VecDeque::new(),
    });
    io.send(Frame::Prefix {
        offset,
        sha256: prefix,
    })?;
    while io.credit.as_ref().unwrap().sent < size {
        io.ingest()?;
        if io.incoming.is_some() || io.eof {
            return invalid();
        }
        let credit = io.credit.as_ref().unwrap();
        if credit.sent - credit.durable >= protocol::DURABLE_WINDOW {
            io.poll(false)?;
            continue;
        }
        let offset = credit.sent;
        let remaining = protocol::DURABLE_WINDOW - (credit.sent - credit.durable);
        let limit = usize::try_from(remaining.min(marmish_stream::MAX_BLOCK as u64))
            .map_err(|_| Failure::File(CopyError::Protocol))?;
        let Event::Data(bytes, prefix) = io.work(worker, Job::ReadBounded(offset, limit), None)?
        else {
            return invalid();
        };
        if bytes.is_empty() {
            return Err(Failure::File(CopyError::SourceChanged));
        }
        io.credit
            .as_mut()
            .unwrap()
            .offer(offset, bytes.len(), prefix)?;
        io.send(Frame::Data { offset, bytes })?;
    }
    io.work(worker, Job::Unchanged, None)?;
    io.send(Frame::Done { size, sha256: hash })?;
    match io.next(None)? {
        Some(Frame::Commit {
            transfer,
            size: n,
            sha256,
        }) if transfer == launch.transfer
            && n == size
            && sha256 == hash
            && io.credit.as_ref().unwrap().durable == size => {}
        _ => return invalid(),
    }
    io.credit = None;
    io.finished = true;
    io.send(Frame::Finish {
        transfer: launch.transfer,
        sha256: hash,
    })?;
    io.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::FromRawFd;

    fn pipe() -> (File, File) {
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // Each newly returned descriptor is transferred into exactly one File.
        unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
    }
    fn incoming(frames: &[Frame], partial: &[u8]) -> Pipes {
        let (input, mut writer) = pipe();
        let (_reader, output) = pipe();
        for frame in frames {
            writer.write_all(&frame.encode().unwrap()).unwrap();
        }
        writer.write_all(partial).unwrap();
        drop(writer);
        nonblocking(&input).ok().unwrap();
        nonblocking(&output).ok().unwrap();
        Pipes::new(input, output)
    }
    fn credit() -> Credit {
        Credit {
            sent: 0,
            durable: 0,
            hash: [0; 32],
            prefixes: VecDeque::new(),
        }
    }
    fn protocol_failure<T>(result: Result<T>) -> bool {
        matches!(result, Err(Failure::File(CopyError::Protocol)))
    }

    #[test]
    fn post_receipt_mapping_preserves_peer_cancel_and_preparation_failures() {
        for code in [CopyError::Protocol, CopyError::LocalIo, CopyError::DiskFull] {
            assert!(matches!(
                after_receipt::<()>(Err(Failure::File(code))),
                Err(Failure::File(CopyError::CommitUncertain))
            ));
        }
        assert!(matches!(
            after_receipt::<()>(Err(Failure::Cancelled)),
            Err(Failure::Cancelled)
        ));
        assert!(matches!(
            after_receipt::<()>(Err(Failure::Peer)),
            Err(Failure::Peer)
        ));
        assert!(matches!(
            after_receipt::<()>(Err(Failure::Build)),
            Err(Failure::Build)
        ));
        assert!(matches!(
            after_receipt::<()>(Err(Failure::Launch)),
            Err(Failure::Launch)
        ));
        assert!(matches!(after_receipt(Ok(7)), Ok(7)));
    }

    #[test]
    fn credit_requires_a_recorded_matching_hash_before_advancing() {
        let mut credit = credit();
        credit.offer(0, MAX_BLOCK, [1; 32]).ok().unwrap();
        credit
            .offer(MAX_BLOCK as u64, MAX_BLOCK, [2; 32])
            .ok()
            .unwrap();
        assert!(credit.acknowledge(MAX_BLOCK as u64, [9; 32]).is_err());
        assert!(credit.acknowledge(1, [1; 32]).is_err());
        assert!(credit.acknowledge(3 * MAX_BLOCK as u64, [2; 32]).is_err());
        assert_eq!(credit.durable, 0);
        credit.acknowledge(MAX_BLOCK as u64, [1; 32]).ok().unwrap();
        assert_eq!(credit.durable, MAX_BLOCK as u64);
        assert_eq!(credit.prefixes.len(), 1);
        // An identical retry is allowed, but rollback and a changed retry fail.
        credit.acknowledge(MAX_BLOCK as u64, [1; 32]).ok().unwrap();
        assert!(credit.acknowledge(0, [0; 32]).is_err());
        assert!(credit.acknowledge(MAX_BLOCK as u64, [8; 32]).is_err());
        assert_eq!(credit.durable, MAX_BLOCK as u64);
    }

    #[test]
    fn durable_window_is_bounded_and_acknowledgement_releases_credit() {
        let mut credit = credit();
        let count = protocol::DURABLE_WINDOW as usize / MAX_BLOCK;
        for _ in 0..count {
            credit.offer(credit.sent, MAX_BLOCK, [1; 32]).ok().unwrap();
        }
        assert_eq!(credit.prefixes.len(), count);
        assert!(credit.offer(credit.sent, 1, [2; 32]).is_err());
        assert_eq!(credit.sent, protocol::DURABLE_WINDOW);
        credit.acknowledge(MAX_BLOCK as u64, [1; 32]).ok().unwrap();
        credit.offer(credit.sent, MAX_BLOCK, [2; 32]).ok().unwrap();
        assert_eq!(credit.sent - credit.durable, protocol::DURABLE_WINDOW);
        assert_eq!(credit.prefixes.len(), count);
    }

    #[test]
    fn credit_rejects_noncontiguous_and_overflowing_offers() {
        let mut credit = credit();
        assert!(credit.offer(1, 1, [1; 32]).is_err());
        assert!(credit.offer(0, 0, [1; 32]).is_err());
        assert!(credit.offer(0, MAX_BLOCK + 1, [1; 32]).is_err());
        credit.sent = u64::MAX;
        credit.durable = u64::MAX;
        assert!(credit.offer(u64::MAX, 1, [1; 32]).is_err());
        assert!(credit.prefixes.is_empty());
    }

    #[test]
    fn progress_before_start_is_not_silently_accepted() {
        let frame = Frame::Progress {
            stage: Stage::Hashing,
            examined: 0,
            total: 0,
        };
        let mut io = incoming(&[frame], &[]);
        io.ingest().ok().unwrap();
        assert!(matches!(io.incoming, Some(Frame::Progress { .. })));
    }

    #[test]
    fn finish_requires_exact_eof_and_rejects_even_trailing_progress() {
        let mut empty = incoming(&[], &[]);
        empty.started = true;
        empty.finish().ok().unwrap();
        for frame in [
            Frame::Progress {
                stage: Stage::Hashing,
                examined: 0,
                total: 0,
            },
            Frame::Done {
                size: 0,
                sha256: [0; 32],
            },
            Frame::Error(CopyError::LocalIo),
        ] {
            let mut io = incoming(&[frame], &[]);
            io.started = true;
            assert!(protocol_failure(io.finish()));
        }
    }

    #[test]
    fn truncated_final_frame_fails_instead_of_becoming_clean_eof() {
        let frame = Frame::Done {
            size: 0,
            sha256: [0; 32],
        }
        .encode()
        .unwrap();
        let mut io = incoming(&[], &frame[..frame.len() - 1]);
        io.started = true;
        assert!(protocol_failure(io.finish()));
    }

    #[test]
    fn reverse_checkpoints_release_credit_without_occupying_the_control_slot() {
        let mut io = incoming(
            &[
                Frame::Checkpoint {
                    offset: MAX_BLOCK as u64,
                    sha256: [1; 32],
                },
                Frame::Done {
                    size: MAX_BLOCK as u64,
                    sha256: [1; 32],
                },
            ],
            &[],
        );
        io.started = true;
        let mut credit = credit();
        credit.offer(0, MAX_BLOCK, [1; 32]).ok().unwrap();
        io.credit = Some(credit);
        io.ingest().ok().unwrap();
        assert_eq!(io.credit.as_ref().unwrap().durable, MAX_BLOCK as u64);
        assert!(matches!(io.incoming, Some(Frame::Done { .. })));
    }

    #[test]
    fn malformed_input_can_still_receive_a_finite_error_response() {
        let (input, mut writer) = pipe();
        let (mut reader, output) = pipe();
        writer.write_all(&[0xff, 0, 0, 0, 0]).unwrap();
        drop(writer);
        nonblocking(&input).ok().unwrap();
        nonblocking(&output).ok().unwrap();
        let mut io = Pipes::new(input, output);
        io.started = true;
        assert!(io.ingest().is_err());
        io.fail(CopyError::Protocol).ok().unwrap();
        drop(io);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        let mut decoder = FrameDecoder::default();
        let (used, frame) = decoder.feed(&bytes).unwrap();
        assert_eq!(used, bytes.len());
        assert!(matches!(frame, Some(Frame::Error(CopyError::Protocol))));
        decoder.finish().unwrap();
    }

    #[test]
    fn startup_error_requires_a_writable_pipe_and_emits_finite_code() {
        let (mut reader, mut writer) = pipe();
        startup_error(&mut writer, CopyError::Unsupported);
        drop(writer);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        let mut decoder = FrameDecoder::default();
        let (_, frame) = decoder.feed(&bytes).unwrap();
        assert!(matches!(frame, Some(Frame::Error(CopyError::Unsupported))));
        let mut ordinary = tempfile::tempfile().unwrap();
        startup_error(&mut ordinary, CopyError::Protocol);
        assert_eq!(ordinary.metadata().unwrap().len(), 0);
        let (mut read_only, writer) = pipe();
        startup_error(&mut read_only, CopyError::Protocol);
        drop(writer);
        let mut bytes = Vec::new();
        read_only.read_to_end(&mut bytes).unwrap();
        assert!(bytes.is_empty());
    }
}

#[cfg(test)]
#[path = "copy_helper_context_tests.rs"]
mod context_tests;
