//! Own the initiating CLI while observing a real receiver checkpoint.
//! This helper deliberately kills its owned CLI, never the remote helper.
use super::{copy_checkpoint::Proof, process, records};
use crate::{Error, Result};
use std::{ffi::OsString, path::Path, time::Duration};
use tokio::{io::AsyncReadExt, time::Instant};

fn bad() -> Error {
    Error::Invalid("installed_copy_detach")
}
pub(super) struct Detached {
    /// These identifiers remain private; public receipts use digests.
    pub stream: String,
    pub transfer: String,
    pub partial: Proof,
}
pub(super) fn start_line(line: &[u8]) -> Result<(String, String)> {
    let text = std::str::from_utf8(line).map_err(|_| bad())?;
    let value = text
        .strip_prefix("marmish copy: transfer ")
        .and_then(|v| v.strip_suffix('\n'))
        .ok_or_else(bad)?;
    let (transfer, stream) = value.split_once(" stream ").ok_or_else(bad)?;
    if !records::hexadecimal(transfer, 64) || !records::hexadecimal(stream, 32) {
        return Err(bad());
    }
    Ok((stream.into(), transfer.into()))
}
/// `checkpoint` is constructed from job-owned principals, paths and known file
/// content. An absent/changing checkpoint may be retried; no elapsed delay can
/// substitute for its checksum/identity-validated intermediate durable prefix.
pub(super) async fn run(
    executable: &Path,
    args: &[OsString],
    cwd: &Path,
    environment: &process::Environment,
    deadline: Instant,
    codec: marmish_stream::codec::Codec,
    checkpoint: impl Fn(&str, &str) -> Result<Proof>,
) -> Result<Detached> {
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    let mut child = process::command(executable, args, cwd, environment).spawn()?;
    let stdout = child.stdout.take().ok_or_else(bad)?;
    let mut stderr = child.stderr.take().ok_or_else(bad)?;
    let mut out = tokio::spawn(process::drain(stdout, 65536));
    let first = tokio::time::timeout_at(deadline, async {
        let mut line = Vec::new();
        loop {
            let byte = stderr.read_u8().await?;
            line.push(byte);
            if line.len() > 128 {
                return Err(bad());
            }
            if byte == b'\n' {
                return start_line(&line);
            }
        }
    })
    .await;
    let (stream, transfer) = match first {
        Ok(Ok(ids)) => ids,
        other => {
            let cleanup = process::terminate(&mut child).await;
            out.abort();
            cleanup?;
            return Err(match other {
                Err(_) => Error::Timeout,
                Ok(Err(e)) => e,
                _ => bad(),
            });
        }
    };
    let mut err = tokio::spawn(process::drain(stderr, 65536));
    let mut last_observer_error = None;
    let observed = loop {
        match child.try_wait() {
            Ok(Some(_)) => break Err(bad()),
            Ok(None) => {}
            Err(error) => break Err(error.into()),
        }
        if Instant::now() >= deadline {
            if let Some(category) = last_observer_error {
                eprintln!("marmish installed check: checkpoint_observation={category}");
            }
            break Err(Error::Timeout);
        }
        match checkpoint(&stream, &transfer) {
            Ok(proof) => {
                // The decoder, not a file size or timer, supplies this evidence.
                if proof.phase == "receiving"
                    && proof.durable_offset > 0
                    && proof.durable_offset < proof.size
                {
                    break Ok(proof);
                }
                last_observer_error = Some("not_intermediate");
            }
            Err(error) => {
                last_observer_error = Some(match error {
                    Error::Io(error) if error.kind() == std::io::ErrorKind::NotFound => "not_found",
                    Error::Io(_) => "local_io",
                    _ => "binding_or_integrity",
                });
            }
        }
        tokio::time::sleep_until(deadline.min(Instant::now() + Duration::from_millis(20))).await;
    };
    // This still-owned leader fences process-group identity; never signal IDs
    // recovered from a receipt or checkpoint. Always reap before returning.
    let cleanup = process::terminate(&mut child).await;
    let partial = match observed {
        Ok(proof) => proof,
        Err(error) => {
            out.abort();
            err.abort();
            cleanup?;
            return Err(error);
        }
    };
    if let Err(error) = cleanup {
        out.abort();
        err.abort();
        return Err(error);
    }
    use std::os::unix::process::ExitStatusExt;
    if child.try_wait()?.and_then(|s| s.signal()) != Some(libc::SIGKILL) {
        out.abort();
        err.abort();
        return Err(bad());
    }
    let pipes = tokio::time::timeout_at(deadline, async {
        Ok::<_, Error>((
            (&mut out).await.map_err(|_| bad())??,
            (&mut err).await.map_err(|_| bad())??,
        ))
    })
    .await;
    let ((stdout, out_overflow), (stderr, err_overflow)) = match pipes {
        Ok(result) => result?,
        Err(_) => {
            out.abort();
            err.abort();
            return Err(Error::Timeout);
        }
    };
    let expected_codec = match codec {
        marmish_stream::codec::Codec::None => b"marmish copy: transport codec none\n".as_slice(),
        marmish_stream::codec::Codec::Zlib => b"marmish copy: transport codec zlib\n".as_slice(),
    };
    if out_overflow || err_overflow || !stdout.is_empty() || stderr != expected_codec {
        return Err(bad());
    }
    Ok(Detached {
        stream,
        transfer,
        partial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initiating_ids_are_closed_canonical_and_never_accept_success_receipts() {
        let t = "a".repeat(64);
        let s = "b".repeat(32);
        assert_eq!(
            start_line(format!("marmish copy: transfer {t} stream {s}\n").as_bytes()).unwrap(),
            (s, t)
        );
        for text in [
            "marmish copy: transfer a stream b\n",
            "marmish copy: committed and verified transfer a\n",
            "marmish copy: transfer a stream b\r\n",
        ] {
            assert!(start_line(text.as_bytes()).is_err());
        }
    }
    #[tokio::test]
    async fn owned_initiator_is_killed_and_reaped_after_observation() {
        let root = crate::journal::private_test_directory();
        let env = process::Environment {
            home: root.path().into(),
            temporary: root.path().into(),
            utilities: root.path().into(),
        };
        let transfer = "a".repeat(64);
        let stream = "b".repeat(32);
        let command = format!(
            "printf '%s\\n' \"$$\" > child.pid; printf 'marmish copy: transfer {transfer} stream {stream}\\nmarmish copy: transport codec none\\n' >&2; exec /bin/sleep 30"
        );
        let result = run(
            Path::new("/bin/sh"),
            &["-c".into(), command.into()],
            root.path(),
            &env,
            Instant::now() + Duration::from_secs(2),
            marmish_stream::codec::Codec::None,
            |s, t| {
                assert_eq!(s, stream);
                assert_eq!(t, transfer);
                // Synthetic proof tests child ownership only, never qualifies a
                // transfer or bypasses the real observer used by the driver.
                Ok(Proof {
                    transfer_sha256: records::digest(t.as_bytes()),
                    stream_sha256: records::digest(s.as_bytes()),
                    manifest_sha256: "c".repeat(64),
                    phase: "receiving".into(),
                    durable_offset: 1_048_576,
                    prefix_sha256: "d".repeat(64),
                    size: 4_194_304,
                    sha256: "e".repeat(64),
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(result.stream, stream);
        assert_eq!(result.transfer, transfer);
        assert_eq!(result.partial.durable_offset, 1_048_576);
        let pid: i32 = std::fs::read_to_string(root.path().join("child.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // Probe only; production run() already requires SIGKILL and reaps the
        // owned leader before returning. This independently catches a survivor.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}
