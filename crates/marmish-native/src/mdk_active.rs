//! Check the original registration at active native effect boundaries.

use super::StartupMode;
#[cfg(any(test, marmish_peer_receiver_candidate))]
use crate::Error;
use crate::Result;
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;

pub(super) const DISPATCHER_LOST: &str = "candidate_dispatcher_lost";

#[derive(Clone, Default)]
pub(super) struct ActiveMode {
    #[cfg(any(test, marmish_peer_receiver_candidate))]
    admission: Option<crate::command_engine::Admission>,
}

impl ActiveMode {
    pub(super) fn from_startup(_mode: &StartupMode) -> Self {
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        if let StartupMode::Candidate { admission, .. } = _mode {
            return Self {
                admission: Some(admission.clone()),
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

    /// The inner outcome stays nested so ordinary error mappings cannot
    /// turn a rejected poll into transport degradation or a retry.
    pub(super) async fn native<F: Future>(&self, future: F) -> Result<F::Output> {
        if !self.checked() {
            return Ok(future.await);
        }
        let mut future = pin!(future);
        poll_fn(|cx| {
            self.admit()?;
            match future.as_mut().poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(result) => Poll::Ready(self.admit().map(|()| result)),
            }
        })
        .await
    }
}
