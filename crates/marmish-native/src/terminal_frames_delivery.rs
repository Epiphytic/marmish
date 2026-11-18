//! Deterministic, volatile owner state. No I/O or externally acquired clocks.
use super::{Admitted, Binding, Data, Direction, Record, Size, encode_app};
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

const WINDOW: usize = 8;
const BYTE_LIMIT: usize = 32 * 1024;
const DELIVERY_WAIT: Duration = Duration::from_secs(10);
const PULSE_INTERVAL: Duration = Duration::from_secs(300);
const LEASE: Duration = Duration::from_secs(660);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fault {
    Closed,
    Conflict,
    Window,
    Counter,
    Expired,
    Lease,
    Sink,
    Handoff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Receipt {
    Ignored,
    Ordered,
    Ack,
    Pulse,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Progress {
    Pending,
    Committed,
}

pub(super) enum Offer<'a> {
    Bytes(u64, &'a [u8]),
    Resize(u64, Size),
}

pub(super) enum Effect {
    Wrote(usize),
    Resized,
    WouldBlock,
    Failed,
}

struct Pending {
    data: Data,
    deadline: Duration,
}

#[derive(Default)]
struct Incoming {
    pending: BTreeMap<u64, Pending>,
    bytes: usize,
    committed: u64,
    offset: usize,
    offered: Option<u64>,
    unresolved_since: Option<Duration>,
}

impl Incoming {
    fn expected(&self) -> Option<u64> {
        self.committed.checked_add(1)
    }

    fn admit(&mut self, sequence: u64, item: Pending, now: Duration) -> Result<Receipt, Fault> {
        if sequence <= self.committed {
            return Ok(Receipt::Ignored);
        }
        if sequence - self.committed > WINDOW as u64 {
            return Err(Fault::Window);
        }
        if let Some(existing) = self.pending.get(&sequence) {
            return if existing.data == item.data {
                Ok(Receipt::Ignored)
            } else {
                Err(Fault::Conflict)
            };
        }
        let bytes = self.bytes + item.data.len();
        if bytes > BYTE_LIMIT {
            return Err(Fault::Window);
        }
        self.pending.insert(sequence, item);
        self.bytes = bytes;
        self.note_gap(now);
        Ok(Receipt::Ordered)
    }

    fn note_gap(&mut self, now: Duration) {
        if let Some((first, _)) = self.pending.first_key_value()
            && Some(*first) != self.expected()
        {
            self.unresolved_since.get_or_insert(now);
        }
    }

    fn check_deadlines(&self, now: Duration) -> Result<(), Fault> {
        if self
            .pending
            .first_key_value()
            .is_some_and(|(_, head)| now >= head.deadline)
        {
            return Err(Fault::Expired);
        }
        if let Some(started) = self.unresolved_since
            && now >= deadline(started, DELIVERY_WAIT)?
        {
            return Err(Fault::Expired);
        }
        Ok(())
    }

    fn progress(
        &mut self,
        sequence: u64,
        effect: Effect,
        now: Duration,
    ) -> Result<Progress, Fault> {
        if self.offered.take() != Some(sequence) || self.expected() != Some(sequence) {
            return Err(Fault::Sink);
        }
        let Some(item) = self.pending.get(&sequence) else {
            return Err(Fault::Sink);
        };
        match (&item.data, effect) {
            (_, Effect::WouldBlock) => return Ok(Progress::Pending),
            (Data::Resize(_), Effect::Resized) => {}
            (Data::Input(bytes) | Data::Output(bytes), Effect::Wrote(count))
                if count != 0 && count <= bytes.len() - self.offset =>
            {
                self.offset += count;
                if self.offset < bytes.len() {
                    return Ok(Progress::Pending);
                }
            }
            _ => return Err(Fault::Sink),
        }
        // No later expected sequence is representable after this effect.
        self.bytes -= item.data.len();
        self.pending.remove(&sequence);
        self.offset = 0;
        self.committed = sequence;
        self.unresolved_since = None;
        self.note_gap(now);
        Ok(Progress::Committed)
    }
}

struct Sent {
    sequence: u64,
    data: Data,
    first_begun: Option<Duration>,
}

struct Prepared {
    sequence: u64,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Outgoing {
    queue: VecDeque<Sent>,
    bytes: usize,
    assigned: u64,
    begun: u64,
    acknowledged: u64,
    prepared: Option<Prepared>,
}

impl Outgoing {
    fn enqueue(&mut self, data: Data, binding: Binding, outgoing: Direction) -> Result<u64, Fault> {
        let sequence = self.assigned.checked_add(1).ok_or(Fault::Counter)?;
        if !data.allowed(outgoing, binding.role) {
            return Err(Fault::Handoff);
        }
        let bytes = self.bytes + data.len();
        if self.queue.len() == WINDOW || bytes > BYTE_LIMIT {
            return Err(Fault::Window);
        }
        self.queue.push_back(Sent {
            sequence,
            data,
            first_begun: None,
        });
        self.bytes = bytes;
        self.assigned = sequence;
        Ok(sequence)
    }

    fn prepare(
        &mut self,
        author: &[u8; 32],
        created: u64,
        binding: Binding,
        outgoing: Direction,
        now: Duration,
    ) -> Result<(), Fault> {
        if self.prepared.is_some() {
            return Ok(());
        }
        let Some(item) = self
            .queue
            .iter_mut()
            .find(|item| item.first_begun.is_none())
        else {
            return Ok(());
        };
        deadline(now, DELIVERY_WAIT)?;
        let record = Record::Ordered(item.sequence, item.data.clone());
        let bytes =
            encode_app(author, created, binding, outgoing, &record).ok_or(Fault::Handoff)?;
        item.first_begun = Some(now);
        self.begun = item.sequence;
        self.prepared = Some(Prepared {
            sequence: item.sequence,
            bytes,
        });
        Ok(())
    }

    fn acknowledge(&mut self, sequence: u64) -> Result<Receipt, Fault> {
        if sequence <= self.acknowledged {
            return Ok(Receipt::Ignored);
        }
        if sequence > self.begun {
            return Err(Fault::Window);
        }
        while self
            .queue
            .front()
            .is_some_and(|item| item.sequence <= sequence)
        {
            self.bytes -= self.queue.pop_front().unwrap().data.len();
        }
        self.acknowledged = sequence;
        Ok(Receipt::Ack)
    }

    fn check_deadline(&self, now: Duration) -> Result<(), Fault> {
        if let Some(begun) = self.queue.front().and_then(|item| item.first_begun)
            && now >= deadline(begun, DELIVERY_WAIT)?
        {
            return Err(Fault::Expired);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Control {
    Ack(u64),
    Pulse(u64),
    Close,
}

impl From<Control> for Record {
    fn from(value: Control) -> Self {
        match value {
            Control::Ack(sequence) => Self::Ack(sequence),
            Control::Pulse(sequence) => Self::Pulse(sequence),
            Control::Close => Self::Close,
        }
    }
}

#[derive(Default)]
struct Controls {
    ack: Option<u64>,
    pulse: Option<u64>,
    last_sent_ack: u64,
    last_sent_pulse: u64,
    last_received_pulse: u64,
    close: bool,
}

pub(super) struct Attachment {
    binding: Binding,
    incoming: Direction,
    rx: Incoming,
    tx: Outgoing,
    controls: Controls,
    last_activity: Duration,
    last_handoff: Duration,
    closed: bool,
}

impl Attachment {
    pub(super) fn new(binding: Binding, incoming: Direction, admitted: Duration) -> Option<Self> {
        if binding.generation == 0 {
            return None;
        }
        Some(Self {
            binding,
            incoming,
            rx: Incoming::default(),
            tx: Outgoing::default(),
            controls: Controls::default(),
            last_activity: admitted,
            last_handoff: admitted,
            closed: false,
        })
    }

    pub(super) fn poll(&mut self, now: Duration) -> Result<(), Fault> {
        if self.closed {
            return Err(Fault::Closed);
        }
        let result = self.check_deadlines(now);
        self.fence_on_fault(result)
    }

    fn check_deadlines(&self, now: Duration) -> Result<(), Fault> {
        if now >= deadline(self.last_activity, LEASE)? {
            return Err(Fault::Lease);
        }
        self.rx.check_deadlines(now)?;
        self.tx.check_deadline(now)
    }

    fn fence_on_fault<T>(&mut self, result: Result<T, Fault>) -> Result<T, Fault> {
        if result.is_err() {
            self.fence();
        }
        result
    }

    fn activity(&mut self, now: Duration) {
        self.last_activity = self.last_activity.max(now);
    }

    pub(super) fn admit(&mut self, item: Admitted, now: Duration) -> Result<Receipt, Fault> {
        if self.closed
            || item.binding != self.binding
            || item.direction != self.incoming
            || now < item.received
            || now >= item.deadline
        {
            return Ok(Receipt::Ignored);
        }
        self.poll(now)?;
        let result = match item.record {
            Record::Ordered(sequence, data) => {
                let result = self.rx.admit(
                    sequence,
                    Pending {
                        data,
                        deadline: item.deadline,
                    },
                    now,
                );
                if sequence <= self.rx.committed {
                    self.controls.ack = Some(self.rx.committed);
                }
                result
            }
            Record::Ack(sequence) => self.tx.acknowledge(sequence),
            Record::Pulse(pulse) if pulse > self.controls.last_received_pulse => {
                self.controls.last_received_pulse = pulse;
                Ok(Receipt::Pulse)
            }
            Record::Pulse(_) => Ok(Receipt::Ignored),
            Record::Close => {
                self.fence();
                Ok(Receipt::Closed)
            }
        };
        let receipt = self.fence_on_fault(result)?;
        if matches!(receipt, Receipt::Ack | Receipt::Pulse) {
            self.activity(now);
        }
        Ok(receipt)
    }

    /// Call after rechecking generation, grant and lease in the same owner turn.
    pub(super) fn begin_sink(&mut self, now: Duration) -> Result<Option<Offer<'_>>, Fault> {
        Ok(self.begin_timed_sink(now)?.map(|(offer, _)| offer))
    }

    /// Preserve the same signed-age and ordered head deadlines for a deferred native sink.
    pub(super) fn begin_timed_sink(
        &mut self,
        now: Duration,
    ) -> Result<Option<(Offer<'_>, Duration)>, Fault> {
        self.poll(now)?;
        let Some(sequence) = self.rx.expected() else {
            return self.fence_on_fault(Err(Fault::Counter));
        };
        let Some(item) = self.rx.pending.get(&sequence) else {
            return Ok(None);
        };
        let started = *self.rx.unresolved_since.get_or_insert(now);
        let until = item.deadline.min(deadline(started, DELIVERY_WAIT)?);
        self.rx.offered = Some(sequence);
        let offer = match &item.data {
            Data::Input(bytes) | Data::Output(bytes) => {
                Offer::Bytes(sequence, &bytes[self.rx.offset..])
            }
            Data::Resize(size) => Offer::Resize(sequence, *size),
        };
        Ok(Some((offer, until)))
    }

    pub(super) fn sink_progress(
        &mut self,
        sequence: u64,
        effect: Effect,
        now: Duration,
    ) -> Result<Progress, Fault> {
        self.poll(now)?;
        let result = self.rx.progress(sequence, effect, now);
        let progress = self.fence_on_fault(result)?;
        if progress == Progress::Committed {
            self.controls.ack = Some(self.rx.committed);
            self.activity(now);
        }
        Ok(progress)
    }

    pub(super) fn enqueue(&mut self, data: Data, now: Duration) -> Result<u64, Fault> {
        self.poll(now)?;
        // A caller trying to enqueue with no credits gets backpressure, not a protocol error.
        // It retains ownership of any upstream bytes until it can enqueue them.
        let result = self.tx.enqueue(data, self.binding, self.incoming.reverse());
        if result == Err(Fault::Counter) {
            self.fence();
        }
        result
    }

    /// The caller owns the single unresolved native publication and frozen signed event.
    /// A definite transient refusal keeps this exact prepared application payload.
    pub(super) fn begin_handoff(
        &mut self,
        author: &[u8; 32],
        created: u64,
        now: Duration,
    ) -> Result<Option<(u64, &[u8])>, Fault> {
        self.poll(now)?;
        let result = self
            .tx
            .prepare(author, created, self.binding, self.incoming.reverse(), now);
        self.fence_on_fault(result)?;
        if self.tx.prepared.is_some() {
            self.note_handoff(now);
        }
        Ok(self
            .tx
            .prepared
            .as_ref()
            .map(|prepared| (prepared.sequence, prepared.bytes.as_slice())))
    }

    /// Report a native handoff when the outer owner retains the frozen signed event.
    pub(super) fn handed_off(&mut self, sequence: u64, now: Duration) -> Result<(), Fault> {
        self.poll(now)?;
        if self
            .tx
            .prepared
            .as_ref()
            .is_none_or(|item| sequence != item.sequence)
        {
            return self.fence_on_fault(Err(Fault::Handoff));
        }
        self.tx.prepared = None;
        Ok(())
    }

    pub(super) fn control(&mut self, now: Duration) -> Result<Option<Control>, Fault> {
        if self.controls.close {
            return Ok(Some(Control::Close));
        }
        self.poll(now)?;
        if self.incoming == Direction::FromServer
            && now >= deadline(self.last_handoff, PULSE_INTERVAL)?
            && self.controls.pulse.is_none()
        {
            let result = self
                .controls
                .last_sent_pulse
                .checked_add(1)
                .ok_or(Fault::Counter);
            let pulse = self.fence_on_fault(result)?;
            self.controls.pulse = Some(pulse);
        }
        Ok(self
            .controls
            .pulse
            .map(Control::Pulse)
            .or_else(|| self.controls.ack.map(Control::Ack)))
    }

    pub(super) fn control_begun(&mut self, sent: Control, now: Duration) {
        match sent {
            Control::Ack(sequence) if self.controls.ack == Some(sequence) => {
                self.controls.ack = None;
                if sequence <= self.controls.last_sent_ack {
                    return;
                }
                self.controls.last_sent_ack = sequence;
            }
            Control::Pulse(pulse) if self.controls.pulse == Some(pulse) => {
                self.controls.pulse = None;
                self.controls.last_sent_pulse = pulse;
            }
            Control::Close => self.controls.close = false,
            _ => return,
        }
        self.note_handoff(now);
    }

    fn note_handoff(&mut self, now: Duration) {
        self.last_handoff = self.last_handoff.max(now);
    }

    pub(super) fn close(&mut self) {
        if !self.closed {
            self.fence();
            self.controls.close = true;
        }
    }

    /// Faults abandon pending controls too. The peer's independent lease
    /// handles closure when no best-effort Close is available.
    pub(super) fn fence(&mut self) {
        self.rx = Incoming::default();
        self.tx = Outgoing::default();
        self.controls = Controls::default();
        self.closed = true;
    }

    #[cfg(test)]
    pub(super) fn seed_counters(&mut self, committed: u64, assigned: u64, sent_pulse: u64) {
        self.rx.committed = committed;
        self.tx.assigned = assigned;
        self.controls.last_sent_pulse = sent_pulse;
    }
}

fn deadline(start: Duration, wait: Duration) -> Result<Duration, Fault> {
    start.checked_add(wait).ok_or(Fault::Counter)
}
