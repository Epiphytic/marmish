//! Synchronous frontend turn over a locally admitted attachment.

use super::terminal_frontend_display::{Display, Marker};
use super::terminal_frontend_signals::Signals;
use super::terminal_frontend_tty::Tty;
use super::terminal_input::Input;
use super::{Error, Result};
use crate::terminal_ipc_wire::Close;
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

const QUEUE: usize = 32 * 1024;
const RECORD: usize = 4096;
const INPUT_CARRY: usize = 6;
const RESTORE_TIME: Duration = Duration::from_millis(50);
const PARTIAL_TIME: Duration = Duration::from_millis(25);
const IPC: Error = Error::Invalid("terminal_local_channel");
const POLL: Error = Error::Invalid("terminal_frontend_poll");

// This seam represents a local attachment already admitted by the daemon.
// One nonblocking disposition per call. A newer resize may replace a pending one.
pub(super) trait Channel {
    fn fd(&self) -> RawFd;
    fn read_output(&mut self, bytes: &mut [u8]) -> Result<Option<usize>>;
    fn write_input(&mut self, bytes: &[u8]) -> Result<Option<usize>>;
    fn resize(&mut self, rows: u16, cols: u16) -> Result<bool>;

    fn framed(&self) -> bool {
        false
    }
    fn wants_read(&self) -> bool {
        true
    }
    fn buffered(&self) -> bool {
        false
    }
    fn wants_write(&self, has_input: bool) -> bool {
        has_input
    }
    fn wait_for_input_progress(&mut self, _has_input: bool, _now: Instant) {}
    fn deadline(&self) -> Option<Instant> {
        None
    }
    fn display_committed(&mut self) -> Result<()> {
        Ok(())
    }
    fn begin_detach(&mut self, _now: Instant) {}
    fn detach_complete(&self) -> bool {
        true
    }
    fn peer_close(&self) -> Option<Close> {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Exit {
    Detached,
    Suspended,
    Interrupted(libc::c_int),
    Closed,
    PeerClosed(Close),
}

pub(super) struct Finished {
    pub(super) reason: Result<Exit>,
    pub(super) restored_display: bool,
    pub(super) restored_termios: Result<()>,
}

pub(super) fn run(
    tty: &mut Tty,
    signals: &Signals,
    channel: &mut impl Channel,
    observer: bool,
    admitted_size: Option<(u16, u16)>,
    discard_typeahead: bool,
    input_batch: Duration,
) -> Finished {
    let mut display = Display::default();
    let startup = signals.take();
    let (raw, reason) = if let Some(signal) = startup.termination {
        (false, Ok(Exit::Interrupted(signal)))
    } else if startup.has(libc::SIGTSTP) {
        (false, Ok(Exit::Suspended))
    } else if startup.has(libc::SIGTTIN) || startup.has(libc::SIGTTOU) {
        (false, Err(POLL))
    } else {
        let entered = if discard_typeahead {
            tty.raw_flushing()
        } else {
            tty.raw()
        };
        match entered {
            Ok(()) => (
                true,
                drive(
                    tty,
                    signals,
                    channel,
                    observer,
                    admitted_size,
                    input_batch,
                    &mut display,
                ),
            ),
            Err(error) => (false, Err(error)),
        }
    };
    let marker = match reason {
        Ok(Exit::Detached) => Marker::Detached,
        Ok(Exit::Suspended) => Marker::Suspended,
        Ok(Exit::Interrupted(_)) => Marker::Interrupted,
        Ok(Exit::PeerClosed(Close::Ended)) => Marker::Ended,
        Ok(Exit::Closed | Exit::PeerClosed(_)) | Err(_) => Marker::TerminalError,
    };
    let mut cleanup = display.cleanup(marker);
    let began = Instant::now();
    while raw && began.elapsed() < RESTORE_TIME && tty.foreground().is_ok() {
        if began.elapsed() >= PARTIAL_TIME {
            cleanup.abandon_partial();
        }
        let mut fd = poll_fd(tty.fd(), libc::POLLOUT);
        if poll(std::slice::from_mut(&mut fd), 1).is_err() {
            break;
        }
        if fd.revents & libc::POLLOUT != 0 {
            match cleanup.write(|bytes| tty.write(bytes)) {
                Ok(true) | Err(_) => break,
                Ok(false) => {}
            }
        }
    }
    let restored_termios = tty.restore();
    // The first externally delivered termination still wins if it arrived
    // during bounded normalization. The wrapper closes IPC before stopping.
    let reason = signals
        .take()
        .termination
        .map_or(reason, |signal| Ok(Exit::Interrupted(signal)));
    Finished {
        reason,
        restored_display: !raw || cleanup.complete(),
        restored_termios,
    }
}

fn drive(
    tty: &mut Tty,
    signals: &Signals,
    channel: &mut impl Channel,
    observer: bool,
    admitted_size: Option<(u16, u16)>,
    input_batch: Duration,
    display: &mut Display,
) -> Result<Exit> {
    let mut input = Input::new(observer);
    let mut queued = Vec::with_capacity(QUEUE);
    let mut keyboard = [0; RECORD];
    let mut output = [0; RECORD];
    let mut pending_output = 0..0;
    let mut record_pending = false;
    let mut detaching = false;
    let framed = channel.framed();
    let mut batch = InputBatch::new(if framed && !observer {
        input_batch
    } else {
        Duration::ZERO
    });
    let mut size = tty.geometry()?;
    let mut resize = (!observer && admitted_size.is_some_and(|sent| sent != size)).then_some(size);
    let mut last_tick = Instant::now();
    loop {
        let events = signals.take();
        if let Some(signal) = events.termination {
            return Ok(Exit::Interrupted(signal));
        }
        if events.has(libc::SIGTSTP) {
            return Ok(Exit::Suspended);
        }
        if events.has(libc::SIGTTIN) || events.has(libc::SIGTTOU) {
            return Err(POLL);
        }
        tty.foreground()?;
        if events.has(libc::SIGWINCH)
            && !observer
            && let Ok(latest) = tty.geometry()
            && latest != size
        {
            size = latest;
            resize = Some(latest);
        }

        let now = Instant::now();
        if channel.deadline().is_some_and(|end| now >= end) {
            return Err(Error::Timeout);
        }
        let elapsed = now.duration_since(last_tick);
        last_tick = now;
        let read_keyboard =
            !detaching && resize.is_none() && queued.len() <= QUEUE - RECORD - INPUT_CARRY;
        input.set_paste_mode(display.paste_enabled(), &mut queued);
        input.advance(
            elapsed,
            if read_keyboard {
                elapsed
            } else {
                Duration::ZERO
            },
            &mut queued,
        );
        display.advance(elapsed)?;
        pending_output.start += display.ingest(&output[pending_output.clone()])?;

        if record_pending && display.record_committed(pending_output.len())? {
            channel.display_committed()?;
            record_pending = false;
        }
        if framed
            && !detaching
            && let Some((rows, cols)) = resize
        {
            channel.resize(rows, cols)?;
            resize = None;
        }
        let read_channel = !detaching
            && pending_output.is_empty()
            && !record_pending
            && display.can_ingest()
            && channel.wants_read();
        let buffered = read_channel && channel.buffered();
        channel.wait_for_input_progress(!queued.is_empty(), now);
        let sampled = Instant::now();
        let ready = batch.ready(queued.len(), sampled);
        let tty_events =
            event(read_keyboard, libc::POLLIN) | event(display.has_pending(), libc::POLLOUT);
        let channel_events = event(read_channel && !buffered, libc::POLLIN)
            | event(
                channel.wants_write(ready) || resize.is_some(),
                libc::POLLOUT,
            );
        let mut fds = [
            poll_fd(tty.fd(), tty_events),
            poll_fd(signals.fd(), libc::POLLIN),
            poll_fd(
                if channel_events == 0 {
                    -1
                } else {
                    channel.fd()
                },
                channel_events,
            ),
        ];
        let timeout = [
            input.deadline(read_keyboard),
            display.deadline(),
            channel
                .deadline()
                .map(|end| end.saturating_duration_since(now)),
            batch.remaining(sampled),
        ];
        poll(&mut fds, poll_timeout(buffered, timeout))?;
        if fds[1].revents != 0 || signals.pending() {
            continue;
        }
        let after_poll = Instant::now();
        let elapsed = after_poll.duration_since(last_tick);
        last_tick = after_poll;
        input.advance(
            elapsed,
            if read_keyboard {
                elapsed
            } else {
                Duration::ZERO
            },
            &mut queued,
        );
        display.advance(elapsed)?;

        let local = fds[0].revents;
        let remote = fds[2].revents;
        if local & (libc::POLLERR | libc::POLLNVAL | libc::POLLHUP) != 0 {
            return Err(POLL);
        }
        if remote & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(IPC);
        }
        if !framed && remote & libc::POLLHUP != 0 {
            return Ok(Exit::Closed);
        }
        if signals.pending() {
            continue;
        }
        if local & libc::POLLOUT != 0 {
            display.write(|bytes| tty.write(bytes))?;
        }
        input.set_paste_mode(display.paste_enabled(), &mut queued);
        if signals.pending() {
            continue;
        }
        if local & libc::POLLIN != 0 {
            match read_keys(tty, &mut keyboard, &mut input, &mut queued)? {
                Some(Exit::Detached) if framed => {
                    queued.clear();
                    batch.ready(0, after_poll);
                    channel.begin_detach(after_poll);
                    detaching = true;
                }
                Some(exit) => {
                    if !framed {
                        try_input(channel, &mut queued, true, false)?;
                    }
                    return Ok(exit);
                }
                None => {}
            }
        }
        if signals.pending() {
            continue;
        }
        let ready = batch.ready(queued.len(), Instant::now());
        if remote & libc::POLLOUT != 0 {
            try_input(channel, &mut queued, ready, framed)?;
            batch.ready(queued.len(), Instant::now());
            if detaching && channel.detach_complete() {
                return Ok(Exit::Detached);
            }
            if !framed
                && let Some((rows, cols)) = resize
                && queued.is_empty()
                && channel.resize(rows, cols)?
            {
                resize = None;
            }
        }
        if framed && detaching && remote & libc::POLLHUP != 0 {
            return Err(IPC);
        }
        if !detaching
            && (buffered || read_channel && remote & (libc::POLLIN | libc::POLLHUP) != 0)
            && let Some(count) = channel.read_output(&mut output)?
        {
            match count {
                0 if framed => return channel.peer_close().map(Exit::PeerClosed).ok_or(IPC),
                0 => return Ok(Exit::Closed),
                1..=RECORD => {
                    pending_output = 0..count;
                    record_pending = framed;
                }
                _ => return Err(IPC),
            }
        }
    }
}

struct InputBatch {
    delay: Duration,
    started: Option<Instant>,
    eligible: bool,
}

impl InputBatch {
    fn new(delay: Duration) -> Self {
        Self {
            delay,
            started: None,
            eligible: false,
        }
    }

    fn ready(&mut self, queued: usize, now: Instant) -> bool {
        if queued == 0 {
            self.started = None;
            self.eligible = false;
            return false;
        }
        let started = *self.started.get_or_insert(now);
        self.eligible |= queued >= RECORD || now.duration_since(started) >= self.delay;
        self.eligible
    }

    fn remaining(&self, now: Instant) -> Option<Duration> {
        let started = self.started.filter(|_| !self.eligible)?;
        let remaining = self.delay.saturating_sub(now.duration_since(started));
        (!remaining.is_zero()).then_some(remaining)
    }
}

fn try_input(
    channel: &mut impl Channel,
    queued: &mut Vec<u8>,
    ready: bool,
    controls: bool,
) -> Result<()> {
    let offered = if ready { queued.len().min(RECORD) } else { 0 };
    if (offered != 0 || controls)
        && let Some(count) = channel.write_input(&queued[..offered])?
    {
        if count == 0 || count > offered {
            return Err(IPC);
        }
        queued.drain(..count);
    }
    Ok(())
}

fn read_keys(
    tty: &mut Tty,
    bytes: &mut [u8; RECORD],
    input: &mut Input,
    queued: &mut Vec<u8>,
) -> Result<Option<Exit>> {
    let Some(count) = tty.read(bytes)? else {
        return Ok(None);
    };
    if count == 0 {
        input.flush(queued);
        return Ok(Some(Exit::Closed));
    }
    let detached = bytes[..count].iter().any(|&byte| input.push(byte, queued));
    Ok(detached.then_some(Exit::Detached))
}

fn poll_timeout(buffered: bool, deadlines: [Option<Duration>; 4]) -> i32 {
    if buffered {
        return 0;
    }
    let Some(deadline) = deadlines.into_iter().flatten().min() else {
        return -1;
    };
    let millis = deadline.as_nanos().div_ceil(1_000_000);
    millis.min(i32::MAX as u128) as i32
}

fn event(enabled: bool, event: libc::c_short) -> libc::c_short {
    if enabled { event } else { 0 }
}

pub(super) fn poll_fd(fd: RawFd, events: libc::c_short) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}

pub(super) fn poll(fds: &mut [libc::pollfd], millis: i32) -> Result<()> {
    if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) } >= 0
        || std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
    {
        Ok(())
    } else {
        Err(POLL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_runtime_core_batch_uses_first_byte_and_stays_ready_after_partial_write() {
        let mut batch = InputBatch::new(Duration::from_millis(8));
        let start = Instant::now();
        let early = start + Duration::from_millis(5);
        let deadline = start + Duration::from_millis(8);
        assert!(!batch.ready(1, start));
        assert!(!batch.ready(2, early));
        assert_eq!(batch.remaining(early), Some(Duration::from_millis(3)));
        assert_eq!(batch.remaining(deadline), None);
        assert!(batch.ready(3, deadline));
        assert!(batch.ready(1, deadline));
        assert_eq!(batch.remaining(deadline), None);
        assert!(!batch.ready(0, deadline));
        assert!(!batch.ready(1, deadline));
        assert_eq!(batch.remaining(deadline), Some(Duration::from_millis(8)));
    }

    #[test]
    fn terminal_runtime_core_batch_full_record_and_zero_delay_are_immediate() {
        let now = Instant::now();
        let mut batch = InputBatch::new(Duration::from_millis(50));
        assert!(!batch.ready(RECORD - 1, now));
        assert!(batch.ready(RECORD, now));
        assert!(batch.ready(1, now));
        assert_eq!(batch.remaining(now), None);
        let mut immediate = InputBatch::new(Duration::ZERO);
        assert!(immediate.ready(1, now));
        assert_eq!(immediate.remaining(now), None);
    }
}
