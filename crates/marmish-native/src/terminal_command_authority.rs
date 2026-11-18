//! Command-owned attachment authority for the isolated Terminal integration.

use super::{
    AuthenticatedCommandContext, CommandDirection, GroupId, IngestOutcome, MdkOwner,
    TransportEnvelope,
};
use crate::command_wire::{Account, Id};
use crate::terminal_fence::{AttachmentFence, FenceState, UpdateToken};
use crate::{Error, Result};
use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

const CAPACITY: usize = 32;

#[derive(Clone)]
pub(super) struct Binding {
    pub(super) context: AuthenticatedCommandContext,
    pub(super) fence: Arc<AttachmentFence>,
}

#[derive(Default)]
pub(super) struct Authority {
    generation: u64,
    entries: BTreeMap<Vec<u8>, Binding>,
    pause_samples: Option<Vec<Duration>>,
}

#[derive(Default)]
pub(super) struct Updates {
    entries: Vec<(Binding, UpdateToken)>,
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
        if self.entries.len() == CAPACITY {
            self.prune_retired();
        }
        if self.entries.len() == CAPACITY {
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
            began: began.filter(|_| !entries.is_empty()),
            entries,
        }
    }

    fn prune_retired(&mut self) {
        self.entries
            .retain(|_, entry| entry.fence.state() != FenceState::Retired);
    }

    pub(super) fn measure_pauses(&mut self) {
        self.pause_samples = Some(Vec::with_capacity(1024));
    }

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
                restored = false;
            }
        }
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

    #[cfg(test)]
    pub(super) fn terminal_client_binding(
        &mut self,
        peer: Account,
        relays: &[String],
        session: Id,
    ) -> Result<Option<Binding>> {
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
    pub(super) fn terminal_server_bindings(&mut self) -> Result<Vec<Binding>> {
        let mut accepted = Vec::new();
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
            match self.terminal_validate(&GroupId::new(room.group)) {
                Ok(context) => accepted.push(context),
                Err(
                    Error::Mdk(_) | Error::Invalid(_) | Error::Transport("group_state_unavailable"),
                ) => {
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        self.terminal_authority.prune_retired();
        let additions = accepted
            .iter()
            .filter(|entry| {
                !self
                    .terminal_authority
                    .entries
                    .contains_key(entry.mls_group())
            })
            .count();
        if self.terminal_authority.entries.len() + additions > CAPACITY {
            return Err(Error::Invalid("terminal_authority_capacity"));
        }
        accepted
            .into_iter()
            .map(|entry| self.terminal_authority.register(entry))
            .collect()
    }
}

#[path = "terminal_command_authority_tests.rs"]
mod tests;
