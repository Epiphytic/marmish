//! Cooked, local-only choice after the first Terminal admission ended.

use super::terminal_frontend::{poll, poll_fd};
use super::{Attach, CLOCK_TICK, Control, Result, Signals, Tty, continuous_time, control};
use std::time::{Duration, Instant};

const PROMPT: &[u8] = b"Terminal busy. [o] Observe, [t] Takeover, [n] New, [c] Cancel: ";
const PROMPT_BUDGET: Duration = Duration::from_secs(25);

pub(super) enum Choice {
    Select(Attach),
    Cancel,
    Control(Control),
}

pub(super) fn choose(tty: &mut Tty, signals: &Signals, arrived: Duration) -> Result<Choice> {
    let choice = tty
        .require_cooked()
        .and_then(|()| tty.flush_input())
        .and_then(|()| read_choice(tty, signals, arrived));
    let flushed = tty.flush_input();
    if let Some(signal) = control(signals.take())? {
        return Ok(Choice::Control(signal));
    }
    let choice = choice?;
    flushed?;
    Ok(choice)
}

fn read_choice(tty: &mut Tty, signals: &Signals, arrived: Duration) -> Result<Choice> {
    let continuous_end = arrived.saturating_add(PROMPT_BUDGET);
    let active_end = Instant::now() + PROMPT_BUDGET;
    let mut prompt = PROMPT;
    loop {
        if let Some(signal) = control(signals.take())? {
            return Ok(Choice::Control(signal));
        }
        let remaining = continuous_end
            .saturating_sub(continuous_time()?)
            .min(active_end.saturating_duration_since(Instant::now()));
        if remaining.is_zero() {
            return Ok(Choice::Cancel);
        }
        tty.foreground()?;
        let event = if prompt.is_empty() {
            libc::POLLIN
        } else {
            libc::POLLOUT
        };
        let mut fds = [
            poll_fd(tty.fd(), event),
            poll_fd(signals.fd(), libc::POLLIN),
        ];
        let millis = remaining.min(CLOCK_TICK).as_micros().div_ceil(1000) as i32;
        poll(&mut fds, millis)?;
        if fds[1].revents != 0 || signals.pending() {
            continue;
        }
        if continuous_time()? >= continuous_end || Instant::now() >= active_end {
            return Ok(Choice::Cancel);
        }
        let ready = fds[0].revents;
        if ready & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Ok(Choice::Cancel);
        }
        if !prompt.is_empty() && ready & libc::POLLOUT != 0 {
            if let Some(count) = tty.write(prompt)? {
                prompt = &prompt[count..];
            }
        } else if prompt.is_empty() && ready & libc::POLLIN != 0 {
            let mut input = [0; 3];
            if let Some(count) = tty.read(&mut input)? {
                return Ok(parse(&input[..count]));
            }
        }
    }
}

fn parse(input: &[u8]) -> Choice {
    match input {
        [b'o', b'\r' | b'\n'] => Choice::Select(Attach::Observe),
        [b't', b'\r' | b'\n'] => Choice::Select(Attach::Takeover),
        [b'n', b'\r' | b'\n'] => Choice::Select(Attach::New),
        _ => Choice::Cancel,
    }
}

#[cfg(test)]
#[test]
fn terminal_foreground_prompt_accepts_only_one_cooked_choice() {
    assert!(matches!(parse(b"o\n"), Choice::Select(Attach::Observe)));
    assert!(matches!(parse(b"t\r"), Choice::Select(Attach::Takeover)));
    assert!(matches!(parse(b"n\n"), Choice::Select(Attach::New)));
    for invalid in [
        b"".as_slice(),
        b"c\n",
        b"T\n",
        b" t\n",
        b"t",
        b"tt\n",
        b"t\nq",
    ] {
        assert!(matches!(parse(invalid), Choice::Cancel));
    }
}
