//! Synchronous owner of the client foreground and its cancellable admission.

use super::terminal_socket_session::{self, ForegroundSocket};
use super::{Attach, Offer, Start, StartOutcome, Target, exchange};
use crate::command_limits::continuous_time;
use crate::terminal_ipc_wire::{Close, Role};
use crate::{Error, Result, terminal_output};
use std::cell::Cell;
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;
use terminal_cli_choice::Choice;
use terminal_frontend::Exit;
pub(crate) use terminal_frontend_signals::block_on_worker;
use terminal_frontend_signals::{Events, Signals};
use terminal_frontend_tty::Tty;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::time::{Instant, sleep};

#[path = "terminal_cli_choice.rs"]
mod terminal_cli_choice;
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
const OFFER_BUDGET: Duration = Duration::from_secs(70);
const TAKEOVER_UNCERTAIN: Error = Error::Invalid("terminal_takeover_uncertain");
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

#[derive(Clone, Copy)]
struct Attempt {
    attach: Attach,
    target: Target,
}

enum Connected {
    Ready(Attachment),
    Busy { offer: Offer, arrived: Duration },
}

enum Controlled<T> {
    Done(T),
    Signal(Control),
}

async fn connect(
    prepared: &Prepared,
    geometry: Geometry,
    attempt: Attempt,
    offer_ceiling: Option<Duration>,
    sent: &Cell<bool>,
) -> Result<Connected> {
    let (session_slot, offer) = match attempt.target {
        Target::Default => (None, None),
        Target::Named(slot) => (Some(hex::encode(slot)), None),
        Target::Offer(offer) => (None, Some(hex::encode(offer.bytes()))),
    };
    let request = Start {
        remote: prepared.remote.clone(),
        session: prepared.session.clone(),
        relays: prepared.relays.clone(),
        rows: geometry.0,
        columns: geometry.1,
        attach: attempt.attach,
        session_slot,
        offer,
    };
    let outcome = wait_for_start(continuous_time, offer_ceiling, async |active_end| {
        exchange(&prepared.socket, request, active_end, || sent.set(true)).await
    })
    .await?;
    Ok(match outcome {
        StartOutcome::Ready {
            stream: socket,
            role,
            slot,
        } => Connected::Ready(Attachment {
            socket,
            geometry,
            role,
            slot,
            attach: attempt.attach,
            flush_input: matches!(attempt.target, Target::Offer(_)),
        }),
        StartOutcome::Busy(offer) => Connected::Busy {
            offer,
            arrived: continuous_time()?,
        },
    })
}

async fn wait_for_start<T>(
    continuous: impl Fn() -> Result<Duration>,
    offer_ceiling: Option<Duration>,
    exchange: impl AsyncFnOnce(Instant) -> Result<T>,
) -> Result<T> {
    let continuous_end = continuous()?
        .checked_add(START_BUDGET)
        .ok_or(Error::Timeout)?
        .min(offer_ceiling.unwrap_or(Duration::MAX));
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

fn with_control<T>(
    signals: &Signals,
    task: impl Future<Output = Result<T>>,
) -> Result<Controlled<T>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(2)
        .on_thread_start(block_on_worker)
        .build()
        .map_err(|_| Error::Invalid("runtime_startup"))?;
    let result = runtime.block_on(async {
        tokio::select! {
            biased;
            signal = read_control(signals) => signal.map(Controlled::Signal),
            ready = task => ready.map(Controlled::Done),
        }
    });
    // An independently launched daemon may finish starting after local
    // cancellation. It has its own startup timeout and profile lease.
    if !matches!(&result, Ok(Controlled::Done(_))) {
        runtime.shutdown_background();
    }
    result
}

struct Attachment {
    socket: UnixStream,
    geometry: Geometry,
    role: Role,
    slot: [u8; 16],
    attach: Attach,
    flush_input: bool,
}

enum AfterSuspend {
    Resume(Attempt),
    NewUnconfirmed,
    Exit,
}

impl AfterSuspend {
    fn before_ready(attempt: Attempt, resuming: bool) -> Self {
        match attempt.attach {
            Attach::Default if !resuming => Self::Resume(attempt),
            Attach::New => Self::NewUnconfirmed,
            _ => Self::Exit,
        }
    }
}

enum Admission {
    Ready(Attachment),
    Control {
        signal: Control,
        after_suspend: AfterSuspend,
    },
    Cancel,
}

fn admit(
    tty: &mut Tty,
    signals: &Signals,
    attempt: Attempt,
    resuming: bool,
    prepare: &impl AsyncFn(libc::sigset_t) -> Result<Prepared>,
) -> Result<Admission> {
    let (prepared, first) = match with_control(signals, async {
        let prepared = prepare(signals.previous_mask()).await?;
        let first = connect(&prepared, tty.geometry()?, attempt, None, &Cell::new(false)).await?;
        Ok((prepared, first))
    })? {
        Controlled::Done(first) => first,
        Controlled::Signal(signal) => {
            return Ok(Admission::Control {
                signal,
                after_suspend: AfterSuspend::before_ready(attempt, resuming),
            });
        }
    };
    let (offer, arrived) = match first {
        Connected::Ready(ready) => return Ok(Admission::Ready(ready)),
        Connected::Busy { offer, arrived } => (offer, arrived),
    };
    if resuming {
        return Err(Error::Invalid("terminal_busy"));
    }
    let attach = if attempt.attach == Attach::Takeover {
        Attach::Takeover
    } else {
        match terminal_cli_choice::choose(tty, signals, arrived)? {
            Choice::Select(attach) => attach,
            Choice::Cancel => return Ok(Admission::Cancel),
            Choice::Control(signal) => {
                return Ok(Admission::Control {
                    signal,
                    after_suspend: AfterSuspend::Exit,
                });
            }
        }
    };
    admit_offer(tty, signals, &prepared, offer, arrived, attach)
}

fn admit_offer(
    tty: &Tty,
    signals: &Signals,
    prepared: &Prepared,
    offer: Offer,
    arrived: Duration,
    attach: Attach,
) -> Result<Admission> {
    let sent = Cell::new(false);
    let ceiling = arrived.checked_add(OFFER_BUDGET).ok_or(Error::Timeout)?;
    let result = with_control(signals, async {
        connect(
            prepared,
            tty.geometry()?,
            Attempt {
                attach,
                target: Target::Offer(offer),
            },
            Some(ceiling),
            &sent,
        )
        .await
    });
    let uncertain = attach == Attach::Takeover && sent.get();
    match result {
        Ok(Controlled::Done(Connected::Ready(ready))) => Ok(Admission::Ready(ready)),
        Ok(Controlled::Done(Connected::Busy { .. })) => {
            Err(Error::Invalid("terminal_start_response"))
        }
        Ok(Controlled::Signal(Control::Suspend)) if uncertain => {
            signals.suspend()?;
            Err(TAKEOVER_UNCERTAIN)
        }
        Ok(Controlled::Signal(_)) if uncertain => Err(TAKEOVER_UNCERTAIN),
        Ok(Controlled::Signal(signal)) => Ok(Admission::Control {
            signal,
            after_suspend: if attach == Attach::New {
                AfterSuspend::NewUnconfirmed
            } else {
                AfterSuspend::Exit
            },
        }),
        Err(error) if uncertain && ambiguous(&error) => Err(TAKEOVER_UNCERTAIN),
        Err(error) => Err(error),
    }
}

fn ambiguous(error: &Error) -> bool {
    matches!(
        error,
        Error::Timeout
            | Error::Io(_)
            | Error::Invalid(
                "terminal_start_response"
                    | "ipc_frame_json"
                    | "ipc_frame_size"
                    | "terminal_attach_deadline"
                    | "terminal_transport_unavailable"
                    | "terminal_unavailable"
                    | "terminal_owner_unavailable"
            )
    )
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

fn run_ready(
    tty: &mut Tty,
    signals: &Signals,
    ready: Attachment,
    input_batch: Duration,
) -> Result<Option<u8>> {
    let mut channel = ForegroundSocket::new(ready.socket, ready.role)?;
    if ready.attach == Attach::New {
        let _ = writeln!(
            std::io::stderr().lock(),
            "Marmish session {}",
            hex::encode(ready.slot)
        );
    }
    let ended = terminal_frontend::run(
        tty,
        signals,
        &mut channel,
        ready.role == Role::Observer,
        Some(ready.geometry),
        ready.flush_input,
        input_batch,
    );
    drop(channel);
    if !matches!(ended.reason, Ok(Exit::Interrupted(_))) {
        ended.restored_termios?;
        if !ended.restored_display {
            eprintln!("marmish: terminal_display_restore");
        }
    }
    match ended.reason? {
        Exit::Detached => Ok(Some(0)),
        Exit::PeerClosed(reason) => close_exit(reason).map(Some),
        Exit::Closed => Err(Error::Invalid("terminal_tty_closed")),
        Exit::Interrupted(signal) => Ok(Some(signal_exit(signal))),
        Exit::Suspended => Ok(None),
    }
}

pub(crate) fn run(
    attach: Attach,
    slot: Option<[u8; 16]>,
    input_batch: Duration,
    prepare: impl AsyncFn(libc::sigset_t) -> Result<Prepared>,
) -> Result<u8> {
    let signals = Signals::install()?;
    let mut attempt = Attempt {
        attach,
        target: slot.map_or(Target::Default, Target::Named),
    };
    let mut resuming = false;
    loop {
        let mut tty = Tty::open()?;
        let after_suspend = match admit(&mut tty, &signals, attempt, resuming, &prepare)? {
            Admission::Cancel => return Ok(0),
            Admission::Control {
                signal: Control::Terminate(signal),
                ..
            } => return Ok(signal_exit(signal)),
            Admission::Control {
                signal: Control::Suspend,
                after_suspend,
            } => after_suspend,
            Admission::Ready(ready) => {
                let after_suspend = if ready.role == Role::Writer {
                    AfterSuspend::Resume(Attempt {
                        attach: Attach::Default,
                        target: Target::Named(ready.slot),
                    })
                } else {
                    AfterSuspend::Exit
                };
                if let Some(code) = run_ready(&mut tty, &signals, ready, input_batch)? {
                    return Ok(code);
                }
                after_suspend
            }
        };
        drop(tty);
        signals.suspend()?;
        if let Some(signal) = signals.termination() {
            return Ok(signal_exit(signal));
        }
        attempt = match after_suspend {
            AfterSuspend::Resume(next) => next,
            AfterSuspend::NewUnconfirmed => {
                return Err(Error::Invalid("terminal_new_reattach_unavailable"));
            }
            AfterSuspend::Exit => return Ok(0),
        };
        resuming = true;
    }
}

#[cfg(test)]
#[tokio::test(start_paused = true)]
async fn foreground_start_has_active_and_continuous_deadlines() {
    let active_start = Instant::now();
    let result = wait_for_start(
        || Ok(Duration::ZERO),
        None,
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
        None,
        async |_| std::future::pending::<Result<()>>().await,
    )
    .await;
    assert!(matches!(result, Err(Error::Timeout)));
    assert_eq!(active_start.elapsed(), CLOCK_TICK);
}

#[cfg(test)]
#[tokio::test(start_paused = true)]
async fn terminal_foreground_offer_never_extends_second_leg() {
    let began = Instant::now();
    let available = Duration::from_millis(725);
    let ceiling = OFFER_BUDGET;
    let result = wait_for_start(
        || Ok(ceiling - available + began.elapsed()),
        Some(ceiling),
        async |_| std::future::pending::<Result<()>>().await,
    )
    .await;
    assert!(matches!(result, Err(Error::Timeout)));
    assert_eq!(began.elapsed(), available);
}
