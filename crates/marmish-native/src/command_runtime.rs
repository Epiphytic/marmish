//! Command lifecycle on the existing serialized identity owner.
use super::*;
use crate::command_config::CommandConfig;
use crate::command_engine::{CommandEngine, EngineSettings};
use crate::command_service::CommandService;
use crate::command_wire::{self as wire, Account, Id, Request, RequestOp};
use crate::journal::{CommandRoom, InvocationRecord};
use std::path::PathBuf;

type PublicationProofs = BTreeMap<(Vec<u8>, Vec<u8>), crate::journal::PublicationKey>;

/// Offline tests replace only network discovery and final publication delivery;
/// group creation, acceptance, authority and durable creation fences stay real.
#[cfg(test)]
pub(super) struct CreationTest {
    pub package: Option<KeyPackage>,
    pub stage: CreationStage,
    pub attempts: usize,
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CreationStage {
    PendingWelcome,
    DiscoveryFailure,
    ExpiredAfterDiscovery,
    StartExpiredAfterDiscovery,
    InterruptAfterFence,
    NativeFailure,
    MismatchedSession,
}

impl MdkOwner {
    pub(crate) fn command_cancel_fixture(&mut self, invocations: &[Id]) -> Result<()> {
        self.commands
            .as_mut()
            .ok_or(Error::Invalid("commands_disabled"))?
            .cancel_fixture_invocations(&mut self.journal, invocations)
    }
    pub(crate) fn command_begin_cleanup(&mut self) -> Result<u64> {
        self.commands
            .as_mut()
            .ok_or(Error::Invalid("commands_disabled"))?
            .begin_cleanup(&mut self.journal)
    }
    pub(crate) fn command_poll_cleanup(
        &mut self,
    ) -> Result<(crate::command_engine::CleanupState, Option<u64>)> {
        let service = self
            .commands
            .as_mut()
            .ok_or(Error::Invalid("commands_disabled"))?;
        service.poll_cleanup(&mut self.journal)?;
        Ok(service.cleanup_snapshot())
    }
    pub(crate) fn take_engine_for_stop(&mut self) -> Option<CommandEngine> {
        self.commands
            .as_mut()
            .and_then(CommandService::take_engine_for_stop)
    }
    pub(crate) fn enable_commands(
        &mut self,
        profile: &ProfileLease,
        material: &IdentityMaterial,
        config: CommandConfig,
        worker: PathBuf,
    ) -> Result<()> {
        if self.commands.is_some()
            || config.relay_policy().urls() != self.connector.policy().urls()
            || config.relay_policy().mode() != self.connector.policy().mode()
        {
            return Err(Error::Invalid("command_runtime_binding"));
        }
        let account = material.public_key().to_bytes();
        if hex::encode(account) != self.identity
            || profile.profile_id() != self.binding.profile_id()
        {
            return Err(Error::Identity("command_runtime_binding"));
        }
        let historical = self.journal.command_historical_principals()?;
        let policy = config.engine_policy(
            profile.root().join("commands"),
            worker,
            account,
            &historical,
        )?;
        self.journal.command_configure(config.limits())?;
        let material = crate::command_identity::derive(
            profile.profile_id(),
            account,
            material.sqlcipher_root(),
        )?;
        self.journal.command_bind_engine(
            crate::command_identity::DERIVATION_VERSION,
            &material.root_id,
            &material.key_id,
        )?;
        let engine = match policy {
            Some(policy) => Some(CommandEngine::start(
                EngineSettings {
                    policy,
                    binding: material.binding,
                    master: material.master,
                },
                self.journal.command_recovery()?,
            )?),
            None => None,
        };
        self.commands = Some(CommandService::new(config, engine));
        Ok(())
    }
    pub(super) fn progress_commands(&mut self) -> Result<()> {
        if let Some(service) = &mut self.commands {
            service.poll_completions(&mut self.journal, now()?)?;
        }
        Ok(())
    }
    pub(crate) async fn open_command_room(
        &mut self,
        peer: Account,
        new_session: bool,
        deadline: Instant,
    ) -> Result<CommandRoom> {
        if !self.commands.as_ref().is_some_and(|s| s.can_open(&peer)) {
            return Err(Error::Invalid("command_unauthorized"));
        }
        if !self.active {
            return Err(Error::Mdk("inactive"));
        }
        self.ensure_ingress()?;
        if !new_session {
            for room in self.journal.command_rooms()? {
                if room.peer == peer
                    && room.direction == CommandDirection::Client
                    && self
                        .journal
                        .command_preparation_session(room.session)?
                        .is_none()
                {
                    match self.command_context(&GroupId::new(room.group.clone())) {
                        Ok(_) => return Ok(room),
                        Err(Error::Mdk("command_room_revoked")) => continue,
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        self.create_command_room(peer, None, deadline).await
    }
    pub(crate) async fn open_prepared_command_room(
        &mut self,
        peer: Account,
        fresh: Option<Id>,
        deadline: Instant,
    ) -> Result<CommandRoom> {
        let Some(token) = fresh else {
            return self.open_command_room(peer, false, deadline).await;
        };
        let relays = self
            .connector
            .policy()
            .urls()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        self.command_can_prepare(peer, &relays)?;
        if let Some(record) =
            self.journal
                .command_preparation(&token, &peer, &self.command_relay_digest()?)?
        {
            let room = self
                .journal
                .command_rooms()?
                .into_iter()
                .find(|r| {
                    r.session == record.session
                        && r.peer == peer
                        && r.direction == CommandDirection::Client
                })
                .ok_or(Error::Invalid("command_room_uncertain"))?;
            self.command_context(&GroupId::new(room.group.clone()))?;
            return Ok(room);
        }
        super::command_local::fresh_start_window(token, now()?)?;
        self.create_command_room(peer, Some(token), deadline).await
    }
    async fn create_command_room(
        &mut self,
        peer: Account,
        fresh: Option<Id>,
        deadline: Instant,
    ) -> Result<CommandRoom> {
        if !self.active {
            return Err(Error::Mdk("inactive"));
        }
        self.ensure_ingress()?;
        if self.journal.command_rooms()?.len() >= 64 {
            return Err(Error::Invalid("command_room_capacity"));
        }
        let current = now()?;
        if current < self.journal.command_floor()? {
            return Err(Error::Invalid("clock_behind_floor"));
        }
        let session = foundation::fresh_id(current)?;
        #[cfg(test)]
        let kp = if let Some(test) = &mut self.command_creation_test {
            if test.stage == CreationStage::DiscoveryFailure {
                return Err(Error::Transport("key_package_discovery"));
            }
            test.package
                .take()
                .ok_or(Error::Invalid("test_key_package"))?
        } else {
            self.discover(&hex::encode(peer), deadline).await?
        };
        #[cfg(not(test))]
        let kp = self.discover(&hex::encode(peer), deadline).await?;
        #[cfg(test)]
        let deadline = if self
            .command_creation_test
            .as_ref()
            .is_some_and(|test| test.stage == CreationStage::ExpiredAfterDiscovery)
        {
            Instant::now()
        } else {
            deadline
        };
        let routing = NostrRoutingV1::new(
            random::<32>()?,
            self.routing.endpoints.iter().map(|v| v.0.clone()).collect(),
        )
        .map_err(|_| Error::Invalid("routing"))?;
        let request = CreateGroupRequest {
            name: format!("{}{session}", RoomPurpose::Command.prefix()),
            description: RoomPurpose::Command.description().into(),
            members: vec![kp],
            required_features: vec![],
            initial_admins: vec![],
            app_components: vec![AppComponentData {
                component_id: NOSTR_ROUTING_COMPONENT_ID,
                data: encode_nostr_routing_v1(&routing).map_err(|_| Error::Invalid("routing"))?,
            }],
        };
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        if let Some(token) = fresh {
            let current = now()?;
            #[cfg(test)]
            let current = if self
                .command_creation_test
                .as_ref()
                .is_some_and(|test| test.stage == CreationStage::StartExpiredAfterDiscovery)
            {
                current.saturating_add(46)
            } else {
                current
            };
            super::command_local::fresh_start_window(token, current)?;
            let relays = self
                .connector
                .policy()
                .urls()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            self.command_can_prepare(peer, &relays)?;
            match self.journal.command_begin_preparation(
                token,
                peer,
                self.command_relay_digest()?,
                super::command_adapter::fixed_hex::<16>(&session)?,
                now()?,
            )? {
                crate::journal::PreparationStart::Created(_) => {}
                crate::journal::PreparationStart::Existing(_) => {
                    return Err(Error::Invalid("command_room_uncertain"));
                }
            }
        }
        #[cfg(test)]
        let deadline = if self
            .command_creation_test
            .as_ref()
            .is_some_and(|test| test.stage == CreationStage::InterruptAfterFence)
        {
            Instant::now()
        } else {
            deadline
        };
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        #[cfg(test)]
        let request = {
            let mut request = request;
            if let Some(test) = &mut self.command_creation_test {
                test.attempts += 1;
                if test.stage == CreationStage::NativeFailure {
                    return Err(Error::Mdk("create_group"));
                }
                if test.stage == CreationStage::MismatchedSession {
                    request.name = format!(
                        "{}{}",
                        RoomPurpose::Command.prefix(),
                        foundation::fresh_id(now()?)?
                    );
                }
            }
            request
        };
        let (group, effects) =
            tokio::time::timeout_at(deadline, self.runtime.create_group(request))
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Mdk("create_group"))?;
        let room = match self.accept_command_group(&group, &self.identity.clone()) {
            Ok(room) => room,
            Err(error) => {
                self.command_forget_and_confirm(&group)?;
                return Err(error);
            }
        };
        if room.peer != peer
            || (fresh.is_some()
                && (room.session != super::command_adapter::fixed_hex::<16>(&session)?
                    || room.direction != CommandDirection::Client))
        {
            self.command_forget_and_confirm(&group)?;
            return Err(Error::Mdk("command_room_peer"));
        }
        #[cfg(test)]
        if self.command_creation_test.is_some() {
            // Keep real native Welcome obligations pending, without a relay.
            return Ok(room);
        }
        self.apply_effects(effects, deadline).await?;
        mdk(self.runtime.sync_transport_groups(None).await)?;
        Ok(room)
    }
    pub(crate) fn command_request(
        &mut self,
        session: Id,
        id: Id,
        op: RequestOp,
    ) -> Result<InvocationRecord> {
        let room = self
            .journal
            .command_rooms()?
            .into_iter()
            .find(|r| r.session == session)
            .ok_or(Error::Invalid("command_session"))?;
        let context = self.command_context(&GroupId::new(room.group))?;
        if context.direction() != CommandDirection::Client {
            return Err(Error::Invalid("command_direction"));
        }
        let request = Request {
            binding: wire::Binding {
                session,
                client: context.local_account(),
                server: context.peer_account(),
            },
            id,
            op,
        };
        self.commands
            .as_mut()
            .ok_or(Error::Invalid("command_disabled"))?
            .client_request(&mut self.journal, &context, &request, now()?)
    }
}

impl MdkOwner {
    pub(super) fn ensure_application_group(&mut self, group: &GroupId) -> Result<()> {
        if self.journal.command_room(group.as_slice())?.is_some() {
            self.command_context(group).map(|_| ())
        } else {
            self.ensure_trusted(group)
        }
    }
    /// Claim before calling MDK: an interrupted send never creates new ciphertext.
    pub(super) async fn submit_command_publications(&mut self, deadline: Instant) -> Result<()> {
        use crate::journal::{CommandNativeUpdate, PublicationState};
        if self.commands.is_none() {
            return Ok(());
        }
        let local: Account = self
            .account
            .as_slice()
            .try_into()
            .map_err(|_| Error::Identity("command_account"))?;
        for publication in self
            .journal
            .command_publications(PublicationState::Unsubmitted, 64)?
        {
            let group = GroupId::new(publication.key.group.clone());
            match self.command_context(&group) {
                Ok(_) => {}
                Err(Error::Mdk("command_room_revoked")) => continue,
                Err(error) => return Err(error),
            }
            self.ensure_ingress()?;
            publication.validate(local)?;
            let Some(publication) = self.journal.command_claim_publication(&publication.key)?
            else {
                continue;
            };
            let effects = tokio::time::timeout_at(
                deadline,
                self.runtime.send(SendIntent::AppMessage {
                    group_id: group,
                    payload: publication.encoded_app_event,
                    expected_epoch: None,
                }),
            )
            .await
            .map_err(|_| Error::Timeout)?;
            let effects = mdk(effects)?;
            let updates = effects
                .queued
                .iter()
                .map(|queued| {
                    if queued.group_id.as_slice() != publication.key.group {
                        return Err(Error::Mdk("command_queued_binding"));
                    }
                    Ok(CommandNativeUpdate::Queued {
                        key: publication.key.clone(),
                        intent: queued.intent_id.as_slice().to_vec(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            self.journal.command_apply_native(&updates)?;
            self.apply_effects(effects, deadline).await?;
        }
        let native_fanouts = mdk(self.storage.list_outbound_fanouts())?;
        for publication in self
            .journal
            .command_publications(PublicationState::Prepared, 64)?
        {
            let group = GroupId::new(publication.key.group.clone());
            let retained = native_fanouts.iter().any(|fanout| {
                fanout.group_id() == Some(&group)
                    && fanout.application_message().is_some_and(|application| {
                        application.app_event_id == publication.key.app_event_id
                    })
            });
            let queued = mdk(self.storage.list_queued_outbound_intents(&group))?;
            // Any queued intent can still own this ambiguity. Convergence alone
            // may retire it; this path never invokes send again.
            if !retained && queued.is_empty() {
                self.journal
                    .command_publication_uncertain(&publication.key)?;
            }
        }
        Ok(())
    }
    /// One fair, finite page per turn. Failed bindings retain their evidence
    /// without preventing later bindings from reaching their genuine ACK.
    pub(super) fn reconcile_command_sources(&mut self) -> Result<()> {
        use crate::journal::{CommandNativeUpdate, PublishedKey};
        let page = self
            .journal
            .command_native_bindings(self.command_source_cursor, 64)?;
        let live = mdk(self.runtime.live_group_ids())?;
        let mut incoming = Vec::new();
        let mut outgoing = Vec::new();
        for binding in &page.rows {
            if binding.quarantined {
                continue;
            }
            let projected = (|| -> Result<()> {
                let id = MessageId::new(binding.source.native_id.clone());
                let terminal_invalid = match self.storage.get_message(&id) {
                    Ok(row) if row.group_id.as_slice() != binding.source.group => true,
                    Ok(row) => match row.state {
                        cgka_traits::MessageState::EpochInvalidated
                        | cgka_traits::MessageState::Failed => true,
                        cgka_traits::MessageState::Sent | cgka_traits::MessageState::Processed => {
                            false
                        }
                        _ => return Ok(()),
                    },
                    // Replay may temporarily remove a source row. Neither its
                    // absence nor a deferred projection is a terminal verdict.
                    Err(cgka_traits::StorageError::NotFound) => return Ok(()),
                    Err(_) => return Err(Error::Storage("command_native_reconcile")),
                };
                if terminal_invalid && !binding.invalidated {
                    self.journal
                        .command_apply_native(&[CommandNativeUpdate::Invalidated {
                            group: binding.source.group.clone(),
                            native_id: binding.source.native_id.clone(),
                            possible_publication: None,
                        }])?;
                }
                if let Some(key) = &binding.publication {
                    if !binding.acked
                        && live.iter().any(|group| group.as_slice() == key.group)
                        && let Some(evidence) = &binding.evidence
                    {
                        outgoing.push(evidence.to_native(key, &binding.source.native_id));
                    }
                } else if !binding.acked {
                    mdk(self.storage.delete_pending_application_events(&[id]))?;
                    incoming.push(binding.source.clone());
                }
                Ok(())
            })();
            if projected.is_err() {
                crate::diagnostics::event("command_reconcile", "binding_deferred");
            }
        }
        self.command_source_cursor = page.next;
        self.journal.command_record_native_acks(&incoming, &[])?;
        if !outgoing.is_empty() {
            if self
                .runtime
                .acknowledge_published_app_messages(&outgoing)
                .is_ok()
            {
                // This unfiltered snapshot is authoritative even for native
                // groups absent from the runtime's live-only fanout view.
                let fanouts = mdk(self.storage.list_outbound_fanouts())?;
                let confirmed = outgoing
                    .iter()
                    .filter(|published| {
                        !fanouts.iter().any(|fanout| {
                            fanout.group_id() == Some(&published.group_id)
                                && (fanout.outcome().message_id == published.message_id
                                    || fanout.application_message().is_some_and(|application| {
                                        application.app_event_id == published.app_event_id
                                    }))
                        })
                    })
                    .map(|published| PublishedKey {
                        key: crate::journal::PublicationKey {
                            group: published.group_id.as_slice().to_vec(),
                            app_event_id: published.app_event_id.clone(),
                        },
                        native_id: published.message_id.as_slice().to_vec(),
                    })
                    .collect::<Vec<_>>();
                self.journal.command_record_native_acks(&[], &confirmed)?;
            } else {
                crate::diagnostics::event("command_reconcile", "publication_ack_deferred");
            }
        }
        Ok(())
    }
}

impl MdkOwner {
    /// Only locally retained native obligations can justify an early outgoing
    /// invalidation. Incoming frames never allocate speculative tombstones.
    pub(super) fn command_publication_proofs(
        &self,
        published: &[marmot_session::PublishedApplicationMessage],
    ) -> Result<PublicationProofs> {
        use crate::journal::PublicationKey;
        let mut proofs = BTreeMap::new();
        for publication in published {
            let key = PublicationKey {
                group: publication.group_id.as_slice().to_vec(),
                app_event_id: publication.app_event_id.clone(),
            };
            if self.journal.command_publication_owned(&key)? {
                proofs.insert(
                    (
                        key.group.clone(),
                        publication.message_id.as_slice().to_vec(),
                    ),
                    key,
                );
            }
        }
        for fanout in mdk(self.storage.list_outbound_fanouts())? {
            if let (Some(group), Some(application)) =
                (fanout.group_id(), fanout.application_message())
            {
                let key = PublicationKey {
                    group: group.as_slice().to_vec(),
                    app_event_id: application.app_event_id.clone(),
                };
                if self.journal.command_publication_owned(&key)? {
                    proofs.insert(
                        (
                            key.group.clone(),
                            fanout.outcome().message_id.as_slice().to_vec(),
                        ),
                        key,
                    );
                }
            }
        }
        Ok(proofs)
    }

    pub(super) fn project_application_publications(
        &mut self,
        effects: &AccountDeviceEffects,
    ) -> Result<()> {
        // Only the purpose that durably owns an event may project or ACK it.
        // Foundation bind_native inserts sources even for unknown events, so an
        // ownership check must precede it.
        use crate::journal::{CommandNativeUpdate, PublicationKey};
        let mut committed = Vec::new();
        let mut command_updates = Vec::new();
        let proofs = self.command_publication_proofs(&effects.published_app_messages)?;
        let mut ownership_diagnostic = false;
        for published in &effects.published_app_messages {
            if self
                .journal
                .stream_publication_owned(published.group_id.as_slice(), &published.app_event_id)?
            {
                continue;
            }
            let key = PublicationKey {
                group: published.group_id.as_slice().to_vec(),
                app_event_id: published.app_event_id.clone(),
            };
            let foundation = self
                .journal
                .publication_owned(&key.group, &key.app_event_id)?;
            let command = self.journal.command_publication_owned(&key)?;
            match (foundation, command) {
                (true, false) => {
                    self.journal.bind_native(
                        &key.group,
                        &key.app_event_id,
                        published.message_id.as_slice(),
                    )?;
                    self.journal
                        .mark(&key.group, &key.app_event_id, "published")?;
                    committed.push(published.clone());
                }
                (false, true) => {
                    command_updates.push(CommandNativeUpdate::Published {
                        key,
                        native_id: published.message_id.as_slice().to_vec(),
                        evidence: crate::command_evidence::PublishedEvidence::from_native(
                            published,
                        ),
                    });
                }
                _ => ownership_diagnostic = true,
            }
        }
        for failed in &effects.failed_app_messages {
            if self
                .journal
                .stream_publication_owned(failed.group_id.as_slice(), &failed.app_event_id)?
            {
                continue;
            }
            let key = PublicationKey {
                group: failed.group_id.as_slice().to_vec(),
                app_event_id: failed.app_event_id.clone(),
            };
            match (
                self.journal
                    .publication_owned(&key.group, &key.app_event_id)?,
                self.journal.command_publication_owned(&key)?,
            ) {
                (true, false) => self
                    .journal
                    .mark(&key.group, &key.app_event_id, "invalidated")?,
                (false, true) => command_updates.push(CommandNativeUpdate::Failed { key }),
                _ => ownership_diagnostic = true,
            }
        }
        // Commit invalidations before publication confirmations, including
        // across transaction boundaries. The store retains terminal tombstones.
        for event in &effects.events {
            match event {
                GroupEvent::QueuedIntentRefused {
                    group_id,
                    intent_id,
                    ..
                } if self.journal.command_room(group_id.as_slice())?.is_some() => command_updates
                    .push(CommandNativeUpdate::RefusedQueued {
                        group: group_id.as_slice().to_vec(),
                        intent: intent_id.as_slice().to_vec(),
                    }),
                GroupEvent::AppMessageInvalidated {
                    group_id,
                    message_id,
                    ..
                } => command_updates.push(CommandNativeUpdate::Invalidated {
                    group: group_id.as_slice().to_vec(),
                    native_id: message_id.as_slice().to_vec(),
                    possible_publication: proofs
                        .get(&(group_id.as_slice().to_vec(), message_id.as_slice().to_vec()))
                        .cloned(),
                }),
                _ => {}
            }
        }
        command_updates.sort_by_key(|update| {
            matches!(
                update,
                CommandNativeUpdate::Published { .. } | CommandNativeUpdate::Queued { .. }
            )
        });
        let mut deferred = BTreeSet::new();
        for update in command_updates {
            if let CommandNativeUpdate::Published { key, native_id, .. } = &update
                && deferred.contains(&(key.group.clone(), native_id.clone()))
            {
                continue;
            }
            match self
                .journal
                .command_apply_native(std::slice::from_ref(&update))
            {
                Err(Error::Storage("command_invalidation_capacity")) => {
                    if let CommandNativeUpdate::Invalidated {
                        group,
                        native_id,
                        possible_publication,
                    } = update
                    {
                        if let Some(key) = possible_publication {
                            // At this MDK pin invalidation events are ephemeral.
                            // Preserve the proved publication's terminal verdict
                            // even when an early-source tombstone cannot fit.
                            self.journal
                                .command_apply_native(&[CommandNativeUpdate::Failed { key }])?;
                        }
                        deferred.insert((group, native_id));
                    }
                    crate::diagnostics::event("command_projection", "invalidation_deferred");
                }
                Err(Error::Invalid("command_queued_unknown")) => {
                    // A crash can occur after native queueing but before the
                    // local binding write. The Prepared sweep owns ambiguity;
                    // an unmatched refusal must not discard later effects.
                    crate::diagnostics::event("command_projection", "queued_binding_unknown");
                }
                other => {
                    other?;
                }
            }
        }
        if ownership_diagnostic {
            crate::diagnostics::event("mdk_projection", "purpose_ownership_unknown");
        }
        mdk(self.runtime.acknowledge_published_app_messages(&committed))?;
        // Outgoing command ACKs are recovered from their committed evidence by
        // the bounded source sweep, including after a crash at this boundary.
        self.retire_command_invalidations(&effects.published_app_messages)?;
        self.maintain_journal()?;
        Ok(())
    }
    fn retire_command_invalidations(
        &mut self,
        published: &[marmot_session::PublishedApplicationMessage],
    ) -> Result<()> {
        let fanouts = mdk(self.storage.list_outbound_fanouts())?;
        let mut pending = self.journal.command_pending_invalidations()?;
        pending.sort();
        let selected = pending
            .into_iter()
            .filter(|source| {
                self.command_invalidation_cursor
                    .as_ref()
                    .is_none_or(|after| source > after)
            })
            .take(64)
            .collect::<Vec<_>>();
        self.command_invalidation_cursor = if selected.len() == 64 {
            selected.last().cloned()
        } else {
            None
        };
        for source in selected {
            let possible = published.iter().any(|publication| {
                publication.group_id.as_slice() == source.group
                    && publication.message_id.as_slice() == source.native_id
            }) || fanouts.iter().any(|fanout| {
                fanout
                    .group_id()
                    .is_some_and(|group| group.as_slice() == source.group)
                    && fanout.outcome().message_id.as_slice() == source.native_id
            });
            if !possible {
                self.journal.command_retire_invalidation(&source)?;
            }
        }
        Ok(())
    }
}

impl MdkOwner {
    pub(super) async fn advance_application_convergence(
        &mut self,
        group: &GroupId,
        deadline: Instant,
    ) -> Result<()> {
        if mdk(self.runtime.has_pending_convergence_inputs(group))?
            || mdk(self.runtime.has_queued_outbound_intents(group))?
        {
            self.ensure_application_group(group)?;
            let effects =
                tokio::time::timeout_at(deadline, self.runtime.advance_convergence(group))
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(|_| Error::Transport("convergence"))?;
            self.apply_effects(effects, deadline).await?;
        }
        Ok(())
    }
}
