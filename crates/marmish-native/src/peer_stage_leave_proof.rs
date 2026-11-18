//! Read-only native evidence for a local custody Leave reservation.

use super::*;
use crate::mdk_adapter::peer_native_signer::self_remove::{self, MAX_STORED_SIGNED_WIRE_BYTES};
use cgka_traits::{
    GroupLifecycleState, MessageId, TransportPublishRequest,
    maintenance::{MaintenanceObligation, MaintenanceTrigger},
    message::{MessageRecord, MessageState, StoredMessagePayload},
    storage::{LeaveRequest, LeaveRequestStorage, MessageStorage, OutboundIntentStorage},
};

pub(crate) struct LeaveInspection {
    pub(crate) authority: JoinedAuthority,
    pub(crate) retained: Option<CapturedLeave>,
}

pub(crate) struct CapturedLeave {
    id: [u8; 32],
    digest: [u8; 32],
    signed: Zeroizing<Vec<u8>>,
}

impl CapturedLeave {
    pub(crate) fn event(&self) -> ([u8; 32], [u8; 32], &[u8]) {
        (self.id, self.digest, &self.signed)
    }
}

impl OpenStage {
    pub(super) fn check_leave_environment(
        &self,
        runtime: &JoinRuntime,
        authority: &JoinedAuthority,
        witness: &JoinWitness<'_>,
    ) -> StageResult<Vec<MaintenanceObligation>> {
        let group = &authority.group;
        if runtime
            .pending_commit(group)
            .map_err(|_| StageError::Storage)?
            .is_some()
            || !runtime
                .outbound_fanouts()
                .map_err(|_| StageError::Storage)?
                .is_empty()
            || !self
                .storage
                .list_queued_outbound_intents(group)
                .map_err(|_| StageError::Storage)?
                .is_empty()
        {
            return Err(StageError::Invalid);
        }
        let account = JoinWitness::member(witness.delivery.flight().recipient);
        let scope = self_remove::Scope {
            group,
            account: &account,
            room_tag: witness.delivery.offer().routing().nostr_group_id.as_slice(),
            origin: witness.origin,
        };
        let signature = runtime.own_signature_key();
        let stable = runtime
            .group_authority_view()
            .with_group_authority_snapshot(group, |storage, current| {
                Ok::<_, marmot_session::SessionError>(
                    current.lifecycle == GroupLifecycleState::Stable
                        && !current.disbanding
                        && current.facts.is_member
                        && !current.facts.is_admin
                        && current.facts.epoch == authority.epoch.epoch
                        && self_remove::local_leaf(storage, &scope, authority.epoch, &signature)
                            .is_ok_and(|leaf| leaf.0 == authority.own_leaf),
                )
            })
            .map_err(|_| StageError::Storage)?;
        if !stable {
            return Err(StageError::Invalid);
        }
        let deferred = runtime
            .maintenance_obligations_for_group(group)
            .map_err(|_| StageError::Storage)?;
        if deferred.len() > 1
            || deferred.iter().any(|entry| {
                entry.group_id != *group
                    || !matches!(entry.trigger, MaintenanceTrigger::PostJoin)
                    || entry.attempt_count != 0
                    || entry.semantic_rearm_count != 0
            })
        {
            return Err(StageError::Invalid);
        }
        Ok(deferred)
    }

    pub(super) fn leave_rows(
        &self,
        authority: &JoinedAuthority,
    ) -> StageResult<(Option<LeaveRequest>, Vec<MessageRecord>)> {
        let group = &authority.group;
        let leave = self
            .storage
            .leave_request(group)
            .map_err(|_| StageError::Storage)?;
        let sent = self
            .storage
            .list_messages_in_states(group, &[MessageState::Sent], authority.epoch.epoch)
            .map_err(|_| StageError::Storage)?;
        Ok((leave, sent))
    }

    pub(crate) async fn inspect_leave(
        self,
        witness: &JoinWitness<'_>,
        expected: Option<[u8; 32]>,
    ) -> StageResult<LeaveInspection> {
        self.check_join_stage(witness)?;
        let io = RefusingIo::default();
        let runtime = self.join_runtime(&io)?;
        let authority = self.joined_authority(&runtime, witness)?;
        self.check_leave_environment(&runtime, &authority, witness)?;
        let group = &authority.group;
        let account = JoinWitness::member(witness.delivery.flight().recipient);
        let room_tag = witness.delivery.offer().routing().nostr_group_id.as_slice();
        let retained = if let Some(id) = expected {
            let id = MessageId::new(id.to_vec());
            let entry = self
                .storage
                .get_message(&id)
                .map_err(|_| StageError::Storage)?;
            let (_, inner) = decode_signed(&entry, &authority)?;
            let marker_id = marker_id(&inner.payload);
            let marker = self
                .storage
                .get_message(&marker_id)
                .map_err(|_| StageError::Storage)?;
            Some(exact_message(&entry, &marker, &authority)?)
        } else {
            let (leave, sent) = self.leave_rows(&authority)?;
            match (leave, sent.as_slice()) {
                (None, []) => None,
                (Some(_), [first, second]) => {
                    let (signed, marker) = if decode_signed(first, &authority).is_ok() {
                        (first, second)
                    } else {
                        (second, first)
                    };
                    Some(exact_message(signed, marker, &authority)?)
                }
                _ => return Err(StageError::Invalid),
            }
        };
        let retained = if let Some(message) = retained {
            let scope = self_remove::Scope {
                group,
                account: &account,
                room_tag,
                origin: witness.origin,
            };
            let request = TransportPublishRequest {
                account_id: account.clone(),
                message,
                target: TransportPublishTarget::Group {
                    group_id: group.clone(),
                    transport_group_id: witness.delivery.offer().routing().nostr_group_id.to_vec(),
                    endpoints: vec![self.endpoint.clone()],
                },
                required_acks: 1,
            };
            let leaf = self_remove::verify(&runtime, &scope, &request)
                .await
                .map_err(|error| match error {
                    self_remove::Refusal::Invalid => StageError::Invalid,
                    self_remove::Refusal::Storage => StageError::Storage,
                })?;
            if leaf.0 != authority.own_leaf {
                return Err(StageError::Invalid);
            }
            Some(CapturedLeave {
                id: request
                    .message
                    .id
                    .as_slice()
                    .try_into()
                    .map_err(|_| StageError::Invalid)?,
                digest: Sha256::digest(&request.message.payload).into(),
                signed: Zeroizing::new(request.message.payload),
            })
        } else {
            None
        };
        drop(runtime);
        io.require_unused()?;
        Ok(LeaveInspection {
            authority,
            retained,
        })
    }
}

fn require_row(record: &MessageRecord, authority: &JoinedAuthority) -> StageResult<()> {
    if record.id.as_slice().len() != 32
        || record.group_id != authority.group
        || record.epoch != authority.epoch.epoch
        || record.state != MessageState::Sent
        || record.deferred_peel.is_some()
        || !(1..=MAX_STORED_SIGNED_WIRE_BYTES).contains(&record.payload.len())
    {
        return Err(StageError::Invalid);
    }
    Ok(())
}

fn decode_signed(
    record: &MessageRecord,
    authority: &JoinedAuthority,
) -> StageResult<(TransportMessage, TransportMessage)> {
    require_row(record, authority)?;
    let StoredMessagePayload::SignedOpenMlsWire {
        exact_message,
        openmls_message,
        ..
    } = StoredMessagePayload::decode(&record.payload).map_err(|_| StageError::Invalid)?
    else {
        return Err(StageError::Invalid);
    };
    if exact_message.id != record.id
        || openmls_message.id != record.id
        || !(1..=65_536).contains(&exact_message.payload.len())
        || !(1..=65_536).contains(&openmls_message.payload.len())
    {
        return Err(StageError::Invalid);
    }
    Ok((exact_message, openmls_message))
}

fn marker_id(inner: &[u8]) -> MessageId {
    MessageId::new(Sha256::digest(inner).to_vec())
}

fn exact_message(
    signed: &MessageRecord,
    marker: &MessageRecord,
    authority: &JoinedAuthority,
) -> StageResult<TransportMessage> {
    let (exact, mut native) = decode_signed(signed, authority)?;
    require_row(marker, authority)?;
    native.id = marker_id(&native.payload);
    let StoredMessagePayload::RawTransport(raw) =
        StoredMessagePayload::decode(&marker.payload).map_err(|_| StageError::Invalid)?
    else {
        return Err(StageError::Invalid);
    };
    if native.id == signed.id || native.id != marker.id || native != raw {
        return Err(StageError::Invalid);
    }
    Ok(exact)
}
