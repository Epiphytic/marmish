//! Bounded command client. The daemon remains the only authority and journal owner.
use crate::{
    Error, Result,
    command_client::{CommandHandle, prepare_operation},
    command_local::{Cursors, InvocationEntry, LocalResult},
    command_wire::{self as wire, Account, Binding, Id, Op, RequestOp, ResponseResult, State},
    ipc::{self, Request},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{IsTerminal, Read, Write},
    os::unix::{
        ffi::OsStringExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::Instant;

pub(crate) struct Context {
    pub socket: PathBuf,
    pub profile: String,
}
pub(crate) enum Selector {
    Handle(CommandHandle),
    HandleFile(PathBuf),
    Invocation(Id),
    Latest,
}
pub(crate) struct Selection {
    pub selector: Selector,
    pub remote: Option<Account>,
}
#[derive(Clone, Copy)]
pub(crate) enum StdinMode {
    Auto,
    Read,
    None,
}
pub(crate) enum Action {
    Submit {
        remote: Account,
        relays: Vec<String>,
        argv: Vec<OsString>,
        stdin: StdinMode,
        detach: bool,
        wait: Duration,
    },
    Commands {
        remote: Option<Account>,
    },
    Status {
        selection: Selection,
    },
    Cancel {
        selection: Selection,
    },
    Wait {
        selection: Selection,
        wait: Duration,
    },
    Output {
        selection: Selection,
        wait: Duration,
    },
}
fn invalid(category: &'static str) -> Error {
    Error::Invalid(category)
}
fn fresh_id() -> Result<Id> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("command_clock"))?
        .as_secs();
    let text = crate::foundation::fresh_id(now)?;
    hex::decode(text)
        .map_err(|_| invalid("command_id"))?
        .try_into()
        .map_err(|_| invalid("command_id"))
}
fn id(text: &str) -> Result<Id> {
    crate::foundation::id_timestamp(text).map_err(|_| invalid("command_id"))?;
    hex::decode(text)
        .map_err(|_| invalid("command_id"))?
        .try_into()
        .map_err(|_| invalid("command_id"))
}
fn account(text: &str) -> Result<Account> {
    if !crate::foundation::valid_hex(text, 64) {
        return Err(invalid("command_account"));
    }
    hex::decode(text)
        .map_err(|_| invalid("command_account"))?
        .try_into()
        .map_err(|_| invalid("command_account"))
}
fn report_handle(handle: &CommandHandle) -> Result<()> {
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(b"marmish handle: ")?;
    stderr.write_all(&handle.encode()?)?;
    stderr.write_all(b"\n")?;
    stderr.flush()?;
    Ok(())
}
fn read_handle(path: PathBuf) -> Result<CommandHandle> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || meta.len() > 1024
    {
        return Err(invalid("command_handle_file"));
    }
    let mut bytes = Vec::with_capacity(1025);
    file.take(1025).read_to_end(&mut bytes)?;
    CommandHandle::decode(&bytes)
}
/// Poll and read in this thread before admission: unlike tokio stdin, no detached
/// blocking reader survives a timeout. No other task may read this descriptor.
fn read_stdin(fd: libc::c_int, mode: StdinMode, tty: bool, budget: Duration) -> Result<Vec<u8>> {
    if matches!(mode, StdinMode::None) || (matches!(mode, StdinMode::Auto) && tty) {
        return Ok(Vec::new());
    }
    let deadline = std::time::Instant::now() + budget;
    let mut bytes = Vec::with_capacity(wire::MAX_BYTES + 1);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(invalid("input_timeout"));
        }
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let milliseconds = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
        let result = unsafe { libc::poll(&mut poll, 1, milliseconds) };
        if result < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(invalid("command_stdin"));
        }
        if result == 0 {
            continue;
        }
        if poll.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(invalid("command_stdin"));
        }
        let mut buffer = [0u8; 4096];
        let max = buffer.len().min(wire::MAX_BYTES + 1 - bytes.len());
        let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), max) };
        if read < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(invalid("command_stdin"));
        }
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..read as usize]);
        if bytes.len() > wire::MAX_BYTES {
            return Err(invalid("command_stdin_limit"));
        }
    }
}
impl Context {
    async fn call(&self, request: &Request, deadline: Instant) -> Result<LocalResult> {
        tokio::time::timeout_at(deadline, ipc::command_request(&self.socket, request))
            .await
            .map_err(|_| Error::Timeout)?
    }
    async fn entries(&self) -> Result<Vec<InvocationEntry>> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut after = None;
        let mut entries = Vec::new();
        loop {
            let result = self
                .call(&Request::CommandInvocations { after, limit: 64 }, deadline)
                .await?;
            let LocalResult::Invocations {
                entries: page,
                next,
            } = result
            else {
                return Err(invalid("command_invocations"));
            };
            if page.len() > 64 || entries.len() + page.len() > 10_000 {
                return Err(invalid("command_invocations"));
            }
            let mut previous = after.unwrap_or(0);
            for entry in &page {
                if entry.cursor <= previous || entry.handle.profile() != self.profile {
                    return Err(invalid("command_invocations"));
                }
                previous = entry.cursor;
            }
            if let Some(next) = next
                && (page.is_empty() || next != previous)
            {
                return Err(invalid("command_invocations"));
            }
            entries.extend(page);
            let Some(next) = next else {
                return Ok(entries);
            };
            after = Some(next);
        }
    }
    async fn select(&self, selection: Selection) -> Result<CommandHandle> {
        let handle = match selection.selector {
            Selector::Handle(handle) => handle,
            Selector::HandleFile(path) => read_handle(path)?,
            selector => {
                let mut matches = self
                    .entries()
                    .await?
                    .into_iter()
                    .filter(|entry| {
                        selection
                            .remote
                            .is_none_or(|remote| entry.handle.remote() == remote)
                            && match selector {
                                Selector::Invocation(invocation) => {
                                    entry.handle.invocation() == invocation
                                }
                                Selector::Latest => true,
                                _ => false,
                            }
                    })
                    .collect::<Vec<_>>();
                match selector {
                    Selector::Invocation(_) if matches.len() == 1 => matches.remove(0).handle,
                    Selector::Latest => matches.pop().ok_or(invalid("command_not_found"))?.handle,
                    _ => return Err(invalid("command_selector")),
                }
            }
        };
        if handle.profile() != self.profile
            || selection
                .remote
                .is_some_and(|remote| remote != handle.remote())
        {
            return Err(invalid("command_handle_binding"));
        }
        Ok(handle)
    }
    async fn cursors(&self, handle: &CommandHandle, deadline: Instant) -> Result<Cursors> {
        match self
            .call(
                &Request::CommandCursors {
                    handle: handle.clone(),
                },
                deadline,
            )
            .await?
        {
            LocalResult::Cursors { stdout, stderr } => Ok(Cursors { stdout, stderr }),
            _ => Err(invalid("command_cursors")),
        }
    }
    async fn consume(
        &self,
        handle: &CommandHandle,
        answer: &Answer,
        expected: Option<Cursors>,
        deadline: Instant,
    ) -> Result<Consumption> {
        match self
            .call(
                &Request::CommandConsume {
                    handle: handle.clone(),
                    request_id: hex::encode(answer.response.id),
                    response_sha256: hex::encode(answer.digest),
                    expected_cursors: expected,
                },
                deadline,
            )
            .await?
        {
            LocalResult::Consumed {
                stdout,
                stderr,
                finish_request_id,
                finish_confirmed,
            } => Ok(Consumption {
                cursors: Cursors { stdout, stderr },
                finish: finish_request_id.map(|text| id(&text)).transpose()?,
                confirmed: finish_confirmed,
            }),
            LocalResult::Failure { category, .. }
                if category == "command_output_cursor_invalid" =>
            {
                Err(invalid("output_gap"))
            }
            _ => Err(invalid("command_consume")),
        }
    }
}
struct Consumption {
    cursors: Cursors,
    finish: Option<Id>,
    confirmed: bool,
}
struct Answer {
    response: wire::Response,
    digest: wire::Digest,
}
fn decode_answer(
    result: LocalResult,
    handle: &CommandHandle,
    request_id: Id,
) -> Result<Option<Answer>> {
    match result {
        LocalResult::LocalBusy => Ok(None),
        LocalResult::RemotePending {
            handle: found,
            request_id: found_id,
        } if found == *handle && id(&found_id)? == request_id => Ok(None),
        LocalResult::Answer {
            handle: found,
            request_id: found_id,
            response_base64,
            response_sha256,
        } => {
            if found != *handle
                || id(&found_id)? != request_id
                || response_base64.len() > 43_692
                || !crate::foundation::valid_hex(&response_sha256, 64)
            {
                return Err(invalid("command_response"));
            }
            let bytes = STANDARD
                .decode(&response_base64)
                .map_err(|_| invalid("command_response"))?;
            if STANDARD.encode(&bytes) != response_base64 || bytes.len() > wire::MAX_CONTENT {
                return Err(invalid("command_response"));
            }
            let digest = wire::request_digest(&bytes);
            if hex::encode(digest) != response_sha256 {
                return Err(invalid("command_response"));
            }
            let response = wire::decode_response(&bytes)?;
            if response.id != request_id
                || response.binding.server != handle.remote()
                || response.binding.session != handle.session()
                || response
                    .result
                    .as_ref()
                    .ok()
                    .and_then(ResponseResult::status)
                    .is_some_and(|s| s.invocation != handle.invocation())
            {
                return Err(invalid("command_response_binding"));
            }
            Ok(Some(Answer { response, digest }))
        }
        LocalResult::Failure { category, .. } if retryable(&category) => Ok(None),
        _ => Err(invalid("command_response")),
    }
}
fn retryable(category: &str) -> bool {
    matches!(
        category,
        "local_busy"
            | "command_poll_busy"
            | "command_slot_capacity"
            | "command_slot_principal_capacity"
            | "command_publication_capacity"
            | "remote_pending"
    )
}
fn status_problem(status: &wire::Status) -> Option<&'static str> {
    if !status.authority_valid {
        Some("invalidated")
    } else if status.state == State::UnknownLaunch {
        Some("unknown_launch")
    } else if status.state == State::Lost {
        Some("lost")
    } else if status.warning == Some(wire::Warning::InputFailed)
        || matches!(
            status.input.health,
            wire::InputHealth::Gap | wire::InputHealth::Uncertain
        )
    {
        Some("input_failed")
    } else if !status.stdout.gaps.is_empty() || !status.stderr.gaps.is_empty() {
        Some("output_gap")
    } else {
        None
    }
}
fn wire_category(code: wire::ErrorCode) -> &'static str {
    use wire::ErrorCode::*;
    match code {
        InvalidRequest => "invalid_request",
        Unauthorized => "unauthorized",
        RoleDisabled => "role_disabled",
        CommandDenied => "command_denied",
        NoncanonicalExecutable => "noncanonical_executable",
        InvocationConflict => "invocation_conflict",
        Expired => "expired",
        FutureDeferred => "future_deferred",
        FutureExcessive => "future_excessive",
        ClockBehindFloor => "clock_behind_floor",
        QuotaExceeded => "quota_exceeded",
        Busy => "busy",
        NotFound => "not_found",
        ResultExpired => "result_expired",
        RecoveryUncertain => "recovery_uncertain",
        InputFailed => "input_failed",
        StorageFailure => "storage_failure",
        TransportDegraded => "transport_degraded",
        AuthorityRevoked => "invalidated",
    }
}
fn report_error(error: &Error) -> u8 {
    let category = match error {
        Error::Invalid(value) => *value,
        Error::Timeout => "local_timeout",
        Error::Io(_) => "local_io",
        _ => "command_failed",
    };
    eprintln!("marmish: {category}");
    if matches!(category, "unknown_launch" | "lost") {
        254
    } else {
        255
    }
}
fn report_remote_error(error: &Error) -> u8 {
    if matches!(error, Error::Timeout) {
        report_error(&invalid("remote_pending"))
    } else {
        report_error(error)
    }
}
fn terminal_exit(status: &wire::Status, cursors: &Cursors) -> Option<u8> {
    if status_problem(status).is_some()
        || status.state != State::Exited
        || !status.stdout.closed
        || !status.stderr.closed
        || cursors.stdout != status.stdout.produced
        || cursors.stderr != status.stderr.produced
    {
        return None;
    }
    let outcome = status.outcome.as_ref()?;
    match (outcome.kind, outcome.value) {
        (wire::OutcomeKind::Code, Some(value)) => Some(value),
        (wire::OutcomeKind::Signal, Some(value)) => Some(128u8.saturating_add(value)),
        (wire::OutcomeKind::NotStarted, _) => Some(255),
        _ => None,
    }
}
/// Copy both streams and flush before requesting durable consumption. An error
/// leaves the answer unconsumed; retry may duplicate bytes, never silently skip.
fn copy_poll(
    result: &ResponseResult,
    start: &Cursors,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<Cursors> {
    let ResponseResult::Poll {
        status,
        stdout,
        stderr,
    } = result
    else {
        return Err(invalid("command_poll"));
    };
    if !chunk_progress(stdout, start.stdout, &status.stdout.gaps)
        || !chunk_progress(stderr, start.stderr, &status.stderr.gaps)
    {
        return Err(invalid("output_gap"));
    }
    out.write_all(&stdout.bytes)?;
    err.write_all(&stderr.bytes)?;
    out.flush()?;
    err.flush()?;
    Ok(Cursors {
        stdout: stdout.next_cursor,
        stderr: stderr.next_cursor,
    })
}
fn chunk_progress(chunk: &wire::Chunk, start: u64, gaps: &[wire::Gap]) -> bool {
    let next = chunk.next_cursor;
    next >= start
        && next <= i64::MAX as u64
        && chunk.bytes.len() as u64 <= next - start
        && (chunk.bytes.len() as u64 == next - start
            || gaps
                .iter()
                .any(|g| g.start < next && g.end.is_none_or(|end| end > start)))
}
enum SubmitAck {
    Queued,
    Busy,
    Uncertain,
}
#[derive(Default)]
struct WaitProgress {
    delivery_confirmed: bool,
    authenticated: bool,
    known_terminal: Option<u8>,
}
struct Applied {
    terminal: Option<u8>,
    accepted: bool,
    poll: bool,
    refusal: Option<wire::ErrorCode>,
}
fn retryable_wire(code: wire::ErrorCode) -> bool {
    matches!(
        code,
        wire::ErrorCode::Busy | wire::ErrorCode::QuotaExceeded | wire::ErrorCode::FutureDeferred
    )
}
impl Context {
    async fn apply_answer(
        &self,
        handle: &CommandHandle,
        answer: Answer,
        start: Option<Cursors>,
        deadline: Instant,
        delivery_confirmed: &mut bool,
        detach: bool,
    ) -> Result<Applied> {
        let is_poll = answer.response.op == Op::Poll;
        let result = match &answer.response.result {
            Ok(result) => result,
            Err(error) => {
                self.consume(handle, &answer, start, deadline).await?;
                return Ok(Applied {
                    terminal: None,
                    accepted: false,
                    poll: is_poll,
                    refusal: Some(error.code),
                });
            }
        };
        let status = result.status().cloned();
        let committed = if is_poll {
            self.write_poll(
                handle,
                &answer,
                start.as_ref().ok_or(invalid("command_cursors"))?,
                deadline,
                &mut std::io::stdout().lock(),
                &mut std::io::stderr().lock(),
            )
            .await?
        } else {
            self.consume(handle, &answer, start, deadline).await?
        };
        if committed.confirmed || matches!(result, ResponseResult::Finish { finished: true }) {
            *delivery_confirmed = true;
        }
        if let Some(status) = status {
            if let Some(category) = status_problem(&status) {
                return Err(invalid(category));
            }
            if status.warning == Some(wire::Warning::StdinUnconsumed) {
                eprintln!("marmish: stdin_unconsumed");
            }
            let terminal = is_poll
                .then(|| terminal_exit(&status, &committed.cursors))
                .flatten();
            if terminal.is_some() && !*delivery_confirmed && !detach {
                match committed.finish {
                    Some(finish) => match self.finish_delivery(handle, finish, deadline).await {
                        Ok(true) => *delivery_confirmed = true,
                        Ok(false) => {
                            // Keep the known outcome while a fresh Poll can prepare another Finish.
                            return Ok(Applied {
                                terminal,
                                accepted: status.authority_valid,
                                poll: is_poll,
                                refusal: Some(wire::ErrorCode::Busy),
                            });
                        }
                        Err(_) => {
                            report_handle(handle)?;
                            eprintln!("marmish: delivery_pending");
                        }
                    },
                    None => {
                        report_handle(handle)?;
                        eprintln!("marmish: delivery_pending");
                    }
                }
            }
            Ok(Applied {
                terminal,
                accepted: status.authority_valid,
                poll: is_poll,
                refusal: None,
            })
        } else {
            Ok(Applied {
                terminal: None,
                accepted: false,
                poll: false,
                refusal: None,
            })
        }
    }
    async fn write_poll(
        &self,
        handle: &CommandHandle,
        answer: &Answer,
        start: &Cursors,
        deadline: Instant,
        out: &mut impl Write,
        err: &mut impl Write,
    ) -> Result<Consumption> {
        let result = answer
            .response
            .result
            .as_ref()
            .map_err(|e| invalid(wire_category(e.code)))?;
        match copy_poll(result, start, out, err) {
            Ok(_) => {
                self.consume(
                    handle,
                    answer,
                    Some(Cursors {
                        stdout: start.stdout,
                        stderr: start.stderr,
                    }),
                    deadline,
                )
                .await
            }
            Err(Error::Invalid("output_gap")) => {
                // The store uses this same progress predicate: only invalid
                // chunks are consumed without advancing either delivery cursor.
                self.consume(
                    handle,
                    answer,
                    Some(Cursors {
                        stdout: start.stdout,
                        stderr: start.stderr,
                    }),
                    deadline,
                )
                .await?;
                Err(invalid("output_gap"))
            }
            Err(e) => Err(e),
        }
    }
    async fn finish_delivery(
        &self,
        handle: &CommandHandle,
        request_id: Id,
        deadline: Instant,
    ) -> Result<bool> {
        loop {
            let response = self
                .call(
                    &Request::CommandResponse {
                        handle: handle.clone(),
                        request_id: hex::encode(request_id),
                    },
                    deadline,
                )
                .await;
            let response = match response {
                Ok(response) => response,
                Err(Error::Timeout | Error::Io(_)) if Instant::now() < deadline => {
                    tokio::time::sleep_until(
                        (Instant::now() + Duration::from_millis(100)).min(deadline),
                    )
                    .await;
                    continue;
                }
                Err(e) => return Err(e),
            };
            if let Some(answer) = decode_answer(response, handle, request_id)? {
                if answer.response.op != Op::Finish {
                    return Err(invalid("command_response_binding"));
                }
                let complete = matches!(
                    answer.response.result,
                    Ok(ResponseResult::Finish { finished: true })
                );
                let busy = matches!(&answer.response.result,Err(error) if error.code==wire::ErrorCode::Busy);
                self.consume(handle, &answer, None, deadline).await?;
                return if complete {
                    Ok(true)
                } else if busy {
                    Ok(false)
                } else {
                    Err(invalid("command_delivery_unconfirmed"))
                };
            }
            tokio::time::sleep_until((Instant::now() + Duration::from_secs(1)).min(deadline)).await;
        }
    }
    async fn recover(
        &self,
        handle: &CommandHandle,
        deadline: Instant,
        detach: bool,
        progress: &mut WaitProgress,
    ) -> Result<(Option<u8>, Option<Id>)> {
        let result = self
            .call(
                &Request::CommandUnconsumed {
                    handle: handle.clone(),
                    limit: 64,
                },
                deadline,
            )
            .await?;
        let LocalResult::Entries { entries } = result else {
            return Err(invalid("command_unconsumed"));
        };
        if entries.len() > 64 {
            return Err(invalid("command_unconsumed"));
        }
        let mut poll = None;
        for entry in entries {
            let request_id = id(&entry.request_id)?;
            let response = self
                .call(
                    &Request::CommandResponse {
                        handle: handle.clone(),
                        request_id: entry.request_id,
                    },
                    deadline,
                )
                .await?;
            let Some(answer) = decode_answer(response, handle, request_id)? else {
                continue;
            };
            if answer.response.op != entry.op || hex::encode(answer.digest) != entry.sha256 {
                return Err(invalid("command_response_binding"));
            }
            let start = if answer.response.op == Op::Poll {
                Some(self.cursors(handle, deadline).await?)
            } else {
                None
            };
            let applied = self
                .apply_answer(
                    handle,
                    answer,
                    start,
                    deadline,
                    &mut progress.delivery_confirmed,
                    detach,
                )
                .await?;
            if applied.poll {
                poll = Some(request_id);
            }
            progress.authenticated |= applied.accepted;
            if let Some(code) = applied.refusal {
                if applied.terminal.is_some() {
                    progress.known_terminal = applied.terminal;
                    continue;
                }
                if retryable_wire(code)
                    || (code == wire::ErrorCode::NotFound
                        && applied.poll
                        && !progress.authenticated)
                {
                    continue;
                }
                return Err(invalid(wire_category(code)));
            }
            if detach && applied.accepted {
                return Ok((Some(0), poll));
            }
            if applied.terminal.is_some() {
                return Ok((applied.terminal, poll));
            }
        }
        Ok((None, poll))
    }
    async fn wait_inner(
        &self,
        handle: &CommandHandle,
        deadline: Instant,
        detach: bool,
        pre_accept: bool,
    ) -> Result<u8> {
        let mut progress = WaitProgress {
            authenticated: !pre_accept,
            ..Default::default()
        };
        self.wait_with_progress(handle, deadline, detach, &mut progress)
            .await
    }
    async fn wait_with_progress(
        &self,
        handle: &CommandHandle,
        deadline: Instant,
        detach: bool,
        progress: &mut WaitProgress,
    ) -> Result<u8> {
        match self.wait_loop(handle, deadline, detach, progress).await {
            Err(Error::Timeout | Error::Io(_)) if progress.known_terminal.is_some() => {
                eprintln!("marmish: delivery_pending");
                Ok(progress.known_terminal.expect("known outcome"))
            }
            result => result,
        }
    }
    async fn wait_loop(
        &self,
        handle: &CommandHandle,
        deadline: Instant,
        detach: bool,
        progress: &mut WaitProgress,
    ) -> Result<u8> {
        let mut pending: Option<(Id, Cursors, bool)> = None;
        loop {
            if Instant::now() >= deadline {
                if let Some(code) = progress.known_terminal {
                    eprintln!("marmish: delivery_pending");
                    return Ok(code);
                }
                return Err(Error::Timeout);
            }
            let (done, recovered_poll) =
                match self.recover(handle, deadline, detach, progress).await {
                    Ok(result) => result,
                    Err(Error::Timeout | Error::Io(_)) if progress.known_terminal.is_some() => {
                        eprintln!("marmish: delivery_pending");
                        return Ok(progress.known_terminal.expect("checked outcome"));
                    }
                    Err(Error::Timeout | Error::Io(_)) if Instant::now() < deadline => {
                        tokio::time::sleep_until(
                            (Instant::now() + Duration::from_millis(100)).min(deadline),
                        )
                        .await;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
            if let Some(code) = done {
                return Ok(code);
            }
            if let Some(recovered_id) = recovered_poll
                && pending
                    .as_ref()
                    .is_some_and(|(pending_id, _, queued)| *pending_id == recovered_id || !*queued)
            {
                pending = None;
            }
            if !progress.authenticated {
                // LocalQueued precedes remote admission. A Poll sent now can
                // freeze NotFound before the Submit arrives, even if that
                // response is delivered after the authenticated Submit answer.
                // Wait for the durable original Submit response first.
                tokio::time::sleep_until(
                    (Instant::now() + Duration::from_millis(100)).min(deadline),
                )
                .await;
                continue;
            }
            if pending.is_none() {
                pending = Some((fresh_id()?, self.cursors(handle, deadline).await?, false));
            }
            let (request_id, start, queued) = pending.as_mut().expect("initialized poll");
            let request = Request::CommandPoll {
                handle: handle.clone(),
                request_id: hex::encode(*request_id),
                stdout_cursor: start.stdout,
                stderr_cursor: start.stderr,
                max_bytes: wire::MAX_BYTES as u32,
            };
            match self.call(&request, deadline).await {
                Ok(LocalResult::Failure { category, .. })
                    if category == "command_response_consumed" =>
                {
                    pending = None
                }
                Ok(LocalResult::Failure { category, .. }) if retryable(&category) => {}
                Ok(LocalResult::LocalBusy) => {}
                Ok(result) => {
                    let answer = decode_answer(result, handle, *request_id)?;
                    *queued = true;
                    if let Some(answer) = answer {
                        if answer.response.op != Op::Poll {
                            return Err(invalid("command_response_binding"));
                        }
                        let applied = self
                            .apply_answer(
                                handle,
                                answer,
                                Some(Cursors {
                                    stdout: start.stdout,
                                    stderr: start.stderr,
                                }),
                                deadline,
                                &mut progress.delivery_confirmed,
                                detach,
                            )
                            .await?;
                        progress.authenticated |= applied.accepted;
                        if let Some(code) = applied.refusal {
                            if applied.terminal.is_some() {
                                progress.known_terminal = applied.terminal;
                                pending = None;
                                continue;
                            }
                            if !retryable_wire(code)
                                && !(code == wire::ErrorCode::NotFound && !progress.authenticated)
                            {
                                return Err(invalid(wire_category(code)));
                            }
                        }
                        if detach && applied.accepted {
                            return Ok(0);
                        }
                        if applied.refusal.is_none()
                            && let Some(code) = applied.terminal
                        {
                            return Ok(code);
                        }
                        pending = None;
                    }
                }
                Err(Error::Timeout | Error::Io(_)) => {}
                Err(error) => return Err(error),
            }
            tokio::time::sleep_until((Instant::now() + Duration::from_secs(1)).min(deadline)).await;
        }
    }
    async fn wait(&self, handle: &CommandHandle, wait: Duration, detach: bool) -> Result<u8> {
        let deadline = Instant::now()
            .checked_add(wait)
            .ok_or(invalid("command_wait"))?;
        self.wait_deadline(handle, deadline, detach, false).await
    }
    async fn wait_deadline(
        &self,
        handle: &CommandHandle,
        deadline: Instant,
        detach: bool,
        pre_accept: bool,
    ) -> Result<u8> {
        tokio::select! {
            result=self.wait_inner(handle,deadline,detach,pre_accept)=>match result {
                Ok(code)=>{if detach||code>=254 {report_handle(handle)?;}
                    if code>=254 {eprintln!("marmish: remote_exit");}Ok(code)},
                Err(error)=>{report_handle(handle)?;Ok(report_remote_error(&error))},
            },
            signal=tokio::signal::ctrl_c()=>{
                signal?;report_handle(handle)?;Ok(130)
            }
        }
    }
    async fn read_operation(&self, handle: &CommandHandle, cancel: bool) -> Result<u8> {
        tokio::select! {
            result=self.read_operation_inner(handle,cancel)=>match result {
                Ok(code)=>Ok(code),
                Err(error)=>{report_handle(handle)?;Ok(report_remote_error(&error))},
            },
            signal=tokio::signal::ctrl_c()=>{signal?;report_handle(handle)?;Ok(130)}
        }
    }
    async fn read_operation_inner(&self, handle: &CommandHandle, cancel: bool) -> Result<u8> {
        let request_id = fresh_id()?;
        let deadline = Instant::now() + Duration::from_secs(180);
        let request = if cancel {
            Request::CommandCancel {
                handle: handle.clone(),
                request_id: hex::encode(request_id),
            }
        } else {
            Request::CommandStatus {
                handle: handle.clone(),
                request_id: hex::encode(request_id),
            }
        };
        loop {
            let result = match self.call(&request, deadline).await {
                Ok(result) => result,
                Err(Error::Timeout | Error::Io(_)) if Instant::now() < deadline => {
                    tokio::time::sleep_until(
                        (Instant::now() + Duration::from_millis(100)).min(deadline),
                    )
                    .await;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if let Some(answer) = decode_answer(result, handle, request_id)? {
                if answer.response.op != if cancel { Op::Cancel } else { Op::Status } {
                    return Err(invalid("command_response_binding"));
                }
                // Status is a finite, closed wire object; command payload and
                // original Submit request ID are never printed in this view.
                if let Ok(result) = &answer.response.result {
                    let status = result.status().ok_or(invalid("command_response"))?;
                    let mut output = std::io::stdout().lock();
                    serde_json::to_writer(&mut output, status)
                        .map_err(|_| invalid("command_status"))?;
                    output.write_all(b"\n")?;
                    output.flush()?;
                }
                let applied = self
                    .apply_answer(handle, answer, None, deadline, &mut false, false)
                    .await?;
                if let Some(code) = applied.refusal {
                    return Err(invalid(wire_category(code)));
                }
                return Ok(0);
            }
            tokio::time::sleep_until((Instant::now() + Duration::from_secs(1)).min(deadline)).await;
        }
    }
    async fn prepare(&self, remote: Account, relays: Vec<String>) -> Result<(Id, Account)> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let request = Request::CommandPrepare {
            remote: hex::encode(remote),
            relays,
        };
        let mut pause = Duration::from_millis(50);
        loop {
            match self.call(&request, deadline).await? {
                LocalResult::Ready {
                    session,
                    local_account,
                } => return Ok((id(&session)?, account(&local_account)?)),
                LocalResult::LocalBusy => {}
                _ => return Err(invalid("command_prepare")),
            }
            tokio::time::sleep_until((Instant::now() + pause).min(deadline)).await;
            if Instant::now() >= deadline {
                return Err(invalid("local_busy"));
            }
            pause = (pause * 2).min(Duration::from_millis(500));
        }
    }
    async fn submit(
        &self,
        remote: Account,
        relays: Vec<String>,
        argv: Vec<OsString>,
        stdin: StdinMode,
        detach: bool,
        wait: Duration,
    ) -> Result<u8> {
        let stdin = read_stdin(
            libc::STDIN_FILENO,
            stdin,
            std::io::stdin().is_terminal(),
            Duration::from_secs(15),
        )?;
        let submit = wire::Submit {
            invocation: fresh_id()?,
            argv: argv.into_iter().map(OsString::into_vec).collect(),
            stdin,
        };
        // Validate bytes/argv and maximum encoded size before preparing a room.
        let request_id = fresh_id()?;
        prepare_operation(
            Binding {
                session: submit.invocation,
                client: [0; 32],
                server: remote,
            },
            request_id,
            RequestOp::Submit(submit.clone()),
        )?;
        let (session, local) = self.prepare(remote, relays.clone()).await?;
        let prepared = prepare_operation(
            Binding {
                session,
                client: local,
                server: remote,
            },
            request_id,
            RequestOp::Submit(submit.clone()),
        )?;
        let handle = CommandHandle::for_submit(&self.profile, &prepared)?;
        let request = Request::CommandSubmit {
            remote: hex::encode(remote),
            relays,
            session: hex::encode(session),
            request_id: hex::encode(request_id),
            submit,
        };
        let deadline = Instant::now()
            .checked_add(wait)
            .ok_or(invalid("command_wait"))?;
        tokio::select! {
            result=self.submit_until(&request,&handle,deadline)=> match result? {
                SubmitAck::Queued=>self.wait_deadline(&handle,deadline,detach,true).await,
                SubmitAck::Busy=>{eprintln!("marmish: local_busy");Ok(255)},
                SubmitAck::Uncertain=>{report_handle(&handle)?;eprintln!("marmish: local_pending");Ok(254)},
            },
            signal=tokio::signal::ctrl_c()=>{signal?;report_handle(&handle)?;Ok(130)}
        }
    }
    async fn submit_until(
        &self,
        request: &Request,
        handle: &CommandHandle,
        deadline: Instant,
    ) -> Result<SubmitAck> {
        let mut ambiguous = false;
        loop {
            if Instant::now() >= deadline {
                return Ok(SubmitAck::Uncertain);
            }
            match self.call(request, deadline).await {
                Ok(LocalResult::LocalQueued { handle: found }) if &found == handle => {
                    return Ok(SubmitAck::Queued);
                }
                Ok(LocalResult::LocalBusy) if !ambiguous => return Ok(SubmitAck::Busy),
                Ok(LocalResult::LocalBusy) => {}
                Ok(LocalResult::Failure { category, .. })
                    if category == "command_room_pending" && !ambiguous =>
                {
                    return Err(invalid("command_room_pending"));
                }
                Ok(LocalResult::Failure { category, .. })
                    if matches!(
                        category.as_str(),
                        "operation_uncertain" | "local_pending" | "command_room_pending"
                    ) =>
                {
                    ambiguous = true;
                }
                Err(Error::Timeout | Error::Io(_)) => {
                    ambiguous = true;
                }
                _ => return Ok(SubmitAck::Uncertain),
            }
            tokio::time::sleep_until((Instant::now() + Duration::from_millis(100)).min(deadline))
                .await;
        }
    }
}
pub(crate) async fn run(context: Context, action: Action) -> Result<u8> {
    match run_inner(context, action).await {
        Ok(code) => Ok(code),
        Err(error) => Ok(report_error(&error)),
    }
}
async fn run_inner(context: Context, action: Action) -> Result<u8> {
    match action {
        Action::Submit {
            remote,
            relays,
            argv,
            stdin,
            detach,
            wait,
        } => {
            context
                .submit(remote, relays, argv, stdin, detach, wait)
                .await
        }
        Action::Commands { remote } => {
            let entries = context.entries().await?;
            let mut output = std::io::stdout().lock();
            for entry in entries {
                if remote.is_none_or(|remote| remote == entry.handle.remote()) {
                    serde_json::to_writer(&mut output, &entry)
                        .map_err(|_| invalid("command_invocations"))?;
                    output.write_all(b"\n")?;
                }
            }
            output.flush()?;
            Ok(0)
        }
        Action::Status { selection } => {
            let handle = context.select(selection).await?;
            context.read_operation(&handle, false).await
        }
        Action::Cancel { selection } => {
            let handle = context.select(selection).await?;
            context.read_operation(&handle, true).await
        }
        Action::Wait { selection, wait } | Action::Output { selection, wait } => {
            let handle = context.select(selection).await?;
            context.wait(&handle, wait, false).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::{AsRawFd, FromRawFd};
    fn pipe() -> (std::fs::File, std::fs::File) {
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        unsafe {
            (
                std::fs::File::from_raw_fd(fds[0]),
                std::fs::File::from_raw_fd(fds[1]),
            )
        }
    }
    fn handle() -> CommandHandle {
        let binding = Binding {
            session: fresh_id().unwrap(),
            client: [1; 32],
            server: [2; 32],
        };
        let prepared = prepare_operation(
            binding,
            fresh_id().unwrap(),
            RequestOp::Submit(wire::Submit {
                invocation: fresh_id().unwrap(),
                argv: vec![b"/bin/cat".to_vec()],
                stdin: Vec::new(),
            }),
        )
        .unwrap();
        CommandHandle::for_submit("test", &prepared).unwrap()
    }
    fn status(invocation: Id) -> wire::Status {
        wire::Status {
            admission: wire::Admission::Engine,
            authority_valid: true,
            cancel_requested: false,
            engine_known: true,
            input: wire::InputStatus {
                accepted: 0,
                delivered: 0,
                eof_sent: true,
                health: wire::InputHealth::Closed,
                length: 0,
            },
            invocation,
            outcome: Some(wire::Outcome {
                kind: wire::OutcomeKind::Code,
                value: Some(7),
            }),
            state: State::Exited,
            stdout: wire::OutputStatus {
                closed: true,
                gaps: Vec::new(),
                produced: 3,
                retained: 3,
            },
            stderr: wire::OutputStatus {
                closed: true,
                gaps: Vec::new(),
                produced: 2,
                retained: 2,
            },
            warning: None,
        }
    }
    fn result(handle: &CommandHandle, request_id: Id) -> LocalResult {
        let response = wire::Response {
            binding: Binding {
                session: handle.session(),
                client: [1; 32],
                server: handle.remote(),
            },
            id: request_id,
            op: Op::Poll,
            request_sha256: [5; 32],
            result: Ok(ResponseResult::Poll {
                status: status(handle.invocation()),
                stdout: wire::Chunk {
                    bytes: vec![0, 255, 10],
                    next_cursor: 3,
                },
                stderr: wire::Chunk {
                    bytes: vec![128, 1],
                    next_cursor: 2,
                },
            }),
        };
        let bytes = wire::encode_response(&response).unwrap();
        LocalResult::Answer {
            handle: handle.clone(),
            request_id: hex::encode(request_id),
            response_base64: STANDARD.encode(&bytes),
            response_sha256: hex::encode(wire::request_digest(&bytes)),
        }
    }
    #[test]
    fn finite_stdin_preserves_binary_and_rejects_overflow_or_missing_eof() {
        for size in [0, 256, wire::MAX_BYTES, wire::MAX_BYTES + 1] {
            let (reader, mut writer) = pipe();
            let bytes = (0..size).map(|i| i as u8).collect::<Vec<_>>();
            let expected = bytes.clone();
            let thread = std::thread::spawn(move || {
                let _ = writer.write_all(&bytes);
            });
            let got = read_stdin(
                reader.as_raw_fd(),
                StdinMode::Read,
                false,
                Duration::from_secs(2),
            );
            drop(reader);
            thread.join().unwrap();
            if size <= wire::MAX_BYTES {
                assert_eq!(got.unwrap(), expected);
            } else {
                assert!(matches!(got, Err(Error::Invalid("command_stdin_limit"))));
            }
        }
        let (reader, _writer) = pipe();
        assert!(matches!(
            read_stdin(
                reader.as_raw_fd(),
                StdinMode::Read,
                false,
                Duration::from_millis(10)
            ),
            Err(Error::Invalid("input_timeout"))
        ));
        assert!(
            read_stdin(reader.as_raw_fd(), StdinMode::Auto, true, Duration::ZERO)
                .unwrap()
                .is_empty()
        );
        assert!(
            read_stdin(reader.as_raw_fd(), StdinMode::None, false, Duration::ZERO)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn exact_answer_verifies_payload_hash_binding_and_binary_poll() {
        let handle = handle();
        let request_id = fresh_id().unwrap();
        let answer = decode_answer(result(&handle, request_id), &handle, request_id)
            .unwrap()
            .unwrap();
        let body = answer.response.result.ok().unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let end = copy_poll(
            &body,
            &Cursors {
                stdout: 0,
                stderr: 0,
            },
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(stdout, vec![0, 255, 10]);
        assert_eq!(stderr, vec![128, 1]);
        assert_eq!(terminal_exit(body.status().unwrap(), &end), Some(7));
        assert!(decode_answer(result(&handle, request_id), &handle, fresh_id().unwrap()).is_err());
        let mut malformed = result(&handle, request_id);
        if let LocalResult::Answer {
            response_sha256, ..
        } = &mut malformed
        {
            *response_sha256 = "00".repeat(32);
        }
        assert!(decode_answer(malformed, &handle, request_id).is_err());
        let mut malformed = result(&handle, request_id);
        if let LocalResult::Answer {
            response_base64, ..
        } = &mut malformed
        {
            response_base64.push('\n');
        }
        assert!(decode_answer(malformed, &handle, request_id).is_err());
        let mut malformed = result(&handle, request_id);
        if let LocalResult::Answer {
            response_base64,
            response_sha256,
            ..
        } = &mut malformed
        {
            let bytes = STANDARD.decode(&*response_base64).unwrap();
            let mut response = wire::decode_response(&bytes).unwrap();
            response.binding.server = [3; 32];
            let bytes = wire::encode_response(&response).unwrap();
            *response_base64 = STANDARD.encode(&bytes);
            *response_sha256 = hex::encode(wire::request_digest(&bytes));
        }
        assert!(decode_answer(malformed, &handle, request_id).is_err());
    }
    struct Fails;
    impl Write for Fails {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("test"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn gaps_uncertainty_and_write_failure_never_claim_complete_output() {
        let handle = handle();
        let request_id = fresh_id().unwrap();
        let answer = decode_answer(result(&handle, request_id), &handle, request_id)
            .unwrap()
            .unwrap();
        let mut body = answer.response.result.ok().unwrap();
        let start = Cursors {
            stdout: 0,
            stderr: 0,
        };
        assert!(matches!(
            copy_poll(&body, &start, &mut Fails, &mut Vec::new()),
            Err(Error::Io(_))
        ));
        if let ResponseResult::Poll { status, .. } = &mut body {
            status.stdout.gaps.push(wire::Gap {
                start: 0,
                end: Some(1),
                reason: wire::GapReason::Limit,
            });
        }
        let mut out = Vec::new();
        assert!(copy_poll(&body, &start, &mut out, &mut Vec::new()).is_ok());
        assert_eq!(out, vec![0, 255, 10]);
        assert_eq!(
            terminal_exit(
                body.status().unwrap(),
                &Cursors {
                    stdout: 3,
                    stderr: 2
                }
            ),
            None
        );
        let mut clean = status(handle.invocation());
        clean.state = State::Lost;
        assert_eq!(
            terminal_exit(
                &clean,
                &Cursors {
                    stdout: 3,
                    stderr: 2
                }
            ),
            None
        );
        clean.state = State::Exited;
        clean.warning = Some(wire::Warning::InputFailed);
        assert_eq!(
            terminal_exit(
                &clean,
                &Cursors {
                    stdout: 3,
                    stderr: 2
                }
            ),
            None
        );
    }
    async fn read_request(stream: &mut tokio::net::UnixStream) -> Request {
        use tokio::io::AsyncReadExt;
        let length = stream.read_u32().await.unwrap();
        assert!(length <= 65_536);
        let mut body = vec![0; length as usize];
        stream.read_exact(&mut body).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }
    async fn reply(stream: &mut tokio::net::UnixStream, result: LocalResult) {
        use tokio::io::AsyncWriteExt;
        let bytes = serde_json::to_vec(&ipc::Response::command(result)).unwrap();
        stream.write_u32(bytes.len() as u32).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.shutdown().await.unwrap();
    }
    async fn serve_replies(
        listener: tokio::net::UnixListener,
        replies: Vec<LocalResult>,
    ) -> Vec<Request> {
        let mut requests = Vec::new();
        for result in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            requests.push(read_request(&mut stream).await);
            reply(&mut stream, result).await;
        }
        requests
    }
    fn response_result(
        handle: &CommandHandle,
        request_id: Id,
        op: Op,
        result: std::result::Result<ResponseResult, wire::WireError>,
    ) -> LocalResult {
        let response = wire::Response {
            binding: Binding {
                session: handle.session(),
                client: [1; 32],
                server: handle.remote(),
            },
            id: request_id,
            op,
            request_sha256: [5; 32],
            result,
        };
        let bytes = wire::encode_response(&response).unwrap();
        LocalResult::Answer {
            handle: handle.clone(),
            request_id: hex::encode(request_id),
            response_base64: STANDARD.encode(&bytes),
            response_sha256: hex::encode(wire::request_digest(&bytes)),
        }
    }
    fn socket_context() -> (tempfile::TempDir, Context, tokio::net::UnixListener) {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("s");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        (
            dir,
            Context {
                socket,
                profile: "test".into(),
            },
            listener,
        )
    }
    #[tokio::test]
    async fn submit_wait_does_not_poll_before_authenticated_admission() {
        let (_dir, context, listener) = socket_context();
        let handle = handle();
        let server_handle = handle.clone();
        let submit_id = fresh_id().unwrap();
        let server = tokio::spawn(async move {
            // Before the Submit answer exists, the sole query must remain
            // unconsumed discovery: no Poll can create a frozen NotFound.
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                assert!(matches!(
                    read_request(&mut stream).await,
                    Request::CommandUnconsumed { .. }
                ));
                reply(&mut stream, LocalResult::Entries { entries: vec![] }).await;
            }
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(matches!(
                read_request(&mut stream).await,
                Request::CommandUnconsumed { .. }
            ));
            let accepted = response_result(
                &server_handle,
                submit_id,
                Op::Submit,
                Ok(ResponseResult::Submit {
                    disposition: wire::Disposition::Accepted,
                    status: status(server_handle.invocation()),
                }),
            );
            let digest = match &accepted {
                LocalResult::Answer {
                    response_sha256, ..
                } => response_sha256.clone(),
                _ => unreachable!(),
            };
            reply(
                &mut stream,
                LocalResult::Entries {
                    entries: vec![crate::command_local::AnswerEntry {
                        request_id: hex::encode(submit_id),
                        op: Op::Submit,
                        sha256: digest,
                    }],
                },
            )
            .await;
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(matches!(
                read_request(&mut stream).await,
                Request::CommandResponse { .. }
            ));
            reply(&mut stream, accepted).await;
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(matches!(
                read_request(&mut stream).await,
                Request::CommandConsume {
                    expected_cursors: None,
                    ..
                }
            ));
            reply(
                &mut stream,
                LocalResult::Consumed {
                    stdout: 0,
                    stderr: 0,
                    finish_request_id: None,
                    finish_confirmed: false,
                },
            )
            .await;
        });
        // Detach returns immediately once genuine acceptance is consumed;
        // absence of earlier Polls is asserted by the actual IPC peer.
        assert_eq!(
            context
                .wait_inner(&handle, Instant::now() + Duration::from_secs(3), true, true)
                .await
                .unwrap(),
            0
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn warning_and_valid_gap_bytes_are_flushed_before_consumption() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        struct Tracked {
            bytes: Vec<u8>,
            flushed: Arc<AtomicBool>,
        }
        impl Write for Tracked {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.flushed.store(true, Ordering::SeqCst);
                Ok(())
            }
        }
        for gap in [false, true] {
            let (_dir, context, listener) = socket_context();
            let handle = handle();
            let request_id = fresh_id().unwrap();
            let mut status = status(handle.invocation());
            status.warning = Some(wire::Warning::StdinUnconsumed);
            if gap {
                status.stdout.gaps.push(wire::Gap {
                    start: 0,
                    end: Some(1),
                    reason: wire::GapReason::Limit,
                });
            }
            let result = response_result(
                &handle,
                request_id,
                Op::Poll,
                Ok(ResponseResult::Poll {
                    status: status.clone(),
                    stdout: wire::Chunk {
                        bytes: if gap { vec![255, 10] } else { vec![0, 255, 10] },
                        next_cursor: 3,
                    },
                    stderr: wire::Chunk {
                        bytes: vec![128, 1],
                        next_cursor: 2,
                    },
                }),
            );
            let answer = decode_answer(result, &handle, request_id).unwrap().unwrap();
            let outflushed = Arc::new(AtomicBool::new(false));
            let errflushed = Arc::new(AtomicBool::new(false));
            let a = outflushed.clone();
            let b = errflushed.clone();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                assert!(
                    a.load(Ordering::SeqCst) && b.load(Ordering::SeqCst),
                    "both streams must flush before consumption"
                );
                assert!(matches!(
                    request,
                    Request::CommandConsume {
                        expected_cursors: Some(Cursors {
                            stdout: 0,
                            stderr: 0
                        }),
                        ..
                    }
                ));
                reply(
                    &mut stream,
                    LocalResult::Consumed {
                        stdout: 3,
                        stderr: 2,
                        finish_request_id: None,
                        finish_confirmed: false,
                    },
                )
                .await;
            });
            let mut out = Tracked {
                bytes: Vec::new(),
                flushed: outflushed,
            };
            let mut err = Tracked {
                bytes: Vec::new(),
                flushed: errflushed,
            };
            let consumed = context
                .write_poll(
                    &handle,
                    &answer,
                    &Cursors {
                        stdout: 0,
                        stderr: 0,
                    },
                    Instant::now() + Duration::from_secs(2),
                    &mut out,
                    &mut err,
                )
                .await
                .unwrap();
            assert_eq!(
                out.bytes,
                if gap { vec![255, 10] } else { vec![0, 255, 10] }
            );
            assert_eq!(err.bytes, vec![128, 1]);
            assert_eq!(
                terminal_exit(&status, &consumed.cursors),
                if gap { None } else { Some(7) }
            );
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn invalid_chunk_store_discard_reports_output_gap() {
        let (_dir, context, listener) = socket_context();
        let handle = handle();
        let request_id = fresh_id().unwrap();
        let mut answer = decode_answer(result(&handle, request_id), &handle, request_id)
            .unwrap()
            .unwrap();
        if let Ok(ResponseResult::Poll { stdout, .. }) = &mut answer.response.result {
            stdout.next_cursor = 2;
        }
        answer.digest = wire::request_digest(&wire::encode_response(&answer.response).unwrap());
        let server = tokio::spawn(serve_replies(
            listener,
            vec![LocalResult::Failure {
                category: "command_output_cursor_invalid".into(),
                handle: None,
            }],
        ));
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert!(matches!(
            context
                .write_poll(
                    &handle,
                    &answer,
                    &Cursors {
                        stdout: 0,
                        stderr: 0
                    },
                    Instant::now() + Duration::from_secs(2),
                    &mut out,
                    &mut err
                )
                .await,
            Err(Error::Invalid("output_gap"))
        ));
        assert!(out.is_empty() && err.is_empty());
        assert!(matches!(
            server.await.unwrap()[0],
            Request::CommandConsume { .. }
        ));
    }
    #[tokio::test]
    async fn finish_busy_known_outcome_survives_cursor_timeout() {
        let (_dir, context, listener) = socket_context();
        let handle = handle();
        let request_id = fresh_id().unwrap();
        let finish = fresh_id().unwrap();
        let mut status = status(handle.invocation());
        status.stdout.produced = 0;
        status.stdout.retained = 0;
        status.stderr.produced = 0;
        status.stderr.retained = 0;
        let answer = decode_answer(
            response_result(
                &handle,
                request_id,
                Op::Poll,
                Ok(ResponseResult::Poll {
                    status,
                    stdout: wire::Chunk {
                        bytes: Vec::new(),
                        next_cursor: 0,
                    },
                    stderr: wire::Chunk {
                        bytes: Vec::new(),
                        next_cursor: 0,
                    },
                }),
            ),
            &handle,
            request_id,
        )
        .unwrap()
        .unwrap();
        let busy = response_result(
            &handle,
            finish,
            Op::Finish,
            Err(wire::WireError {
                code: wire::ErrorCode::Busy,
                detail: None,
            }),
        );
        let server = tokio::spawn(async move {
            for result in [
                LocalResult::Consumed {
                    stdout: 0,
                    stderr: 0,
                    finish_request_id: Some(hex::encode(finish)),
                    finish_confirmed: false,
                },
                busy,
                LocalResult::Consumed {
                    stdout: 0,
                    stderr: 0,
                    finish_request_id: None,
                    finish_confirmed: false,
                },
                LocalResult::Entries {
                    entries: Vec::new(),
                },
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let _ = read_request(&mut stream).await;
                reply(&mut stream, result).await;
            }
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(matches!(
                read_request(&mut stream).await,
                Request::CommandCursors { .. }
            ));
            tokio::time::sleep(Duration::from_millis(80)).await;
            drop(stream);
        });
        let applied = context
            .apply_answer(
                &handle,
                answer,
                Some(Cursors {
                    stdout: 0,
                    stderr: 0,
                }),
                Instant::now() + Duration::from_secs(2),
                &mut false,
                false,
            )
            .await
            .unwrap();
        assert_eq!(applied.terminal, Some(7));
        assert!(applied.refusal == Some(wire::ErrorCode::Busy));
        let mut progress = WaitProgress {
            authenticated: true,
            known_terminal: applied.terminal,
            ..Default::default()
        };
        assert_eq!(
            context
                .wait_with_progress(
                    &handle,
                    Instant::now() + Duration::from_millis(40),
                    false,
                    &mut progress
                )
                .await
                .unwrap(),
            7
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn lost_submit_reply_retries_identical_bytes_and_busy_cannot_downgrade() {
        let (_dir, context, listener) = socket_context();
        let handle = handle();
        let request = Request::CommandSubmit {
            remote: hex::encode(handle.remote()),
            relays: vec!["wss://example.test/".into()],
            session: hex::encode(handle.session()),
            request_id: hex::encode(fresh_id().unwrap()),
            submit: wire::Submit {
                invocation: handle.invocation(),
                argv: vec![b"/bin/cat".to_vec()],
                stdin: vec![0, 255],
            },
        };
        let found = handle.clone();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let initial = read_request(&mut first).await;
            drop(first);
            let (mut second, _) = listener.accept().await.unwrap();
            let retry = read_request(&mut second).await;
            assert_eq!(
                serde_json::to_vec(&initial).unwrap(),
                serde_json::to_vec(&retry).unwrap()
            );
            reply(
                &mut second,
                LocalResult::Failure {
                    category: "command_room_pending".into(),
                    handle: None,
                },
            )
            .await;
            let (mut third, _) = listener.accept().await.unwrap();
            let retry = read_request(&mut third).await;
            assert_eq!(
                serde_json::to_vec(&initial).unwrap(),
                serde_json::to_vec(&retry).unwrap()
            );
            reply(&mut third, LocalResult::LocalQueued { handle: found }).await;
        });
        assert!(matches!(
            context
                .submit_until(&request, &handle, Instant::now() + Duration::from_secs(2))
                .await
                .unwrap(),
            SubmitAck::Queued
        ));
        server.await.unwrap();
        let (_dir, context, listener) = socket_context();
        let server = tokio::spawn(serve_replies(
            listener,
            vec![LocalResult::Failure {
                category: "command_room_pending".into(),
                handle: None,
            }],
        ));
        assert!(matches!(
            context
                .submit_until(&request, &handle, Instant::now() + Duration::from_secs(2))
                .await,
            Err(Error::Invalid("command_room_pending"))
        ));
        server.await.unwrap();
        let (_dir, context, listener) = socket_context();
        let server = tokio::spawn(serve_replies(
            listener,
            vec![LocalResult::Failure {
                category: "post_commit_failure".into(),
                handle: None,
            }],
        ));
        assert!(matches!(
            context
                .submit_until(&request, &handle, Instant::now() + Duration::from_secs(2))
                .await
                .unwrap(),
            SubmitAck::Uncertain
        ));
        server.await.unwrap();
    }
    #[tokio::test]
    async fn status_and_cancel_busy_are_consumed_without_false_success() {
        for cancel in [false, true] {
            let (_dir, context, listener) = socket_context();
            let handle = handle();
            let found = handle.clone();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                let request_id = match request {
                    Request::CommandStatus { request_id, .. }
                    | Request::CommandCancel { request_id, .. } => id(&request_id).unwrap(),
                    _ => panic!("wrong operation"),
                };
                reply(
                    &mut stream,
                    response_result(
                        &found,
                        request_id,
                        if cancel { Op::Cancel } else { Op::Status },
                        Err(wire::WireError {
                            code: wire::ErrorCode::Busy,
                            detail: None,
                        }),
                    ),
                )
                .await;
                let (mut stream, _) = listener.accept().await.unwrap();
                assert!(matches!(
                    read_request(&mut stream).await,
                    Request::CommandConsume { .. }
                ));
                reply(
                    &mut stream,
                    LocalResult::Consumed {
                        stdout: 0,
                        stderr: 0,
                        finish_request_id: None,
                        finish_confirmed: false,
                    },
                )
                .await;
            });
            assert!(matches!(
                context.read_operation_inner(&handle, cancel).await,
                Err(Error::Invalid("busy"))
            ));
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn poll_not_found_before_authenticated_acceptance_is_retryable() {
        for authenticated in [false, true] {
            let (_dir, context, listener) = socket_context();
            let handle = handle();
            let request_id = fresh_id().unwrap();
            let answer = response_result(
                &handle,
                request_id,
                Op::Poll,
                Err(wire::WireError {
                    code: wire::ErrorCode::NotFound,
                    detail: None,
                }),
            );
            let LocalResult::Answer {
                response_sha256, ..
            } = &answer
            else {
                unreachable!()
            };
            let server = tokio::spawn(serve_replies(
                listener,
                vec![
                    LocalResult::Entries {
                        entries: vec![crate::command_local::AnswerEntry {
                            request_id: hex::encode(request_id),
                            op: Op::Poll,
                            sha256: response_sha256.clone(),
                        }],
                    },
                    answer,
                    LocalResult::Cursors {
                        stdout: 0,
                        stderr: 0,
                    },
                    LocalResult::Consumed {
                        stdout: 0,
                        stderr: 0,
                        finish_request_id: None,
                        finish_confirmed: false,
                    },
                ],
            ));
            let mut progress = WaitProgress {
                authenticated,
                ..Default::default()
            };
            if authenticated {
                assert!(matches!(
                    context
                        .wait_inner(
                            &handle,
                            Instant::now() + Duration::from_secs(2),
                            false,
                            false
                        )
                        .await,
                    Err(Error::Invalid("not_found"))
                ));
            } else {
                assert!(
                    context
                        .recover(
                            &handle,
                            Instant::now() + Duration::from_secs(2),
                            false,
                            &mut progress
                        )
                        .await
                        .unwrap()
                        .0
                        .is_none()
                );
            }
            assert_eq!(server.await.unwrap().len(), 4);
        }
    }
    #[test]
    fn warning_and_terminal_categories_preserve_remote_outcomes() {
        let mut value = status(fresh_id().unwrap());
        value.warning = Some(wire::Warning::StdinUnconsumed);
        assert_eq!(
            terminal_exit(
                &value,
                &Cursors {
                    stdout: 3,
                    stderr: 2
                }
            ),
            Some(7)
        );
        for (state, category) in [
            (State::UnknownLaunch, "unknown_launch"),
            (State::Lost, "lost"),
        ] {
            value.state = state;
            assert_eq!(status_problem(&value), Some(category));
            assert_eq!(report_error(&invalid(category)), 254);
        }
        value.state = State::Exited;
        value.warning = Some(wire::Warning::InputFailed);
        assert_eq!(status_problem(&value), Some("input_failed"));
    }
    #[tokio::test]
    async fn finish_retries_exact_durable_id_and_consumes_only_authenticated_answer() {
        let (_dir, context, listener) = socket_context();
        let handle = handle();
        let request_id = fresh_id().unwrap();
        let response = wire::Response {
            binding: Binding {
                session: handle.session(),
                client: [1; 32],
                server: handle.remote(),
            },
            id: request_id,
            op: Op::Finish,
            request_sha256: [9; 32],
            result: Ok(ResponseResult::Finish { finished: true }),
        };
        let bytes = wire::encode_response(&response).unwrap();
        let digest = hex::encode(wire::request_digest(&bytes));
        let server = tokio::spawn(serve_replies(
            listener,
            vec![
                LocalResult::RemotePending {
                    handle: handle.clone(),
                    request_id: hex::encode(request_id),
                },
                LocalResult::Answer {
                    handle: handle.clone(),
                    request_id: hex::encode(request_id),
                    response_base64: STANDARD.encode(bytes),
                    response_sha256: digest.clone(),
                },
                LocalResult::Consumed {
                    stdout: 3,
                    stderr: 2,
                    finish_request_id: None,
                    finish_confirmed: true,
                },
            ],
        ));
        assert!(
            context
                .finish_delivery(&handle, request_id, Instant::now() + Duration::from_secs(4))
                .await
                .unwrap()
        );
        let requests = server.await.unwrap();
        for request in &requests[..2] {
            assert!(
                matches!(request, Request::CommandResponse { request_id: actual,.. } if actual==&hex::encode(request_id))
            );
        }
        assert!(
            matches!(&requests[2], Request::CommandConsume { request_id: actual, response_sha256, expected_cursors:None,.. }
            if actual==&hex::encode(request_id) && response_sha256==&digest)
        );
    }
    #[tokio::test]
    async fn selector_pages_all_entries_and_latest_is_explicit() {
        let (_dir, context, listener) = socket_context();
        let first = handle();
        let second = handle();
        let entry = |handle, cursor| InvocationEntry {
            handle,
            cursor,
            created: 1,
            admission: wire::Admission::Accepted,
            state: None,
            authority_valid: true,
            refusal: None,
        };
        let server = tokio::spawn(serve_replies(
            listener,
            vec![
                LocalResult::Invocations {
                    entries: vec![entry(first, 1)],
                    next: Some(1),
                },
                LocalResult::Invocations {
                    entries: vec![entry(second.clone(), 8)],
                    next: None,
                },
            ],
        ));
        let selected = context
            .select(Selection {
                selector: Selector::Latest,
                remote: None,
            })
            .await
            .unwrap();
        assert!(selected == second);
        let requests = server.await.unwrap();
        assert!(matches!(
            requests[0],
            Request::CommandInvocations {
                after: None,
                limit: 64
            }
        ));
        assert!(matches!(
            requests[1],
            Request::CommandInvocations {
                after: Some(1),
                limit: 64
            }
        ));
    }
    #[test]
    fn handle_file_requires_private_owned_regular_nonsymlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handle");
        let fifo = dir.path().join("fifo");
        let cpath = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let began = std::time::Instant::now();
        assert!(read_handle(fifo).is_err());
        assert!(began.elapsed() < Duration::from_secs(1));
        let original = handle();
        std::fs::write(&path, original.encode().unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_handle(path.clone()).unwrap() == original);
        let link = dir.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(read_handle(link).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_handle(path.clone()).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, vec![b' '; 1025]).unwrap();
        assert!(read_handle(path).is_err());
    }
}
