//! Local permit and confirmation ownership for native Terminal admission.

use super::{Arc, Claim, Duration, MAX_LIVE, Mutex, Ordering, Reason, Role, SlotId, Source};
use std::sync::atomic::AtomicUsize;
use tokio::sync::oneshot;

#[derive(Default)]
pub(super) struct StartLimit {
    used: AtomicUsize,
}

pub(super) struct StartPermit(Arc<StartLimit>);

impl StartLimit {
    pub(super) fn try_acquire(self: &Arc<Self>) -> Option<StartPermit> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < MAX_LIVE).then_some(used + 1)
            })
            .ok()?;
        Some(StartPermit(Arc::clone(self)))
    }
}

impl Drop for StartPermit {
    fn drop(&mut self) {
        self.0.used.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct Confirmation {
    pub(super) until: Duration,
    active: Mutex<bool>,
}

impl Confirmation {
    pub(super) fn new(until: Duration) -> Arc<Self> {
        Arc::new(Self {
            until,
            active: Mutex::new(false),
        })
    }

    pub(super) fn active(&self) -> bool {
        *self.active.lock().expect("local confirmation mutex")
    }

    pub(super) fn acknowledge(&self, reply: oneshot::Sender<super::Result<()>>) -> bool {
        // An awakened receiver can only enter the local gate after the send result is recorded.
        let mut active = self.active.lock().expect("local confirmation mutex");
        *active = reply.send(Ok(())).is_ok();
        *active
    }
}

pub(super) struct Request {
    pub(super) slot: SlotId,
    pub(super) claim: Claim,
    pub(super) role: Role,
    pub(super) source: Arc<Source>,
    pub(super) until: Duration,
    pub(super) reply: oneshot::Sender<super::Result<()>>,
}

impl Request {
    pub(super) fn matches(&self, link: &super::Active, slot: SlotId) -> bool {
        self.slot == slot
            && self.claim == link.claim
            && self.role == link.role
            && Arc::ptr_eq(&self.source, &link.port.source)
    }

    pub(super) fn refusal(
        &self,
        confirmation: &Confirmation,
        now: Option<Duration>,
    ) -> Option<Reason> {
        if self.reply.is_closed() {
            return Some(Reason::Detached);
        }
        let Some(now) = now else {
            return Some(Reason::Clock);
        };
        (now >= confirmation.until.min(self.until)).then_some(Reason::Deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_tests_terminal_live_failed_acknowledgement_does_not_activate() {
        let gate = Confirmation::new(Duration::from_secs(5));
        let (reply, receipt) = oneshot::channel();
        drop(receipt);
        assert!(!gate.acknowledge(reply));
        assert!(!gate.active());

        let (reply, mut receipt) = oneshot::channel();
        assert!(gate.acknowledge(reply));
        assert!(receipt.try_recv().unwrap().is_ok());
        assert!(gate.active());
    }

    #[test]
    fn core_tests_terminal_live_start_units_follow_permit_custody() {
        let limit = Arc::new(StartLimit::default());
        let mut reservations: Vec<_> = (0..MAX_LIVE)
            .map(|_| limit.try_acquire().unwrap())
            .collect();
        assert!(limit.try_acquire().is_none());
        reservations.pop();
        let replacement = limit.try_acquire().unwrap();
        assert!(limit.try_acquire().is_none());
        drop(replacement);
        drop(reservations);
        assert_eq!(limit.used.load(Ordering::Acquire), 0);
    }
}
