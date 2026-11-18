//! A single pre-owned registered relay slot and its read-only drain witness.

use super::{ConnectorState, Error, Fatal, Result};
use std::sync::Arc;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Instant, timeout_at},
};
use tokio_util::sync::CancellationToken;

pub(crate) struct PeerSocketStart {
    slot: Arc<Semaphore>,
    permit: Arc<OwnedSemaphorePermit>,
    fatal: Fatal,
    #[cfg(test)]
    attempts: Arc<std::sync::atomic::AtomicUsize>,
}

pub(crate) struct PeerSocketDrain {
    slot: Arc<Semaphore>,
    fatal: Fatal,
    route: CancellationToken,
    #[cfg(test)]
    attempts: Arc<std::sync::atomic::AtomicUsize>,
}

impl PeerSocketStart {
    pub(crate) fn pair(route: CancellationToken, fatal: Fatal) -> Result<(Self, PeerSocketDrain)> {
        let slot = Arc::new(Semaphore::new(1));
        let permit = slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Transport("peer_socket_slot"))?;
        #[cfg(test)]
        let attempts = Arc::default();
        let drain = PeerSocketDrain {
            slot: slot.clone(),
            fatal: fatal.clone(),
            route,
            #[cfg(test)]
            attempts: Arc::clone(&attempts),
        };
        Ok((
            Self {
                slot,
                permit: Arc::new(permit),
                fatal,
                #[cfg(test)]
                attempts,
            },
            drain,
        ))
    }

    pub(crate) fn fatal(&self) -> &Fatal {
        &self.fatal
    }

    pub(super) fn install(self, state: &mut ConnectorState) -> (Fatal, Arc<OwnedSemaphorePermit>) {
        state.slots[0] = self.slot;
        #[cfg(test)]
        {
            state.direct_attempts = self.attempts;
        }
        (self.fatal, self.permit)
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl PeerSocketDrain {
    pub(crate) async fn reap(&self, deadline: Instant) -> Result<()> {
        if !self.fatal.is_tripped() && !self.route.is_cancelled() {
            return Err(Error::Transport("peer_socket_revoke_required"));
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let _reaped = timeout_at(deadline, self.slot.acquire())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport("peer_socket_drain"))?;
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn tcp_attempts(&self) -> usize {
        self.attempts.load(std::sync::atomic::Ordering::Relaxed)
    }
}
