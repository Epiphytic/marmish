//! Dedicated command-room trust checks on the existing account owner.
use super::*;
use crate::command_wire::{self as wire, Account};
use crate::journal::{CommandRoom, CommandSource, SourceKey};

pub(crate) enum CommandEnvelope {
    Request(wire::Request),
    Response(wire::Response),
}
fn fixed_hex<const N: usize>(value: &str) -> Result<[u8; N]> {
    let bytes = hex::decode(value).map_err(|_| Error::Mdk("command_room_binding"))?;
    bytes
        .try_into()
        .map_err(|_| Error::Mdk("command_room_binding"))
}
impl MdkOwner {
    /// A dispatch hint only: full room authority is checked before admission.
    pub(super) fn is_command_join(&self, event: &GroupEvent) -> Result<bool> {
        let GroupEvent::GroupJoined { group_id, .. } = event else {
            return Ok(false);
        };
        if self.journal.command_room(group_id.as_slice())?.is_some() {
            return Ok(true);
        }
        let record = self
            .runtime
            .group_record(group_id)
            .map_err(|_| Error::Transport("group_state_unavailable"))?;
        Ok(record.name.starts_with(RoomPurpose::Command.prefix()))
    }

    pub(super) fn command_forget_and_confirm(&mut self, group: &GroupId) -> Result<()> {
        if self.journal.command_room(group.as_slice())?.is_some() {
            self.journal.command_revoke_room(group.as_slice())?;
        }
        self.routing.remove(group);
        mdk(self.runtime.forget_group_local(group))?;
        let pending = self
            .storage
            .list_pending_application_events()
            .map_err(|_| Error::Storage("command_native_pending"))?;
        if pending.iter().any(|event| {
            matches!(event,
            GroupEvent::GroupJoined { group_id, .. }
            | GroupEvent::MessageReceived { group_id, .. }
            | GroupEvent::AppMessageInvalidated { group_id, .. } if group_id == group)
        }) {
            return Err(Error::Storage("command_group_cleanup_pending"));
        }
        Ok(())
    }

    pub(super) fn project_command_join(&mut self, event: GroupEvent) -> Result<()> {
        let GroupEvent::GroupJoined {
            group_id,
            via_welcome,
            welcomer,
            ..
        } = event
        else {
            return Err(Error::Mdk("command_join_event"));
        };
        let creator = welcomer.map(|key| hex::encode(key.as_slice()));
        let verified = creator
            .as_deref()
            .ok_or(Error::Mdk("command_welcome_author"))
            .and_then(|creator| {
                self.check_group_for(&group_id, Some(creator), RoomPurpose::Command)
            });
        let (session, peer, _, _) = match verified {
            Ok(value) => value,
            Err(Error::Mdk(_) | Error::Invalid(_)) => {
                if self.journal.command_room(group_id.as_slice())?.is_some() {
                    self.journal.command_revoke_room(group_id.as_slice())?;
                }
                return self.command_forget_and_confirm(&group_id);
            }
            Err(error) => return Err(error),
        };
        let creator = creator.ok_or(Error::Mdk("command_welcome_author"))?;
        let peer = fixed_hex::<32>(&peer)?;
        let authorized = self.commands.as_ref().is_some_and(|service| {
            if creator == self.identity {
                service.can_open(&peer)
            } else {
                service.can_accept(&peer)
            }
        });
        if !authorized {
            return self.command_forget_and_confirm(&group_id);
        }
        match self.journal.command_room(group_id.as_slice())? {
            Some(room) if room.revoked => return self.command_forget_and_confirm(&group_id),
            Some(_) => {}
            None => {
                let current = now()?;
                let floor = self.journal.command_floor()?;
                if current < floor {
                    return Ok(());
                }
                match foundation::classify_new_id(&session, current, floor)? {
                    foundation::Freshness::Eligible => {}
                    foundation::Freshness::Deferred => return Ok(()),
                    foundation::Freshness::Expired | foundation::Freshness::ExcessiveFuture => {
                        return self.command_forget_and_confirm(&group_id);
                    }
                }
            }
        }
        match self.accept_command_group(&group_id, &creator) {
            Ok(_) => {}
            Err(error) if command_projection_deferred(&error) => return Ok(()),
            Err(Error::Mdk(_) | Error::Invalid(_)) => {
                return self.command_forget_and_confirm(&group_id);
            }
            Err(error) => return Err(error),
        }
        self.storage
            .delete_pending_application_events(&[via_welcome])
            .map_err(|_| Error::Storage("command_native_ack"))
    }

    pub(super) fn project_command_message(&mut self, event: GroupEvent) -> Result<()> {
        let GroupEvent::MessageReceived { ref message_id, .. } = event else {
            return Err(Error::Mdk("command_message_event"));
        };
        let message_id = message_id.clone();
        let (context, source, envelope) = match self.decode_command_event(&event) {
            Ok(value) => value,
            // The invalidation must be durably applied by its own event before
            // either native source is acknowledged.
            Err(Error::Mdk("command_message_invalidated")) => return Ok(()),
            Err(error) if command_projection_deferred(&error) => return Ok(()),
            Err(Error::Invalid(_)) => return self.reject_command_frame(message_id),
            Err(Error::Mdk(category)) if category != "runtime" => {
                return self.reject_command_frame(message_id);
            }
            Err(error) => return Err(error),
        };
        let source_key = SourceKey {
            group: source.group.clone(),
            native_id: source.native_id.clone(),
        };
        let projected = match self.commands.as_mut() {
            Some(service) => match envelope {
                CommandEnvelope::Request(request) => service
                    .project_request(&mut self.journal, &context, source, request, now()?)
                    .map(|_| ()),
                CommandEnvelope::Response(response) => service
                    .project_response(&mut self.journal, &context, &source, &response, now()?)
                    .map(|_| ()),
            },
            None => return self.reject_command_frame(message_id),
        };
        match projected {
            Ok(()) => {}
            Err(error) if command_projection_deferred(&error) => return Ok(()),
            Err(Error::Invalid(_)) => return self.reject_command_frame(message_id),
            Err(error) => return Err(error),
        }
        // Projection owns the exact source first. A crash at either subsequent
        // boundary reprojects idempotently or retries the durable ACK intent.
        mdk(self
            .storage
            .delete_pending_application_events(&[message_id]))?;
        self.journal.command_record_native_acks(&[source_key], &[])
    }

    fn reject_command_frame(&self, message_id: MessageId) -> Result<()> {
        crate::diagnostics::event("command_projection", "frame_refused");
        mdk(self
            .storage
            .delete_pending_application_events(&[message_id]))
    }

    /// Called only after native Current-profile founding or authenticated Welcome.
    /// The opener is permanently the command client, irrespective of local roles.
    pub(super) fn accept_command_group(
        &mut self,
        group: &GroupId,
        creator: &str,
    ) -> Result<CommandRoom> {
        let (session, peer, route, digest) =
            self.check_group_for(group, Some(creator), RoomPurpose::Command)?;
        let direction = if creator == self.identity {
            CommandDirection::Client
        } else if creator == peer {
            CommandDirection::Server
        } else {
            return Err(Error::Mdk("command_room_creator"));
        };
        let room = CommandRoom {
            group: group.as_slice().to_vec(),
            transport: route.nostr_group_id.to_vec(),
            session: fixed_hex::<16>(&session)?,
            peer: fixed_hex::<32>(&peer)?,
            direction,
            authority: fixed_hex::<32>(&digest)?,
            revoked: false,
        };
        self.journal.command_accept_room(&room)?;
        self.routing.group(group.clone(), route)?;
        Ok(room)
    }
    pub(super) fn command_context(
        &mut self,
        group: &GroupId,
    ) -> Result<AuthenticatedCommandContext> {
        let room = self
            .journal
            .command_room(group.as_slice())?
            .ok_or(Error::Mdk("command_room_unaccepted"))?;
        if room.revoked {
            return Err(Error::Mdk("command_room_revoked"));
        }
        let peer = hex::encode(room.peer);
        let creator = match room.direction {
            CommandDirection::Client => self.identity.as_str(),
            CommandDirection::Server => peer.as_str(),
        };
        let verified = self
            .check_group_for(group, Some(creator), RoomPurpose::Command)
            .and_then(|(session, actual_peer, route, authority)| {
                if fixed_hex::<16>(&session)? != room.session
                    || actual_peer != peer
                    || route.nostr_group_id.as_slice() != room.transport
                    || fixed_hex::<32>(&authority)? != room.authority
                {
                    return Err(Error::Mdk("command_authority_changed"));
                }
                Ok(())
            });
        match verified {
            Ok(()) => Ok(AuthenticatedCommandContext {
                local_account: fixed_hex::<32>(&self.identity)?,
                peer_account: room.peer,
                mls_group: room.group,
                transport_group: room.transport,
                session: room.session,
                direction: room.direction,
                target_uid: unsafe { libc::geteuid() },
                authority_digest: room.authority,
            }),
            Err(Error::Mdk(_) | Error::Invalid(_)) => {
                self.journal.command_revoke_room(group.as_slice())?;
                self.routing.remove(group);
                Err(Error::Mdk("command_room_revoked"))
            }
            Err(error) => Err(error),
        }
    }
    pub(super) fn rebuild_command_routes(&mut self) -> Result<()> {
        for room in self.journal.command_rooms()? {
            let group = GroupId::new(room.group);
            match self.command_context(&group) {
                Ok(_) => {
                    let (_, _, route, _) =
                        self.check_group_for(&group, None, RoomPurpose::Command)?;
                    self.routing.group(group, route)?;
                }
                Err(Error::Mdk("command_room_revoked")) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
    /// Native source provenance and fixed direction are checked before service
    /// policy, durable admission, or constructing an executable Owner.
    pub(super) fn decode_command_event(
        &mut self,
        event: &GroupEvent,
    ) -> Result<(AuthenticatedCommandContext, CommandSource, CommandEnvelope)> {
        let GroupEvent::MessageReceived {
            group_id,
            message_id,
            sender,
            sender_leaf_index,
            epoch,
            payload,
            authority,
            retention,
            ..
        } = event
        else {
            return Err(Error::Mdk("command_message_event"));
        };
        if self.storage.list_pending_application_events().map_err(|_| Error::Storage("command_native_pending"))?.iter().any(|pending| {
            matches!(pending, GroupEvent::AppMessageInvalidated { group_id: g, message_id: id, .. }
                if g == group_id && id == message_id)
        }) {
            return Err(Error::Mdk("command_message_invalidated"));
        }
        let context = self.command_context(group_id)?;
        let row = self
            .storage
            .get_message(message_id)
            .map_err(|_| Error::Storage("command_native_message"))?;
        if row.group_id != *group_id
            || !matches!(
                row.state,
                cgka_traits::MessageState::Sent | cgka_traits::MessageState::Processed
            )
            || payload.len() > wire::MAX_CONTENT + 4096
            || sender_leaf_index.is_none()
            || sender.as_slice() != context.peer_account()
        {
            return Err(Error::Mdk("command_message_authority"));
        }
        let app = cgka_traits::app_event::MarmotAppEvent::decode(payload)
            .map_err(|_| Error::Invalid("command_app_event"))?;
        if app.pubkey != hex::encode(context.peer_account())
            || app.kind != wire::KIND
            || app.tags != vec![wire::TAG.map(str::to_owned).to_vec()]
        {
            return Err(Error::Invalid("command_app_binding"));
        }
        let (envelope, binding) = match context.direction() {
            CommandDirection::Server => {
                let request = wire::decode_request(app.content.as_bytes())?;
                let binding = request.binding.clone();
                (CommandEnvelope::Request(request), binding)
            }
            CommandDirection::Client => {
                let response = wire::decode_response(app.content.as_bytes())?;
                let binding = response.binding.clone();
                (CommandEnvelope::Response(response), binding)
            }
        };
        let (client, server): (Account, Account) = match context.direction() {
            CommandDirection::Client => (context.local_account(), context.peer_account()),
            CommandDirection::Server => (context.peer_account(), context.local_account()),
        };
        if binding.session != context.session()
            || binding.client != client
            || binding.server != server
        {
            return Err(Error::Invalid("command_wire_binding"));
        }
        let metadata = serde_json::to_vec(&(sender_leaf_index, epoch, authority, retention))
            .map_err(|_| Error::Storage("command_message_authority"))?;
        let source = CommandSource {
            group: group_id.as_slice().to_vec(),
            native_id: message_id.as_slice().to_vec(),
            app_event_id: app.id,
            author: context.peer_account(),
            authority_metadata: metadata,
        };
        Ok((context, source, envelope))
    }
}

// Capacity and admission pressure never turn a valid native source into a
// permanent rejection; the existing bounded pending queue is the retry owner.
fn command_projection_deferred(error: &Error) -> bool {
    match error {
        Error::Invalid(category) | Error::Mdk(category) => {
            category.starts_with("command_")
                && (category.ends_with("_capacity")
                    || matches!(
                        *category,
                        "command_operation_busy"
                            | "command_poll_busy"
                            | "command_admission_closed"
                            | "command_duplicate_sources"
                            | "command_future_deferred"
                            | "command_clock_behind_floor"
                    ))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_pressure_retains_native_source() {
        for category in [
            "command_source_capacity",
            "command_room_capacity",
            "command_slot_capacity",
            "command_operation_busy",
            "command_poll_busy",
            "command_admission_closed",
            "command_future_deferred",
            "command_clock_behind_floor",
        ] {
            assert!(command_projection_deferred(&Error::Invalid(category)));
            assert!(command_projection_deferred(&Error::Mdk(category)));
        }
        for error in [
            Error::Invalid("command_wire_binding"),
            Error::Storage("command_slot_capacity"),
            Error::Mdk("runtime"),
            Error::Transport("group_state_unavailable"),
        ] {
            assert!(!command_projection_deferred(&error));
        }
    }
}
