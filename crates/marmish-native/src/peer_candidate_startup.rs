//! Local readiness edges for one candidate worker and the existing daemon listener.

use std::time::{Duration, Instant};
use tokio::sync::watch;

const STARTUP_BUDGET: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Waiting,
    Registered,
    OrdinaryReady,
    Ready(Instant),
    Stopped,
}

#[derive(Clone, Copy)]
enum Edge {
    Registered,
    OrdinaryReady,
    Stop,
}

struct StopOnDrop {
    state: watch::Sender<State>,
}

impl StopOnDrop {
    fn record(&self, edge: Edge) {
        self.state.send_if_modified(|state| {
            let next = match (*state, edge) {
                (_, Edge::Stop) => State::Stopped,
                (State::Waiting, Edge::Registered) => State::Registered,
                (State::Waiting, Edge::OrdinaryReady) => State::OrdinaryReady,
                (State::Registered, Edge::OrdinaryReady)
                | (State::OrdinaryReady, Edge::Registered) => State::Ready(Instant::now()),
                (state, _) => state,
            };
            let changed = *state != next;
            *state = next;
            changed
        });
    }
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.record(Edge::Stop);
    }
}

pub(crate) struct Startup(StopOnDrop);

impl Startup {
    pub(crate) fn ordinary_ready(&self) {
        self.0.record(Edge::OrdinaryReady);
    }

    pub(crate) fn stop(&self) {
        self.0.record(Edge::Stop);
    }
}

pub(crate) struct ListenerRegistration(StopOnDrop);

impl ListenerRegistration {
    pub(crate) fn registered(&self) {
        self.0.record(Edge::Registered);
    }

    pub(crate) fn readiness(&self) -> Readiness {
        Readiness(self.0.state.subscribe())
    }
}

#[derive(Clone)]
pub(crate) struct Readiness(watch::Receiver<State>);

impl Readiness {
    pub(crate) fn is_current(&self) -> bool {
        self.0.has_changed().is_ok() && matches!(*self.0.borrow(), State::Ready(_))
    }
}

pub(crate) struct WorkerStart {
    readiness: Readiness,
    stop: StopOnDrop,
}

impl WorkerStart {
    pub(crate) async fn ready(mut self) -> Option<Ready> {
        let state = *self
            .readiness
            .0
            .wait_for(|state| matches!(state, State::Ready(_) | State::Stopped))
            .await
            .ok()?;
        let State::Ready(completed_at) = state else {
            return None;
        };
        Some(Ready {
            readiness: self.readiness,
            completed_at,
            _stop: self.stop,
        })
    }
}

/// Revocable startup observation. The worker still checks its actual scope.
pub(crate) struct Ready {
    readiness: Readiness,
    completed_at: Instant,
    _stop: StopOnDrop,
}

impl Ready {
    #[allow(dead_code, reason = "consumed by the separately mounted Manual actor")]
    pub(crate) fn readiness(&self) -> Readiness {
        self.readiness.clone()
    }

    pub(crate) fn is_current(&self) -> bool {
        self.readiness.is_current()
    }

    pub(crate) fn startup_remaining(&self) -> Option<Duration> {
        self.startup_remaining_at(Instant::now())
    }

    fn startup_remaining_at(&self, now: Instant) -> Option<Duration> {
        if !self.is_current() {
            return None;
        }
        let elapsed = now.checked_duration_since(self.completed_at)?;
        STARTUP_BUDGET
            .checked_sub(elapsed)
            .filter(|remaining| !remaining.is_zero())
    }

    pub(crate) async fn stopped(&mut self) {
        let _ = self
            .readiness
            .0
            .wait_for(|state| *state == State::Stopped)
            .await;
    }
}

pub(crate) fn pair() -> (Startup, ListenerRegistration, WorkerStart) {
    let (state, observer) = watch::channel(State::Waiting);
    let stop_on_drop = || StopOnDrop {
        state: state.clone(),
    };
    (
        Startup(stop_on_drop()),
        ListenerRegistration(stop_on_drop()),
        WorkerStart {
            readiness: Readiness(observer),
            stop: StopOnDrop { state },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::Future, pin::Pin, task::Context, task::Waker};

    fn assert_waiting<T>(pending: Pin<&mut impl Future<Output = T>>) {
        assert!(
            pending
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn both_edge_orders_release_once_and_duplicates_do_not_release_early() {
        for listener_first in [false, true] {
            let (startup, listener, worker) = pair();
            let intake = listener.readiness();
            let mut waiting = Box::pin(worker.ready());
            assert_waiting(waiting.as_mut());
            assert!(!intake.is_current());
            if listener_first {
                listener.registered();
                listener.registered();
            } else {
                startup.ordinary_ready();
                startup.ordinary_ready();
            }
            assert_waiting(waiting.as_mut());
            assert!(!intake.is_current());
            if listener_first {
                startup.ordinary_ready();
            } else {
                listener.registered();
            }
            let ready = tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
                .await
                .unwrap()
                .unwrap();
            let native_readiness = ready.readiness();
            assert!(ready.is_current());
            assert!(native_readiness.is_current());
            assert!(intake.is_current());
            startup.stop();
            assert!(!native_readiness.is_current());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn startup_budget_starts_once_at_the_second_edge() {
        for listener_first in [false, true] {
            let (startup, listener, worker) = pair();
            let intake = listener.readiness();
            if listener_first {
                listener.registered();
            } else {
                startup.ordinary_ready();
            }

            let before = Instant::now();
            if listener_first {
                startup.ordinary_ready();
            } else {
                listener.registered();
            }
            let after = Instant::now();
            let State::Ready(completed_at) = *intake.0.borrow() else {
                panic!("both edges must record one readiness instant");
            };
            assert!((before..=after).contains(&completed_at));

            listener.registered();
            startup.ordinary_ready();
            assert!(matches!(*intake.0.borrow(), State::Ready(time) if time == completed_at));

            let ready = worker.ready().await.unwrap();
            assert_eq!(ready.completed_at, completed_at);
            assert_eq!(
                ready.startup_remaining_at(completed_at),
                Some(STARTUP_BUDGET)
            );
            assert_eq!(
                ready.startup_remaining_at(completed_at + Duration::from_secs(14)),
                Some(Duration::from_secs(1))
            );
            for now in [
                completed_at - Duration::from_millis(1),
                completed_at + STARTUP_BUDGET,
                completed_at + STARTUP_BUDGET + Duration::from_secs(1),
            ] {
                assert_eq!(ready.startup_remaining_at(now), None);
            }
            startup.stop();
            assert_eq!(ready.startup_remaining_at(completed_at), None);
            assert_eq!(ready.startup_remaining(), None);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn waiting_worker_wakes_for_both_readiness_and_stop() {
        for release in [false, true] {
            let (startup, listener, worker) = pair();
            let (first_poll, polled) = tokio::sync::oneshot::channel();
            let waiting = tokio::spawn(async move {
                let mut ready = std::pin::pin!(worker.ready());
                let mut first_poll = Some(first_poll);
                std::future::poll_fn(|context| {
                    let state = ready.as_mut().poll(context);
                    if let Some(polled) = first_poll.take() {
                        assert!(state.is_pending());
                        polled.send(()).unwrap();
                    }
                    state
                })
                .await
            });
            polled.await.unwrap();
            if release {
                startup.ordinary_ready();
                listener.registered();
            } else {
                startup.stop();
            }
            let ready = tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(ready.as_ref().is_some_and(Ready::is_current), release);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stop_absorbs_later_edges_from_every_state_before_worker_release() {
        for (register, ordinary) in [(false, false), (false, true), (true, false), (true, true)] {
            let (startup, listener, worker) = pair();
            let intake = listener.readiness();
            if register {
                listener.registered();
            }
            if ordinary {
                startup.ordinary_ready();
            }
            startup.stop();
            listener.registered();
            startup.ordinary_ready();
            assert!(!intake.is_current());
            assert!(worker.ready().await.is_none());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stop_revokes_ready_before_drain_and_wakes_worker() {
        let (startup, listener, worker) = pair();
        let intake = listener.readiness();
        startup.ordinary_ready();
        listener.registered();
        let mut ready = worker.ready().await.unwrap();
        let (started, start) = tokio::sync::oneshot::channel();
        let waiting = tokio::spawn(async move {
            started.send(()).unwrap();
            ready.stopped().await;
            assert!(!ready.is_current());
        });
        start.await.unwrap();
        startup.stop();
        assert!(!intake.is_current());
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_daemon_or_listener_revokes_ready() {
        for listener_exits in [false, true] {
            let (startup, listener, worker) = pair();
            let intake = listener.readiness();
            let concurrent_intake = intake.clone();
            startup.ordinary_ready();
            listener.registered();
            let mut ready = worker.ready().await.unwrap();
            if listener_exits {
                drop(listener);
            } else {
                drop(startup);
            }
            assert!(!intake.is_current());
            assert!(!concurrent_intake.is_current());
            ready.stopped().await;
            assert!(!ready.is_current());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_unstarted_waiting_or_ready_worker_closes_only_that_generation() {
        let (other, other_listener, other_worker) = pair();
        let other_intake = other_listener.readiness();
        other.ordinary_ready();
        other_listener.registered();
        let other_ready = other_worker.ready().await.unwrap();
        for stage in 0..3 {
            let (startup, listener, worker) = pair();
            let intake = listener.readiness();
            let concurrent_intake = intake.clone();
            match stage {
                0 => drop(worker),
                1 => {
                    let mut waiting = Box::pin(worker.ready());
                    assert_waiting(waiting.as_mut());
                    drop(waiting);
                }
                _ => {
                    listener.registered();
                    startup.ordinary_ready();
                    drop(worker.ready().await.unwrap());
                }
            }
            listener.registered();
            startup.ordinary_ready();
            assert!(!intake.is_current());
            assert!(!concurrent_intake.is_current());
            assert!(other_intake.is_current());
            assert!(other_ready.is_current());
        }
    }
}
