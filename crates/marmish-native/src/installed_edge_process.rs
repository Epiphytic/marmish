//! Retain the initiating CLI's owned process-group identity through signal tests.
use super::{edge_rsync, process, streaming_cases};
use crate::{Error, Result};
use std::{ffi::OsString, path::Path, process::ExitStatus};
use tokio::{io::AsyncReadExt, process::Child, task::JoinHandle, time::Instant};
fn bad() -> Error {
    Error::Invalid("installed_edge_process")
}
type Drain = JoinHandle<std::io::Result<(Vec<u8>, bool)>>;
pub(super) struct Started {
    pub stream: String,
    child: Child,
    stdout: Option<Drain>,
    stderr: Option<Drain>,
}
pub(super) struct Finished {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
impl Started {
    pub(super) async fn start(
        executable: &Path,
        args: &[OsString],
        cwd: &Path,
        environment: &process::Environment,
        deadline: Instant,
        broken_stdout: bool,
    ) -> Result<Self> {
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let mut child = process::command(executable, args, cwd, environment).spawn()?;
        let stdout = child.stdout.take().ok_or_else(bad)?;
        // Deliberately dropping the sole reader creates a real pipe EPIPE.
        let out = if broken_stdout {
            drop(stdout);
            None
        } else {
            Some(tokio::spawn(process::drain(stdout, 65536)))
        };
        let mut stderr = child.stderr.take().ok_or_else(bad)?;
        let first = tokio::time::timeout_at(deadline, async {
            let mut bytes = Vec::new();
            loop {
                bytes.push(stderr.read_u8().await?);
                if bytes.len() > 128 {
                    return Err(bad());
                }
                if bytes.last() == Some(&b'\n') {
                    let (stream, tail) = streaming_cases::stderr(&bytes)?;
                    if !tail.is_empty() {
                        return Err(bad());
                    }
                    return Ok(stream);
                }
            }
        })
        .await;
        let stream = match first {
            Ok(Ok(s)) => s,
            other => {
                let cleanup = edge_rsync::reap_owned_group(&mut child).await.map(|_| ());
                if let Some(out) = out {
                    out.abort();
                }
                cleanup?;
                return Err(match other {
                    Err(_) => Error::Timeout,
                    Ok(Err(e)) => e,
                    _ => bad(),
                });
            }
        };
        Ok(Self {
            stream,
            child,
            stdout: out,
            stderr: Some(tokio::spawn(process::drain(stderr, 65536))),
        })
    }
    pub(super) fn live(&mut self) -> Result<bool> {
        if self.child.id().is_none() {
            return Ok(false);
        }
        Ok(!edge_rsync::leader_exited(&self.child)?)
    }
    pub(super) fn signal(&mut self, signal: i32, group: bool) -> Result<()> {
        if !matches!(
            signal,
            libc::SIGINT | libc::SIGTERM | libc::SIGHUP | libc::SIGUSR1 | libc::SIGKILL
        ) || !self.live()?
        {
            return Err(bad());
        }
        let pid = i32::try_from(self.child.id().ok_or_else(bad)?).map_err(|_| bad())?;
        // Never derive signal authority from a public receipt or persisted PID.
        if unsafe { libc::kill(if group { -pid } else { pid }, signal) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    pub(super) async fn close_stdout(&mut self) {
        if let Some(output) = self.stdout.take() {
            output.abort();
            let _ = output.await;
        }
    }
    pub(super) async fn abort(&mut self) -> Result<()> {
        let result = if self.child.id().is_some() {
            edge_rsync::reap_owned_group(&mut self.child)
                .await
                .map(|_| ())
        } else {
            Ok(())
        };
        if let Some(out) = self.stdout.take() {
            out.abort();
        }
        if let Some(err) = self.stderr.take() {
            err.abort();
        }
        result
    }
    pub(super) async fn finish(&mut self, deadline: Instant) -> Result<Finished> {
        if let Err(error) = edge_rsync::wait_unreaped(&mut self.child, deadline).await {
            self.abort().await?;
            return Err(error);
        }
        let pipes = tokio::time::timeout_at(deadline, async {
            let out = if let Some(out) = self.stdout.as_mut() {
                out.await.map_err(|_| bad())??
            } else {
                (Vec::new(), false)
            };
            let err = self
                .stderr
                .as_mut()
                .ok_or_else(bad)?
                .await
                .map_err(|_| bad())??;
            Ok::<_, Error>((out, err))
        })
        .await;
        let ((stdout, overflow_out), (stderr, overflow_err)) = match pipes {
            Ok(Ok(p)) => p,
            other => {
                self.abort().await?;
                return Err(match other {
                    Err(_) => Error::Timeout,
                    Ok(Err(e)) => e,
                    _ => bad(),
                });
            }
        };
        let status = edge_rsync::reap_owned_group(&mut self.child).await?;
        self.stdout = None;
        self.stderr = None;
        if overflow_out || overflow_err {
            return Err(bad());
        }
        Ok(Finished {
            status,
            stdout,
            stderr,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[tokio::test]
    async fn signals_use_owned_group_and_reap_without_accepting_malformed_start() {
        let root = crate::journal::private_test_directory();
        let env = process::Environment {
            home: root.path().into(),
            temporary: root.path().into(),
            utilities: root.path().into(),
        };
        let args: Vec<OsString> = vec![
            "-c".into(),
            format!(
                "printf 'marmish stream: {}\\n' >&2; exec /bin/sleep 30",
                "a".repeat(32)
            )
            .into(),
        ];
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut owned = Started::start(
            Path::new("/bin/sh"),
            &args,
            root.path(),
            &env,
            deadline,
            false,
        )
        .await
        .unwrap();
        assert!(owned.live().unwrap());
        assert!(owned.signal(libc::SIGSTOP, false).is_err());
        owned.signal(libc::SIGKILL, true).unwrap();
        let result = owned.finish(deadline).await.unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(result.status.signal(), Some(libc::SIGKILL));
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
        assert!(!owned.live().unwrap());
        let malformed = vec![
            "-c".into(),
            "printf 'banner\\n' >&2; exec /bin/sleep 30".into(),
        ];
        assert!(
            Started::start(
                Path::new("/bin/sh"),
                &malformed,
                root.path(),
                &env,
                deadline,
                false
            )
            .await
            .is_err()
        );
    }
}
