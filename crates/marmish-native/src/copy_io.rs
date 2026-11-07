//! Bounded application framing over the independently cancellable stream lane.
use crate::{
    Error, Result,
    copy_worker::{Event, Job, Worker},
    stream_session::{OutputAcks, Snapshot, StreamSession},
};
use marmish_stream::{
    MAX_BLOCK, MAX_OFFSET,
    codec::Codec,
    copy_protocol::{self as protocol, CopyError, Frame, FrameDecoder},
    wire::Terminal,
};
use std::{collections::VecDeque, sync::mpsc::TryRecvError, time::Duration};
use tokio::time::Instant;

pub(crate) fn fail(code: CopyError) -> Error {
    Error::Copy(code)
}
fn invalid() -> Error {
    fail(CopyError::Protocol)
}
fn protocol<T>(value: marmish_stream::Result<T>) -> Result<T> {
    value.map_err(|_| invalid())
}

struct Credit {
    sent: u64,
    durable: u64,
    hash: [u8; 32],
    prefixes: VecDeque<(u64, [u8; 32])>,
}
impl Credit {
    fn acknowledge(&mut self, offset: u64, hash: [u8; 32]) -> Result<()> {
        let expected = if offset == self.durable {
            Some(self.hash)
        } else {
            self.prefixes
                .iter()
                .find(|(n, _)| *n == offset)
                .map(|(_, h)| *h)
        };
        if offset < self.durable || offset > self.sent || expected != Some(hash) {
            return Err(fail(CopyError::HashMismatch));
        }
        self.durable = offset;
        self.hash = hash;
        while self.prefixes.front().is_some_and(|(n, _)| *n <= offset) {
            self.prefixes.pop_front();
        }
        Ok(())
    }
}

pub(crate) struct CopyIo {
    session: StreamSession,
    codec: Codec,
    decoder: FrameDecoder,
    pending: VecDeque<Frame>,
    queued_bytes: usize,
    acks: OutputAcks,
    stdout_eof: Option<u64>,
    stderr_eof: Option<u64>,
    terminal: Option<Terminal>,
    credit: Option<Credit>,
    diagnostic_bytes: usize,
    finishing: bool,
    keepalive: Duration,
    last_keepalive: Instant,
    next_poll: Instant,
    idle_backoff: Duration,
    aborting: bool,
    work_inflight: bool,
    peer_error: bool,
    cleanup_confirmed: bool,
}
impl CopyIo {
    pub(crate) fn new(session: StreamSession, codec: Codec) -> Self {
        Self {
            session,
            codec,
            decoder: FrameDecoder::default(),
            pending: VecDeque::new(),
            queued_bytes: 0,
            acks: OutputAcks::default(),
            stdout_eof: None,
            stderr_eof: None,
            terminal: None,
            credit: None,
            diagnostic_bytes: 0,
            finishing: false,
            keepalive: Duration::from_secs(300),
            last_keepalive: Instant::now(),
            next_poll: Instant::now(),
            idle_backoff: Duration::from_millis(250),
            aborting: false,
            work_inflight: false,
            peer_error: false,
            cleanup_confirmed: false,
        }
    }
    fn cleanup_terminal(&self) -> Result<bool> {
        match self.terminal {
            Some(Terminal::Failed { code }) => Err(Error::Stream(code)),
            Some(Terminal::Exited { .. } | Terminal::Signaled { .. } | Terminal::Cancelled) => {
                Ok(true)
            }
            None => Ok(false),
        }
    }
    pub(crate) fn peer_error_received(&self) -> bool {
        self.peer_error
    }
    pub(crate) fn cleanup_confirmed(&self) -> bool {
        self.cleanup_confirmed
    }
    pub(crate) fn set_keepalive(&mut self, interval: Duration) {
        self.keepalive = interval;
    }
    pub(crate) fn codec(&self) -> Codec {
        self.codec
    }
    pub(crate) fn configure_credit(&mut self, offset: u64, hash: [u8; 32]) -> Result<()> {
        if self.credit.is_some() || offset > MAX_OFFSET {
            return Err(invalid());
        }
        self.credit = Some(Credit {
            sent: offset,
            durable: offset,
            hash,
            prefixes: VecDeque::new(),
        });
        Ok(())
    }
    pub(crate) fn credit(&self) -> (u64, u64) {
        self.credit.as_ref().map_or((0, 0), |c| (c.sent, c.durable))
    }
    pub(crate) fn offer(&mut self, offset: u64, length: usize, hash: [u8; 32]) -> Result<()> {
        let credit = self.credit.as_mut().ok_or_else(invalid)?;
        let end = offset.checked_add(length as u64).ok_or_else(invalid)?;
        if offset != credit.sent
            || length == 0
            || length > MAX_BLOCK
            || end > MAX_OFFSET
            || end - credit.durable > protocol::DURABLE_WINDOW
        {
            return Err(invalid());
        }
        credit.sent = end;
        credit.prefixes.push_back((end, hash));
        Ok(())
    }
    pub(crate) fn clear_credit(&mut self) -> Result<()> {
        if self.credit.as_ref().is_some_and(|c| c.durable != c.sent) {
            return Err(invalid());
        }
        self.credit = None;
        Ok(())
    }
    pub(crate) fn ensure_no_pending(&self) -> Result<()> {
        if !self.pending.is_empty() {
            Err(invalid())
        } else {
            Ok(())
        }
    }
    fn absorb(&mut self, snapshot: Snapshot) -> Result<()> {
        if snapshot.stdin_delivered > snapshot.stdin_accepted {
            return Err(invalid());
        }
        self.last_keepalive = Instant::now();
        let mut peer_failure = None;
        if let Some(block) = snapshot.stdout {
            if block.offset != self.acks.stdout {
                return Err(invalid());
            }
            let mut consumed = 0;
            // Failure drain acknowledges transport bytes without interpreting
            // the application decoder, which may be poisoned or half-consumed.
            while !self.aborting && consumed < block.bytes.len() {
                let (n, frame) = protocol(self.decoder.feed(&block.bytes[consumed..]))?;
                if n == 0 {
                    return Err(invalid());
                }
                consumed += n;
                match frame {
                    _ if self.aborting || peer_failure.is_some() => (),
                    None | Some(Frame::Progress { .. }) => (),
                    Some(Frame::Checkpoint { offset, sha256 }) if self.credit.is_some() => {
                        self.credit.as_mut().unwrap().acknowledge(offset, sha256)?
                    }
                    Some(Frame::Error(code)) => {
                        self.peer_error = true;
                        peer_failure = Some(code);
                    }
                    Some(frame) => {
                        let size = protocol(frame.encode())?.len();
                        if self.queued_bytes + size > protocol::MAX_CONTROL + MAX_BLOCK
                            || self.pending.len() >= 1024
                        {
                            return Err(invalid());
                        }
                        self.queued_bytes += size;
                        self.pending.push_back(frame);
                    }
                }
            }
            self.acks.stdout += block.bytes.len() as u64;
        }
        if let Some(block) = snapshot.stderr {
            if block.offset != self.acks.stderr {
                return Err(invalid());
            }
            // Do not reflect arbitrary remote stderr into a terminal. Report a
            // bounded presence notice; the helper's finite Error is authoritative.
            self.diagnostic_bytes = self
                .diagnostic_bytes
                .saturating_add(block.bytes.len())
                .min(1024);
            self.acks.stderr += block.bytes.len() as u64;
        }
        self.stdout_eof = snapshot.stdout_eof;
        self.stderr_eof = snapshot.stderr_eof;
        self.terminal = snapshot.terminal;
        if let Some(code) = peer_failure {
            return Err(fail(code));
        }
        if !self.aborting
            && self
                .terminal
                .as_ref()
                .is_some_and(|t| !matches!(t, Terminal::Exited { code: 0 }))
        {
            return Err(match self.terminal.as_ref().unwrap() {
                Terminal::Failed { code } => Error::Stream(*code),
                _ if self.stdout_eof != Some(self.acks.stdout) => return Ok(()),
                _ => fail(CopyError::Protocol),
            });
        }
        Ok(())
    }
    async fn pump(&mut self) -> Result<()> {
        // Timer state survives a dropped pump, including work()'s bounded poll.
        tokio::time::sleep_until(self.next_poll).await;
        self.next_poll = Instant::now() + Duration::from_millis(125);
        let snapshot = if self.session.has_pending() {
            self.session.resume().await?
        } else {
            self.session.exchange(None, false, self.acks).await?
        };
        let progress = snapshot.stdout.is_some()
            || snapshot.stderr.is_some()
            || snapshot.terminal != self.terminal
            || snapshot.stdout_eof != self.stdout_eof
            || snapshot.stderr_eof != self.stderr_eof;
        self.absorb(snapshot)?;
        self.idle_backoff = if progress {
            Duration::from_millis(250)
        } else {
            (self.idle_backoff * 2).min(Duration::from_secs(2))
        };
        self.next_poll = Instant::now()
            + if progress {
                Duration::from_millis(125)
            } else {
                self.idle_backoff
            };
        Ok(())
    }
    async fn settle(&mut self) -> Result<()> {
        if self.session.has_pending() {
            let snapshot = self.session.resume().await?;
            self.absorb(snapshot)?;
        }
        Ok(())
    }
    pub(crate) async fn send(&mut self, frame: Frame) -> Result<()> {
        let bytes = protocol(frame.encode())?;
        self.settle().await?;
        for chunk in bytes.chunks(MAX_BLOCK) {
            while !self.session.can_send(chunk.len()) {
                if self.session.input_closed() && self.aborting {
                    return Ok(());
                }
                if self.stdout_eof == Some(self.acks.stdout) {
                    return Err(invalid());
                }
                self.pump().await?;
            }
            let snapshot = self.session.exchange(Some(chunk), false, self.acks).await?;
            self.absorb(snapshot)?;
        }
        Ok(())
    }
    pub(crate) async fn next(&mut self) -> Result<Frame> {
        loop {
            if let Some(frame) = self.pending.pop_front() {
                self.queued_bytes -= protocol(frame.encode())?.len();
                return Ok(frame);
            }
            if self.stdout_eof == Some(self.acks.stdout) {
                return Err(invalid());
            }
            self.pump().await?;
        }
    }
    /// Timeout/drop preserves the decoder and the exact outstanding request.
    pub(crate) async fn next_until(&mut self, deadline: Option<Instant>) -> Result<Option<Frame>> {
        match deadline {
            None => self.next().await.map(Some),
            Some(deadline) => match tokio::time::timeout_at(deadline, self.next()).await {
                Ok(result) => result.map(Some),
                Err(_) => Ok(None),
            },
        }
    }
    pub(crate) async fn wait_credit(&mut self) -> Result<()> {
        self.ensure_no_pending()?;
        while self
            .credit
            .as_ref()
            .is_some_and(|c| c.sent - c.durable >= protocol::DURABLE_WINDOW)
        {
            self.pump().await?;
            self.ensure_no_pending()?;
        }
        Ok(())
    }
    pub(crate) async fn work(&mut self, worker: &Worker, job: Job) -> Result<Event> {
        if self.work_inflight {
            return Err(invalid());
        }
        use protocol::Stage;
        let progress = match &job {
            Job::Hash(total, prefix) => Some((
                if *prefix {
                    Stage::PrefixHash
                } else {
                    Stage::Hashing
                },
                *total,
            )),
            Job::Prepare(_) => Some((Stage::Verifying, 0)),
            Job::Commit | Job::Checkpoint(_) => Some((Stage::Syncing, 0)),
            _ => None,
        };
        worker.jobs.try_send(job).map_err(|_| invalid())?;
        self.work_inflight = true;
        // Most small filesystem jobs finish before a relay round trip could.
        // Give the owned worker a bounded head start instead of creating an
        // otherwise unnecessary network exchange for every local read/write.
        tokio::time::sleep(Duration::from_millis(10)).await;
        let mut last_progress = Instant::now();
        loop {
            match worker.events.try_recv() {
                Ok(value) => {
                    self.work_inflight = false;
                    return value.map_err(|e| fail(e.code));
                }
                Err(TryRecvError::Disconnected) => return Err(fail(CopyError::LocalIo)),
                Err(TryRecvError::Empty) => (),
            }
            if let Some((stage, total)) = progress
                && last_progress.elapsed() >= Duration::from_secs(1)
            {
                self.send(Frame::Progress {
                    stage,
                    examined: 0,
                    total,
                })
                .await?;
                last_progress = Instant::now();
            }
            // A cancelled keepalive future retains its exact pending exchange;
            // subsequent pump/send resumes it before attempting different data.
            if self.pending.is_empty() {
                tokio::select! { result=self.pump()=>result?, ()=tokio::time::sleep(Duration::from_millis(100))=>() }
            } else if self.last_keepalive.elapsed() >= self.keepalive {
                self.session.control().status().await?;
                self.last_keepalive = Instant::now();
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    /// A deterministic application failure ends this protocol, without Cancel.
    /// Transport loss and attachment drop never invoke this method.
    pub(crate) async fn abort_failure(&mut self, code: CopyError) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(15), async {
            self.aborting = true;
            self.pending.clear();
            self.queued_bytes = 0;
            self.settle().await?;
            if !self.peer_error && !self.session.input_closed() {
                self.send(Frame::Error(code)).await?;
            }
            self.settle().await?;
            self.finishing = true;
            let snapshot = self
                .session
                .exchange(None, !self.session.input_closed(), self.acks)
                .await?;
            self.absorb(snapshot)?;
            loop {
                // Terminal confirms the owned helper exited, not transfer success.
                if self.cleanup_terminal()?
                    && self.stdout_eof == Some(self.acks.stdout)
                    && self.stderr_eof == Some(self.acks.stderr)
                {
                    break;
                }
                self.pending.clear();
                self.queued_bytes = 0;
                self.pump().await?;
            }
            self.pump().await?;
            self.cleanup_confirmed = true;
            Ok(())
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub(crate) async fn finish(&mut self) -> Result<()> {
        self.ensure_no_pending()?;
        self.settle().await?;
        self.finishing = true;
        let snapshot = self
            .session
            .exchange(None, !self.session.input_closed(), self.acks)
            .await?;
        self.absorb(snapshot)?;
        loop {
            self.ensure_no_pending()?;
            if self.terminal == Some(Terminal::Exited { code: 0 })
                && self.stdout_eof == Some(self.acks.stdout)
                && self.stderr_eof == Some(self.acks.stderr)
            {
                break;
            }
            self.pump().await?;
        }
        protocol(self.decoder.finish())?;
        // Publication of the final ACK must succeed as well as receiving EOF.
        self.pump().await?;
        self.ensure_no_pending()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{command_cli::Context, stream_session::OutputBlock};
    use marmish_stream::wire::{Binding, WireErrorCode};
    use std::sync::Arc;

    fn io() -> CopyIo {
        CopyIo::new(
            StreamSession::new(
                Arc::new(Context {
                    socket: "/nonexistent/socket".into(),
                    profile: "test".into(),
                }),
                vec![],
                Binding {
                    stream: [1; 16],
                    session: [2; 16],
                    client: [3; 32],
                    server: [4; 32],
                },
                Duration::from_secs(2),
                Codec::None,
            ),
            Codec::None,
        )
    }
    fn snapshot(bytes: Vec<u8>) -> Snapshot {
        Snapshot {
            stdin_accepted: 0,
            stdin_delivered: 0,
            stdout: (!bytes.is_empty()).then_some(OutputBlock { offset: 0, bytes }),
            stderr: None,
            stdout_eof: None,
            stderr_eof: None,
            terminal: Some(Terminal::Exited { code: 1 }),
        }
    }
    #[test]
    fn helper_error_is_preserved_when_stdin_closes_and_helper_exits() {
        for code in [
            CopyError::NotFound,
            CopyError::Conflict,
            CopyError::PermissionDenied,
            CopyError::CommitUncertain,
        ] {
            assert!(
                matches!(io().absorb(snapshot(Frame::Error(code).encode().unwrap())), Err(Error::Copy(actual)) if actual == code)
            );
        }
    }
    #[test]
    fn closed_stdin_waits_for_delayed_error_and_accounts_the_entire_block() {
        let mut io = io();
        let mut closed = snapshot(vec![]);
        closed.terminal = None;
        io.absorb(closed).unwrap();
        let mut bytes = Frame::Error(CopyError::Conflict).encode().unwrap();
        bytes.extend(
            Frame::Progress {
                stage: protocol::Stage::Syncing,
                examined: 0,
                total: 0,
            }
            .encode()
            .unwrap(),
        );
        let length = bytes.len() as u64;
        let mut failed = snapshot(bytes);
        failed.stdout_eof = Some(length);
        failed.stderr_eof = Some(0);
        assert!(matches!(
            io.absorb(failed),
            Err(Error::Copy(CopyError::Conflict))
        ));
        assert_eq!(io.acks.stdout, length);
        assert_eq!(io.stdout_eof, Some(length));
        assert!(io.peer_error);
        assert!(io.decoder.finish().is_ok());
    }
    #[test]
    fn abort_drain_acks_replayed_block_after_decoder_or_credit_failure() {
        for bytes in [
            vec![255; 40],
            Frame::Checkpoint {
                offset: 100,
                sha256: [9; 32],
            }
            .encode()
            .unwrap(),
        ] {
            let mut io = io();
            io.configure_credit(0, [1; 32]).unwrap();
            assert!(io.absorb(snapshot(bytes.clone())).is_err());
            assert_eq!(io.acks.stdout, 0);
            io.aborting = true;
            io.absorb(snapshot(bytes.clone())).unwrap();
            assert_eq!(io.acks.stdout, bytes.len() as u64);
            assert!(io.pending.is_empty());
        }
    }
    #[test]
    fn uncertain_terminal_never_confirms_owned_helper_cleanup() {
        let mut io = io();
        io.terminal = Some(Terminal::Failed {
            code: WireErrorCode::Uncertain,
        });
        assert!(matches!(
            io.cleanup_terminal(),
            Err(Error::Stream(WireErrorCode::Uncertain))
        ));
        assert!(!io.cleanup_confirmed());
        for terminal in [
            Terminal::Cancelled,
            Terminal::Exited { code: 1 },
            Terminal::Signaled { signal: 15 },
        ] {
            io.terminal = Some(terminal);
            assert!(io.cleanup_terminal().unwrap());
        }
    }
    #[test]
    fn terminal_with_backpressured_output_does_not_mask_later_error() {
        let mut io = io();
        let bytes = Frame::Error(CopyError::Conflict).encode().unwrap();
        let mut first = snapshot(vec![]);
        first.stdout_eof = Some(bytes.len() as u64);
        first.stderr_eof = Some(0);
        io.absorb(first).unwrap();
        let mut last = snapshot(bytes.clone());
        last.stdout_eof = Some(bytes.len() as u64);
        last.stderr_eof = Some(0);
        assert!(matches!(
            io.absorb(last),
            Err(Error::Copy(CopyError::Conflict))
        ));
        assert_eq!(io.acks.stdout, bytes.len() as u64);
    }
    #[test]
    fn aborting_discards_coalesced_frames_without_credit_or_queue_failures() {
        let mut io = io();
        io.aborting = true;
        io.configure_credit(0, [1; 32]).unwrap();
        let mut bytes = Frame::Checkpoint {
            offset: 100,
            sha256: [9; 32],
        }
        .encode()
        .unwrap();
        bytes.extend(
            Frame::Data {
                offset: 0,
                bytes: vec![8; 1024],
            }
            .encode()
            .unwrap(),
        );
        bytes.extend(Frame::Error(CopyError::Protocol).encode().unwrap());
        let length = bytes.len() as u64;
        io.absorb(snapshot(bytes)).unwrap();
        assert_eq!(io.acks.stdout, length);
        assert!(io.pending.is_empty());
        assert_eq!(io.queued_bytes, 0);
    }
    #[test]
    fn typed_preparation_failure_survives_closed_stdin() {
        let mut snapshot = snapshot(vec![]);
        snapshot.terminal = Some(Terminal::Failed {
            code: WireErrorCode::Denied,
        });
        assert!(matches!(
            io().absorb(snapshot),
            Err(Error::Stream(WireErrorCode::Denied))
        ));
    }
    #[tokio::test]
    async fn coalesced_prefix_and_data_are_bounded_and_delivered_in_order() {
        let prefix = Frame::Prefix {
            offset: 0,
            sha256: [6; 32],
        };
        let data = Frame::Data {
            offset: 0,
            bytes: vec![9; 1024],
        };
        let mut bytes = prefix.encode().unwrap();
        bytes.extend(data.encode().unwrap());
        let mut snapshot = snapshot(bytes);
        snapshot.terminal = None;
        let mut io = io();
        io.absorb(snapshot).unwrap();
        assert!(matches!(
            io.next().await.unwrap(),
            Frame::Prefix { offset: 0, .. }
        ));
        assert!(
            matches!(io.next().await.unwrap(), Frame::Data {offset:0,bytes} if bytes == vec![9;1024])
        );
        assert_eq!(io.queued_bytes, 0);
    }
    #[test]
    fn credit_requires_exact_recorded_durable_prefix() {
        let mut io = io();
        io.configure_credit(0, [1; 32]).unwrap();
        io.offer(0, 1024, [2; 32]).unwrap();
        assert!(
            io.credit
                .as_mut()
                .unwrap()
                .acknowledge(1024, [3; 32])
                .is_err()
        );
        assert!(
            io.credit
                .as_mut()
                .unwrap()
                .acknowledge(1023, [2; 32])
                .is_err()
        );
        io.credit
            .as_mut()
            .unwrap()
            .acknowledge(1024, [2; 32])
            .unwrap();
        assert_eq!(io.credit(), (1024, 1024));
        io.clear_credit().unwrap();
    }
}
