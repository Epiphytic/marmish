//! Fixed-size nonblocking PTY dispatcher, separate from tmux and SDK ownership.
use super::guard::{self, Sample, Source};
use super::{Claim, GuardResult, MAX_LIVE, Reason, STOPPED, SlotId, StartOutcome, backend, refuse};
use backend::{Attachment, ReadStatus, WriteStatus};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::{
    Arc, Mutex, MutexGuard, TryLockError,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, oneshot};

pub(super) const BYTES: usize = 4096;
const BATCH: Duration = Duration::from_millis(8);
const HEALTH: Duration = Duration::from_millis(10);
const STALL: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub(super) struct Wake(Arc<UnixStream>);

impl Wake {
    pub fn open() -> io::Result<(Self, UnixStream)> {
        let (writer, reader) = UnixStream::pair()?;
        writer.set_nonblocking(true)?;
        reader.set_nonblocking(true)?;
        Ok((Self(Arc::new(writer)), reader))
    }

    pub fn notify(&self) {
        let _ = self.0.as_ref().write(&[0]);
    }
}

pub(super) enum Input {
    Bytes(Vec<u8>),
    Resize { rows: u16, cols: u16 },
}

pub(super) enum PushError {
    Contended(Operation),
    Full,
    Closed(Reason),
}

pub(super) struct Operation {
    pub sequence: u64,
    pub deadline: Duration,
    pub input: Input,
    offset: usize,
}

impl Operation {
    pub fn new(sequence: u64, deadline: Duration, input: Input) -> Option<Self> {
        let valid = match &input {
            Input::Bytes(bytes) => (1..=BYTES).contains(&bytes.len()),
            Input::Resize { rows, cols } => super::valid_size(*rows, *cols),
        };
        (valid && sequence != 0).then_some(Self {
            sequence,
            deadline,
            input,
            offset: 0,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Completion {
    Written(u64, usize),
    Resized(u64),
}

struct Output {
    bytes: Vec<u8>,
    at: Duration,
}

#[derive(Default)]
struct Buffer {
    input: Option<Operation>,
    in_flight: bool,
    complete: Option<Completion>,
    output: Option<Output>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Event {
    Complete(Completion),
    Output(Vec<u8>),
    Closed(Reason),
}

pub(super) struct Port {
    pub source: Arc<Source>,
    wake: Wake,
    changed: Notify,
    event_retry: AtomicBool,
    buffer: Mutex<Buffer>,
}

impl Port {
    pub(super) fn new(source: Arc<Source>, wake: Wake) -> Arc<Self> {
        Arc::new(Self {
            source,
            wake,
            changed: Notify::new(),
            event_retry: AtomicBool::new(false),
            buffer: Mutex::default(),
        })
    }

    fn lock(&self) -> Option<MutexGuard<'_, Buffer>> {
        match self.buffer.try_lock() {
            Ok(buffer) => Some(buffer),
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Poisoned(_)) => {
                self.close(Reason::Io);
                None
            }
        }
    }

    pub(super) fn push(&self, operation: Operation) -> Result<(), PushError> {
        let Some(mut buffer) = self.lock() else {
            return Err(match self.source.reason() {
                Some(reason) => PushError::Closed(reason),
                None => PushError::Contended(operation),
            });
        };
        if let Some(reason) = self.source.reason() {
            return Err(PushError::Closed(reason));
        }
        if buffer.in_flight {
            return Err(PushError::Full);
        }
        buffer.in_flight = true;
        buffer.input = Some(operation);
        drop(buffer);
        self.wake.notify();
        Ok(())
    }

    pub(super) fn event(&self) -> Option<Event> {
        let reason = self.source.reason();
        if let Some(reason) = reason.filter(|reason| *reason != Reason::ShellExit) {
            return Some(Event::Closed(reason));
        }
        let Some(mut buffer) = self.lock() else {
            // The one consumer may already have consumed the producer's
            // coalesced permit. A contested probe needs another look even
            // when the owner publishes no additional event.
            self.event_retry.store(true, Ordering::Release);
            return None;
        };
        let event = if let Some(complete) = buffer.complete.take() {
            buffer.in_flight = false;
            Some(Event::Complete(complete))
        } else if let Some(output) = buffer.output.take() {
            Some(Event::Output(output.bytes))
        } else {
            reason.map(Event::Closed)
        };
        drop(buffer);
        if event.is_some() {
            self.wake.notify();
        }
        event
    }

    #[cfg(test)]
    pub(super) fn output_is_queued(&self) -> bool {
        self.lock().is_some_and(|buffer| buffer.output.is_some())
    }

    pub(super) async fn changed(&self) {
        if self.event_retry.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
            self.event_retry.store(false, Ordering::Release);
        } else {
            self.changed.notified().await;
        }
    }

    pub(super) fn close(&self, reason: Reason) {
        self.source.retire(reason);
        self.wake.notify();
        self.changed.notify_one();
    }
}

pub(super) struct Native {
    pub slot: SlotId,
    pub claim: Claim,
    pub port: Arc<Port>,
    pub attachment: Attachment,
}

pub(super) struct Installing {
    pub native: Native,
    pub deadline: Duration,
    pub reply: oneshot::Sender<StartOutcome>,
}

struct Session {
    native: Option<Native>,
    pending: Option<Operation>,
    complete: Option<Completion>,
    stage: Vec<u8>,
    first_byte: Option<Duration>,
    next: Duration,
    interests: libc::c_short,
    retry_write: Duration,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(native) = self.native.as_ref() {
            native.port.close(Reason::OwnerLost);
        }
    }
}

impl Session {
    fn new(native: Native) -> Self {
        Self {
            native: Some(native),
            pending: None,
            complete: None,
            stage: Vec::with_capacity(BYTES),
            first_byte: None,
            next: Duration::ZERO,
            interests: libc::POLLIN,
            retry_write: Duration::ZERO,
        }
    }

    fn next_poll(&mut self, at: Sample) -> Option<Reason> {
        let native = self.native.as_ref()?;
        let port = &native.port;
        let Some(mut next) = at.continuous.checked_add(HEALTH) else {
            return Some(Reason::Clock);
        };
        match port.source.health(native.claim, guard::sample) {
            GuardResult::Applied(Some(deadline)) => next = next.min(deadline),
            GuardResult::Retired(reason) => return Some(reason),
            GuardResult::Withheld => {
                self.next = next;
                self.interests = 0;
                return None;
            }
            GuardResult::Applied(None) => return Some(Reason::Clock),
        }
        let mut staged_output_blocked = true;
        if let Some(mut buffer) = port.lock() {
            if self.pending.is_none() {
                self.pending = buffer.input.take();
            }
            if let Some(complete) = self.complete.take() {
                buffer.complete = Some(complete);
                port.changed.notify_one();
            }
            if let Some(output) = buffer.output.as_ref() {
                let until = output.at.checked_add(STALL).unwrap_or_default();
                if at.continuous >= until {
                    return Some(Reason::Deadline);
                }
                next = next.min(until);
            } else {
                staged_output_blocked = false;
            }
            if let Some(first) = self.first_byte {
                let batch = first.checked_add(BATCH).unwrap_or_default();
                if !staged_output_blocked && (at.continuous >= batch || self.stage.len() == BYTES) {
                    if let Some(reason) = port.source.reason() {
                        return Some(reason);
                    }
                    buffer.output = Some(Output {
                        bytes: std::mem::replace(&mut self.stage, Vec::with_capacity(BYTES)),
                        at: at.continuous,
                    });
                    self.first_byte = None;
                    port.changed.notify_one();
                } else if !staged_output_blocked {
                    next = next.min(batch);
                }
            }
        }
        if let Some(pending) = &self.pending {
            if at.continuous >= pending.deadline {
                return Some(Reason::Deadline);
            }
            next = next.min(pending.deadline);
        }
        self.interests = 0;
        if self.stage.len() < BYTES {
            self.interests |= libc::POLLIN;
        }
        if self.pending.is_some() && at.continuous < self.retry_write {
            next = next.min(self.retry_write);
        } else if self.pending.is_some() && matches!(port.source.check(), GuardResult::Applied(()))
        {
            // Resize is also eligible on the bounded health turn if the master
            // is temporarily not writable.
            self.interests |= libc::POLLOUT;
        }
        self.next = next;
        None
    }

    fn ready(&mut self, flags: libc::c_short, stopping: &AtomicBool) -> Option<Reason> {
        if stopping.load(Ordering::Acquire) {
            return Some(Reason::OwnerLost);
        }
        let native = self.native.as_mut()?;
        if let Some(reason) = native.port.source.reason() {
            return Some(reason);
        }
        let Some(at) = guard::sample() else {
            return Some(Reason::Clock);
        };
        if flags & libc::POLLNVAL != 0 {
            return Some(Reason::Io);
        }
        let hung = flags & (libc::POLLHUP | libc::POLLERR) != 0;
        let exited = match native
            .attachment
            .child
            .as_ref()
            .map(|child| child.waitable())
        {
            Some(Ok(exited)) => exited,
            _ => return Some(Reason::Io),
        };
        let may_read = match native.port.source.health(native.claim, guard::sample) {
            GuardResult::Retired(reason) => return Some(reason),
            GuardResult::Withheld => false,
            GuardResult::Applied(_) => true,
        };
        if may_read && flags & libc::POLLIN != 0 && self.stage.len() < BYTES {
            let mut bytes = [0u8; BYTES];
            let left = BYTES - self.stage.len();
            match native.attachment.read(&mut bytes[..left]) {
                Ok(ReadStatus::Data(count)) => {
                    self.first_byte.get_or_insert(at.continuous);
                    self.stage.extend_from_slice(&bytes[..count]);
                }
                Ok(ReadStatus::WouldBlock) => (),
                Ok(ReadStatus::Hangup) => return Some(Reason::ShellExit),
                Err(_) => return Some(Reason::Io),
            }
        }
        if hung || exited {
            return Some(Reason::ShellExit);
        }
        if at.continuous >= self.retry_write
            && let Some(pending) = self.pending.as_mut()
            && (flags & libc::POLLOUT != 0 || matches!(pending.input, Input::Resize { .. }))
        {
            match native.port.source.with_effect(
                native.claim,
                pending.deadline,
                guard::sample,
                || {
                    if stopping.load(Ordering::Acquire) {
                        return Err(Reason::OwnerLost);
                    }
                    apply(&mut native.attachment, native.claim, pending)
                },
            ) {
                GuardResult::Applied(Ok(Some(complete))) => {
                    self.complete = Some(complete);
                    self.pending = None;
                }
                GuardResult::Applied(Ok(None)) => (),
                GuardResult::Withheld => {
                    let Some(retry) = at.continuous.checked_add(HEALTH) else {
                        return Some(Reason::Clock);
                    };
                    self.retry_write = retry;
                }
                GuardResult::Applied(Err(reason)) | GuardResult::Retired(reason) => {
                    return Some(reason);
                }
            }
        }
        None
    }

    fn retire(mut self, reason: Reason) -> Option<Native> {
        let mut native = self.native.take()?;
        if reason == Reason::ShellExit
            && native.port.source.reason().is_none()
            && let Some(mut buffer) = native.port.lock()
        {
            if buffer.complete.is_none() {
                buffer.complete = self.complete.take();
            }
            if buffer.output.is_none()
                && !self.stage.is_empty()
                && let Some(at) = guard::sample()
                && matches!(
                    native.port.source.health(native.claim, guard::sample),
                    GuardResult::Applied(_)
                )
            {
                buffer.output = Some(Output {
                    bytes: std::mem::take(&mut self.stage),
                    at: at.continuous,
                });
            }
        }
        native.port.close(reason);
        drop(native.attachment.master.take());
        Some(native)
    }
}

fn apply(
    attachment: &mut Attachment,
    claim: Claim,
    operation: &mut Operation,
) -> Result<Option<Completion>, Reason> {
    match &operation.input {
        Input::Resize { rows, cols } => {
            attachment
                .resize(claim.generation, *rows, *cols)
                .map_err(|_| Reason::Io)?;
            Ok(Some(Completion::Resized(operation.sequence)))
        }
        Input::Bytes(bytes) => match attachment.write(claim.generation, &bytes[operation.offset..])
        {
            Ok(WriteStatus::Complete) => {
                Ok(Some(Completion::Written(operation.sequence, bytes.len())))
            }
            Ok(WriteStatus::Partial(count)) => {
                operation.offset += count;
                Ok(None)
            }
            Ok(WriteStatus::WouldBlock) => Ok(None),
            Ok(WriteStatus::Hangup) => Err(Reason::ShellExit),
            Err(_) => Err(Reason::Io),
        },
    }
}

pub(super) fn run(
    mut wake: UnixStream,
    install: mpsc::Receiver<Installing>,
    reaping: mpsc::SyncSender<Native>,
    stopping: &AtomicBool,
) -> crate::Result<()> {
    let mut sessions = Vec::<Session>::with_capacity(MAX_LIVE);
    loop {
        if stopping.load(Ordering::Acquire) {
            for session in sessions.drain(..) {
                send_retiring(&reaping, session, Reason::OwnerLost);
            }
            // Control closes the sender before joining this owner. Drain all
            // transferred handles, including a transfer racing the stop flag.
            for request in &install {
                send_retiring(&reaping, Session::new(request.native), Reason::OwnerLost);
                refuse(request.reply, STOPPED);
            }
            return Ok(());
        }
        if let Ok(request) = install.try_recv() {
            install_one(&mut sessions, &reaping, request, stopping);
        }
        let at = guard::sample();
        let mut pos = 0;
        while pos < sessions.len() {
            let reason = at.map_or(Some(Reason::Clock), |at| sessions[pos].next_poll(at));
            if let Some(reason) = reason {
                send_retiring(&reaping, sessions.remove(pos), reason);
            } else {
                pos += 1;
            }
        }
        let mut events = [backend::poll_descriptor(-1, 0); MAX_LIVE + 1];
        events[0] = backend::poll_descriptor(wake.as_raw_fd(), libc::POLLIN);
        let mut delay = HEALTH;
        for (session, event) in sessions.iter().zip(&mut events[1..]) {
            if let Some(native) = session.native.as_ref()
                && let Some(master) = native.attachment.master.as_ref()
            {
                *event = backend::poll_descriptor(master.as_raw_fd(), session.interests);
            }
            if let Some(at) = at {
                delay = delay.min(session.next.saturating_sub(at.continuous));
            }
        }
        backend::poll_fds(&mut events, Instant::now() + delay)?;
        if events[0].revents != 0 {
            // Bound the wake-drain under a continually writing peer. A
            // remaining readable wake simply starts the next health turn.
            let mut bytes = [0u8; BYTES];
            for _ in 0..16 {
                match wake.read(&mut bytes) {
                    Ok(count) if count > 0 => (),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
                    _ => break,
                }
            }
        }
        for (pos, event) in events[1..1 + sessions.len()].iter().enumerate().rev() {
            if let Some(reason) = sessions[pos].ready(event.revents, stopping) {
                send_retiring(&reaping, sessions.remove(pos), reason);
            }
        }
        if !sessions.is_empty() {
            sessions.rotate_right(1);
        }
    }
}

fn install_one(
    sessions: &mut Vec<Session>,
    reaping: &mpsc::SyncSender<Native>,
    request: Installing,
    stopping: &AtomicBool,
) {
    let Installing {
        native,
        deadline,
        reply,
    } = request;
    let reason = match guard::sample() {
        _ if stopping.load(Ordering::Acquire) => Some(Reason::OwnerLost),
        None => Some(Reason::Clock),
        Some(at) if at.continuous >= deadline || reply.is_closed() => Some(Reason::Deadline),
        Some(_) if sessions.len() == MAX_LIVE => Some(Reason::OwnerLost),
        Some(at) => match native.port.source.install(native.claim, at) {
            GuardResult::Applied(()) => None,
            GuardResult::Withheld => Some(Reason::Authority),
            GuardResult::Retired(reason) => Some(reason),
        },
    };
    if let Some(reason) = reason {
        send_retiring(reaping, Session::new(native), reason);
        let _ = reply.send(StartOutcome::Refused(reason.start_error()));
        return;
    }
    let reason = match (guard::sample(), native.port.source.check()) {
        _ if stopping.load(Ordering::Acquire) => Some(Reason::OwnerLost),
        (None, _) => Some(Reason::Clock),
        (Some(at), _) if at.continuous >= deadline || reply.is_closed() => Some(Reason::Deadline),
        (_, GuardResult::Withheld) => Some(Reason::Authority),
        (_, GuardResult::Retired(reason)) => Some(reason),
        (_, GuardResult::Applied(())) => None,
    };
    if let Some(reason) = reason {
        send_retiring(reaping, Session::new(native), reason);
        let _ = reply.send(StartOutcome::Refused(reason.start_error()));
        return;
    }
    let admitted = super::AdmittedTerminal {
        slot: native.slot,
        claim: native.claim,
        port: Arc::clone(&native.port),
        closed_reported: false,
    };
    sessions.push(Session::new(native));
    let _ = reply.send(StartOutcome::Attached(admitted));
}

fn send_retiring(reaping: &mpsc::SyncSender<Native>, session: Session, reason: Reason) {
    if let Some(native) = session.retire(reason) {
        // A full/disconnected return path cannot transfer ownership. Drop
        // falls back to the reaper that owns the same profile lease.
        let _ = reaping.try_send(native);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_fence::AttachmentFence;
    use std::fs::File;
    use std::num::NonZeroU64;
    use std::os::fd::OwnedFd;
    use std::process::Command;

    fn native(master: File, profile: Option<Arc<crate::identity::ProfileLease>>) -> Native {
        let claim = super::super::tests::claim();
        let (source, _) = super::super::tests::source(8);
        let (wake, _reader) = Wake::open().unwrap();
        let mut command = Command::new("/bin/sleep");
        command.arg("20");
        Native {
            slot: SlotId([8; 16]),
            attachment: Attachment::with_test_master(master, claim.generation, command, profile)
                .unwrap(),
            claim,
            port: Port::new(source, wake),
        }
    }

    fn socket_native() -> (Native, UnixStream) {
        let (master, peer) = UnixStream::pair().unwrap();
        master.set_nonblocking(true).unwrap();
        let master: OwnedFd = master.into();
        (native(master.into(), None), peer)
    }

    fn installed_session(native: Native) -> Session {
        let at = guard::sample().unwrap();
        assert_eq!(
            native.port.source.install(native.claim, at),
            GuardResult::Applied(())
        );
        Session::new(native)
    }

    #[cfg(target_os = "linux")]
    fn partial_wire() -> (Session, UnixStream) {
        let (native, peer) = socket_native();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut session = installed_session(native);
        let native = session.native.as_mut().unwrap();
        let master = native.attachment.master.as_mut().unwrap();
        let capacity = 1024_i32;
        // SAFETY: `master` owns the descriptor and the option copies one i32.
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    master.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&raw const capacity).cast(),
                    std::mem::size_of_val(&capacity) as libc::socklen_t,
                )
            },
            0
        );
        // One full skb fills half the Linux socket's minimum send buffer. A
        // 4 KiB head can fit only its first skb until the peer reads again.
        master.write_all(&[7; 2240]).unwrap();
        let at = guard::sample().unwrap();
        let due = at.continuous + Duration::from_secs(4);
        assert!(
            native
                .port
                .push(Operation::new(1, due, Input::Bytes(vec![9; BYTES])).unwrap())
                .is_ok()
        );
        assert_eq!(session.next_poll(at), None);
        assert_eq!(session.ready(libc::POLLOUT, &AtomicBool::new(false)), None);
        assert!((1..BYTES).contains(&session.pending.as_ref().unwrap().offset));
        (session, peer)
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn core_tests_terminal_live_supervisor_partial_write_one_receipt() {
        let (mut session, mut peer) = partial_wire();
        let port = Arc::clone(&session.native.as_ref().unwrap().port);
        let offset = session.pending.as_ref().unwrap().offset;
        let mut bytes = vec![0; 2240 + offset];
        peer.read_exact(&mut bytes).unwrap();
        assert!(bytes[..2240].iter().all(|byte| *byte == 7));
        assert!(bytes[2240..].iter().all(|byte| *byte == 9));
        assert_eq!(port.event(), None);
        assert_eq!(session.ready(libc::POLLOUT, &AtomicBool::new(false)), None);
        assert_eq!(session.next_poll(guard::sample().unwrap()), None);
        assert_eq!(
            port.event(),
            Some(Event::Complete(Completion::Written(1, BYTES)))
        );
        assert_eq!(port.event(), None);
        let mut suffix = vec![0; BYTES - offset];
        peer.read_exact(&mut suffix).unwrap();
        assert!(suffix.iter().all(|byte| *byte == 9));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn core_tests_terminal_live_supervisor_deadline_mid_partial() {
        let (mut session, mut peer) = partial_wire();
        let port = Arc::clone(&session.native.as_ref().unwrap().port);
        let head = session.pending.as_ref().unwrap();
        let now = guard::sample().unwrap();
        let at = Sample {
            continuous: head.deadline,
            wall: now.wall + head.deadline.saturating_sub(now.continuous),
        };
        let written = head.offset;
        assert_eq!(session.next_poll(at), Some(Reason::Deadline));
        drop(session.retire(Reason::Deadline));
        assert_eq!(port.event(), Some(Event::Closed(Reason::Deadline)));
        let mut bytes = Vec::new();
        peer.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len(), 2240 + written);
    }

    #[test]
    fn core_tests_terminal_live_supervisor_queued_install_cancels_without_native_effect() {
        let stopping = AtomicBool::new(false);
        let mut sessions = Vec::new();
        let (returning, returned) = mpsc::sync_channel(1);
        for case in 0..3 {
            let (native, mut peer) = socket_native();
            let (reply, receiver) = oneshot::channel();
            let at = guard::sample().unwrap();
            let deadline = if case == 0 {
                at.continuous
            } else {
                at.continuous + HEALTH
            };
            native
                .port
                .push(Operation::new(1, deadline, Input::Bytes(vec![1])).unwrap())
                .ok()
                .unwrap();
            if case == 1 {
                native.port.source.retire(Reason::Authority);
            }
            stopping.store(case == 2, Ordering::Release);
            install_one(
                &mut sessions,
                &returning,
                Installing {
                    native,
                    deadline,
                    reply,
                },
                &stopping,
            );
            assert!(matches!(
                receiver.blocking_recv().unwrap(),
                StartOutcome::Refused(_)
            ));
            assert!(sessions.is_empty());
            drop(returned.try_recv().unwrap());
            assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        }
    }

    #[test]
    fn core_tests_terminal_live_supervisor_hup_and_late_resize() {
        let (native, peer) = socket_native();
        let port = Arc::clone(&native.port);
        let mut session = installed_session(native);
        let at = guard::sample().unwrap();
        port.push(
            Operation::new(
                1,
                at.continuous + HEALTH,
                Input::Resize { rows: 24, cols: 80 },
            )
            .unwrap(),
        )
        .ok()
        .unwrap();
        assert_eq!(session.next_poll(at), None);
        // The fixture descriptor is a socket. Resize would report I/O error
        // if applied after the independent owner-stop fence.
        assert_eq!(
            session.ready(libc::POLLOUT, &AtomicBool::new(true)),
            Some(Reason::OwnerLost)
        );
        drop(peer);
        assert_eq!(
            session.ready(libc::POLLHUP, &AtomicBool::new(false)),
            Some(Reason::ShellExit)
        );
        drop(session.retire(Reason::ShellExit));
        assert_eq!(port.event(), Some(Event::Closed(Reason::ShellExit)));
    }

    #[tokio::test]
    #[ignore = "uses real profile lease, child, and private tmux fixture"]
    async fn terminal_live_supervisor_io_panic_releases_lease_after_reaper() {
        let fixture = super::super::super::tests::Fixture::new().await;
        let (profile, _material) = fixture.identity();
        let profile = Arc::new(profile);
        let (master, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let master: OwnedFd = master.into();
        let native = native(master.into(), Some(Arc::clone(&profile)));
        let port = Arc::clone(&native.port);
        let worker = std::thread::spawn(move || {
            let _session = installed_session(native);
            panic!("fixture I/O owner unwind");
        });
        assert!(worker.join().is_err());
        assert_eq!(port.event(), Some(Event::Closed(Reason::OwnerLost)));
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        tokio::time::timeout(Duration::from_secs(4), async {
            while Arc::strong_count(&profile) > 1 {
                tokio::time::sleep(HEALTH).await;
            }
        })
        .await
        .unwrap();
        assert!(Arc::try_unwrap(profile).is_ok());
    }

    #[test]
    fn core_tests_terminal_live_supervisor_contended_push_preserves_operation() {
        let (native, _peer) = socket_native();
        let port = &native.port;
        let deadline = Duration::from_secs(42);
        let operation = Operation::new(7, deadline, Input::Bytes(vec![0, 0xff])).unwrap();
        let buffer = port.buffer.lock().unwrap();
        let Err(PushError::Contended(operation)) = port.push(operation) else {
            panic!("contention must leave the entire input with the caller");
        };
        assert!(!buffer.in_flight);
        drop(buffer);

        assert!(port.push(operation).is_ok());
        let duplicate = Operation::new(8, deadline, Input::Resize { rows: 30, cols: 90 }).unwrap();
        assert!(matches!(port.push(duplicate), Err(PushError::Full)));
        let buffer = port.buffer.lock().unwrap();
        let saved = buffer.input.as_ref().unwrap();
        assert_eq!(
            (saved.sequence, saved.deadline, saved.offset),
            (7, deadline, 0)
        );
        assert!(matches!(&saved.input, Input::Bytes(bytes) if bytes == &[0, 0xff]));
        drop(buffer);

        port.close(Reason::Detached);
        let stale = Operation::new(9, deadline, Input::Bytes(vec![1])).unwrap();
        assert!(matches!(
            port.push(stale),
            Err(PushError::Closed(Reason::Detached))
        ));
    }

    #[tokio::test]
    async fn core_tests_terminal_live_supervisor_contended_event_rechecks_after_a_coalesced_wake() {
        let source = Source::new(
            super::super::SourceId {
                leaf: [3; 32],
                epoch: 2,
                subscription: NonZeroU64::MIN,
            },
            AttachmentFence::new(NonZeroU64::MIN, [4; 32]),
        );
        let (wake, _reader) = Wake::open().unwrap();
        let port = Port::new(source, wake);
        {
            let mut owned = port.buffer.lock().unwrap();
            owned.complete = Some(Completion::Resized(1));
            owned.output = Some(Output {
                bytes: vec![5],
                at: Duration::ZERO,
            });
            port.changed.notify_one();
        }
        port.changed().await;
        assert_eq!(port.event(), Some(Event::Complete(Completion::Resized(1))));
        {
            let _during_owner_turn = port.buffer.lock().unwrap();
            assert_eq!(port.event(), None);
        }
        tokio::time::timeout(Duration::from_secs(1), port.changed())
            .await
            .expect("contested event forgot the prior wake");
        assert_eq!(port.event(), Some(Event::Output(vec![5])));
        assert_eq!(port.event(), None);
    }
}
