//! Bounded access to the candidate's local read projection on its existing listener.
use super::{Response, SocketFence, peer_read};
use crate::{
    dispatcher_fatal::Fatal,
    peer_candidate_startup::{ListenerRegistration, Readiness},
};
use peer_read::pending_view::{self, Producer, Reader};
use std::{num::NonZeroU64, path::PathBuf, time::Duration};
use tokio::{
    net::{UnixListener, UnixStream},
    time::{Instant, timeout_at},
};

pub(crate) struct Offer {
    path: PathBuf,
    registration: ListenerRegistration,
    intake: Intake,
}

impl Offer {
    #[allow(
        dead_code,
        reason = "installed actor does not bind the read projection yet"
    )]
    pub(crate) fn new(
        path: PathBuf,
        registration: ListenerRegistration,
        generation: NonZeroU64,
        fatal: Fatal,
    ) -> Option<(Self, Producer)> {
        let (producer, reader) = pending_view::pair(generation)?;
        let readiness = registration.readiness();
        let intake = Intake {
            reader,
            generation,
            readiness,
            fatal,
            #[cfg(test)]
            pause: None,
        };
        Some((
            Self {
                path,
                registration,
                intake,
            },
            producer,
        ))
    }

    pub(super) fn register(self, listener: &UnixListener) -> Option<Self> {
        let bound = listener.local_addr().ok()?;
        if bound.as_pathname() != Some(self.path.as_path()) {
            return None;
        }
        self.registration.registered();
        Some(self)
    }

    pub(super) fn intake(&self) -> Intake {
        self.intake.clone()
    }
}

impl Drop for Offer {
    fn drop(&mut self) {
        self.intake.reader.stop();
    }
}

#[derive(Clone)]
pub(super) struct Intake {
    reader: Reader,
    generation: NonZeroU64,
    readiness: Readiness,
    fatal: Fatal,
    #[cfg(test)]
    pause: Option<TestPause>,
}

impl Intake {
    fn current(&self) -> bool {
        self.readiness.is_current() && !self.fatal.is_tripped()
    }

    pub(super) async fn write(
        &self,
        request: peer_read::Request,
        stream: &mut UnixStream,
        exchange_deadline: Instant,
        fence: &SocketFence,
    ) {
        let admitted = self
            .current()
            .then(|| self.reader.admit(self.generation))
            .flatten();
        #[cfg(test)]
        if admitted.is_some()
            && let Some(pause) = &self.pause
        {
            let _drop_probe = TestTaskDrop {
                intake: self,
                pause,
            };
            pause.arrived.send(()).unwrap();
            let _ = timeout_at(exchange_deadline, pause.resume.acquire()).await;
        }
        let reply_deadline = exchange_deadline - Duration::from_millis(100);
        let response = admitted
            .as_ref()
            .filter(|_| self.current())
            .and_then(|read| read.reply_before_write(request, self.generation, reply_deadline))
            .filter(|_| self.current())
            .map_or_else(
                || Response::failure(peer_read::UNAVAILABLE),
                Response::success,
            );
        if Instant::now() < exchange_deadline {
            let _ = timeout_at(exchange_deadline, fence.write(stream, &response)).await;
        }
        drop(admitted);
    }
}

#[cfg(test)]
#[derive(Clone)]
struct TestPause {
    arrived: tokio::sync::mpsc::UnboundedSender<()>,
    resume: std::sync::Arc<tokio::sync::Semaphore>,
    dropped: tokio::sync::mpsc::UnboundedSender<(bool, bool)>,
}

#[cfg(test)]
struct TestTaskDrop<'a> {
    intake: &'a Intake,
    pause: &'a TestPause,
}

#[cfg(test)]
impl Drop for TestTaskDrop<'_> {
    fn drop(&mut self) {
        let ready = self.intake.readiness.is_current();
        let can_admit = self.intake.reader.admit(self.intake.generation).is_some();
        let _ = self.pause.dropped.send((ready, can_admit));
    }
}

#[cfg(test)]
#[path = "peer_read_listener_tests.rs"]
mod tests;
