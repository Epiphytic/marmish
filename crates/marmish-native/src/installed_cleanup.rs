//! PID-bound local acceptance proofs. Artifacts cannot construct cleanup authority.
use super::records;
use crate::{Error, Result, ipc};
use serde::{Deserialize, de::DeserializeOwned};
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
    engine_quiesced: bool,
    observation_pending: bool,
    cleanup: String,
    final_seq: Option<u64>,
}
impl Status {
    fn fence(&self) -> Result<u64> {
        if self.active != 0
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stream {
    produced: u64,
    closed: bool,
    gaps: u32,
    sha256: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    cursor: u64,
    principal: String,
    group_sha256: String,
    session: String,
    invocation: String,
    submit_sha256: String,
    engine_admitted: Option<u8>,
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
pub(super) async fn request<T: DeserializeOwned>(
    child: &mut Child,
    socket: &Path,
    request: &ipc::Request,
    deadline: Instant,
) -> Result<T> {
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
            if response.error.is_some() || response.command.is_some() {
                return Err(bad());
            }
            serde_json::from_value(response.result.ok_or_else(bad)?).map_err(|_| bad())
        },
    )
    .await
    .map_err(|_| Error::Timeout)?;
    live(child)?;
    result
}
fn evidence(
    page: &Page,
    after: Option<u64>,
    fence: u64,
    actual: &mut BTreeMap<(String, String), (String, String)>,
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
            || row.worker_state != crate::journal::WorkerState::Exited
            || row.observed_seq < fence
            || row.engine_admitted != Some(1)
            || row.outcome.as_ref().is_none_or(|o| {
                o.value != Some(7) || o.kind != crate::command_wire::OutcomeKind::Code
            })
            || !row.stdout.closed
            || !row.stderr.closed
            || row.stdout.gaps != 0
            || row.stderr.gaps != 0
            || row.stdout.produced != 131072
            || row.stderr.produced != 131072
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
/// Private fields and constructor bind authority to the actual retained child.
/// Only the build-only driver can mint this after explicit actor cleanup proof.
pub(in crate::mdk_adapter) struct OwnedCleanupGuard<'a> {
    child: &'a mut Child,
    socket: PathBuf,
    profile: PathBuf,
    final_seq: u64,
    expected: BTreeMap<(String, String), (String, String)>,
    deadline: Instant,
}
impl OwnedCleanupGuard<'_> {
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
            evidence(&page, after, self.final_seq, &mut actual)?;
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
    let mut lines = stdout.split(|b| *b == b'\n').filter(|l| !l.is_empty());
    let report: Stop = serde_json::from_slice(lines.next().ok_or_else(bad)?).map_err(|_| bad())?;
    if lines.next().is_some()
        || report.schema != 1
        || !report.engine_quiesced
        || !report.engine_stop_complete
        || !report.mdk_stop_complete
        || !report.runtime_cleanup_complete
    {
        return Err(bad());
    }
    Ok(())
}

/// Best-effort explicit fixture cancellation, only actor-owned ledger cursors.
pub(super) async fn cancel_fixture(
    child: &mut Child,
    socket: &Path,
    expected: &BTreeMap<(String, String), (String, String)>,
    deadline: Instant,
) -> Result<()> {
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
            if count > 3 {
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
        serde_json::json!({"cursor":1,"principal":"a".repeat(64),"group_sha256":"b".repeat(64),"session":"c".repeat(32),"invocation":"d".repeat(32),"submit_sha256":"e".repeat(64),"engine_admitted":1,"outcome":{"kind":"code","value":7},"stdout":{"produced":131072,"closed":true,"gaps":0,"sha256":null},"stderr":{"produced":131072,"closed":true,"gaps":0,"sha256":null},"authority_valid":true,"worker_state":"exited","observed_seq":9})
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
    fn cleanup_fence_needs_explicit_quiescence_and_nonzero_freshness() {
        let mut value = serde_json::json!({"active":0,"uncertain":0,"worker_running":0,"worker_unknown":0,"engine_quiesced":true,"observation_pending":false,"cleanup":"quiesced","final_seq":3});
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
