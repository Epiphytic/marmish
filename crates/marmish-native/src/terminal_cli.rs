//! Synchronous owner of the client foreground and its cancellable admission.

use super::terminal_socket_session::{self, ForegroundSocket};
use super::{Attach, Start, start};
use crate::command_limits::continuous_time;
use crate::terminal_ipc_wire::{Close, Role};
use crate::{Error, Result, terminal_output};
use std::cell::Cell;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;
use terminal_frontend::Exit;
pub(crate) use terminal_frontend_signals::block_on_worker;
use terminal_frontend_signals::{Events, Signals};
use terminal_frontend_tty::Tty;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::time::{Instant, sleep};

#[path = "terminal_frontend.rs"]
mod terminal_frontend;
#[path = "terminal_frontend_display.rs"]
mod terminal_frontend_display;
#[path = "terminal_frontend_signals.rs"]
mod terminal_frontend_signals;
#[path = "terminal_frontend_tty.rs"]
mod terminal_frontend_tty;
#[path = "terminal_input.rs"]
mod terminal_input;
#[path = "terminal_socket_frontend.rs"]
mod terminal_socket_frontend;

const START_BUDGET: Duration = Duration::from_secs(60);
const CLOCK_TICK: Duration = Duration::from_millis(250);

pub(crate) struct Prepared {
    pub(crate) socket: PathBuf,
    pub(crate) remote: String,
    pub(crate) session: String,
    pub(crate) relays: Vec<String>,
}

#[derive(Clone, Copy)]
enum Control {
    Suspend,
    Terminate(libc::c_int),
}

fn control(events: Events) -> Result<Option<Control>> {
    if let Some(signal) = events.termination {
        return Ok(Some(Control::Terminate(signal)));
    }
    if events.has(libc::SIGTSTP) {
        return Ok(Some(Control::Suspend));
    }
    if events.has(libc::SIGTTIN) || events.has(libc::SIGTTOU) {
        return Err(Error::Invalid("terminal_foreground"));
    }
    Ok(None)
}

struct WakePipe(RawFd);

impl AsRawFd for WakePipe {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

async fn read_control(signals: &Signals) -> Result<Control> {
    // SAFETY: Signals uses a process-lifetime nonblocking pipe with a stable fd.
    let fd = unsafe { AsyncFd::register_with_interest(WakePipe(signals.fd()), Interest::READABLE) }
        .map_err(std::io::Error::from)?;
    loop {
        if let Some(control) = control(signals.take())? {
            return Ok(control);
        }
        fd.readable().await?.clear_ready();
    }
}

type Geometry = (u16, u16);
type Ready = (UnixStream, Geometry);

enum Admission {
    Ready(UnixStream, Geometry),
    Control(Control),
}

async fn connect(prepared: Prepared, geometry: Geometry, attach: Attach) -> Result<Ready> {
    let request = Start {
        remote: prepared.remote,
        session: prepared.session,
        relays: prepared.relays,
        rows: geometry.0,
        columns: geometry.1,
        attach,
    };
    wait_for_start(continuous_time, async move |active_end| {
        start(&prepared.socket, request, active_end).await
    })
    .await
    .map(|socket| (socket, geometry))
}

async fn wait_for_start<T>(
    continuous: impl Fn() -> Result<Duration>,
    exchange: impl AsyncFnOnce(Instant) -> Result<T>,
) -> Result<T> {
    let continuous_end = continuous()?
        .checked_add(START_BUDGET)
        .ok_or(Error::Timeout)?;
    let active_end = Instant::now() + START_BUDGET;
    let exchange = exchange(active_end);
    tokio::pin!(exchange);
    loop {
        let remaining = continuous_end
            .saturating_sub(continuous()?)
            .min(active_end.saturating_duration_since(Instant::now()));
        if remaining.is_zero() {
            return Err(Error::Timeout);
        }
        tokio::select! {
            result = &mut exchange => {
                if continuous()? >= continuous_end || Instant::now() >= active_end {
                    return Err(Error::Timeout);
                }
                return result;
            }
            _ = sleep(remaining.min(CLOCK_TICK)) => (),
        }
    }
}

fn admit(
    tty: &Tty,
    signals: &Signals,
    attach: Attach,
    attempted: &Cell<bool>,
    prepare: &impl AsyncFn(libc::sigset_t) -> Result<Prepared>,
) -> Result<Admission> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(2)
        .on_thread_start(block_on_worker)
        .build()
        .map_err(|_| Error::Invalid("runtime_startup"))?;
    let result = runtime.block_on(async {
        tokio::select! {
            biased;
            signal = read_control(signals) => signal.map(Admission::Control),
            ready = async {
                let prepared = prepare(signals.previous_mask()).await?;
                let geometry = tty.geometry()?;
                attempted.set(true);
                connect(prepared, geometry, attach).await
            } => ready.map(|(socket, geometry)| Admission::Ready(socket, geometry)),
        }
    });
    // An independently launched daemon can finish startup if the foreground
    // client canceled its wait. It has its own startup timeout and profile lease.
    if !matches!(&result, Ok(Admission::Ready(..))) {
        runtime.shutdown_background();
    }
    result
}

fn signal_exit(signal: libc::c_int) -> u8 {
    128 + signal as u8
}

fn close_exit(reason: Close) -> Result<u8> {
    Err(Error::Invalid(match reason {
        Close::Ended => return Ok(0),
        Close::Idle => "terminal_idle",
        Close::Replaced => "terminal_replaced",
        Close::Authority => "terminal_authority",
        Close::Transport => "terminal_transport_closed",
        Close::Budget => "terminal_budget",
    }))
}

pub(crate) fn run(
    new: bool,
    prepare: impl AsyncFn(libc::sigset_t) -> Result<Prepared>,
) -> Result<u8> {
    let signals = Signals::install()?;
    let attach = if new { Attach::New } else { Attach::Default };
    loop {
        let mut tty = Tty::open()?;
        let attempted = Cell::new(false);
        let signal = match admit(&tty, &signals, attach, &attempted, &prepare)? {
            Admission::Control(signal) => signal,
            Admission::Ready(socket, geometry) => {
                let mut channel = ForegroundSocket::new(socket, Role::Writer)?;
                let ended =
                    terminal_frontend::run(&mut tty, &signals, &mut channel, false, Some(geometry));
                drop(channel);
                if !matches!(ended.reason, Ok(Exit::Interrupted(_))) {
                    ended.restored_termios?;
                    if !ended.restored_display {
                        eprintln!("marmish: terminal_display_restore");
                    }
                }
                match ended.reason? {
                    Exit::Detached => return Ok(0),
                    Exit::PeerClosed(reason) => return close_exit(reason),
                    Exit::Closed => return Err(Error::Invalid("terminal_tty_closed")),
                    Exit::Interrupted(signal) => Control::Terminate(signal),
                    Exit::Suspended => Control::Suspend,
                }
            }
        };
        match signal {
            Control::Terminate(signal) => return Ok(signal_exit(signal)),
            Control::Suspend => {
                drop(tty);
                signals.suspend()?;
                if let Some(signal) = signals.termination() {
                    return Ok(signal_exit(signal));
                }
                if new && attempted.get() {
                    return Err(Error::Invalid("terminal_new_reattach_unavailable"));
                }
            }
        }
    }
}

#[cfg(test)]
#[tokio::test(start_paused = true)]
async fn foreground_start_has_active_and_continuous_deadlines() {
    let active_start = Instant::now();
    let result = wait_for_start(
        || Ok(Duration::ZERO),
        async |_| std::future::pending::<Result<()>>().await,
    )
    .await;
    assert!(matches!(result, Err(Error::Timeout)));
    assert_eq!(active_start.elapsed(), START_BUDGET);

    let active_start = Instant::now();
    let result = wait_for_start(
        || {
            if active_start.elapsed().is_zero() {
                Ok(Duration::ZERO)
            } else {
                Ok(START_BUDGET)
            }
        },
        async |_| std::future::pending::<Result<()>>().await,
    )
    .await;
    assert!(matches!(result, Err(Error::Timeout)));
    assert_eq!(active_start.elapsed(), CLOCK_TICK);
}
