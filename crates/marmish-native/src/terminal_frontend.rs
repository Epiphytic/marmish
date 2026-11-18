//! Synchronous frontend turn over a locally admitted attachment.

use super::terminal_frontend_display::{Display, Marker};
use super::terminal_frontend_signals::Signals;
use super::terminal_frontend_tty::Tty;
use super::terminal_input::Input;
use super::{Error, Result};
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Exit {
    Detached,
    Suspended,
    Interrupted(libc::c_int),
    Closed,
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
        match tty.raw() {
            Ok(()) => (true, drive(tty, signals, channel, observer, &mut display)),
            Err(error) => (false, Err(error)),
        }
    };
    let marker = match reason {
        Ok(Exit::Detached) => Marker::Detached,
        Ok(Exit::Suspended) => Marker::Suspended,
        Ok(Exit::Interrupted(_)) => Marker::Interrupted,
        Ok(Exit::Closed) | Err(_) => Marker::TerminalError,
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
    display: &mut Display,
) -> Result<Exit> {
    let mut input = Input::new(observer);
    let mut queued = Vec::with_capacity(QUEUE);
    let mut keyboard = [0; RECORD];
    let mut output = [0; RECORD];
    let mut pending_output = 0..0;
    let mut size = tty.geometry()?;
    let mut resize = None;
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
        let elapsed = now.duration_since(last_tick);
        last_tick = now;
        let read_keyboard = resize.is_none() && queued.len() <= QUEUE - RECORD - INPUT_CARRY;
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

        let tty_events =
            event(read_keyboard, libc::POLLIN) | event(display.has_pending(), libc::POLLOUT);
        let channel_events = event(
            pending_output.is_empty() && display.can_ingest(),
            libc::POLLIN,
        ) | event(!queued.is_empty() || resize.is_some(), libc::POLLOUT);
        let mut fds = [
            poll_fd(tty.fd(), tty_events),
            poll_fd(signals.fd(), libc::POLLIN),
            poll_fd(channel.fd(), channel_events),
        ];
        let timeout = input
            .deadline(read_keyboard)
            .into_iter()
            .chain(display.deadline())
            .min()
            .map_or(-1, ceil_millis);
        poll(&mut fds, timeout)?;
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
        if remote & libc::POLLHUP != 0 {
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
        if local & libc::POLLIN != 0
            && let Some(count) = tty.read(&mut keyboard)?
        {
            if count == 0 {
                input.flush(&mut queued);
                try_input(channel, &mut queued)?;
                return Ok(Exit::Closed);
            }
            if keyboard[..count]
                .iter()
                .any(|&byte| input.push(byte, &mut queued))
            {
                try_input(channel, &mut queued)?;
                return Ok(Exit::Detached);
            }
        }
        if signals.pending() {
            continue;
        }
        if remote & libc::POLLOUT != 0 {
            try_input(channel, &mut queued)?;
            if let Some((rows, cols)) = resize
                && queued.is_empty()
                && channel.resize(rows, cols)?
            {
                resize = None;
            }
        }
        if remote & libc::POLLIN != 0
            && let Some(count) = channel.read_output(&mut output)?
        {
            match count {
                0 => return Ok(Exit::Closed),
                1..=RECORD => pending_output = 0..count,
                _ => return Err(IPC),
            }
        }
    }
}

fn try_input(channel: &mut impl Channel, queued: &mut Vec<u8>) -> Result<()> {
    let offered = queued.len().min(RECORD);
    if offered != 0
        && let Some(count) = channel.write_input(&queued[..offered])?
    {
        if count == 0 || count > offered {
            return Err(IPC);
        }
        queued.drain(..count);
    }
    Ok(())
}

fn ceil_millis(duration: Duration) -> i32 {
    let millis = duration.as_nanos().div_ceil(1_000_000);
    millis.min(i32::MAX as u128) as i32
}

fn event(enabled: bool, event: libc::c_short) -> libc::c_short {
    if enabled { event } else { 0 }
}

fn poll_fd(fd: RawFd, events: libc::c_short) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}

fn poll(fds: &mut [libc::pollfd], millis: i32) -> Result<()> {
    if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, millis) } >= 0
        || std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
    {
        Ok(())
    } else {
        Err(POLL)
    }
}
