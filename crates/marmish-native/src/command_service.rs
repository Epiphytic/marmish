//! Synchronous actor coordination. The engine thread never receives the journal,
//! and a popped completion remains owned here until its durable ACK is delivered.
use crate::command_config::{CommandConfig, Role};
use crate::command_engine::{CleanupState, CommandEngine, Readiness, next_feeder_operation};
use crate::command_wire::{
    self as wire, Account, Admission, Disposition, ErrorCode, Request, RequestOp, Response,
    ResponseResult, State, Status, WireError,
};
use crate::journal::{
    AdmissionDecision, CommandProjection, CommandSource, CommitAck, CommittedDecision,
    EngineCompletion, EngineOperation, InvocationKey, InvocationRecord, Journal, OperationRecord,
    Owner, PreparedCommandPublication, ProjectedRequest, ProjectedResponse, WorkerState,
    project_completion,
};
use crate::mdk_adapter::{
    AuthenticatedCommandContext, CommandDirection, prepare_command_publication,
};
use crate::{Error, Result};
use std::collections::{BTreeSet, VecDeque};

const TURN: usize = 64;
const SLOTS: usize = 256;

struct CleanupPreparation {
    fence: u64,
    cursor: Option<u64>,
    state: CleanupState,
    consuming: bool,
}

pub(crate) struct CommandService {
    config: CommandConfig,
    stopping: bool,
    engine: Option<CommandEngine>,
    completion: Option<EngineCompletion>,
    ack: Option<CommitAck>,
    enqueued: BTreeSet<[u8; 16]>,
    waiting: VecDeque<OperationRecord>,
    recovery: Option<BTreeSet<InvocationKey>>,
    pending_cursor: usize,
    cleanup: Option<CleanupPreparation>,
    lifetime: Option<std::sync::Arc<crate::command_limits::ServiceLifetime>>,
}
impl CommandService {
    pub(crate) fn new(config: CommandConfig, engine: Option<CommandEngine>) -> Self {
        Self {
            config,
            stopping: false,
            engine,
            completion: None,
            ack: None,
            enqueued: BTreeSet::new(),
            waiting: VecDeque::new(),
            recovery: None,
            pending_cursor: 0,
            cleanup: None,
            lifetime: None,
        }
    }
    pub(crate) fn close_admission(&mut self) {
        self.stopping = true;
    }
    pub(crate) fn set_lifetime(
        &mut self,
        lifetime: std::sync::Arc<crate::command_limits::ServiceLifetime>,
    ) {
        self.lifetime = Some(lifetime);
    }
    pub(crate) fn lifetime(
        &self,
    ) -> Option<std::sync::Arc<crate::command_limits::ServiceLifetime>> {
        self.lifetime.clone()
    }
    fn lifetime_expired(&self) -> Result<bool> {
        self.lifetime.as_ref().map_or(Ok(false), |l| l.expired())
    }
    pub(crate) fn count_exit_ready(&self, journal: &Journal) -> Result<bool> {
        let b = journal.command_admission_budget()?;
        Ok(crate::command_limits::CountDrain {
            maximum: b.maximum,
            admitted: b.admitted,
            reserved: b.reserved,
            finished: b.finished,
            control_idle: self.completion.is_none()
                && self.ack.is_none()
                && self.enqueued.is_empty()
                && self.waiting.is_empty(),
            publications_terminal: journal.command_publications_terminal()?,
        }
        .ready())
    }
    pub(crate) fn copy_cwd(&self) -> &std::path::Path {
        self.config.cwd()
    }
    pub(crate) fn copy_receipt_retention(&self) -> std::time::Duration {
        self.config.limits().copy_receipt_retention
    }
    pub(crate) fn engine_mut(&mut self) -> Option<&mut CommandEngine> {
        self.engine.as_mut()
    }
    pub(crate) fn take_engine_for_stop(&mut self) -> Option<CommandEngine> {
        self.engine.take()
    }
    pub(crate) fn can_open(&self, peer: &Account) -> bool {
        self.cleanup.is_none()
            && matches!(self.config.role(), Role::Client | Role::Both)
            && self.config.trusted_remotes().contains(peer)
    }
    pub(crate) fn can_accept(&self, peer: &Account) -> bool {
        self.cleanup.is_none()
            && matches!(self.config.role(), Role::Server | Role::Both)
            && self.config.execution_clients().contains(peer)
    }
    fn authorize(
        &self,
        ctx: &AuthenticatedCommandContext,
        direction: CommandDirection,
    ) -> Result<Owner> {
        if ctx.direction() != direction {
            return Err(Error::Invalid("command_direction"));
        }
        let allowed = match direction {
            CommandDirection::Client => self.can_open(&ctx.peer_account()),
            CommandDirection::Server => self.can_accept(&ctx.peer_account()),
        };
        if !allowed {
            return Err(Error::Invalid("command_unauthorized"));
        }
        Ok(Owner::from_context(ctx))
    }
    pub(crate) fn project_request(
        &mut self,
        journal: &mut Journal,
        ctx: &AuthenticatedCommandContext,
        source: CommandSource,
        request: Request,
        now: u64,
    ) -> Result<ProjectedRequest> {
        let owner = self.authorize(ctx, CommandDirection::Server)?;
        if request.binding != *owner.binding() {
            return Err(Error::Invalid("command_request_binding"));
        }
        let key = invocation_key(&owner, &request);
        let old = journal.command_find_request(&owner, &request)?;
        let record = journal.command_get(&key)?;
        let (decision, response) = if old.is_some() {
            (AdmissionDecision::Existing, None)
        } else if matches!(request.op, RequestOp::Finish { .. }) {
            self.finish_response(journal, &owner, &request)?
        } else {
            let admission = self.admission_response(&owner, &request, record.as_ref())?;
            // Only a new authenticated Submit can reserve the bounded budget.
            // Executable resolution and policy run on the dedicated engine owner.
            if matches!(admission.0, AdmissionDecision::New) {
                self.bounded_admission(journal, &owner, &request, admission)?
            } else {
                admission
            }
        };
        journal.command_project_request(
            &CommandProjection {
                owner,
                source,
                request,
                decision,
                response,
            },
            now,
        )
    }
    fn finish_response(
        &self,
        journal: &Journal,
        owner: &Owner,
        request: &Request,
    ) -> Result<(AdmissionDecision, Option<PreparedCommandPublication>)> {
        let result = journal.command_finish_result(owner, request)?;
        let decision = match &result {
            Ok(_) => AdmissionDecision::Existing,
            Err(e) => AdmissionDecision::Refused(e.clone()),
        };
        Ok((
            decision,
            Some(response_publication(owner, request, result)?),
        ))
    }
    fn bounded_admission(
        &self,
        journal: &Journal,
        owner: &Owner,
        request: &Request,
        admission: (AdmissionDecision, Option<PreparedCommandPublication>),
    ) -> Result<(AdmissionDecision, Option<PreparedCommandPublication>)> {
        if self.stopping || self.lifetime_expired()? {
            return Err(Error::Invalid("command_admission_closed"));
        }
        let budget = journal.command_admission_budget()?;
        if budget.reserved_full() && !budget.exhausted() {
            return Err(Error::Invalid("command_admission_closed"));
        }
        if budget.exhausted() {
            let error = WireError {
                code: ErrorCode::QuotaExceeded,
                detail: None,
            };
            return Ok((
                AdmissionDecision::Refused(error.clone()),
                Some(response_publication(owner, request, Err(error))?),
            ));
        }
        Ok(admission)
    }
    fn admission_response(
        &self,
        owner: &Owner,
        request: &Request,
        record: Option<&InvocationRecord>,
    ) -> Result<(AdmissionDecision, Option<PreparedCommandPublication>)> {
        if let Some(record) = record {
            // Immutable owner and submit digest are rechecked inside the transaction.
            if !record.authority_valid {
                return Err(Error::Invalid("command_authority_revoked"));
            }
            if let RequestOp::Submit(submit) = &request.op
                && wire::submit_digest(owner.group(), owner.binding(), submit)? != record.digest
            {
                let error = WireError {
                    code: ErrorCode::InvocationConflict,
                    detail: None,
                };
                return Ok((
                    AdmissionDecision::Refused(error.clone()),
                    Some(response_publication(owner, request, Err(error))?),
                ));
            }
            if !matches!(request.op, RequestOp::Submit(_)) && record.refusal.is_none() {
                return Ok((AdmissionDecision::Existing, None));
            }
            let result = match &record.refusal {
                Some(error) => Err(error.clone()),
                None => Ok(ResponseResult::Submit {
                    disposition: Disposition::Existing,
                    status: status(record),
                }),
            };
            return Ok((
                AdmissionDecision::Existing,
                Some(response_publication(owner, request, result)?),
            ));
        }
        let RequestOp::Submit(submit) = &request.op else {
            let error = WireError {
                code: ErrorCode::NotFound,
                detail: None,
            };
            return Ok((
                AdmissionDecision::Refused(error.clone()),
                Some(response_publication(owner, request, Err(error))?),
            ));
        };
        let result = Ok(ResponseResult::Submit {
            disposition: Disposition::Accepted,
            status: accepted_status(submit.invocation, submit.stdin.len() as u32),
        });
        Ok((
            AdmissionDecision::New,
            Some(response_publication(owner, request, result)?),
        ))
    }
    pub(crate) fn project_response(
        &mut self,
        journal: &mut Journal,
        ctx: &AuthenticatedCommandContext,
        source: &CommandSource,
        response: &Response,
        now: u64,
    ) -> Result<ProjectedResponse> {
        let owner = self.authorize(ctx, CommandDirection::Client)?;
        if source.group != owner.group() || response.binding != *owner.binding() {
            return Err(Error::Invalid("command_response_binding"));
        }
        journal.command_project_response(source, response, now)
    }
    pub(crate) fn client_request(
        &mut self,
        journal: &mut Journal,
        ctx: &AuthenticatedCommandContext,
        request: &Request,
        now: u64,
    ) -> Result<InvocationRecord> {
        let owner = self.authorize(ctx, CommandDirection::Client)?;
        let encoded = wire::encode_request(request)?;
        let publication = prepare_command_publication(
            owner.group(),
            owner.binding().clone(),
            request.id,
            wire::request_digest(&encoded),
            encoded,
            owner.binding().client,
        )?;
        journal.command_submit_client(&owner, request, &publication, now)
    }
    // The authenticated context and transaction owner stay separate from the
    // exact consume/Finish inputs; changing either must not alter their binding.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn consume_client_response(
        &self,
        journal: &mut Journal,
        ctx: &AuthenticatedCommandContext,
        id: wire::Id,
        response_sha256: wire::Digest,
        expected: Option<crate::journal::ClientDelivery>,
        finish_id: wire::Id,
        now: u64,
    ) -> Result<(crate::journal::ClientDelivery, Option<wire::Id>)> {
        let owner = self.authorize(ctx, CommandDirection::Client)?;
        let request =
            journal.command_finish_candidate(&owner, id, response_sha256, expected, finish_id)?;
        let publication = request
            .map(|r| {
                let encoded = wire::encode_request(&r)?;
                prepare_command_publication(
                    owner.group(),
                    owner.binding().clone(),
                    r.id,
                    wire::request_digest(&encoded),
                    encoded,
                    owner.binding().client,
                )
            })
            .transpose()?;
        journal.command_consume_and_finish(
            &owner,
            id,
            response_sha256,
            expected,
            publication.as_ref(),
            now,
        )
    }
    /// One bounded actor turn; engine calls only touch nonblocking channels.
    pub(crate) fn poll_completions(&mut self, journal: &mut Journal, now: u64) -> Result<()> {
        let result = self.process_completions(journal, now);
        if result.is_err() && self.cleanup.is_some() {
            self.fail_cleanup();
        }
        result
    }
    fn process_completions(&mut self, journal: &mut Journal, _now: u64) -> Result<()> {
        if self.engine.is_none() {
            return Ok(());
        }
        if self.recovery.is_none() {
            self.recovery = Some(
                journal
                    .command_recovery()?
                    .into_iter()
                    .map(|r| r.key)
                    .collect(),
            );
        }
        for _ in 0..TURN {
            if !self.deliver_ack()? {
                return Ok(());
            }
            if self.completion.is_none() {
                self.completion = self.engine.as_mut().and_then(CommandEngine::try_completion);
            }
            let Some(completion) = self.completion.as_ref() else {
                break;
            };
            let publications = completion_publications(journal, completion)?;
            let ack = journal.command_commit_completion(completion, &publications)?;
            self.ack = Some(ack);
            self.completion = None;
        }
        if !self.deliver_ack()? {
            return Ok(());
        }
        if self
            .engine
            .as_mut()
            .is_none_or(|engine| engine.readiness() != Readiness::Ready)
        {
            return Ok(());
        }
        if self.cleanup.is_some() {
            self.advance_cleanup(journal)?;
            self.cleanup_snapshot();
            Ok(())
        } else {
            self.schedule(journal)
        }
    }
    /// Explicit local destructive lifecycle; ordinary stop never calls this.
    /// Explicit local fixture failure handling; never called on disconnect/stop.
    pub(crate) fn cancel_fixture_invocations(
        &mut self,
        journal: &mut Journal,
        ids: &[wire::Id],
    ) -> Result<()> {
        if self.cleanup.is_some() {
            return Err(Error::Invalid("server_draining"));
        }
        if ids.len() > 4 || ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
            return Err(Error::Invalid("command_fixture_ids"));
        }
        let mut cursor = None;
        let mut selected = Vec::new();
        loop {
            let page = journal.command_server_records(cursor, TURN)?;
            if page.is_empty() {
                break;
            }
            cursor = page.last().map(|r| r.cursor);
            for row in page {
                if ids.contains(&row.key.invocation) {
                    selected.push(row);
                }
            }
        }
        if ids
            .iter()
            .any(|id| selected.iter().filter(|r| r.key.invocation == *id).count() != 1)
        {
            return Err(Error::Invalid("command_fixture_unknown"));
        }
        if selected
            .iter()
            .any(|row| row.active() && (!row.engine_known || row.uncertain))
        {
            return Err(Error::Storage("command_cleanup_busy"));
        }
        for row in selected {
            if row.active() {
                let operation = register(
                    journal,
                    &row.key,
                    row.revision,
                    None,
                    EngineOperation::Cancel,
                )?
                .ok_or(Error::Storage("command_cleanup_busy"))?;
                self.enqueue(operation);
            }
        }
        Ok(())
    }
    /// Same-UID fixture control, bound to a retained server invocation key.
    pub(crate) fn cancel_fixture_key(
        &mut self,
        journal: &mut Journal,
        key: &InvocationKey,
    ) -> Result<bool> {
        if self.cleanup.is_some() {
            return Err(Error::Invalid("server_draining"));
        }
        let row = journal
            .command_get(key)?
            .filter(|row| row.owner.server_side())
            .ok_or(Error::Invalid("command_not_found"))?;
        if !row.active() {
            return Ok(false);
        }
        if !row.engine_known || row.uncertain {
            return Err(Error::Storage("command_cleanup_busy"));
        }
        if self.engine.is_none() {
            return Err(Error::Invalid("commands_disabled"));
        }
        let operation = register(journal, key, row.revision, None, EngineOperation::Cancel)?
            .ok_or(Error::Storage("command_cleanup_busy"))?;
        self.enqueue(operation);
        Ok(true)
    }
    pub(crate) fn begin_cleanup(&mut self, journal: &mut Journal) -> Result<u64> {
        if self.cleanup.is_some() {
            return Err(Error::Invalid("server_draining"));
        }
        let counts = journal.command_cleanup_counts()?;
        let engine = self
            .engine
            .as_mut()
            .ok_or(Error::Invalid("command_engine_missing"))?;
        if counts.active != 0
            || counts.uncertain != 0
            || counts.worker_running != 0
            || counts.worker_unknown != 0
            || counts.observation_pending
            || engine.readiness() != Readiness::Ready
            || engine.observation_pending()
            || self.completion.is_some()
            || self.ack.is_some()
            || !self.enqueued.is_empty()
            || !self.waiting.is_empty()
        {
            return Err(Error::Storage("command_cleanup_busy"));
        }
        let fence = journal.command_allocate_fence()?;
        self.cleanup = Some(CleanupPreparation {
            fence,
            cursor: None,
            state: CleanupState::Draining,
            consuming: false,
        });
        Ok(fence)
    }
    /// Cached actor facts only: this never polls the engine or reconciles jobs.
    pub(crate) fn cleanup_snapshot_readonly(&self) -> (CleanupState, Option<u64>) {
        self.cleanup
            .as_ref()
            .map_or((CleanupState::Open, None), |p| (p.state, Some(p.fence)))
    }
    pub(crate) fn engine_present_readonly(&self) -> bool {
        self.engine.is_some()
    }
    pub(crate) fn cleanup_observation_pending(&self) -> bool {
        self.completion.is_some()
            || self.ack.is_some()
            || !self.enqueued.is_empty()
            || !self.waiting.is_empty()
    }
    pub(crate) fn cleanup_snapshot(&mut self) -> (CleanupState, Option<u64>) {
        let Some(preparation) = self.cleanup.as_mut() else {
            return (CleanupState::Open, None);
        };
        if preparation.consuming && preparation.state != CleanupState::Failed {
            preparation.state = self
                .engine
                .as_mut()
                .map(CommandEngine::cleanup_state)
                .unwrap_or(CleanupState::Failed);
        }
        (preparation.state, Some(preparation.fence))
    }
    pub(crate) fn poll_cleanup(&mut self, journal: &mut Journal) -> Result<CleanupState> {
        self.poll_completions(journal, 0)?;
        Ok(self.cleanup_snapshot().0)
    }
    fn advance_cleanup(&mut self, journal: &mut Journal) -> Result<()> {
        let result = self.refresh_cleanup(journal);
        match result {
            Ok(()) => Ok(()),
            Err(Error::Invalid(
                "command_cleanup_authority"
                | "command_cleanup_observation"
                | "command_cleanup_busy"
                | "engine_cleanup_state",
            ))
            | Err(Error::Storage("engine_control_busy")) => {
                self.fail_cleanup();
                Ok(())
            }
            Err(error) => {
                self.fail_cleanup();
                Err(error)
            }
        }
    }
    fn fail_cleanup(&mut self) {
        if let Some(preparation) = &mut self.cleanup {
            preparation.state = CleanupState::Failed;
        }
        // Failure cannot release the still-owned engine or authorize deletion.
        if let Some(engine) = &mut self.engine {
            let _ = engine.fail_cleanup();
        }
    }
    fn refresh_cleanup(&mut self, journal: &mut Journal) -> Result<()> {
        let preparation = self
            .cleanup
            .as_ref()
            .ok_or(Error::Invalid("command_cleanup_state"))?;
        if preparation.state == CleanupState::Failed {
            return Ok(());
        }
        if preparation.consuming {
            if !journal.command_cleanup_fresh(preparation.fence)? {
                return Err(Error::Invalid("command_cleanup_authority"));
            }
            return Ok(());
        }
        if !self.enqueued.is_empty() || !self.waiting.is_empty() {
            return Ok(());
        }
        let fence = preparation.fence;
        let rows = journal.command_server_records(preparation.cursor, TURN)?;
        for row in rows {
            if !row.authority_valid {
                return Err(Error::Invalid("command_cleanup_authority"));
            }
            if row.observed_seq < fence {
                let operation = register(
                    journal,
                    &row.key,
                    row.revision,
                    None,
                    EngineOperation::FinalObserve,
                )?
                .ok_or(Error::Storage("command_cleanup_busy"))?;
                self.enqueue(operation);
                return Ok(());
            }
            validate_cleanup_row(&row, fence)?;
            self.cleanup.as_mut().expect("cleanup").cursor = Some(row.cursor);
        }
        // A full page needs another bounded scan; an empty next page completes.
        if journal
            .command_server_records(self.cleanup.as_ref().expect("cleanup").cursor, 1)?
            .is_empty()
        {
            if !journal.command_cleanup_fresh(fence)? {
                return Err(Error::Invalid("command_cleanup_observation"));
            }
            let counts = journal.command_cleanup_counts()?;
            if counts.active != 0
                || counts.uncertain != 0
                || counts.worker_running != 0
                || counts.worker_unknown != 0
                || counts.observation_pending
            {
                return Err(Error::Invalid("command_cleanup_observation"));
            }
            self.engine
                .as_mut()
                .ok_or(Error::Invalid("command_engine_missing"))?
                .try_prepare_cleanup()?;
            let preparation = self.cleanup.as_mut().expect("cleanup");
            preparation.consuming = true;
            preparation.cursor = None;
        }
        Ok(())
    }
    fn deliver_ack(&mut self) -> Result<bool> {
        let Some(ack) = &self.ack else {
            return Ok(true);
        };
        let copy = CommitAck {
            operation: ack.operation,
            key: ack.key.clone(),
            revision: ack.revision,
            decision: ack.decision,
            answered: ack.answered.clone(),
            suppressed: ack.suppressed.clone(),
        };
        match self
            .engine
            .as_mut()
            .ok_or(Error::Invalid("command_engine_missing"))?
            .ack_committed(copy)
        {
            Ok(()) => {
                let ack = self.ack.take().expect("retained ACK");
                self.enqueued.remove(&ack.operation.0);
                if ack.decision != CommittedDecision::StillUncertain
                    && let Some(recovery) = &mut self.recovery
                {
                    recovery.remove(&ack.key);
                }
                Ok(true)
            }
            Err(Error::Storage("engine_ack_pending")) => Ok(false),
            Err(error) => Err(error),
        }
    }
    fn enqueue(&mut self, operation: OperationRecord) {
        if self.enqueued.contains(&operation.id.0)
            || self.waiting.iter().any(|op| op.id == operation.id)
        {
            return;
        }
        self.try_operation(operation);
    }
    fn try_operation(&mut self, operation: OperationRecord) {
        // A full queue preserves an unbegun Dispatch locally. Recheck on every
        // attempt, including that waiting path. Clock errors fail closed.
        if matches!(operation.operation, EngineOperation::Dispatch { .. })
            && self.lifetime_expired().unwrap_or(true)
        {
            self.waiting.push_back(operation);
            return;
        }
        let retry = operation.clone();
        if self
            .engine
            .as_mut()
            .is_some_and(|engine| engine.try_enqueue(operation).is_ok())
        {
            self.enqueued.insert(retry.id.0);
        } else if retry.request_id.is_none() {
            // A failed queue insertion proves no side effect began. Preserve
            // this exact local intention; restart instead requires status-first.
            self.waiting.push_back(retry);
        }
    }
    fn schedule(&mut self, journal: &mut Journal) -> Result<()> {
        for _ in 0..self.waiting.len() {
            if let Some(operation) = self.waiting.pop_front() {
                self.try_operation(operation);
            }
        }
        // Only these operations are replayable across queue-full/restart with
        // their original registration. Dispatch and feeder writes are not.
        for operation in journal.command_read_operations(TURN)? {
            self.enqueue(operation);
        }
        self.schedule_requests(journal)?;
        self.schedule_active(journal)
    }
    fn schedule_requests(&mut self, journal: &mut Journal) -> Result<()> {
        let pending = journal.command_pending(SLOTS)?;
        if !pending.is_empty() {
            let start = self.pending_cursor % pending.len();
            for slot in pending
                .iter()
                .cycle()
                .skip(start)
                .take(pending.len().min(TURN))
            {
                if slot.operation.is_some() {
                    continue;
                }
                let key = invocation_key(&slot.owner, &slot.request);
                let Some(record) = journal.command_get(&key)? else {
                    continue;
                };
                if !record.authority_valid {
                    continue;
                }
                // A prelaunch cancel cannot be misreported as a successful
                // cancellation of an engine job which has not existed yet.
                if matches!(slot.request.op, RequestOp::Cancel { .. }) && !record.engine_known {
                    continue;
                }
                let operation = match slot.request.op {
                    RequestOp::Status { .. } => EngineOperation::Observe,
                    RequestOp::Cancel { .. } => EngineOperation::Cancel,
                    RequestOp::Poll {
                        stdout_cursor,
                        stderr_cursor,
                        max_bytes,
                        ..
                    } => EngineOperation::Poll {
                        stdout_cursor,
                        stderr_cursor,
                        max_bytes,
                    },
                    RequestOp::Submit(_) | RequestOp::Finish { .. } => continue,
                };
                if let Some(operation) = register(
                    journal,
                    &key,
                    record.revision,
                    Some(slot.request.id),
                    operation,
                )? {
                    self.enqueue(operation);
                }
            }
            self.pending_cursor = (start + pending.len().min(TURN)) % pending.len();
        }
        Ok(())
    }
    fn schedule_active(&mut self, journal: &mut Journal) -> Result<()> {
        for recovery in journal.command_recovery()? {
            let Some(record) = journal.command_get(&recovery.key)? else {
                continue;
            };
            let recovering = self
                .recovery
                .as_ref()
                .is_some_and(|set| set.contains(&record.key));
            let operation = if recovering
                || !record.authority_valid
                || record.admission == Admission::Dispatching
            {
                EngineOperation::Observe
            } else if !record.engine_known && record.refusal.is_none() {
                if self.lifetime_expired()? {
                    continue;
                }
                EngineOperation::Dispatch {
                    submit: record
                        .submit
                        .clone()
                        .ok_or(Error::Storage("command_payload_expired"))?,
                }
            } else {
                next_feeder_operation(&record)?.unwrap_or(EngineOperation::Observe)
            };
            if let Some(operation) =
                register(journal, &record.key, record.revision, None, operation)?
            {
                self.enqueue(operation);
            }
        }
        Ok(())
    }
}
fn validate_cleanup_row(row: &InvocationRecord, fence: u64) -> Result<()> {
    let refused_before_engine = row.refusal.is_some() && !row.engine_known;
    let exited = row.engine_known
        && row.worker == WorkerState::Exited
        && row
            .status
            .as_ref()
            .is_some_and(|s| s.state == State::Exited);
    if !row.authority_valid
        || row.observed_seq < fence
        || row.uncertain
        || !(refused_before_engine || exited)
    {
        return Err(Error::Invalid("command_cleanup_observation"));
    }
    Ok(())
}
fn register(
    journal: &mut Journal,
    key: &InvocationKey,
    revision: u64,
    request: Option<wire::Id>,
    operation: EngineOperation,
) -> Result<Option<OperationRecord>> {
    let result = match request {
        Some(id) => journal.command_begin_request_operation(key, revision, id, operation),
        None => journal.command_begin_operation(key, revision, operation),
    };
    match result {
        Ok(operation) => Ok(Some(operation)),
        Err(Error::Invalid("command_operation_busy")) => Ok(None),
        Err(error) => Err(error),
    }
}
fn invocation_key(owner: &Owner, request: &Request) -> InvocationKey {
    InvocationKey {
        server: owner.binding().server,
        principal: owner.binding().client,
        invocation: request.op.invocation(),
    }
}
fn response_publication(
    owner: &Owner,
    request: &Request,
    result: std::result::Result<ResponseResult, WireError>,
) -> Result<PreparedCommandPublication> {
    let response = Response {
        binding: request.binding.clone(),
        id: request.id,
        op: request.op.op(),
        request_sha256: wire::request_digest(&wire::encode_request(request)?),
        result,
    };
    prepare_command_publication(
        owner.group(),
        owner.binding().clone(),
        request.id,
        response.request_sha256,
        wire::encode_response(&response)?,
        owner.binding().server,
    )
}
fn completion_publications(
    journal: &Journal,
    completion: &EngineCompletion,
) -> Result<Vec<PreparedCommandPublication>> {
    let record = journal
        .command_get(&completion.key)?
        .ok_or(Error::Invalid("command_record"))?;
    let operation = journal
        .command_operation(completion.operation)?
        .ok_or(Error::Invalid("command_operation"))?;
    let pending = journal.command_operation_request(completion.operation)?;
    let projection = project_completion(&record, &operation, completion, pending.as_ref())?;
    projection
        .response
        .map(|response| {
            prepare_command_publication(
                record.owner.group(),
                record.owner.binding().clone(),
                response.id,
                response.request_sha256,
                wire::encode_response(&response)?,
                record.owner.binding().server,
            )
        })
        .transpose()
        .map(|value| value.into_iter().collect())
}
fn status(record: &InvocationRecord) -> Status {
    let mut status = record
        .status
        .clone()
        .unwrap_or_else(|| accepted_status(record.key.invocation, record.stdin_length));
    status.admission = record.admission;
    status.authority_valid = record.authority_valid;
    status.engine_known = record.engine_known;
    status.warning = record.warning;
    status
}
fn accepted_status(invocation: wire::Id, length: u32) -> Status {
    let output = wire::OutputStatus {
        closed: false,
        gaps: Vec::new(),
        produced: 0,
        retained: 0,
    };
    Status {
        admission: Admission::Accepted,
        authority_valid: true,
        cancel_requested: false,
        engine_known: false,
        input: wire::InputStatus {
            accepted: 0,
            delivered: 0,
            eof_sent: false,
            health: wire::InputHealth::Open,
            length,
        },
        invocation,
        outcome: None,
        state: State::Accepted,
        stderr: output.clone(),
        stdout: output,
        warning: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{EngineObservation, OperationalFailure, PublicationState};
    use serde_json::json;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    const NOW: u64 = 2_000_000_000;
    fn id(n: u8) -> wire::Id {
        let mut id = [n; 16];
        id[..8].copy_from_slice(&NOW.to_be_bytes());
        id
    }
    fn context(direction: CommandDirection) -> AuthenticatedCommandContext {
        AuthenticatedCommandContext::test_fixture([2; 32], [1; 32], vec![3; 32], id(1), direction)
    }
    fn setup(denied: bool) -> (tempfile::TempDir, Journal, CommandService) {
        let tmp = crate::journal::private_test_directory();
        let policy = if denied {
            json!({"version":1,"defaults":{"allow_exec":[]}})
        } else {
            json!({"version":1})
        };
        let config = json!({"version":1,"role":"both","relays":["wss://relay.example/"],"tor":"auto","trusted_remotes":[hex::encode([1;32])],"execution_clients":[hex::encode([1;32])],"command_policy":policy,"cwd":null,"limits":{},"aliases":{}});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(tmp.path().join("config.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        let config = CommandConfig::load(tmp.path()).unwrap();
        let journal = Journal::open(
            &tmp.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        (tmp, journal, CommandService::new(config, None))
    }
    fn request() -> Request {
        Request {
            binding: Owner::from_context(&context(CommandDirection::Server))
                .binding()
                .clone(),
            id: id(2),
            op: RequestOp::Submit(wire::Submit {
                argv: vec![b"/nonexistent/executable".to_vec(), vec![0xff, b'\n']],
                invocation: id(3),
                stdin: vec![0, 0xff, b'\n'],
            }),
        }
    }
    fn source(request: &Request, n: u8) -> CommandSource {
        let bytes = wire::encode_request(request).unwrap();
        let publication = prepare_command_publication(
            &[3; 32],
            request.binding.clone(),
            request.id,
            wire::request_digest(&bytes),
            bytes,
            [1; 32],
        )
        .unwrap();
        CommandSource {
            group: vec![3; 32],
            native_id: vec![n],
            app_event_id: publication.key.app_event_id,
            author: [1; 32],
            authority_metadata: vec![9],
        }
    }
    fn maximum(service: &mut CommandService, journal: &mut Journal, n: u64) {
        service
            .config
            .apply_limit_overrides(crate::command_limits::LimitOverrides {
                max_commands: std::num::NonZeroU64::new(n),
                max_uptime: None,
            })
            .unwrap();
        journal.command_configure(service.config.limits()).unwrap();
    }
    #[test]
    fn stopping_closes_new_admission_but_preserves_status_and_cancel() {
        let (_tmp, mut journal, mut service) = setup(false);
        let submit = request();
        let ctx = context(CommandDirection::Server);
        service
            .project_request(&mut journal, &ctx, source(&submit, 1), submit.clone(), NOW)
            .unwrap();
        service.close_admission();
        let mut next = submit.clone();
        next.id = id(4);
        if let RequestOp::Submit(ref mut body) = next.op {
            body.invocation = id(5);
        }
        assert!(matches!(
            service.project_request(&mut journal, &ctx, source(&next, 2), next, NOW),
            Err(Error::Invalid("command_admission_closed"))
        ));
        // Retry and existing-job controls retain their durable projection semantics.
        service
            .project_request(&mut journal, &ctx, source(&submit, 3), submit.clone(), NOW)
            .unwrap();
        for (n, op) in [
            (6, RequestOp::Status { invocation: id(3) }),
            (7, RequestOp::Cancel { invocation: id(3) }),
        ] {
            let mut control = submit.clone();
            control.id = id(n);
            control.op = op;
            service
                .project_request(&mut journal, &ctx, source(&control, n), control, NOW)
                .unwrap();
        }
    }
    #[test]
    fn reserved_capacity_defers_but_exhaustion_freezes_refusal_across_limit_raise() {
        let (_tmp, mut journal, mut service) = setup(false);
        maximum(&mut service, &mut journal, 1);
        let first = request();
        let key = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&first, 1),
                first.clone(),
                NOW,
            )
            .unwrap()
            .key;
        let mut second = first.clone();
        second.id = id(4);
        if let RequestOp::Submit(s) = &mut second.op {
            s.invocation = id(5);
        }
        assert!(matches!(
            service.project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&second, 2),
                second.clone(),
                NOW
            ),
            Err(Error::Invalid("command_admission_closed"))
        ));
        assert!(
            journal
                .command_find_request(
                    &Owner::from_context(&context(CommandDirection::Server)),
                    &second
                )
                .unwrap()
                .is_none()
        );
        let record = journal.command_get(&key).unwrap().unwrap();
        let operation = journal
            .command_begin_operation(&key, record.revision, EngineOperation::Observe)
            .unwrap();
        let mut status = accepted_status(key.invocation, 3);
        status.engine_known = true;
        status.admission = Admission::Engine;
        status.state = State::Running;
        journal
            .command_commit_completion(
                &EngineCompletion {
                    operation: operation.id,
                    key: key.clone(),
                    revision: operation.revision,
                    result: EngineObservation::Found {
                        status,
                        worker: WorkerState::Running,
                    },
                },
                &[],
            )
            .unwrap();
        service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&second, 2),
                second.clone(),
                NOW,
            )
            .unwrap();
        let slot = journal
            .command_find_request(
                &Owner::from_context(&context(CommandDirection::Server)),
                &second,
            )
            .unwrap()
            .unwrap();
        let response_key = slot.response.unwrap();
        let frozen = journal
            .command_publications(PublicationState::Unsubmitted, 64)
            .unwrap()
            .into_iter()
            .find(|p| p.key == response_key)
            .unwrap();
        let answer = wire::decode_response(&frozen.encoded_wire).unwrap();
        assert!(matches!(
            answer.result,
            Err(WireError {
                code: ErrorCode::QuotaExceeded,
                ..
            })
        ));
        maximum(&mut service, &mut journal, 2);
        assert!(
            service
                .project_request(
                    &mut journal,
                    &context(CommandDirection::Server),
                    source(&second, 3),
                    second.clone(),
                    NOW
                )
                .unwrap()
                .existing
        );
        let mut third = second;
        third.id = id(6);
        if let RequestOp::Submit(s) = &mut third.op {
            s.invocation = id(7);
        }
        let thirdkey = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&third, 4),
                third,
                NOW,
            )
            .unwrap()
            .key;
        assert!(journal.command_get(&thirdkey).unwrap().is_some());
        assert_eq!(journal.command_admission_budget().unwrap().admitted, 1);
        assert!(service.engine.is_none());
    }
    #[test]
    fn uptime_defers_new_submit_and_keeps_queued_dispatch_outside_engine() {
        let (_tmp, mut journal, mut service) = setup(false);
        let request = request();
        let key = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request.clone(),
                NOW,
            )
            .unwrap()
            .key;
        let row = journal.command_get(&key).unwrap().unwrap();
        let operation = journal
            .command_begin_operation(
                &key,
                row.revision,
                EngineOperation::Dispatch {
                    submit: row.submit.unwrap(),
                },
            )
            .unwrap();
        let (gate, rx) = std::sync::mpsc::channel();
        service.engine = Some(CommandEngine::gated_stop_for_test(rx));
        // Saturate the real bounded bridge queue, then preserve the exact Dispatch.
        for n in 0..64u8 {
            let mut fake = operation.clone();
            fake.id = crate::journal::OperationId([n; 16]);
            fake.key.invocation = [n; 16];
            service
                .engine
                .as_mut()
                .unwrap()
                .try_enqueue(fake)
                .ok()
                .unwrap();
        }
        service.try_operation(operation.clone());
        assert_eq!(service.waiting.len(), 1);
        let lifetime = std::sync::Arc::new(
            crate::command_limits::ServiceLifetime::start(Some(std::time::Duration::from_nanos(1)))
                .unwrap(),
        );
        while !lifetime.expired().unwrap() {
            std::hint::spin_loop();
        }
        service.set_lifetime(lifetime);
        // Replace the saturated bridge with an empty, live bounded queue. Without
        // the lifetime check the retry now enqueues successfully, so this tests
        // the expiry gate rather than a second queue-full failure.
        gate.send(()).unwrap();
        service
            .engine
            .take()
            .unwrap()
            .shutdown(std::time::Instant::now() + std::time::Duration::from_secs(1))
            .unwrap();
        let (gate, rx) = std::sync::mpsc::channel();
        service.engine = Some(CommandEngine::gated_stop_for_test(rx));
        let retry = service.waiting.pop_front().unwrap();
        service.try_operation(retry);
        assert_eq!(service.waiting.len(), 1);
        assert!(!service.enqueued.contains(&operation.id.0));
        assert_eq!(journal.command_admission_budget().unwrap().reserved, 1);
        let mut fresh = request;
        fresh.id = id(8);
        if let RequestOp::Submit(s) = &mut fresh.op {
            s.invocation = id(9);
        }
        assert!(matches!(
            service.project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&fresh, 3),
                fresh,
                NOW
            ),
            Err(Error::Invalid("command_admission_closed"))
        ));
        gate.send(()).unwrap();
        service
            .engine
            .take()
            .unwrap()
            .shutdown(std::time::Instant::now() + std::time::Duration::from_secs(1))
            .unwrap();
    }
    #[test]
    fn expired_lifetime_does_not_register_fresh_dispatch() {
        let (_tmp, mut journal, mut service) = setup(false);
        let request = request();
        let key = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request,
                NOW,
            )
            .unwrap()
            .key;
        let lifetime = std::sync::Arc::new(
            crate::command_limits::ServiceLifetime::start(Some(std::time::Duration::from_nanos(1)))
                .unwrap(),
        );
        while !lifetime.expired().unwrap() {
            std::hint::spin_loop();
        }
        service.set_lifetime(lifetime);
        let before = journal.command_get(&key).unwrap().unwrap();
        service.schedule_active(&mut journal).unwrap();
        let after = journal.command_get(&key).unwrap().unwrap();
        assert!(after.admission == Admission::Accepted);
        assert_eq!(after.revision, before.revision);
        assert!(service.waiting.is_empty());
        assert!(service.enqueued.is_empty());
        assert_eq!(journal.command_admission_budget().unwrap().reserved, 1);
    }
    #[test]
    fn finish_unknown_is_frozen_without_engine_admission() {
        let (_tmp, mut journal, mut service) = setup(false);
        let mut request = request();
        request.op = RequestOp::Finish {
            invocation: id(3),
            submit_sha256: [4; 32],
            terminal_sha256: [5; 32],
            stdout_cursor: 0,
            stderr_cursor: 0,
        };
        let projected = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request.clone(),
                NOW,
            )
            .unwrap();
        assert!(journal.command_get(&projected.key).unwrap().is_none());
        assert_eq!(journal.command_admission_budget().unwrap().admitted, 0);
        let duplicate = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 2),
                request,
                NOW,
            )
            .unwrap();
        assert!(duplicate.key == projected.key);
        assert!(!service.count_exit_ready(&journal).unwrap());
        assert!(service.waiting.is_empty() && service.enqueued.is_empty());
    }
    #[test]
    fn cleanup_requires_fresh_definitive_facts_and_closes_admission() {
        let (_tmp, mut journal, mut service) = setup(true);
        let request = request();
        let projected = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request,
                NOW,
            )
            .unwrap();
        let mut row = journal.command_get(&projected.key).unwrap().unwrap();
        // New requests await the engine; only a definitive refusal is safe.
        assert!(validate_cleanup_row(&row, 0).is_err());
        row.refusal = Some(WireError {
            code: ErrorCode::CommandDenied,
            detail: Some("policy_restriction".into()),
        });
        row.feeder = crate::journal::FeederState::Failed;
        row.observed_seq = 10;
        assert!(validate_cleanup_row(&row, 10).is_ok());
        assert!(validate_cleanup_row(&row, 11).is_err());
        row.uncertain = true;
        assert!(validate_cleanup_row(&row, 10).is_err());
        row.uncertain = false;
        row.authority_valid = false;
        assert!(validate_cleanup_row(&row, 10).is_err());
        row.authority_valid = true;
        row.engine_known = true;
        assert!(validate_cleanup_row(&row, 10).is_err());
        assert!(service.can_accept(&[1; 32]));
        service.cleanup = Some(CleanupPreparation {
            fence: 10,
            cursor: None,
            state: CleanupState::Draining,
            consuming: false,
        });
        assert!(!service.can_accept(&[1; 32]) && !service.can_open(&[1; 32]));
        service.fail_cleanup();
        assert!(service.cleanup_snapshot().0 == CleanupState::Failed);
        assert!(service.begin_cleanup(&mut journal).is_err());
    }
    #[test]
    fn fixture_cancel_cannot_erase_unknown_dispatch_or_use_unsealed_ids() {
        let (_tmp, mut journal, mut service) = setup(false);
        let request = request();
        let key = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request,
                NOW,
            )
            .unwrap()
            .key;
        assert!(matches!(
            service.cancel_fixture_invocations(&mut journal, &[key.invocation]),
            Err(Error::Storage("command_cleanup_busy"))
        ));
        assert!(
            journal
                .command_operation_request(crate::journal::OperationId([0; 16]))
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .cancel_fixture_invocations(&mut journal, &[id(99)])
                .is_err()
        );
        assert!(
            service
                .cancel_fixture_invocations(&mut journal, &[key.invocation, key.invocation])
                .is_err()
        );
        assert!(journal.command_get(&key).unwrap().unwrap().admission == Admission::Accepted);
        assert!(service.waiting.is_empty() && service.enqueued.is_empty());
    }
    #[test]
    fn keyed_fixture_cancel_requires_known_server_and_registers_exact_key() {
        let (_tmp, mut journal, mut service) = setup(false);
        let req = request();
        let key = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&req, 1),
                req,
                NOW,
            )
            .unwrap()
            .key;
        assert!(matches!(
            service.cancel_fixture_key(&mut journal, &key),
            Err(Error::Storage("command_cleanup_busy"))
        ));
        let mut missing = key.clone();
        missing.invocation = id(99);
        assert!(matches!(
            service.cancel_fixture_key(&mut journal, &missing),
            Err(Error::Invalid("command_not_found"))
        ));
        let row = journal.command_get(&key).unwrap().unwrap();
        let op = journal
            .command_begin_operation(&key, row.revision, EngineOperation::Observe)
            .unwrap();
        let mut status = accepted_status(key.invocation, 3);
        status.engine_known = true;
        status.admission = Admission::Engine;
        status.state = State::Running;
        journal
            .command_commit_completion(
                &EngineCompletion {
                    operation: op.id,
                    key: key.clone(),
                    revision: op.revision,
                    result: EngineObservation::Found {
                        status,
                        worker: WorkerState::Running,
                    },
                },
                &[],
            )
            .unwrap();
        assert!(matches!(
            service.cancel_fixture_key(&mut journal, &key),
            Err(Error::Invalid("commands_disabled"))
        ));
        let (gate, rx) = std::sync::mpsc::channel();
        service.engine = Some(CommandEngine::gated_stop_for_test(rx));
        assert!(service.cancel_fixture_key(&mut journal, &key).unwrap());
        assert!(matches!(
            service.cancel_fixture_key(&mut journal, &key),
            Err(Error::Storage("command_cleanup_busy"))
        ));
        gate.send(()).unwrap();
        service
            .engine
            .take()
            .unwrap()
            .shutdown(std::time::Instant::now() + std::time::Duration::from_secs(1))
            .unwrap();
    }
    #[test]
    fn keyed_fixture_cancel_terminal_is_noop_and_draining_denies_control() {
        let (_tmp, mut journal, mut service) = setup(true);
        let req = request();
        let key = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&req, 1),
                req,
                NOW,
            )
            .unwrap()
            .key;
        let record = journal.command_get(&key).unwrap().unwrap();
        let operation = journal
            .command_begin_operation(&key, record.revision, EngineOperation::Observe)
            .unwrap();
        journal
            .command_commit_completion(
                &EngineCompletion {
                    operation: operation.id,
                    key: key.clone(),
                    revision: operation.revision,
                    result: crate::journal::EngineObservation::Refused(WireError {
                        code: ErrorCode::CommandDenied,
                        detail: Some("policy_restriction".into()),
                    }),
                },
                &[],
            )
            .unwrap();
        assert!(!service.cancel_fixture_key(&mut journal, &key).unwrap());
        assert!(service.enqueued.is_empty() && service.waiting.is_empty());
        service.cleanup = Some(CleanupPreparation {
            fence: 1,
            cursor: None,
            state: CleanupState::Draining,
            consuming: false,
        });
        assert!(matches!(
            service.cancel_fixture_key(&mut journal, &key),
            Err(Error::Invalid("server_draining"))
        ));
    }
    #[test]
    fn real_engine_cleanup_refreshes_refusal_before_consuming_guard() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        use crate::command_engine::EngineSettings;
        use std::time::{Duration, Instant};
        let (tmp, mut journal, mut service) = setup(true);
        let worker = tmp.path().join("worker");
        fs::copy(std::env::current_exe().unwrap(), &worker).unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
        service.engine = Some(
            CommandEngine::start(
                EngineSettings {
                    policy: marmish_engine::EnginePolicy {
                        state_root: tmp.path().join("engine"),
                        worker_executable: worker,
                        server: marmish_engine::ServerId([2; 32]),
                        allowed_principals: vec![marmish_engine::PrincipalId([1; 32])],
                        allow_shell_command: false,
                        command_policy: None,
                        cwd: Some(tmp.path().to_owned()),
                        limits: marmish_engine::EngineLimits::default(),
                    },
                    binding: marmish_engine::NativeBinding::new([8; 32], [9; 32]),
                    master: zeroize::Zeroizing::new([10; 32]),
                    canonical_native_image: None,
                },
                vec![],
            )
            .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while service.engine_mut().unwrap().readiness() == Readiness::Opening {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let request = request();
        let key = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request,
                NOW,
            )
            .unwrap()
            .key;
        // Executable refusal is now asynchronous; cleanup must not discard an
        // accepted request before its engine decision has been committed.
        while journal
            .command_get(&key)
            .unwrap()
            .unwrap()
            .refusal
            .is_none()
        {
            assert!(Instant::now() < deadline);
            service.poll_completions(&mut journal, NOW).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        // The durable refusal precedes the worker's observation of its ACK.
        // Cleanup must wait for that registration to drain as well.
        let fence = loop {
            match service.begin_cleanup(&mut journal) {
                Ok(fence) => break fence,
                Err(Error::Storage("command_cleanup_busy")) => {
                    assert!(Instant::now() < deadline);
                    service.poll_completions(&mut journal, NOW).unwrap();
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("unexpected cleanup failure: {error:?}"),
            }
        };
        while service.poll_cleanup(&mut journal).unwrap() != CleanupState::Quiesced {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let row = journal.command_get(&key).unwrap().unwrap();
        assert!(row.refusal.is_some() && !row.engine_known && row.observed_seq >= fence);
        assert!(!service.can_accept(&[1; 32]));
        journal.command_revoke_group(&[3; 32]).unwrap();
        assert!(service.poll_cleanup(&mut journal).unwrap() == CleanupState::Failed);
        assert!(service.cleanup_snapshot().0 == CleanupState::Failed);
        assert!(
            service
                .take_engine_for_stop()
                .unwrap()
                .shutdown(deadline)
                .unwrap()
                .complete
        );
    }
    #[test]
    fn accepted_bytes_duplicate_attach_and_conflict_are_atomic() {
        let (_tmp, mut journal, mut service) = setup(false);
        let request = request();
        let projected = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request.clone(),
                NOW,
            )
            .unwrap();
        let row = journal.command_get(&projected.key).unwrap().unwrap();
        assert!(
            row.submit.as_ref().unwrap().stdin == vec![0, 0xff, b'\n']
                && row.admission == Admission::Accepted
                && !row.engine_known
        );
        assert!(
            service
                .project_request(
                    &mut journal,
                    &context(CommandDirection::Server),
                    source(&request, 2),
                    request.clone(),
                    NOW
                )
                .unwrap()
                .existing
        );
        assert_eq!(
            journal.command_requests(&projected.key, 64).unwrap().len(),
            1
        );
        let mut conflict = request.clone();
        if let RequestOp::Submit(submit) = &mut conflict.op {
            submit.stdin.push(1);
        }
        assert!(
            service
                .project_request(
                    &mut journal,
                    &context(CommandDirection::Server),
                    source(&conflict, 3),
                    conflict,
                    NOW
                )
                .is_err()
        );
        assert_eq!(
            journal
                .command_get(&projected.key)
                .unwrap()
                .unwrap()
                .submit
                .unwrap()
                .stdin,
            row.submit.unwrap().stdin
        );
    }
    #[test]
    fn changed_submit_new_request_has_durable_conflict_response() {
        let (_tmp, mut journal, mut service) = setup(false);
        let original = request();
        let owner = Owner::from_context(&context(CommandDirection::Server));
        let projected = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&original, 1),
                original.clone(),
                NOW,
            )
            .unwrap();
        let before = journal.command_get(&projected.key).unwrap().unwrap();
        let mut conflict = original.clone();
        conflict.id = id(4);
        if let RequestOp::Submit(submit) = &mut conflict.op {
            submit.stdin.push(1);
        }
        service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&conflict, 2),
                conflict.clone(),
                NOW,
            )
            .unwrap();
        let slot = journal
            .command_find_request(&owner, &conflict)
            .unwrap()
            .unwrap();
        let response_key = slot.response.unwrap();
        let publications = journal
            .command_publications(PublicationState::Unsubmitted, 64)
            .unwrap();
        let publication = publications.iter().find(|p| p.key == response_key).unwrap();
        let encoded = publication.encoded_wire.clone();
        let response = wire::decode_response(&encoded).unwrap();
        assert!(matches!(
            response.result,
            Err(WireError {
                code: ErrorCode::InvocationConflict,
                detail: None
            })
        ));
        assert!(
            service
                .project_request(
                    &mut journal,
                    &context(CommandDirection::Server),
                    source(&conflict, 3),
                    conflict,
                    NOW
                )
                .unwrap()
                .existing
        );
        let after = journal.command_get(&projected.key).unwrap().unwrap();
        assert_eq!(after.digest, before.digest);
        assert_eq!(after.submit.unwrap().stdin, before.submit.unwrap().stdin);
        assert!(after.admission == Admission::Accepted && !after.engine_known);
        assert_eq!(after.revision, before.revision);
        let repeated = journal
            .command_publications(PublicationState::Unsubmitted, 64)
            .unwrap();
        assert_eq!(
            repeated
                .iter()
                .find(|p| p.key == response_key)
                .unwrap()
                .encoded_wire,
            encoded
        );
        assert_eq!(
            journal.command_requests(&projected.key, 64).unwrap().len(),
            2
        );
    }
    #[test]
    fn executable_policy_is_deferred_without_filesystem_work_on_actor() {
        let (_tmp, mut journal, mut service) = setup(true);
        let request = request();
        let projected = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request,
                NOW,
            )
            .unwrap();
        let record = journal.command_get(&projected.key).unwrap().unwrap();
        assert!(record.refusal.is_none());
        assert!(record.admission == Admission::Accepted && !record.engine_known);
        assert_eq!(journal.command_recovery().unwrap().len(), 1);
        assert!(service.engine_mut().is_none());
    }
    #[test]
    fn current_direction_binding_and_grants_are_checked_before_admission() {
        let (_tmp, mut journal, mut service) = setup(false);
        assert!(service.can_open(&[1; 32]) && service.can_accept(&[1; 32]));
        assert!(!service.can_accept(&[9; 32]));
        let request = request();
        assert!(
            service
                .project_request(
                    &mut journal,
                    &context(CommandDirection::Client),
                    source(&request, 1),
                    request.clone(),
                    NOW
                )
                .is_err()
        );
        let mut changed = request.clone();
        changed.binding.session = id(9);
        assert!(
            service
                .project_request(
                    &mut journal,
                    &context(CommandDirection::Server),
                    source(&changed, 1),
                    changed,
                    NOW
                )
                .is_err()
        );
        assert!(journal.command_recovery().unwrap().is_empty());
    }
    #[test]
    fn unknown_read_has_frozen_refusal_without_fake_invocation() {
        let (_tmp, mut journal, mut service) = setup(false);
        let mut request = request();
        request.op = RequestOp::Status { invocation: id(9) };
        let projected = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request.clone(),
                NOW,
            )
            .unwrap();
        assert!(journal.command_get(&projected.key).unwrap().is_none());
        let owner = Owner::from_context(&context(CommandDirection::Server));
        let slot = journal
            .command_find_request(&owner, &request)
            .unwrap()
            .unwrap();
        assert!(slot.response.is_some());
        assert!(
            service
                .project_request(
                    &mut journal,
                    &context(CommandDirection::Server),
                    source(&request, 2),
                    request,
                    NOW
                )
                .unwrap()
                .existing
        );
        assert!(journal.command_recovery().unwrap().is_empty());
    }
    #[test]
    fn failed_poll_answers_only_after_three_committed_attempts() {
        let (_tmp, mut journal, mut service) = setup(false);
        let mut request = request();
        let projected = service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 1),
                request.clone(),
                NOW,
            )
            .unwrap();
        request.id = id(4);
        request.op = RequestOp::Poll {
            invocation: projected.key.invocation,
            stdout_cursor: 0,
            stderr_cursor: 0,
            max_bytes: 16384,
        };
        service
            .project_request(
                &mut journal,
                &context(CommandDirection::Server),
                source(&request, 2),
                request.clone(),
                NOW,
            )
            .unwrap();
        for attempt in 1..=3 {
            let row = journal.command_get(&projected.key).unwrap().unwrap();
            let operation = journal
                .command_begin_request_operation(
                    &projected.key,
                    row.revision,
                    request.id,
                    EngineOperation::Poll {
                        stdout_cursor: 0,
                        stderr_cursor: 0,
                        max_bytes: 16384,
                    },
                )
                .unwrap();
            let completion = EngineCompletion {
                operation: operation.id,
                key: projected.key.clone(),
                revision: operation.revision,
                result: EngineObservation::Failed(OperationalFailure::Storage),
            };
            let publications = completion_publications(&journal, &completion).unwrap();
            assert_eq!(publications.len(), usize::from(attempt == 3));
            if let Some(publication) = publications.first() {
                let response = wire::decode_response(&publication.encoded_wire).unwrap();
                assert!(matches!(
                    response.result,
                    Err(WireError {
                        code: ErrorCode::RecoveryUncertain,
                        ..
                    })
                ));
            }
            let ack = journal
                .command_commit_completion(&completion, &publications)
                .unwrap();
            assert_eq!(ack.answered.len(), usize::from(attempt == 3));
            let slots = journal.command_requests(&projected.key, 64).unwrap();
            let poll = slots
                .iter()
                .find(|slot| slot.request.id == request.id)
                .unwrap();
            assert_eq!(poll.failed_attempts, attempt);
            assert_eq!(poll.response.is_some(), attempt == 3);
        }
    }
}
