//! Same-UID command projections on the single leased account actor.
use super::command_adapter::fixed_hex;
use super::*;
use crate::command_client::CommandHandle;
use crate::command_wire::{self as wire, Account, Id, RequestOp};
use crate::ipc::Request;
use crate::journal::{ClientSlotState, Owner};
use crate::transport::RelayPolicy;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursors {
    pub stdout: u64,
    pub stderr: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnswerEntry {
    pub request_id: String,
    pub op: wire::Op,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationEntry {
    pub handle: CommandHandle,
    pub cursor: u64,
    pub created: u64,
    pub admission: wire::Admission,
    pub state: Option<wire::State>,
    pub authority_valid: bool,
    pub refusal: Option<wire::ErrorCode>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum LocalResult {
    LocalBusy,
    Ready {
        session: String,
        local_account: String,
    },
    LocalQueued {
        handle: CommandHandle,
    },
    RemotePending {
        handle: CommandHandle,
        request_id: String,
    },
    Answer {
        handle: CommandHandle,
        request_id: String,
        response_base64: String,
        response_sha256: String,
    },
    Entries {
        entries: Vec<AnswerEntry>,
    },
    Invocations {
        entries: Vec<InvocationEntry>,
        next: Option<u64>,
    },
    Consumed {
        stdout: u64,
        stderr: u64,
        finish_request_id: Option<String>,
        finish_confirmed: bool,
    },
    Cursors {
        stdout: u64,
        stderr: u64,
    },
    Failure {
        category: String,
        handle: Option<CommandHandle>,
    },
}

pub(crate) fn failure(error: Error) -> LocalResult {
    let category = match error {
        Error::Invalid(category)
        | Error::Storage(category)
        | Error::Mdk(category)
        | Error::Identity(category)
        | Error::Transport(category) => category,
        Error::Io(_) => "local_io",
        Error::Timeout => "deadline",
    };
    LocalResult::Failure {
        category: category.into(),
        handle: None,
    }
}

fn acceptance_category(error: &Error) -> &'static str {
    match error {
        Error::Invalid("server_draining") => "server_draining",
        Error::Invalid("command_not_found") => "not_found",
        Error::Storage("command_cleanup_busy") | Error::Timeout => "busy",
        _ => "unknown",
    }
}

fn id(value: &str) -> Result<Id> {
    let id = fixed_hex::<16>(value)?;
    crate::foundation::id_timestamp(value)?;
    Ok(id)
}

/// Only public remote accounts are queued; no invocation or command is admitted
/// while discovery runs. Coalescing never renews the original start-by deadline.
#[derive(Default)]
pub(crate) struct RoomIntents {
    queue: VecDeque<(Account, Instant)>,
    outcomes: VecDeque<(Account, Instant, &'static str)>,
}
impl RoomIntents {
    pub(crate) fn enqueue(&mut self, remote: Account, at: Instant) -> LocalResult {
        self.outcomes.retain(|(_, expires, _)| *expires > at);
        if let Some((_, _, category)) = self.outcomes.iter().find(|(peer, _, _)| *peer == remote) {
            return LocalResult::Failure {
                category: (*category).into(),
                handle: None,
            };
        }
        if !self.queue.iter().any(|(peer, _)| *peer == remote) && self.queue.len() < 8 {
            self.queue.push_back((remote, at + Duration::from_secs(60)));
        }
        LocalResult::LocalBusy
    }
    pub(crate) fn next(&mut self, at: Instant) -> Option<Account> {
        while let Some((remote, deadline)) = self.queue.pop_front() {
            if deadline > at {
                return Some(remote);
            }
            self.failed(remote, at, "command_room_start_expired");
        }
        None
    }
    pub(crate) fn failed(&mut self, remote: Account, at: Instant, category: &'static str) {
        self.outcomes
            .retain(|(peer, expires, _)| *peer != remote && *expires > at);
        if self.outcomes.len() == 8 {
            self.outcomes.pop_front();
        }
        self.outcomes
            .push_back((remote, at + Duration::from_secs(60), category));
    }
}

impl MdkOwner {
    /// Administrative projections use only this leased actor's journal and
    /// cached bridge state. Explicit cancel/prepare enqueue their existing
    /// destructive lifecycle operations; projections never reconcile the engine.
    pub(crate) fn command_acceptance(
        &mut self,
        request: &Request,
    ) -> Result<Option<crate::ipc::Response>> {
        use crate::command_engine::CleanupState;
        use crate::ipc::Response;
        if !matches!(
            request,
            Request::CommandEvidence { .. }
                | Request::CommandCleanupStatus {}
                | Request::CommandLocalCancel { .. }
                | Request::CommandPrepareCleanup {}
        ) {
            return Ok(None);
        }
        let response=(||->Result<Response> { Ok(match request {
            Request::CommandEvidence {after,limit} => {
                if !(1..=64).contains(limit) {return Err(Error::Invalid("command_evidence_limit"));}
                if self.commands.is_none() {return Err(Error::Invalid("commands_disabled"));}
                Response::success(self.journal.command_evidence(*after,*limit as usize)?)
            }
            Request::CommandCleanupStatus {} => {
                let service=self.commands.as_ref().ok_or(Error::Invalid("commands_disabled"))?;
                let counts=self.journal.command_cleanup_counts()?;
                let (cleanup,final_seq)=service.cleanup_snapshot_readonly();
                Response::success(serde_json::json!({
                    "active":counts.active,"uncertain":counts.uncertain,
                    "worker_running":counts.worker_running,"worker_unknown":counts.worker_unknown,
                    "engine_quiesced":cleanup==CleanupState::Quiesced,
                    "observation_pending":counts.observation_pending || service.cleanup_observation_pending(),
                    "cleanup":match cleanup {CleanupState::Open=>"open",CleanupState::Draining=>"draining",CleanupState::Quiesced=>"quiesced",CleanupState::Failed=>"failed"},"final_seq":final_seq
                }))
            }
            Request::CommandLocalCancel {cursor} => {
                if *cursor==0 {return Err(Error::Invalid("command_not_found"));}
                let row=self.journal.command_server_records(Some(cursor-1),1)?.into_iter().next()
                    .filter(|row|row.cursor==*cursor).ok_or(Error::Invalid("command_not_found"))?;
                let queued=self.commands.as_mut().ok_or(Error::Invalid("commands_disabled"))?
                    .cancel_fixture_key(&mut self.journal,&row.key)?;
                Response::success(serde_json::json!({"queued":queued}))
            }
            Request::CommandPrepareCleanup {} => {
                self.command_begin_cleanup()?;
                Response::success(serde_json::json!({"queued":true}))
            }
            _=>unreachable!(),
        }) })().unwrap_or_else(|error|Response::failure(acceptance_category(&error)));
        Ok(Some(response))
    }
    pub(crate) fn command_lifetime(
        &self,
    ) -> Option<std::sync::Arc<crate::command_limits::ServiceLifetime>> {
        self.commands.as_ref().and_then(|s| s.lifetime())
    }
    pub(crate) fn command_count_exit_ready(&self) -> Result<bool> {
        if !self
            .commands
            .as_ref()
            .map_or(Ok(false), |s| s.count_exit_ready(&self.journal))?
        {
            return Ok(false);
        }
        Ok(self.journal.command_sources_all_acknowledged()?
            && mdk(self.storage.list_pending_application_events())?.is_empty())
    }
    pub(crate) fn command_set_lifetime(
        &mut self,
        lifetime: std::sync::Arc<crate::command_limits::ServiceLifetime>,
    ) {
        if let Some(service) = &mut self.commands {
            service.set_lifetime(lifetime);
        }
    }
    pub(crate) async fn command_open_room_turn(
        &mut self,
        remote: Account,
        deadline: Instant,
    ) -> Result<()> {
        self.reconcile_orphans()?;
        tokio::time::timeout_at(deadline, self.open_command_room(remote, false, deadline))
            .await
            .map_err(|_| Error::Timeout)??;
        Ok(())
    }
    pub(crate) fn command_can_prepare(&self, remote: Account, relays: &[String]) -> Result<()> {
        let service = self
            .commands
            .as_ref()
            .ok_or(Error::Invalid("commands_disabled"))?;
        let policy = RelayPolicy::parse(
            relays,
            self.connector.policy().mode(),
            crate::transport::TransportLimits::default(),
        )?;
        if policy.urls() != self.connector.policy().urls() {
            return Err(Error::Invalid("command_relay_binding"));
        }
        if !service.can_open(&remote) {
            return Err(Error::Invalid("command_unauthorized"));
        }
        Ok(())
    }

    /// A prepared room is not proof of peer admission. Retained native control
    /// work must drain before a CLI freezes its first Submit against this room.
    pub(crate) fn command_ready_room(
        &mut self,
        remote: Account,
        relays: &[String],
    ) -> Result<Option<Id>> {
        self.command_can_prepare(remote, relays)?;
        for room in self.journal.command_rooms()? {
            if room.peer != remote || room.direction != CommandDirection::Client {
                continue;
            }
            let group = GroupId::new(room.group);
            match self.command_context(&group) {
                Ok(_) => {}
                Err(Error::Mdk("command_room_revoked")) => continue,
                Err(error) => return Err(error),
            }
            let fanouts = mdk(self.storage.list_outbound_fanouts())?;
            if fanouts
                .iter()
                .any(|f| f.group_id() == Some(&group) && f.application_message().is_none())
                || !mdk(self.storage.list_queued_outbound_intents(&group))?.is_empty()
            {
                return Ok(None);
            }
            return Ok(Some(room.session));
        }
        Ok(None)
    }

    fn local_owner(&mut self, profile: &str, handle: &CommandHandle) -> Result<Owner> {
        let room = self
            .journal
            .command_rooms()?
            .into_iter()
            .find(|room| {
                room.direction == CommandDirection::Client
                    && room.session == handle.session()
                    && room.peer == handle.remote()
            })
            .ok_or(Error::Invalid("command_not_found"))?;
        self.command_can_prepare(
            room.peer,
            &self
                .connector
                .policy()
                .urls()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        )?;
        let context = self.command_context(&GroupId::new(room.group))?;
        let owner = Owner::from_context(&context);
        let entry = self
            .journal
            .command_client_invocation(&owner, handle.invocation())?;
        handle.validate_entry(profile, context.local_account(), &entry)?;
        Ok(owner)
    }

    fn local_answer(
        &self,
        owner: &Owner,
        handle: &CommandHandle,
        request_id: Id,
    ) -> Result<LocalResult> {
        self.check_local_slot(owner, handle, request_id)?;
        match self.journal.command_client_slot_state(owner, request_id)? {
            ClientSlotState::Absent => Err(Error::Invalid("command_response_retired")),
            ClientSlotState::Consumed => Err(Error::Invalid("command_response_consumed")),
            ClientSlotState::Pending => Ok(LocalResult::RemotePending {
                handle: handle.clone(),
                request_id: hex::encode(request_id),
            }),
            ClientSlotState::Answered => {
                let (answer, digest) = self
                    .journal
                    .command_client_response(owner, request_id)?
                    .ok_or(Error::Storage("command_response_pending"))?;
                if answer
                    .result
                    .as_ref()
                    .ok()
                    .and_then(|v| v.status())
                    .is_some_and(|s| s.invocation != handle.invocation())
                {
                    return Err(Error::Invalid("command_response_binding"));
                }
                let bytes = wire::encode_response(&answer)?;
                Ok(LocalResult::Answer {
                    handle: handle.clone(),
                    request_id: hex::encode(request_id),
                    response_base64: STANDARD.encode(bytes),
                    response_sha256: hex::encode(digest),
                })
            }
        }
    }

    fn check_local_slot(
        &self,
        owner: &Owner,
        handle: &CommandHandle,
        request_id: Id,
    ) -> Result<()> {
        if self
            .journal
            .command_client_request_invocation(owner, request_id)?
            .is_some_and(|invocation| invocation != handle.invocation())
        {
            return Err(Error::Invalid("command_response_binding"));
        }
        Ok(())
    }

    pub(crate) fn command_local(
        &mut self,
        profile: &str,
        request: &Request,
    ) -> Result<LocalResult> {
        match request {
            Request::CommandPrepare { remote, relays } => {
                let remote = fixed_hex::<32>(remote)?;
                Ok(match self.command_ready_room(remote, relays)? {
                    Some(session) => LocalResult::Ready {
                        session: hex::encode(session),
                        local_account: self.identity.clone(),
                    },
                    None => LocalResult::LocalBusy,
                })
            }
            Request::CommandSubmit {
                remote,
                relays,
                session,
                request_id,
                submit,
            } => {
                let remote = fixed_hex::<32>(remote)?;
                let session = id(session)?;
                let request_id = id(request_id)?;
                self.command_can_prepare(remote, relays)?;
                let prepared = crate::command_client::prepare_operation(
                    wire::Binding {
                        session,
                        client: fixed_hex::<32>(&self.identity)?,
                        server: remote,
                    },
                    request_id,
                    RequestOp::Submit(submit.clone()),
                )?;
                let handle = CommandHandle::for_submit(profile, &prepared)?;
                match self.local_owner(profile, &handle) {
                    Ok(_) => {} // Exact retry attaches despite its own queued publication.
                    Err(
                        Error::Invalid("command_not_found") | Error::Storage("command_not_found"),
                    ) => {
                        if self.command_ready_room(remote, relays)? != Some(session) {
                            return Err(Error::Invalid("command_room_pending"));
                        }
                    }
                    Err(error) => return Err(error),
                }
                self.command_request(session, request_id, prepared.request.op)?;
                Ok(LocalResult::LocalQueued { handle })
            }
            Request::CommandInvocations { after, limit } => {
                let page = self.journal.command_client_invocations(*after, *limit)?;
                let entries = page
                    .entries
                    .into_iter()
                    .map(|entry| {
                        Ok(InvocationEntry {
                            handle: CommandHandle::from_invocation(profile, &entry)?,
                            cursor: entry.cursor,
                            created: entry.created,
                            admission: entry.admission,
                            state: entry.state,
                            authority_valid: entry.authority_valid,
                            refusal: entry.refusal,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(LocalResult::Invocations {
                    entries,
                    next: page.next,
                })
            }
            Request::CommandCursors { handle } => {
                let owner = self.local_owner(profile, handle)?;
                let pair = self
                    .journal
                    .command_client_cursors(&owner, handle.invocation())?;
                Ok(LocalResult::Cursors {
                    stdout: pair.stdout,
                    stderr: pair.stderr,
                })
            }
            Request::CommandUnconsumed { handle, limit } => {
                let owner = self.local_owner(profile, handle)?;
                let entries = self
                    .journal
                    .command_client_unconsumed(&owner, handle.invocation(), *limit)?
                    .into_iter()
                    .map(|(request_id, op, sha)| AnswerEntry {
                        request_id: hex::encode(request_id),
                        op,
                        sha256: hex::encode(sha),
                    })
                    .collect();
                Ok(LocalResult::Entries { entries })
            }
            Request::CommandConsume {
                handle,
                request_id,
                response_sha256,
                expected_cursors,
            } => {
                let owner = self.local_owner(profile, handle)?;
                self.check_local_slot(&owner, handle, id(request_id)?)?;
                let context = self.command_context(&GroupId::new(owner.group().to_vec()))?;
                let finish_id = id(&crate::foundation::fresh_id(now()?)?)?;
                let (pair, finish) = self
                    .commands
                    .as_mut()
                    .ok_or(Error::Invalid("commands_disabled"))?
                    .consume_client_response(
                        &mut self.journal,
                        &context,
                        id(request_id)?,
                        fixed_hex::<32>(response_sha256)?,
                        expected_cursors.map(|v| crate::journal::ClientDelivery {
                            stdout: v.stdout,
                            stderr: v.stderr,
                        }),
                        finish_id,
                        now()?,
                    )?;
                let finish_state = self
                    .journal
                    .command_client_finish_state(&owner, handle.invocation())?;
                Ok(LocalResult::Consumed {
                    stdout: pair.stdout,
                    stderr: pair.stderr,
                    finish_request_id: finish.map(hex::encode),
                    finish_confirmed: finish_state.confirmed,
                })
            }
            Request::CommandResponse { handle, request_id } => {
                let owner = self.local_owner(profile, handle)?;
                self.local_answer(&owner, handle, id(request_id)?)
            }
            Request::CommandStatus { handle, request_id }
            | Request::CommandCancel { handle, request_id }
            | Request::CommandPoll {
                handle, request_id, ..
            } => {
                let owner = self.local_owner(profile, handle)?;
                let request_id = id(request_id)?;
                self.check_local_slot(&owner, handle, request_id)?;
                {
                    let invocation = handle.invocation();
                    let op = match request {
                        Request::CommandStatus { .. } => RequestOp::Status { invocation },
                        Request::CommandCancel { .. } => RequestOp::Cancel { invocation },
                        Request::CommandPoll {
                            stdout_cursor,
                            stderr_cursor,
                            max_bytes,
                            ..
                        } => RequestOp::Poll {
                            invocation,
                            stdout_cursor: *stdout_cursor,
                            stderr_cursor: *stderr_cursor,
                            max_bytes: *max_bytes,
                        },
                        _ => return Err(Error::Invalid("command_local_operation")),
                    };
                    self.command_request(handle.session(), request_id, op)?;
                }
                self.local_answer(&owner, handle, request_id)
            }
            _ => Err(Error::Invalid("command_local_operation")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn room_intents_coalesce_cap_expire_and_keep_original_deadline() {
        let start = Instant::now();
        let mut queue = RoomIntents::default();
        for i in 0..9 {
            assert!(matches!(
                queue.enqueue([i; 32], start),
                LocalResult::LocalBusy
            ));
        }
        assert_eq!(queue.queue.len(), 8);
        queue.enqueue([0; 32], start + Duration::from_secs(59));
        assert_eq!(queue.queue[0].1, start + Duration::from_secs(60));
        assert_eq!(queue.next(start + Duration::from_secs(60)), None);
        assert!(matches!(
            queue.enqueue([0; 32], start + Duration::from_secs(61)),
            LocalResult::Failure { .. }
        ));
        assert!(matches!(
            queue.enqueue([0; 32], start + Duration::from_secs(121)),
            LocalResult::LocalBusy
        ));
        assert_eq!(queue.next(start + Duration::from_secs(121)), Some([0; 32]));
    }
    #[test]
    fn room_failure_cache_is_bounded_and_other_peers_progress() {
        let start = Instant::now();
        let mut queue = RoomIntents::default();
        for i in 0..9 {
            queue.failed([i; 32], start, "command_room_creation");
        }
        assert_eq!(queue.outcomes.len(), 8);
        assert!(matches!(
            queue.enqueue([0; 32], start),
            LocalResult::LocalBusy
        ));
        assert!(matches!(
            queue.enqueue([1; 32], start),
            LocalResult::Failure { .. }
        ));
        queue.enqueue([9; 32], start);
        assert_eq!(queue.next(start), Some([0; 32]));
        assert_eq!(queue.next(start), Some([9; 32]));
    }
}

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    #[test]
    fn administrative_failures_are_finite_and_distinguishable() {
        for (error, expected) in [
            (Error::Invalid("server_draining"), "server_draining"),
            (Error::Invalid("command_not_found"), "not_found"),
            (Error::Storage("command_cleanup_busy"), "busy"),
            (Error::Timeout, "busy"),
            (Error::Invalid("command_fixture_unknown"), "unknown"),
            (Error::Invalid("commands_disabled"), "unknown"),
        ] {
            assert_eq!(acceptance_category(&error), expected);
        }
    }
}
