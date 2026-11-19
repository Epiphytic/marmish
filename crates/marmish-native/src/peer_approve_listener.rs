//! One exchange on the existing registered listener. A paired worker is the
//! only source of consumed approval authority.
use super::{Response, SocketFence, peer_approve};
use crate::{dispatcher_fatal::Fatal, identity::ProfileLease, peer_candidate_startup::Readiness};
use peer_approve::{Failure, Selection};
use std::{
    io,
    os::fd::AsRawFd,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::Interest,
    net::{UnixListener, UnixStream, unix::OwnedReadHalf},
    sync::{oneshot, watch},
    time::{Instant, sleep_until, timeout_at},
};

const CLAIM_WINDOW: Duration = Duration::from_secs(9);
const CLAIM_BUDGET: Duration = Duration::from_secs(5);
const WRITE_RESERVE: Duration = Duration::from_millis(100);
type Decision = Result<(), Failure>;

pub(super) struct Listener(Arc<Pair>);
#[derive(Clone)]
pub(super) struct Intake(Arc<Pair>);

pub(crate) struct Worker(Arc<Pair>);

struct Pair {
    path: PathBuf,
    lease: Arc<ProfileLease>,
    readiness: Readiness,
    fatal: Fatal,
    state: Mutex<PairState>,
    stopped: watch::Sender<bool>,
}

#[derive(Default)]
struct PairState {
    registered: bool,
    slot: Option<Slot>,
}

struct Slot {
    exchange: Arc<Exchange>,
    phase: Phase,
}

enum Phase {
    Waiting(oneshot::Sender<Decision>),
    Claimed(Instant),
}

struct Exchange {
    reader: Arc<OwnedReadHalf>,
    uid: u32,
    selection: Selection,
    claim_until: Instant,
    reply_until: Instant,
    claim_cutoff: watch::Sender<Option<Instant>>,
}

pub(super) fn pair(
    path: PathBuf,
    lease: Arc<ProfileLease>,
    readiness: Readiness,
    fatal: Fatal,
) -> (Listener, Worker) {
    let (stopped, _) = watch::channel(false);
    let pair = Arc::new(Pair {
        path,
        lease,
        readiness,
        fatal,
        state: Mutex::new(PairState::default()),
        stopped,
    });
    (Listener(pair.clone()), Worker(pair))
}

impl Listener {
    pub(super) fn register(&self, listener: &UnixListener, fatal: Option<&Fatal>) -> bool {
        let Ok(bound) = listener.local_addr() else {
            return false;
        };
        let Ok(mut state) = self.0.state.lock() else {
            return false;
        };
        if bound.as_pathname() != Some(self.0.path.as_path())
            || self.0.path != self.0.lease.socket_path()
            || state.registered
            || self.0.is_closed()
            || !fatal.is_some_and(|fatal| self.0.fatal.same_lifetime(fatal))
        {
            return false;
        }
        state.registered = true;
        true
    }

    pub(super) fn intake(&self) -> Intake {
        Intake(self.0.clone())
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.0.stop();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.0.stop();
    }
}

impl Pair {
    fn is_closed(&self) -> bool {
        *self.stopped.borrow() || self.fatal.is_tripped()
    }

    fn current(&self, uid: u32) -> bool {
        !self.is_closed()
            && self.readiness.is_current()
            && self.lease.checked_owner_uid().ok() == Some(uid)
    }

    fn stop(&self) {
        self.stopped.send_replace(true);
        if let Ok(mut state) = self.state.lock() {
            state.slot = None;
        }
    }

    fn revoke(&self, exchange: &Arc<Exchange>) {
        if let Ok(mut state) = self.state.lock()
            && state
                .slot
                .as_ref()
                .is_some_and(|slot| Arc::ptr_eq(&slot.exchange, exchange))
        {
            state.slot = None;
        }
    }

    fn expire_waiting(&self, exchange: &Arc<Exchange>) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.slot.as_ref().is_some_and(|slot| {
            Arc::ptr_eq(&slot.exchange, exchange) && matches!(slot.phase, Phase::Waiting(_))
        }) {
            state.slot = None;
            return true;
        }
        false
    }

    fn exact_claim(&self, exchange: &Arc<Exchange>, cutoff: Instant) -> bool {
        self.state.lock().is_ok_and(|state| {
            state.slot.as_ref().is_some_and(|slot| {
                Arc::ptr_eq(&slot.exchange, exchange)
                    && matches!(slot.phase, Phase::Claimed(deadline) if deadline == cutoff)
            })
        })
    }
}

struct Revocation {
    pair: Arc<Pair>,
    exchange: Arc<Exchange>,
}

impl Drop for Revocation {
    fn drop(&mut self) {
        self.pair.revoke(&self.exchange);
    }
}

// Err(WouldBlock) means there is no EOF or data queued. The reader owns its
// socket throughout the synchronous call and the buffer has space for one byte.
fn peek(reader: &OwnedReadHalf) -> io::Result<()> {
    let mut byte = 0_u8;
    let result = unsafe {
        libc::recv(
            reader.as_ref().as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_DONTWAIT | libc::MSG_PEEK,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn awaiting_reply(reader: &OwnedReadHalf) -> bool {
    peek(reader).is_err_and(|error| error.kind() == io::ErrorKind::WouldBlock)
}

async fn disconnected(reader: &OwnedReadHalf) {
    let _ = reader
        .as_ref()
        .async_io(Interest::READABLE, || peek(reader))
        .await;
}

impl Worker {
    pub(crate) fn try_claim(&self) -> Option<Claimed> {
        let exchange = {
            let state = self.0.state.lock().ok()?;
            let slot = state.slot.as_ref()?;
            if !matches!(slot.phase, Phase::Waiting(_)) {
                return None;
            }
            slot.exchange.clone()
        };
        if !self.0.current(exchange.uid) || !awaiting_reply(&exchange.reader) {
            return None;
        }
        let mut state = self.0.state.lock().ok()?;
        let slot = state.slot.as_mut()?;
        if self.0.is_closed()
            || !Arc::ptr_eq(&slot.exchange, &exchange)
            || !matches!(slot.phase, Phase::Waiting(_))
        {
            return None;
        }
        let now = Instant::now();
        if now >= exchange.claim_until {
            return None;
        }
        let cutoff = (now + CLAIM_BUDGET).min(exchange.reply_until);
        if now >= cutoff {
            return None;
        }
        let Phase::Waiting(reply) = std::mem::replace(&mut slot.phase, Phase::Claimed(cutoff))
        else {
            return None;
        };
        exchange.claim_cutoff.send_replace(Some(cutoff));
        Some(Claimed {
            pair: self.0.clone(),
            exchange,
            cutoff,
            reply,
        })
    }
}

pub(crate) struct Claimed {
    pair: Arc<Pair>,
    exchange: Arc<Exchange>,
    cutoff: Instant,
    reply: oneshot::Sender<Decision>,
}

impl Claimed {
    pub(crate) fn selection(&self) -> Selection {
        self.exchange.selection
    }

    pub(crate) fn matches_lease(&self, lease: &Arc<ProfileLease>) -> bool {
        Arc::ptr_eq(&self.pair.lease, lease)
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.cutoff
    }

    pub(crate) fn check_live(&self) -> bool {
        Instant::now() < self.cutoff
            && self.pair.exact_claim(&self.exchange, self.cutoff)
            && self.pair.current(self.exchange.uid)
            && awaiting_reply(&self.exchange.reader)
            && Instant::now() < self.cutoff
    }

    pub(crate) fn consume(
        self,
        lease: &Arc<ProfileLease>,
        selection: Selection,
    ) -> Option<Consumed> {
        (self.matches_lease(lease) && self.selection() == selection && self.check_live())
            .then_some(Consumed(self))
    }

    pub(crate) fn refuse(self, reason: Failure) {
        self.send(Err(reason));
    }

    fn send(self, decision: Decision) {
        if self.check_live() {
            let limited = match decision {
                Err(Failure::Busy | Failure::InvalidRequest) => Err(Failure::Uncertain),
                other => other,
            };
            let _ = self.reply.send(limited);
        }
    }
}

pub(crate) struct Consumed(Claimed);

impl Consumed {
    pub(crate) fn selection(&self) -> Selection {
        self.0.selection()
    }

    pub(crate) fn matches_lease(&self, lease: &Arc<ProfileLease>) -> bool {
        self.0.matches_lease(lease)
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.0.deadline()
    }

    pub(crate) fn check_live(&self) -> bool {
        self.0.check_live()
    }

    pub(crate) fn complete(self, decision: Decision) {
        self.0.send(decision);
    }
}

impl Intake {
    fn install(
        &self,
        exchange: Arc<Exchange>,
        reply: oneshot::Sender<Decision>,
    ) -> Result<Revocation, Failure> {
        if !self.0.current(exchange.uid) || !awaiting_reply(&exchange.reader) {
            return Err(Failure::Unavailable);
        }
        let mut state = self.0.state.lock().map_err(|_| Failure::Unavailable)?;
        if !state.registered || self.0.is_closed() {
            return Err(Failure::Unavailable);
        }
        if state.slot.is_some() || Instant::now() >= exchange.claim_until {
            return Err(Failure::Busy);
        }
        state.slot = Some(Slot {
            exchange: exchange.clone(),
            phase: Phase::Waiting(reply),
        });
        Ok(Revocation {
            pair: self.0.clone(),
            exchange,
        })
    }

    async fn wait(
        &self,
        exchange: &Arc<Exchange>,
        mut receiver: oneshot::Receiver<Decision>,
    ) -> Option<Decision> {
        let mut stopped = self.0.stopped.subscribe();
        let mut claimed = exchange.claim_cutoff.subscribe();
        let expiry = async {
            if timeout_at(exchange.claim_until, claimed.wait_for(Option::is_some))
                .await
                .is_err()
                && self.0.expire_waiting(exchange)
            {
                return Failure::Busy;
            }

            let Some(cutoff) = *claimed.borrow() else {
                return Failure::Uncertain;
            };
            sleep_until(cutoff).await;
            Failure::Uncertain
        };
        tokio::select! {
            biased;
            _ = self.0.fatal.tripped() => None,
            _ = stopped.wait_for(|closed| *closed) => None,
            _ = disconnected(&exchange.reader) => None,
            result = &mut receiver => Some(result.unwrap_or(Err(Failure::Uncertain))),
            reason = expiry => Some(Err(reason)),
        }
    }
}

fn intact_claim(intake: &Intake, exchange: &Arc<Exchange>) -> bool {
    let cutoff = *exchange.claim_cutoff.borrow();
    cutoff.is_some_and(|cutoff| intake.0.exact_claim(exchange, cutoff))
}

pub(super) async fn serve(
    intake: Option<&Intake>,
    request: peer_approve::Request,
    mut stream: UnixStream,
    uid: u32,
    accepted_at: Instant,
    exchange_deadline: Instant,
    fence: &SocketFence,
) {
    let early = match intake {
        _ if !request.is_supported() => Some(Failure::InvalidRequest),
        None => Some(Failure::Unavailable),
        Some(_) => None,
    };
    if let Some(reason) = early {
        let response = peer_approve::response(request.selection, Err(reason));
        let _ = timeout_at(exchange_deadline, fence.write(&mut stream, &response)).await;
        return;
    }
    let Some(intake) = intake else { return };
    let (reader, mut writer) = stream.into_split();
    let (send, receive) = oneshot::channel();
    let exchange = Arc::new(Exchange {
        reader: Arc::new(reader),
        uid,
        selection: request.selection,
        claim_until: accepted_at + CLAIM_WINDOW,
        reply_until: exchange_deadline - WRITE_RESERVE,
        claim_cutoff: watch::channel(None).0,
    });
    let admitted = intake.install(exchange.clone(), send);
    let decision = match &admitted {
        Err(reason) => Err(*reason),
        Ok(_) => match timeout_at(exchange.reply_until, intake.wait(&exchange, receive)).await {
            Ok(Some(Err(reason))) => Err(reason),
            Ok(Some(Ok(()))) if intact_claim(intake, &exchange) => Ok(()),
            Ok(Some(Ok(())) | None) => return,
            Err(_) => Err(Failure::Uncertain),
        },
    };
    if admitted.is_ok() && (!intake.0.current(uid) || !awaiting_reply(&exchange.reader)) {
        return;
    }
    if Instant::now() < exchange_deadline {
        let response: Response = peer_approve::response(request.selection, decision);
        let mut stopped = intake.0.stopped.subscribe();
        tokio::select! {
            biased;
            _ = stopped.wait_for(|closed| *closed) => {},
            _ = timeout_at(exchange_deadline, fence.write(&mut writer, &response)) => {},
        }
    }
}

#[cfg(test)]
#[path = "peer_approve_listener_tests.rs"]
mod tests;
