//! PID-bound local acceptance proofs. Artifacts cannot construct cleanup authority.
use super::records;
use crate::{Error, Result, ipc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    process::Child,
    time::Instant,
};
fn bad() -> Error {
    Error::Invalid("installed_cleanup_proof")
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    active: u32,
    uncertain: u32,
    worker_running: u32,
    worker_unknown: u32,
    engine_present: bool,
    engine_quiesced: bool,
    observation_pending: bool,
    cleanup: String,
    #[serde(deserialize_with = "required_nullable")]
    final_seq: Option<u64>,
}
impl Status {
    fn fence(&self) -> Result<u64> {
        if !self.engine_present
            || self.active != 0
            || self.uncertain != 0
            || self.worker_running != 0
            || self.worker_unknown != 0
            || !self.engine_quiesced
            || self.observation_pending
            || self.cleanup != "quiesced"
        {
            return Err(bad());
        }
        self.final_seq.filter(|v| *v > 0).ok_or_else(bad)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stream {
    produced: u64,
    closed: bool,
    gaps: u32,
    #[serde(deserialize_with = "required_nullable")]
    sha256: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    cursor: u64,
    principal: String,
    group_sha256: String,
    session: String,
    invocation: String,
    submit_sha256: String,
    #[serde(deserialize_with = "required_nullable")]
    engine_admitted: Option<u8>,
    #[serde(deserialize_with = "required_nullable")]
    refusal: Option<crate::command_wire::ErrorCode>,
    #[serde(deserialize_with = "required_nullable")]
    outcome: Option<crate::command_wire::Outcome>,
    stdout: Stream,
    stderr: Stream,
    authority_valid: bool,
    worker_state: crate::journal::WorkerState,
    observed_seq: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    rows: Vec<Evidence>,
    #[serde(deserialize_with = "required_nullable")]
    next: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Queued {
    queued: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stop {
    schema: u8,
    engine_quiesced: bool,
    engine_stop_complete: bool,
    mdk_stop_complete: bool,
    runtime_cleanup_complete: bool,
}
fn live(child: &mut Child) -> Result<u32> {
    if child.try_wait()?.is_some() {
        return Err(bad());
    }
    child.id().ok_or_else(bad)
}
fn peer(stream: &UnixStream, pid: u32) -> Result<()> {
    let cred = stream.peer_cred()?;
    if cred.uid() != unsafe { libc::geteuid() } || cred.pid() != Some(pid as _) {
        return Err(bad());
    }
    Ok(())
}
async fn response(
    child: &mut Child,
    socket: &Path,
    request: &ipc::Request,
    deadline: Instant,
) -> Result<ipc::Response> {
    let pid = live(child)?;
    let result = tokio::time::timeout_at(
        deadline.min(Instant::now() + Duration::from_secs(15)),
        async {
            let mut stream = UnixStream::connect(socket).await?;
            peer(&stream, pid)?;
            let bytes = serde_json::to_vec(request).map_err(|_| bad())?;
            if bytes.len() > 65536 {
                return Err(bad());
            }
            stream.write_u32(bytes.len() as u32).await?;
            stream.write_all(&bytes).await?;
            let size = stream.read_u32().await? as usize;
            if size == 0 || size > 65536 {
                return Err(bad());
            }
            let mut bytes = vec![0; size];
            stream.read_exact(&mut bytes).await?;
            let response: ipc::Response = serde_json::from_slice(&bytes).map_err(|_| bad())?;
            if !response.ok {
                return Err(match response.error.as_deref() {
                    Some("busy") => Error::Storage("installed_admin_busy"),
                    Some("server_draining") => Error::Storage("installed_admin_draining"),
                    _ => bad(),
                });
            }
            if response.error.is_some() {
                return Err(bad());
            }
            Ok(response)
        },
    )
    .await
    .map_err(|_| Error::Timeout)?;
    live(child)?;
    result
}
pub(super) fn required_nullable<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
pub(super) async fn request<T: DeserializeOwned>(
    child: &mut Child,
    socket: &Path,
    request: &ipc::Request,
    deadline: Instant,
) -> Result<T> {
    let response = response(child, socket, request, deadline).await?;
    if response.command.is_some() {
        return Err(bad());
    }
    serde_json::from_value(response.result.ok_or_else(bad)?).map_err(|_| bad())
}
pub(super) async fn command_request(
    child: &mut Child,
    socket: &Path,
    request: &ipc::Request,
    deadline: Instant,
) -> Result<crate::command_local::LocalResult> {
    let response = response(child, socket, request, deadline).await?;
    if response.result.is_some() {
        return Err(bad());
    }
    response.command.ok_or_else(bad)
}
/// Absence only; it never grants authority to remove an existing engine tree.
pub(super) fn require_no_engine(profile: &Path) -> Result<()> {
    crate::identity::check_dir(profile)?;
    match std::fs::symlink_metadata(profile.join("commands")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(bad()),
    }
}
fn no_engine_status(status: &Status) -> Result<()> {
    if status.engine_present
        || status.active != 0
        || status.uncertain != 0
        || status.worker_running != 0
        || status.worker_unknown != 0
        || status.engine_quiesced
        || status.observation_pending
        || status.cleanup != "open"
        || status.final_seq.is_some()
    {
        return Err(bad());
    }
    Ok(())
}
/// Only fresh no-grant profiles without a previous engine receipt may use this.
/// Caller must repeat immediately before normal joined stop, then check absence.
pub(super) async fn verify_no_engine(
    child: &mut Child,
    profile: &Path,
    socket: &Path,
    deadline: Instant,
) -> Result<()> {
    require_no_engine(profile)?;
    let status: Status = request(
        child,
        socket,
        &ipc::Request::CommandCleanupStatus {},
        deadline,
    )
    .await?;
    no_engine_status(&status)?;
    let page: Page = request(
        child,
        socket,
        &ipc::Request::CommandEvidence {
            after: None,
            limit: 64,
        },
        deadline,
    )
    .await?;
    if !page.rows.is_empty() || page.next.is_some() {
        return Err(bad());
    }
    let status: Status = request(
        child,
        socket,
        &ipc::Request::CommandCleanupStatus {},
        deadline,
    )
    .await?;
    no_engine_status(&status)?;
    require_no_engine(profile)
}
/// Retains the original owned directory inode; grants no deletion authority.
pub(super) struct ProfileReceipt(std::fs::File);
impl ProfileReceipt {
    pub(super) fn capture(profile: &Path) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        crate::identity::check_dir(profile)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(profile)?;
        let receipt = Self(file);
        receipt.verify(profile)?;
        Ok(receipt)
    }
    pub(super) fn verify(&self, profile: &Path) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        crate::identity::check_dir(profile)?;
        let held = self.0.metadata()?;
        let current = std::fs::symlink_metadata(profile)?;
        if !current.is_dir() || held.dev() != current.dev() || held.ino() != current.ino() {
            return Err(bad());
        }
        Ok(())
    }
}
pub(super) async fn client_group_submit(
    child: &mut Child,
    socket: &Path,
    handle: &crate::command_client::CommandHandle,
    deadline: Instant,
) -> Result<String> {
    use crate::command_local::LocalResult;
    let mut after = None;
    let mut count = 0;
    let mut found = None;
    loop {
        let LocalResult::Invocations { entries, next } = command_request(
            child,
            socket,
            &ipc::Request::CommandInvocations { after, limit: 64 },
            deadline,
        )
        .await?
        else {
            return Err(bad());
        };
        if entries.len() > 64 {
            return Err(bad());
        }
        let mut previous = after.unwrap_or(0);
        for entry in entries {
            count += 1;
            if count > 4096 || entry.cursor <= previous {
                return Err(bad());
            }
            previous = entry.cursor;
            track_client_entry(&entry, handle, &mut found)?;
        }
        if next.is_some_and(|n| n != previous || Some(n) == after || previous == after.unwrap_or(0))
        {
            return Err(bad());
        }
        if next.is_none() {
            return found.ok_or_else(bad);
        }
        after = next;
    }
}
fn track_client_entry(
    entry: &crate::command_local::InvocationEntry,
    handle: &crate::command_client::CommandHandle,
    found: &mut Option<String>,
) -> Result<()> {
    if !records::hexadecimal(&entry.group_submit_sha256, 64)
        || (entry.handle == *handle
            && (!entry.authority_valid
                || found.replace(entry.group_submit_sha256.clone()).is_some()))
    {
        return Err(bad());
    }
    Ok(())
}
#[cfg(test)]
fn evidence(
    page: &Page,
    after: Option<u64>,
    fence: u64,
    actual: &mut BTreeMap<(String, String), (String, String)>,
) -> Result<()> {
    evidence_expected(page, after, fence, actual, None)
}
#[derive(Clone, Copy)]
pub(super) enum ExpectedDisposition {
    ExecutedFixture,
    Refused(crate::command_wire::ErrorCode),
}
pub(super) type ExpectedRows = BTreeMap<(String, String), ((String, String), ExpectedDisposition)>;
fn disposition_valid(row: &Evidence, disposition: ExpectedDisposition) -> bool {
    match disposition {
        ExpectedDisposition::ExecutedFixture => {
            row.worker_state == crate::journal::WorkerState::Exited
                && row.refusal.is_none()
                && row.engine_admitted == Some(1)
                && row.outcome.as_ref().is_some_and(|o| {
                    o.value == Some(7) && o.kind == crate::command_wire::OutcomeKind::Code
                })
                && row.stdout.closed
                && row.stderr.closed
                && row.stdout.gaps == 0
                && row.stderr.gaps == 0
                && row.stdout.produced == 131072
                && row.stderr.produced == 131072
        }
        ExpectedDisposition::Refused(code) => {
            code == crate::command_wire::ErrorCode::CommandDenied
                && row.refusal == Some(code)
                && row.engine_admitted == Some(0)
                && row.worker_state == crate::journal::WorkerState::Unknown
                && row.outcome.is_none()
                && [&row.stdout, &row.stderr].into_iter().all(|stream| {
                    stream.produced == 0
                        && stream.gaps == 0
                        && !stream.closed
                        && stream.sha256.is_none()
                })
        }
    }
}
fn evidence_expected(
    page: &Page,
    after: Option<u64>,
    fence: u64,
    actual: &mut BTreeMap<(String, String), (String, String)>,
    expected: Option<&ExpectedRows>,
) -> Result<()> {
    if page.rows.len() > 64 {
        return Err(bad());
    }
    let mut previous = after.unwrap_or(0);
    for row in &page.rows {
        if row.cursor <= previous
            || !records::hexadecimal(&row.principal, 64)
            || !records::hexadecimal(&row.invocation, 32)
            || !records::hexadecimal(&row.session, 32)
            || !records::hexadecimal(&row.group_sha256, 64)
            || !records::hexadecimal(&row.submit_sha256, 64)
            || !row.authority_valid
            || !disposition_valid(
                row,
                match expected {
                    Some(rows) => {
                        rows.get(&(row.principal.clone(), row.invocation.clone()))
                            .ok_or_else(bad)?
                            .1
                    }
                    None => ExpectedDisposition::ExecutedFixture,
                },
            )
            || row.observed_seq < fence
            || row
                .stdout
                .sha256
                .as_ref()
                .is_some_and(|h| !records::hexadecimal(h, 64))
            || row
                .stderr
                .sha256
                .as_ref()
                .is_some_and(|h| !records::hexadecimal(h, 64))
            || actual
                .insert(
                    (row.principal.clone(), row.invocation.clone()),
                    (row.session.clone(), row.submit_sha256.clone()),
                )
                .is_some()
        {
            return Err(bad());
        }
        previous = row.cursor;
    }
    if page
        .next
        .is_some_and(|n| n != previous || page.rows.is_empty())
    {
        return Err(bad());
    }
    Ok(())
}
/// Read-only terminal fixture bindings, never cleanup authority. Recovery admits
/// exactly two jobs; a third row or a continuation is an explicit failure.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecoveryRow {
    pub principal: String,
    pub invocation: String,
    pub session: String,
    pub group_sha256: String,
    pub submit_sha256: String,
}
pub(super) async fn recovery_rows(
    child: &mut Child,
    socket: &Path,
    deadline: Instant,
) -> Result<Vec<RecoveryRow>> {
    let page: Page = request(
        child,
        socket,
        &ipc::Request::CommandEvidence {
            after: None,
            limit: 64,
        },
        deadline,
    )
    .await?;
    if page.rows.len() > 2 || page.next.is_some() {
        return Err(bad());
    }
    let mut actual = BTreeMap::new();
    evidence_expected(&page, None, 0, &mut actual, None)?;
    Ok(page
        .rows
        .into_iter()
        .map(|r| RecoveryRow {
            principal: r.principal,
            invocation: r.invocation,
            session: r.session,
            group_sha256: r.group_sha256,
            submit_sha256: r.submit_sha256,
        })
        .collect())
}
/// Private fields and constructor bind authority to the actual retained child.
/// Only the build-only driver can mint this after explicit actor cleanup proof.
pub(in crate::mdk_adapter) struct OwnedCleanupGuard<'a> {
    child: &'a mut Child,
    socket: PathBuf,
    profile: PathBuf,
    final_seq: u64,
    expected: BTreeMap<(String, String), (String, String)>,
    deadline: Instant,
    dispositions: Option<ExpectedRows>,
}
impl OwnedCleanupGuard<'_> {
    pub(super) fn final_seq(&self) -> u64 {
        self.final_seq
    }
    pub(in crate::mdk_adapter) fn profile(&self) -> &Path {
        &self.profile
    }
    async fn status(&mut self) -> Result<()> {
        let value: Status = request(
            self.child,
            &self.socket,
            &ipc::Request::CommandCleanupStatus {},
            self.deadline,
        )
        .await?;
        if value.fence()? != self.final_seq {
            return Err(bad());
        }
        Ok(())
    }
    pub(in crate::mdk_adapter) async fn revalidate(&mut self) -> Result<()> {
        crate::identity::check_dir(&self.profile)?;
        self.status().await?;
        let mut after = None;
        let mut count = 0;
        let mut actual = BTreeMap::new();
        loop {
            let page: Page = request(
                self.child,
                &self.socket,
                &ipc::Request::CommandEvidence { after, limit: 64 },
                self.deadline,
            )
            .await?;
            count += page.rows.len();
            if count > 4096 {
                return Err(bad());
            }
            evidence_expected(
                &page,
                after,
                self.final_seq,
                &mut actual,
                self.dispositions.as_ref(),
            )?;
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        if actual != self.expected {
            return Err(bad());
        }
        self.status().await
    }
    pub(in crate::mdk_adapter) async fn after_unlink(&mut self) -> Result<()> {
        self.status().await
    }
}
/// Mint only after this driver's own prepare request; never from a record's PID/fence.
pub(super) async fn prepare<'a>(
    child: &'a mut Child,
    profile: &Path,
    socket: &Path,
    expected: BTreeMap<(String, String), (String, String)>,
    deadline: Instant,
) -> Result<OwnedCleanupGuard<'a>> {
    prepare_inner(child, profile, socket, expected, None, deadline).await
}
pub(super) async fn prepare_policy<'a>(
    child: &'a mut Child,
    profile: &Path,
    socket: &Path,
    rows: ExpectedRows,
    deadline: Instant,
) -> Result<OwnedCleanupGuard<'a>> {
    if rows.len() != 5
        || rows
            .values()
            .filter(|(_, d)| matches!(d, ExpectedDisposition::ExecutedFixture))
            .count()
            != 1
    {
        return Err(bad());
    }
    let expected = rows
        .iter()
        .map(|(k, (binding, _))| (k.clone(), binding.clone()))
        .collect();
    prepare_inner(child, profile, socket, expected, Some(rows), deadline).await
}
async fn prepare_inner<'a>(
    child: &'a mut Child,
    profile: &Path,
    socket: &Path,
    expected: BTreeMap<(String, String), (String, String)>,
    dispositions: Option<ExpectedRows>,
    deadline: Instant,
) -> Result<OwnedCleanupGuard<'a>> {
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        match request::<Queued>(
            child,
            socket,
            &ipc::Request::CommandPrepareCleanup {},
            deadline,
        )
        .await
        {
            Ok(q) if q.queued => break,
            Err(Error::Storage("installed_admin_busy")) => {}
            Err(Error::Timeout) => break,
            _ => return Err(bad()),
        }
        tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(50))).await;
    }
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let value: Status = request(
            child,
            socket,
            &ipc::Request::CommandCleanupStatus {},
            deadline,
        )
        .await?;
        match value.fence() {
            Ok(final_seq) => {
                let mut guard = OwnedCleanupGuard {
                    child,
                    socket: socket.to_owned(),
                    profile: profile.to_owned(),
                    final_seq,
                    expected,
                    deadline,
                    dispositions,
                };
                guard.revalidate().await?;
                return Ok(guard);
            }
            Err(_) if value.cleanup == "draining" => {}
            _ => return Err(bad()),
        }
        tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(50))).await;
    }
}
/// Normal Stop preserves workers. It cannot itself authorize deletion.
pub(super) async fn stop(child: &mut Child, deadline: Instant) -> Result<()> {
    let pid = live(child)?;
    if unsafe { libc::kill(pid as i32, libc::SIGTERM) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let status = match tokio::time::timeout_at(deadline, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            // The retained owned leader fences the process group; never use an artifact PID.
            tokio::time::timeout(Duration::from_secs(5), super::process::terminate(child))
                .await
                .map_err(|_| Error::Timeout)??;
            return Err(Error::Timeout);
        }
    };
    if !status.success() {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn stop_report(stdout: &[u8]) -> Result<()> {
    check_stop_report(stdout, true)
}
pub(super) fn no_engine_stop_report(stdout: &[u8]) -> Result<()> {
    check_stop_report(stdout, false)
}
fn check_stop_report(stdout: &[u8], engine_quiesced: bool) -> Result<()> {
    let mut lines = stdout.split(|b| *b == b'\n').filter(|l| !l.is_empty());
    let report: Stop = serde_json::from_slice(lines.next().ok_or_else(bad)?).map_err(|_| bad())?;
    if lines.next().is_some()
        || report.schema != 1
        || report.engine_quiesced != engine_quiesced
        || !report.engine_stop_complete
        || !report.mdk_stop_complete
        || !report.runtime_cleanup_complete
    {
        return Err(bad());
    }
    Ok(())
}

/// Public evidence is checked through the same closed parser as live cleanup, but
/// never creates a guard. The original client digest remains the expected value.
pub(super) fn validate_policy_evidence(
    rows: &[serde_json::Value],
    fence: u64,
    expected: &ExpectedRows,
) -> Result<()> {
    if rows.len() != 5 || fence == 0 {
        return Err(bad());
    }
    let page: Page = serde_json::from_value(serde_json::json!({"rows": rows, "next": null}))
        .map_err(|_| bad())?;
    let mut actual = BTreeMap::new();
    evidence_expected(&page, None, fence, &mut actual, Some(expected))?;
    let wanted: BTreeMap<_, _> = expected
        .iter()
        .map(|(k, (v, _))| (k.clone(), v.clone()))
        .collect();
    if actual != wanted
        || page
            .rows
            .iter()
            .any(|r| r.group_sha256 != page.rows[0].group_sha256)
    {
        return Err(bad());
    }
    Ok(())
}
pub(super) async fn policy_evidence(
    child: &mut Child,
    socket: &Path,
    fence: u64,
    expected: &ExpectedRows,
    deadline: Instant,
) -> Result<Vec<serde_json::Value>> {
    let mut after = None;
    let mut rows = Vec::new();
    loop {
        let page: Page = request(
            child,
            socket,
            &ipc::Request::CommandEvidence { after, limit: 64 },
            deadline,
        )
        .await?;
        let mut previous = after.unwrap_or(0);
        for row in page.rows {
            if row.cursor <= previous || rows.len() >= 5 {
                return Err(bad());
            }
            previous = row.cursor;
            rows.push(serde_json::to_value(row).map_err(|_| bad())?);
        }
        if page.next.is_some_and(|n| n != previous || Some(n) == after) {
            return Err(bad());
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    validate_policy_evidence(&rows, fence, expected)?;
    Ok(rows)
}
/// Only cancel the one independently known possibly-executed policy invocation.
/// Refusals and unknown ledger entries never acquire cancellation authority.
pub(super) async fn cancel_policy_execution(
    child: &mut Child,
    socket: &Path,
    expected: &ExpectedRows,
    deadline: Instant,
) -> Result<()> {
    let mut after = None;
    let mut count = 0;
    let mut found = None;
    loop {
        let page: Page = request(
            child,
            socket,
            &ipc::Request::CommandEvidence { after, limit: 64 },
            deadline,
        )
        .await?;
        let mut previous = after.unwrap_or(0);
        for row in page.rows {
            count += 1;
            if count > 5 || row.cursor <= previous {
                return Err(bad());
            }
            previous = row.cursor;
            let (binding, disposition) = expected
                .get(&(row.principal.clone(), row.invocation.clone()))
                .ok_or_else(bad)?;
            if binding != &(row.session.clone(), row.submit_sha256.clone()) {
                return Err(bad());
            }
            if matches!(disposition, ExpectedDisposition::ExecutedFixture)
                && found.replace(row.cursor).is_some()
            {
                return Err(bad());
            }
        }
        if page.next.is_some_and(|n| n != previous || Some(n) == after) {
            return Err(bad());
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    if let Some(cursor) = found {
        let queued: Queued = request(
            child,
            socket,
            &ipc::Request::CommandLocalCancel { cursor },
            deadline,
        )
        .await?;
        if !queued.queued {
            return Err(bad());
        }
    }
    Ok(())
}

/// Best-effort explicit fixture cancellation, only actor-owned ledger cursors.
pub(super) async fn cancel_fixture(
    child: &mut Child,
    socket: &Path,
    expected: &BTreeMap<(String, String), (String, String)>,
    max_rows: usize,
    deadline: Instant,
) -> Result<()> {
    if max_rows == 0 || max_rows > 4 {
        return Err(bad());
    }
    let mut after = None;
    let mut count = 0;
    loop {
        let page: Page = request(
            child,
            socket,
            &ipc::Request::CommandEvidence { after, limit: 64 },
            deadline,
        )
        .await?;
        for row in &page.rows {
            count += 1;
            if count > max_rows {
                return Err(bad());
            }
            if !expected.contains_key(&(row.principal.clone(), row.invocation.clone())) {
                continue;
            }
            let _: Queued = request(
                child,
                socket,
                &ipc::Request::CommandLocalCancel { cursor: row.cursor },
                deadline,
            )
            .await?;
        }
        after = page.next;
        if after.is_none() {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stop_timeout_kills_and_reaps_owned_group_and_stays_red() {
        use tokio::io::AsyncReadExt;
        let root = tempfile::tempdir().unwrap();
        let env = super::super::process::Environment {
            home: root.path().into(),
            temporary: root.path().into(),
            utilities: root.path().into(),
        };
        let mut child = super::super::process::command(
            Path::new("/bin/sh"),
            &[
                "-c".into(),
                "trap '' TERM; printf ready; while :; do /bin/sleep 1; done".into(),
            ],
            root.path(),
            &env,
        )
        .spawn()
        .unwrap();
        let mut out = child.stdout.take().unwrap();
        let mut ready = [0; 5];
        tokio::time::timeout(Duration::from_secs(5), out.read_exact(&mut ready))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&ready, b"ready");
        assert!(matches!(
            stop(&mut child, Instant::now() + Duration::from_millis(20)).await,
            Err(Error::Timeout)
        ));
        assert!(child.try_wait().unwrap().is_some());
        let mut rest = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), out.read_to_end(&mut rest))
            .await
            .unwrap()
            .unwrap();
    }
    fn row() -> serde_json::Value {
        serde_json::json!({"cursor":1,"principal":"a".repeat(64),"group_sha256":"b".repeat(64),"session":"c".repeat(32),"invocation":"d".repeat(32),"submit_sha256":"e".repeat(64),"engine_admitted":1,"refusal":null,"outcome":{"kind":"code","value":7},"stdout":{"produced":131072,"closed":true,"gaps":0,"sha256":null},"stderr":{"produced":131072,"closed":true,"gaps":0,"sha256":null},"authority_valid":true,"worker_state":"exited","observed_seq":9})
    }
    #[test]
    fn actor_evidence_requires_fresh_exact_terminal_fixture_facts() {
        let r = row();
        let p: Page = serde_json::from_value(serde_json::json!({"rows":[r],"next":null})).unwrap();
        let mut actual = BTreeMap::new();
        evidence(&p, None, 9, &mut actual).unwrap();
        assert_eq!(
            actual.get(&("a".repeat(64), "d".repeat(32))),
            Some(&("c".repeat(32), "e".repeat(64)))
        );
        for (key, value) in [
            ("authority_valid", false.into()),
            ("engine_admitted", 0.into()),
            ("observed_seq", 8.into()),
            ("worker_state", "running".into()),
        ] {
            let mut r = row();
            r[key] = value;
            let p: Page =
                serde_json::from_value(serde_json::json!({"rows":[r],"next":null})).unwrap();
            assert!(evidence(&p, None, 9, &mut BTreeMap::new()).is_err());
        }
        let p: Page =
            serde_json::from_value(serde_json::json!({"rows":[row(),row()],"next":null})).unwrap();
        assert!(evidence(&p, None, 9, &mut BTreeMap::new()).is_err());
    }
    #[test]
    fn profile_receipt_rejects_replaced_directory_and_symlink() {
        use std::os::unix::fs::{DirBuilderExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("client");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&profile)
            .unwrap();
        let receipt = ProfileReceipt::capture(&profile).unwrap();
        receipt.verify(&profile).unwrap();
        std::fs::rename(&profile, root.path().join("original")).unwrap();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&profile)
            .unwrap();
        assert!(receipt.verify(&profile).is_err());
        std::fs::remove_dir(&profile).unwrap();
        symlink(root.path().join("original"), &profile).unwrap();
        assert!(receipt.verify(&profile).is_err());
    }
    #[test]
    fn maximal_client_invocation_page_fits_actual_ipc_frame() {
        use crate::{
            command_local::{InvocationEntry, LocalResult},
            command_wire,
        };
        let handle = serde_json::json!({"format":1,"profile":"a".repeat(64),"remote":"b".repeat(64),"session":"c".repeat(32),"invocation":"d".repeat(32),"submit_sha256":"e".repeat(64)});
        let entry: InvocationEntry = serde_json::from_value(serde_json::json!({"handle":handle,"cursor":u64::MAX,"created":u64::MAX,"admission":"accepted","state":"unknown_launch","authority_valid":true,"refusal":"noncanonical_executable","group_submit_sha256":"f".repeat(64)})).unwrap();
        let bytes = serde_json::to_vec(&ipc::Response::command(LocalResult::Invocations {
            entries: vec![entry; 64],
            next: Some(u64::MAX),
        }))
        .unwrap();
        assert!(bytes.len() <= 65536);
        let response: ipc::Response = serde_json::from_slice(&bytes).unwrap();
        assert!(
            matches!(response.command, Some(LocalResult::Invocations {entries,next:Some(u64::MAX)}) if entries.len()==64 && entries[0].refusal==Some(command_wire::ErrorCode::NoncanonicalExecutable))
        );
    }
    #[test]
    fn client_digest_selection_requires_exact_complete_handle_and_cached_projection() {
        use crate::{command_client::CommandHandle, command_local::InvocationEntry};
        let h = serde_json::json!({"format":1,"profile":"client","remote":"a".repeat(64),"session":"b".repeat(32),"invocation":"c".repeat(32),"submit_sha256":"d".repeat(64)});
        let handle: CommandHandle = serde_json::from_value(h.clone()).unwrap();
        let value = serde_json::json!({"handle":h,"cursor":1,"created":1,"admission":"accepted","state":null,"authority_valid":true,"refusal":null,"group_submit_sha256":"e".repeat(64)});
        let entry: InvocationEntry = serde_json::from_value(value.clone()).unwrap();
        let mut found = None;
        track_client_entry(&entry, &handle, &mut found).unwrap();
        assert_eq!(found, Some("e".repeat(64)));
        assert!(track_client_entry(&entry, &handle, &mut found).is_err());
        for (key, v) in [
            ("profile", "other".to_string()),
            ("remote", "1".repeat(64)),
            ("session", "2".repeat(32)),
            ("invocation", "3".repeat(32)),
            ("submit_sha256", "4".repeat(64)),
        ] {
            let mut changed = value.clone();
            changed["handle"][key] = v.into();
            let e: InvocationEntry = serde_json::from_value(changed).unwrap();
            let mut found = None;
            track_client_entry(&e, &handle, &mut found).unwrap();
            assert!(found.is_none());
        }
        let mut changed = value.clone();
        changed["authority_valid"] = false.into();
        assert!(
            track_client_entry(
                &serde_json::from_value(changed).unwrap(),
                &handle,
                &mut None
            )
            .is_err()
        );
        let mut changed = value.clone();
        changed["group_submit_sha256"] = "E".repeat(64).into();
        assert!(
            track_client_entry(
                &serde_json::from_value(changed).unwrap(),
                &handle,
                &mut None
            )
            .is_err()
        );
        let mut changed = value;
        changed
            .as_object_mut()
            .unwrap()
            .remove("group_submit_sha256");
        assert!(serde_json::from_value::<InvocationEntry>(changed).is_err());
    }
    #[test]
    fn no_engine_proof_is_explicit_and_nullable_fields_are_required() {
        let value = serde_json::json!({"active":0,"uncertain":0,"worker_running":0,"worker_unknown":0,"engine_present":false,"engine_quiesced":false,"observation_pending":false,"cleanup":"open","final_seq":null});
        no_engine_status(&serde_json::from_value(value.clone()).unwrap()).unwrap();
        for (key, v) in [
            ("engine_present", true.into()),
            ("active", 1.into()),
            ("uncertain", 1.into()),
            ("worker_running", 1.into()),
            ("worker_unknown", 1.into()),
            ("engine_quiesced", true.into()),
            ("observation_pending", true.into()),
            ("final_seq", 1.into()),
            ("cleanup", "quiesced".into()),
        ] {
            let mut changed = value.clone();
            changed[key] = v;
            assert!(no_engine_status(&serde_json::from_value(changed).unwrap()).is_err());
        }
        for key in ["engine_present", "final_seq"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<Status>(missing).is_err());
        }
        let mut missing = row();
        missing.as_object_mut().unwrap().remove("refusal");
        assert!(serde_json::from_value::<Evidence>(missing).is_err());
        let mut missing = row();
        missing["stdout"].as_object_mut().unwrap().remove("sha256");
        assert!(serde_json::from_value::<Evidence>(missing).is_err());
        assert!(serde_json::from_value::<Page>(serde_json::json!({"rows":[]})).is_err());
    }
    #[test]
    fn refused_policy_rows_require_exact_non_execution_facts() {
        let mut value = row();
        value["engine_admitted"] = 0.into();
        value["refusal"] = "command_denied".into();
        value["outcome"] = serde_json::Value::Null;
        value["worker_state"] = "unknown".into();
        for name in ["stdout", "stderr"] {
            value[name] = serde_json::json!({"produced":0,"closed":false,"gaps":0,"sha256":null});
        }
        let disposition =
            ExpectedDisposition::Refused(crate::command_wire::ErrorCode::CommandDenied);
        let row: Evidence = serde_json::from_value(value.clone()).unwrap();
        assert!(disposition_valid(&row, disposition));
        assert!(!disposition_valid(
            &row,
            ExpectedDisposition::ExecutedFixture
        ));
        for (field, replacement) in [
            ("engine_admitted", 1.into()),
            ("refusal", "not_found".into()),
            ("worker_state", "exited".into()),
            ("outcome", serde_json::json!({"kind":"code","value":7})),
        ] {
            let mut changed = value.clone();
            changed[field] = replacement;
            assert!(!disposition_valid(
                &serde_json::from_value(changed).unwrap(),
                disposition
            ));
        }
        for stream in ["stdout", "stderr"] {
            for (field, replacement) in [
                ("produced", 1.into()),
                ("closed", true.into()),
                ("gaps", 1.into()),
                ("sha256", "a".repeat(64).into()),
            ] {
                let mut changed = value.clone();
                changed[stream][field] = replacement;
                assert!(!disposition_valid(
                    &serde_json::from_value(changed).unwrap(),
                    disposition
                ));
            }
        }
    }
    #[test]
    fn no_engine_stop_report_is_separate_and_joins_are_mandatory() {
        let value = serde_json::json!({"schema":1,"engine_quiesced":false,"engine_stop_complete":true,"mdk_stop_complete":true,"runtime_cleanup_complete":true});
        let bytes = serde_json::to_vec(&value).unwrap();
        no_engine_stop_report(&bytes).unwrap();
        assert!(stop_report(&bytes).is_err());
        for key in [
            "engine_stop_complete",
            "mdk_stop_complete",
            "runtime_cleanup_complete",
        ] {
            let mut v = value.clone();
            v[key] = false.into();
            assert!(no_engine_stop_report(&serde_json::to_vec(&v).unwrap()).is_err());
        }
        let mut v = value;
        v["engine_quiesced"] = true.into();
        assert!(no_engine_stop_report(&serde_json::to_vec(&v).unwrap()).is_err());
        stop_report(&serde_json::to_vec(&v).unwrap()).unwrap();
    }
    #[test]
    fn no_engine_absence_rejects_directory_file_and_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        require_no_engine(root.path()).unwrap();
        let p = root.path().join("commands");
        std::fs::write(&p, b"unexpected").unwrap();
        assert!(require_no_engine(root.path()).is_err());
        std::fs::remove_file(&p).unwrap();
        symlink("missing", &p).unwrap();
        assert!(require_no_engine(root.path()).is_err());
        std::fs::remove_file(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
        assert!(require_no_engine(root.path()).is_err());
    }
    #[test]
    fn cleanup_fence_needs_explicit_quiescence_and_nonzero_freshness() {
        let mut value = serde_json::json!({"active":0,"uncertain":0,"worker_running":0,"worker_unknown":0,"engine_present":true,"engine_quiesced":true,"observation_pending":false,"cleanup":"quiesced","final_seq":3});
        assert_eq!(
            serde_json::from_value::<Status>(value.clone())
                .unwrap()
                .fence()
                .unwrap(),
            3
        );
        value["observation_pending"] = true.into();
        assert!(
            serde_json::from_value::<Status>(value.clone())
                .unwrap()
                .fence()
                .is_err()
        );
        value["observation_pending"] = false.into();
        value["final_seq"] = 0.into();
        assert!(
            serde_json::from_value::<Status>(value)
                .unwrap()
                .fence()
                .is_err()
        );
    }
}
