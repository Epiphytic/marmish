//! A paused owned server cannot answer Hello and therefore cannot authorize Open.
use super::{
    cleanup, edge_fixture, edge_process::Started, edge_signals::Cohort, matrix, records,
    streaming_cleanup,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::{process::Child, sync::MutexGuard, time::Instant};
fn bad() -> Error {
    Error::Invalid("installed_edge_preopen")
}

/// Keeps the original unreaped Child and its mutex through synchronous resume.
struct Pause<'a, 'b> {
    child: MutexGuard<'a, &'b mut Child>,
    pid: i32,
    paused: bool,
}
impl<'a, 'b> Pause<'a, 'b> {
    fn start(mut child: MutexGuard<'a, &'b mut Child>) -> Result<Self> {
        if child.try_wait()?.is_some() {
            return Err(bad());
        }
        let pid = i32::try_from(child.id().ok_or_else(bad)?).map_err(|_| bad())?;
        if unsafe { libc::kill(pid, libc::SIGSTOP) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self {
            child,
            pid,
            paused: true,
        })
    }
    async fn wait_stopped(&self, deadline: Instant) -> Result<()> {
        loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            if unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid as u32,
                    &mut info,
                    libc::WSTOPPED | libc::WNOHANG | libc::WNOWAIT,
                )
            } != 0
            {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(error.into());
                }
            } else if unsafe { info.si_pid() } == self.pid {
                if info.si_code != libc::CLD_STOPPED || unsafe { info.si_status() } != libc::SIGSTOP
                {
                    return Err(bad());
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(5))).await;
        }
    }
    fn resume(&mut self) -> Result<()> {
        if self.paused {
            if unsafe { libc::kill(self.pid, libc::SIGCONT) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            self.paused = false;
        }
        if self.child.try_wait()?.is_some() {
            return Err(bad());
        }
        Ok(())
    }
}
impl Drop for Pause<'_, '_> {
    fn drop(&mut self) {
        if self.paused {
            unsafe {
                libc::kill(self.pid, libc::SIGCONT);
            }
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub wall_ns: u64,
    pub stream_sha256: String,
    pub session_sha256: String,
    pub group_sha256: String,
    pub local_exit: i32,
    pub queued_hello: bool,
    pub server_hello_only: bool,
    pub unchanged_previous_fences: bool,
}
impl Observation {
    pub(super) fn validate(&self) -> Result<()> {
        if self.wall_ns == 0
            || self.wall_ns > 180_000_000_000
            || self.local_exit != 255
            || !self.queued_hello
            || !self.server_hello_only
            || !self.unchanged_previous_fences
            || [
                &self.stream_sha256,
                &self.session_sha256,
                &self.group_sha256,
            ]
            .into_iter()
            .any(|v| !records::hexadecimal(v, 64))
        {
            return Err(bad());
        }
        Ok(())
    }
}
fn hello(e: &streaming_cleanup::Entry, c: &Cohort<'_, '_>, direction: &str) -> bool {
    e.direction == direction
        && e.client == c.identities[c.role].client_account
        && e.server == c.ready.server_account
        && e.state == "hello"
        && e.open_digest.is_none()
        && !e.cancel_requested
        && !e.active
        && e.terminal.is_none()
}
fn fences(entries: &[streaming_cleanup::Entry]) -> Result<Vec<u8>> {
    let mut entries: Vec<_> = entries
        .iter()
        .map(|e| (&e.stream, &e.open_digest, e.cancel_requested))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    records::canonical(&entries)
}
pub(super) async fn run(c: Cohort<'_, '_>) -> Result<Observation> {
    let start = Instant::now();
    let baseline =
        streaming_cleanup::entries(*c.server.daemon.lock().await, c.server.socket, c.deadline)
            .await?;
    streaming_cleanup::status(*c.server.daemon.lock().await, c.server.socket, c.deadline)
        .await?
        .validate_idle()?;
    let prior = fences(&baseline)?;
    if c.ready.stop_by.saturating_sub(super::now()?) < 200 {
        return Err(Error::Timeout);
    }
    let nonce = records::digest(&records::canonical(&(
        "installed-preopen/1",
        c.context.digest()?,
        c.role,
    ))?);
    let marker = edge_fixture::marker(c.server.workspace, &nonce)?;
    if marker.try_exists()? {
        return Err(bad());
    }
    let until = c.deadline.min(Instant::now() + Duration::from_secs(20));
    let mut paused = Pause::start(c.server.daemon.lock().await)?;
    paused.wait_stopped(until).await?;
    let mut args = matrix::base(
        c.client.workspace,
        "client",
        c.client.socket,
        &c.context.relays,
    );
    args.extend([
        "--no-autostart".into(),
        "stream".into(),
        c.ready.server_account.clone().into(),
        "--".into(),
        "./fixture/main".into(),
        "edge-v1".into(),
        "hold".into(),
        nonce.into(),
        c.role.into(),
    ]);
    let mut client = match Started::start(
        &c.client.workspace.join("bin/marmish"),
        &args,
        c.client.workspace,
        c.client.environment,
        until,
        false,
    )
    .await
    {
        Ok(client) => client,
        Err(e) => {
            paused.resume()?;
            return Err(e);
        }
    };
    let result = async {
        loop {
            let entries = streaming_cleanup::entries(*c.client.daemon.lock().await,c.client.socket,until).await?;
            if entries.iter().any(|e|e.stream==client.stream && hello(e,&c,"client")) { break; }
            if Instant::now() >= until { return Err(Error::Timeout); }
            tokio::time::sleep_until(until.min(Instant::now()+Duration::from_millis(25))).await;
        }
        client.signal(libc::SIGINT,false)?;
        let output = client.finish(until).await?;
        if output.status.code()!=Some(255) || !output.stdout.is_empty()
            || output.stderr != b"marmish stream: interrupted before remote Open; no remote child was launched\nmarmish: invalid input: stream_interrupted_before_open\n" { return Err(bad()); }
        Ok(())
    }.await;
    if result.is_err() {
        let cleanup = client.abort().await;
        paused.resume()?;
        cleanup?;
        return result.map(|_| unreachable!());
    }
    paused.resume()?;
    let status: serde_json::Value = cleanup::request(
        *paused.child,
        c.server.socket,
        &crate::ipc::Request::Status {},
        c.deadline,
    )
    .await?;
    if status.get("state").and_then(serde_json::Value::as_str) != Some("active") {
        return Err(bad());
    }
    loop {
        let entries =
            streaming_cleanup::entries(*paused.child, c.server.socket, c.deadline).await?;
        if let Some(e) = entries.iter().find(|e| e.stream == client.stream) {
            if !hello(e, &c, "server")
                || baseline.iter().any(|b| b.stream == client.stream)
                || marker.try_exists()?
            {
                return Err(bad());
            }
            let previous: Vec<_> = entries
                .iter()
                .filter(|e| e.stream != client.stream)
                .cloned()
                .collect();
            let observation = Observation {
                wall_ns: u64::try_from(start.elapsed().as_nanos()).map_err(|_| bad())?,
                stream_sha256: records::digest(e.stream.as_bytes()),
                session_sha256: records::digest(e.session.as_bytes()),
                group_sha256: e.group_sha256.clone(),
                local_exit: 255,
                queued_hello: true,
                server_hello_only: true,
                unchanged_previous_fences: fences(&previous)? == prior,
            };
            observation.validate()?;
            return Ok(observation);
        }
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(50))).await;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preopen_requires_present_hello_not_missing_execution() {
        let mut o = Observation {
            wall_ns: 1,
            stream_sha256: "a".repeat(64),
            session_sha256: "b".repeat(64),
            group_sha256: "c".repeat(64),
            local_exit: 255,
            queued_hello: true,
            server_hello_only: true,
            unchanged_previous_fences: true,
        };
        o.validate().unwrap();
        o.server_hello_only = false;
        assert!(o.validate().is_err());
        o.server_hello_only = true;
        o.local_exit = 130;
        assert!(o.validate().is_err());
    }
    #[tokio::test]
    async fn pause_drop_resumes_owned_leader() {
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "sleep 0.1; exit 7"])
            .spawn()
            .unwrap();
        let mutex = tokio::sync::Mutex::new(&mut child);
        {
            let pause = Pause::start(mutex.lock().await).unwrap();
            pause
                .wait_stopped(Instant::now() + Duration::from_secs(1))
                .await
                .unwrap();
        }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), child.wait())
                .await
                .unwrap()
                .unwrap()
                .code(),
            Some(7)
        );
    }
}
