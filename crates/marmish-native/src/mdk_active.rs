//! Check the original registration at active native effect boundaries.

use super::StartupMode;
#[cfg(any(test, marmish_peer_receiver_candidate))]
use crate::Error;
use crate::Result;
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;

pub(super) const DISPATCHER_LOST: &str = "candidate_dispatcher_lost";

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Observation {
    ReceiveBeforePoll,
    ReceiveReady,
    OrphanBeforeRecord,
    OrphanAfterForget,
    InventoryBeforeQuery,
    InventoryAfterQuery,
    Publish,
    PreparePackage,
}

#[cfg(test)]
type Observer = std::sync::Arc<dyn Fn(Observation) + Send + Sync>;

#[derive(Clone, Default)]
pub(super) struct ActiveMode {
    #[cfg(any(test, marmish_peer_receiver_candidate))]
    admission: Option<crate::command_engine::Admission>,
    #[cfg(test)]
    observer: Option<std::sync::Arc<std::sync::Mutex<Option<Observer>>>>,
    #[cfg(test)]
    observe_receive: bool,
}

impl ActiveMode {
    pub(super) fn from_startup(_mode: &StartupMode) -> Self {
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        if let StartupMode::Candidate { admission, .. } = _mode {
            return Self {
                admission: Some(admission.clone()),
                #[cfg(test)]
                observer: Some(Default::default()),
                #[cfg(test)]
                observe_receive: false,
            };
        }
        Self::default()
    }

    fn checked(&self) -> bool {
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        if self.admission.is_some() {
            return true;
        }
        false
    }

    pub(super) fn admit(&self) -> Result<()> {
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        if self
            .admission
            .as_ref()
            .is_some_and(|admission| !admission.admit())
        {
            return Err(Error::Storage(DISPATCHER_LOST));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn set_observer(&self, observer: impl Fn(Observation) + Send + Sync + 'static) {
        if let Some(slot) = &self.observer {
            *slot.lock().unwrap() = Some(std::sync::Arc::new(observer));
        }
    }

    #[cfg(test)]
    pub(super) fn observe(&self, location: Observation) {
        let observer = self
            .observer
            .as_ref()
            .and_then(|slot| slot.lock().unwrap().clone());
        if let Some(observer) = observer {
            observer(location);
        }
    }

    #[cfg(test)]
    pub(super) fn observe_receive(mut self) -> Self {
        self.observe_receive = true;
        self
    }

    /// The inner outcome stays nested so ordinary error mappings cannot
    /// turn a rejected poll into transport degradation or a retry.
    pub(super) async fn native<F: Future>(&self, future: F) -> Result<F::Output> {
        if !self.checked() {
            return Ok(future.await);
        }
        let mut future = pin!(future);
        poll_fn(|cx| {
            #[cfg(test)]
            if self.observe_receive {
                self.observe(Observation::ReceiveBeforePoll);
            }
            self.admit()?;
            match future.as_mut().poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(result) => {
                    #[cfg(test)]
                    if self.observe_receive {
                        self.observe(Observation::ReceiveReady);
                    }
                    Poll::Ready(self.admit().map(|()| result))
                }
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stock_active_owner_does_not_install_effect_observers() {
        let mode = ActiveMode::from_startup(&StartupMode::Stock);
        mode.set_observer(|_| panic!("stock test owner must not invoke an observer"));
        mode.observe(Observation::InventoryBeforeQuery);
        mode.observe_receive().native(async {}).await.unwrap();
    }
}
