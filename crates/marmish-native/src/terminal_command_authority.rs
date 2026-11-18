//! Command-owned attachment authority for the isolated Terminal integration.

use super::{
    AuthenticatedCommandContext, CommandDirection, GroupId, IngestOutcome, MdkOwner,
    TransportEnvelope,
};
use crate::command_wire::{Account, Id};
use crate::terminal_fence::{AttachmentFence, FenceState, UpdateToken};
use crate::{Error, Result};
use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CAPACITY: usize = 32;
const MAX_LISTENER_CANDIDATES: usize = 64;
const LISTENER_PASS: Duration = Duration::from_secs(30);

type RoomKey = (Id, Vec<u8>);

#[derive(Clone, Default)]
struct ListenerSchedule {
    next_pass: Option<Instant>,
    start_peer: Option<Account>,
    after_room: BTreeMap<Account, RoomKey>,
    credited: Vec<(Account, RoomKey)>,
}

impl ListenerSchedule {
    fn select(
        &mut self,
        by_peer: BTreeMap<Account, VecDeque<AuthenticatedCommandContext>>,
        now: Instant,
    ) -> Vec<AuthenticatedCommandContext> {
        let count: usize = by_peer.values().map(VecDeque::len).sum();
        let mut peers = by_peer.into_iter().collect::<Vec<_>>();
        if count <= CAPACITY {
            *self = Self::default();
        } else {
            self.order_pass(&mut peers, now);
        }
        let mut selected = Vec::with_capacity(count.min(CAPACITY));
        while !peers.is_empty() && selected.len() < CAPACITY {
            let remaining = CAPACITY - selected.len();
            selected.extend(
                peers
                    .iter_mut()
                    .filter_map(|(_, rooms)| rooms.pop_front())
                    .take(remaining),
            );
            peers.retain(|(_, rooms)| !rooms.is_empty());
        }
        selected
    }

    fn order_pass(
        &mut self,
        peers: &mut [(Account, VecDeque<AuthenticatedCommandContext>)],
        now: Instant,
    ) {
        let mut start =
            peers.partition_point(|(peer, _)| self.start_peer.is_some_and(|start| *peer < start));
        if self.next_pass.is_some_and(|deadline| now >= deadline) {
            start = (start + self.credited.len().max(1)) % peers.len();
            self.start_peer = Some(peers[start].0);
            self.after_room.extend(self.credited.drain(..));
            self.next_pass = None;
        }
        self.start_peer.get_or_insert(peers[0].0);
        self.next_pass.get_or_insert(now + LISTENER_PASS);
        self.after_room
            .retain(|peer, _| peers.binary_search_by_key(peer, |(peer, _)| *peer).is_ok());
        for (peer, rooms) in peers.iter_mut() {
            if let Some((session, group)) = self.after_room.get(peer) {
                let offset = rooms.partition_point(|context| {
                    (context.session(), context.mls_group()) <= (*session, group.as_slice())
                });
                rooms.rotate_left(offset);
            }
        }
        peers.rotate_left(start);
    }

    fn record(&mut self, prefix: &[Binding]) {
        if self.next_pass.is_some() && prefix.len() >= self.credited.len() {
            self.credited = prefix
                .iter()
                .map(|binding| {
                    let context = &binding.context;
                    (
                        context.peer_account(),
                        (context.session(), context.mls_group().to_vec()),
                    )
                })
                .collect();
        }
    }
}

#[derive(Clone)]
pub(crate) struct Binding {
    pub(crate) context: AuthenticatedCommandContext,
    pub(crate) fence: Arc<AttachmentFence>,
}

#[derive(Default)]
pub(super) struct Authority {
    generation: u64,
    entries: BTreeMap<Vec<u8>, Binding>,
    listeners: ListenerSchedule,
    #[cfg(test)]
    pause_samples: Option<Vec<Duration>>,
}

#[derive(Default)]
pub(super) struct Updates {
    entries: Vec<(Binding, UpdateToken)>,
    #[cfg(test)]
    began: Option<Instant>,
}

impl Authority {
    pub(super) fn register(&mut self, context: AuthenticatedCommandContext) -> Result<Binding> {
        let group = context.mls_group();
        if let Some(existing) = self.entries.get(group) {
            if existing.context == context && existing.fence.state() == FenceState::Live {
                return Ok(existing.clone());
            }
            self.retire(group);
        }
        self.prune_retired();
        if self
            .entries
            .values()
            .filter(|entry| entry.context.direction() == context.direction())
            .count()
            >= CAPACITY
        {
            return Err(Error::Invalid("terminal_authority_capacity"));
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(Error::Invalid("terminal_authority_generation"))?;
        let fence = AttachmentFence::new(
            NonZeroU64::new(self.generation).expect("incremented generation"),
            context.authority_digest(),
        );
        let group = group.to_vec();
        let binding = Binding { context, fence };
        self.entries.insert(group, binding.clone());
        Ok(binding)
    }

    fn register_listeners(
        &mut self,
        candidates: BTreeMap<Account, VecDeque<AuthenticatedCommandContext>>,
        now: Instant,
    ) -> Result<Vec<Binding>> {
        // Publish cursor progress only after building the complete snapshot.
        let mut schedule = self.listeners.clone();
        let mut bindings = Vec::new();
        let mut prefix_end = None;
        for context in schedule.select(candidates, now) {
            match self.register(context) {
                Ok(binding) => bindings.push(binding),
                Err(Error::Invalid("terminal_authority_capacity")) => {
                    prefix_end.get_or_insert(bindings.len());
                }
                Err(error) => return Err(error),
            }
        }
        schedule.record(&bindings[..prefix_end.unwrap_or(bindings.len())]);
        self.listeners = schedule;
        Ok(bindings)
    }

    pub(super) fn retire(&mut self, group: &[u8]) {
        if let Some(binding) = self.entries.remove(group) {
            binding.fence.retire();
        }
    }

    pub(super) fn retire_all(&mut self) {
        for binding in std::mem::take(&mut self.entries).into_values() {
            binding.fence.retire();
        }
    }

    pub(super) fn stage(&mut self, group: Option<&GroupId>) -> Updates {
        if self.entries.is_empty() {
            return Updates::default();
        }
        self.prune_retired();
        #[cfg(test)]
        let began = self.pause_samples.as_ref().map(|_| Instant::now());
        let entries: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, _)| group.is_none_or(|group| key.as_slice() == group.as_slice()))
            .filter_map(|(_, entry)| {
                entry
                    .fence
                    .begin_update()
                    .map(|token| (entry.clone(), token))
            })
            .collect();
        Updates {
            #[cfg(test)]
            began: began.filter(|_| !entries.is_empty()),
            entries,
        }
    }

    fn prune_retired(&mut self) {
        self.entries
            .retain(|_, entry| entry.fence.state() != FenceState::Retired);
    }

    fn prune_unused(&mut self) {
        self.entries.retain(|_, entry| {
            if Arc::strong_count(&entry.fence) == 1 {
                entry.fence.retire();
                false
            } else {
                true
            }
        });
    }

    #[cfg(test)]
    pub(super) fn measure_pauses(&mut self) {
        self.pause_samples = Some(Vec::with_capacity(1024));
    }

    #[cfg(test)]
    pub(super) fn measured_pauses(&mut self) -> Vec<Duration> {
        self.pause_samples.take().unwrap_or_default()
    }

    pub(super) fn delivery_group(&self, envelope: &TransportEnvelope) -> Option<GroupId> {
        let TransportEnvelope::GroupMessage { transport_group_id } = envelope else {
            return None;
        };
        let mut candidates = self
            .entries
            .values()
            .filter(|entry| entry.context.transport_group() == transport_group_id.as_slice());
        let entry = candidates.next()?;
        candidates
            .next()
            .is_none()
            .then(|| GroupId::new(entry.context.mls_group().to_vec()))
    }
}

impl Drop for Authority {
    fn drop(&mut self) {
        self.retire_all();
    }
}

impl MdkOwner {
    fn terminal_granted(&self, context: &AuthenticatedCommandContext) -> bool {
        let peer = context.peer_account();
        self.allowed.contains(&hex::encode(peer))
            && self
                .commands
                .as_ref()
                .is_some_and(|service| match context.direction() {
                    CommandDirection::Client => service.can_open(&peer),
                    CommandDirection::Server => service.can_accept_terminal(&peer),
                })
    }

    pub(super) fn terminal_prune_grants(&mut self) {
        let revoked: Vec<_> = self
            .terminal_authority
            .entries
            .iter()
            .filter(|(_, entry)| !self.terminal_granted(&entry.context))
            .map(|(group, _)| group.clone())
            .collect();
        for group in revoked {
            self.terminal_authority.retire(&group);
        }
    }

    fn terminal_validate(&mut self, group: &GroupId) -> Result<AuthenticatedCommandContext> {
        let result = match self.command_context(group) {
            Ok(mut context) if self.active && !self.recovery && self.terminal_granted(&context) => {
                match crate::transport::RelayPolicy::parse(
                    &context.relay_endpoints,
                    self.connector.policy().mode(),
                    self.connector.policy().limits(),
                ) {
                    Ok(policy) => {
                        context.relay_endpoints =
                            policy.urls().iter().map(ToString::to_string).collect();
                        return Ok(context);
                    }
                    Err(error) => error,
                }
            }
            Ok(_) => Error::Invalid("terminal_authority"),
            Err(error) => error,
        };
        self.terminal_authority.retire(group.as_slice());
        Err(result)
    }

    pub(super) fn terminal_finish(&mut self, updates: Updates) {
        #[cfg(test)]
        let mut restored = true;
        for (entry, token) in updates.entries {
            let group = GroupId::new(entry.context.mls_group().to_vec());
            let digest = self
                .terminal_validate(&group)
                .ok()
                .filter(|current| current == &entry.context)
                .map(|current| current.authority_digest());
            if token.resolve(digest) == FenceState::Retired {
                self.terminal_authority.retire(group.as_slice());
                #[cfg(test)]
                {
                    restored = false;
                }
            }
        }
        #[cfg(test)]
        if restored
            && let Some(began) = updates.began
            && let Some(samples) = self.terminal_authority.pause_samples.as_mut()
            && samples.len() < samples.capacity()
        {
            samples.push(began.elapsed());
        }
    }

    pub(super) fn terminal_after_ingest(
        &mut self,
        updates: Updates,
        outcome: &IngestOutcome,
        durable: bool,
        projected: Result<()>,
    ) -> Result<()> {
        projected?;
        // Buffered effects can still converge later. Local state, deferred
        // transport and resource refusal leave classification uncertain.
        if durable
            && matches!(
                outcome,
                IngestOutcome::Processed
                    | IngestOutcome::Ignored { .. }
                    | IngestOutcome::Stale { .. }
                    | IngestOutcome::Rejected { .. }
            )
        {
            self.terminal_finish(updates);
        }
        Ok(())
    }

    pub(crate) fn terminal_client_binding(
        &mut self,
        peer: Account,
        relays: &[String],
        session: Id,
    ) -> Result<Option<Binding>> {
        if !self.active || self.recovery {
            return Err(Error::Transport("terminal_transport_unavailable"));
        }
        self.terminal_authority.prune_unused();
        let ready = match self.command_can_prepare(peer, relays) {
            Ok(()) => self.command_ready_session(peer, relays, session),
            Err(error @ Error::Invalid("commands_disabled" | "command_unauthorized")) => Err(error),
            Err(error) => return Err(error),
        };
        if !matches!(ready, Ok(true)) {
            self.terminal_authority.entries.retain(|_, entry| {
                let matches = entry.context.peer_account() == peer
                    && entry.context.session() == session
                    && entry.context.direction() == CommandDirection::Client;
                if matches {
                    entry.fence.retire();
                }
                !matches
            });
            return ready.map(|_| None);
        }
        let room = self
            .journal
            .command_rooms()?
            .into_iter()
            .find(|row| {
                row.peer == peer
                    && row.session == session
                    && row.direction == CommandDirection::Client
            })
            .ok_or(Error::Invalid("terminal_command_binding"))?;
        let context = self.terminal_validate(&GroupId::new(room.group))?;
        self.terminal_authority.register(context).map(Some)
    }

    #[cfg(test)]
    pub(super) fn terminal_server_binding_for_test(&mut self, group: &[u8]) -> Result<Binding> {
        let context = self.terminal_validate(&GroupId::new(group.to_vec()))?;
        self.terminal_authority.register(context)
    }

    pub(crate) fn terminal_server_bindings(&mut self) -> Result<Vec<Binding>> {
        self.terminal_server_bindings_at(Instant::now())
    }

    pub(super) fn terminal_server_bindings_at(&mut self, now: Instant) -> Result<Vec<Binding>> {
        if !self.active || self.recovery {
            return Err(Error::Transport("terminal_transport_unavailable"));
        }
        self.terminal_authority.prune_unused();
        let mut candidates = 0;
        let mut accepted: BTreeMap<_, VecDeque<_>> = BTreeMap::new();
        for room in self.journal.command_rooms()? {
            if room.revoked
                || room.direction != CommandDirection::Server
                || !self
                    .commands
                    .as_ref()
                    .is_some_and(|service| service.can_accept_terminal(&room.peer))
            {
                continue;
            }
            candidates += 1;
            if candidates > MAX_LISTENER_CANDIDATES {
                return Err(Error::Invalid("terminal_listener_candidates"));
            }
            match self.terminal_validate(&GroupId::new(room.group)) {
                Ok(context)
                    if context.relay_endpoints().iter().any(|origin| {
                        self.connector.policy().direct_wss_scope(origin).is_some()
                    }) =>
                {
                    accepted
                        .entry(context.peer_account())
                        .or_default()
                        .push_back(context);
                }
                Ok(_) => (),
                Err(
                    Error::Mdk(_) | Error::Invalid(_) | Error::Transport("group_state_unavailable"),
                ) => {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        self.terminal_authority.prune_retired();
        for entries in accepted.values_mut() {
            entries.make_contiguous().sort_by(|left, right| {
                (left.session(), left.mls_group()).cmp(&(right.session(), right.mls_group()))
            });
        }
        self.terminal_authority.register_listeners(accepted, now)
    }
}

#[cfg(test)]
#[path = "terminal_command_authority_tests.rs"]
mod tests;
