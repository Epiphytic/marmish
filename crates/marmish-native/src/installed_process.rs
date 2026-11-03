//! Credential-free, bounded children owned by the build-only acceptance driver.
use crate::{Error, Result};
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    time::Instant,
};

pub(super) struct Environment {
    pub home: PathBuf,
    pub temporary: PathBuf,
    pub utilities: PathBuf,
}
pub(super) struct Output {
    pub status: std::process::ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
pub(super) fn command(
    executable: &Path,
    args: &[OsString],
    cwd: &Path,
    environment: &Environment,
) -> Command {
    let mut c = Command::new(executable);
    c.args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", &environment.utilities)
        .env("HOME", &environment.home)
        .env("TMPDIR", &environment.temporary)
        .env("LANG", "C");
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    c.process_group(0);
    c
}
pub(super) async fn drain(
    mut stream: impl AsyncRead + Unpin,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::new();
    let mut overflow = false;
    let mut block = [0u8; 8192];
    loop {
        let n = stream.read(&mut block).await?;
        if n == 0 {
            break;
        }
        let room = limit.saturating_sub(kept.len());
        kept.extend_from_slice(&block[..n.min(room)]);
        overflow |= n > room;
    }
    Ok((kept, overflow))
}
pub(super) async fn terminate(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_none() {
        let id = child.id().ok_or(Error::Invalid("installed_child"))?;
        // The unreaped owned leader fences this process-group identity against PID reuse.
        if unsafe { libc::kill(-(id as i32), libc::SIGKILL) } != 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ESRCH) {
                return Err(e.into());
            }
        }
    }
    child.wait().await?;
    Ok(())
}
pub(super) async fn run(
    executable: &Path,
    args: &[OsString],
    cwd: &Path,
    environment: &Environment,
    deadline: Instant,
    limit: usize,
) -> Result<Output> {
    run_input(executable, args, cwd, environment, deadline, limit, None).await
}
pub(super) async fn run_input(
    executable: &Path,
    args: &[OsString],
    cwd: &Path,
    environment: &Environment,
    deadline: Instant,
    limit: usize,
    input: Option<&[u8]>,
) -> Result<Output> {
    if limit == 0 || limit > 1024 * 1024 || Instant::now() >= deadline {
        return Err(Error::Invalid("installed_child_limit"));
    }
    let mut cmd = command(executable, args, cwd, environment);
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or(Error::Invalid("installed_child"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(Error::Invalid("installed_child"))?;
    let mut out = tokio::spawn(drain(stdout, limit));
    let mut err = tokio::spawn(drain(stderr, limit));
    if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or(Error::Invalid("installed_child"))?;
        let wrote = tokio::time::timeout_at(deadline, async {
            stdin.write_all(input).await?;
            stdin.shutdown().await
        })
        .await;
        if !matches!(wrote, Ok(Ok(()))) {
            let cleanup = terminate(&mut child).await;
            out.abort();
            err.abort();
            cleanup?;
            return Err(Error::Invalid("installed_child_input"));
        }
    }
    let status = match tokio::time::timeout_at(deadline, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            let cleanup = terminate(&mut child).await;
            out.abort();
            err.abort();
            cleanup?;
            return Err(Error::Timeout);
        }
    };
    let result = tokio::time::timeout_at(deadline, async {
        Ok::<_, Error>((
            (&mut out)
                .await
                .map_err(|_| Error::Invalid("installed_child_pipe"))??,
            (&mut err)
                .await
                .map_err(|_| Error::Invalid("installed_child_pipe"))??,
        ))
    })
    .await;
    let result = match result {
        Ok(result) => result?,
        Err(_) => {
            out.abort();
            err.abort();
            return Err(Error::Timeout);
        }
    };
    if result.0.1 || result.1.1 {
        return Err(Error::Invalid("installed_child_output_limit"));
    }
    Ok(Output {
        status,
        stdout: result.0.0,
        stderr: result.1.0,
    })
}
pub(super) fn args(values: &[&OsStr]) -> Vec<OsString> {
    values.iter().map(|v| v.to_os_string()).collect()
}

/// The runtime PATH contains the installer's audited utilities, never language/build tools.
pub(super) fn utilities(path: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    std::fs::DirBuilder::new().mode(0o700).create(path)?;
    for name in [
        "sh",
        "curl",
        "uname",
        "id",
        "mktemp",
        "chmod",
        "mkdir",
        "rm",
        "rmdir",
        "mv",
        "cat",
        "wc",
        "tr",
        "awk",
        "sha256sum",
        "shasum",
        "sysctl",
    ] {
        let source = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
            .into_iter()
            .map(|p| Path::new(p).join(name))
            .find(|p| p.is_file());
        let Some(source) = source else {
            if matches!(name, "sha256sum" | "shasum" | "sysctl") {
                continue;
            }
            return Err(Error::Invalid("installed_utility"));
        };
        let source = source.canonicalize()?;
        let m = std::fs::metadata(&source)?;
        if m.uid() != 0 || m.mode() & 0o022 != 0 || !m.is_file() {
            return Err(Error::Invalid("installed_utility"));
        }
        std::os::unix::fs::symlink(source, path.join(name))?;
    }
    for name in ["python", "python3", "uv", "cargo", "rustc", "node"] {
        if path.join(name).try_exists()? {
            return Err(Error::Invalid("installed_runtime_path"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[tokio::test]
    async fn owned_children_preserve_bytes_and_bound_output_and_deadline() {
        let d = tempfile::tempdir().unwrap();
        let env = Environment {
            home: d.path().into(),
            temporary: d.path().into(),
            utilities: d.path().into(),
        };
        let invoke = |script: &str| vec![OsString::from("-c"), OsString::from(script)];
        let output = run_input(
            Path::new("/bin/sh"),
            &invoke("/bin/cat; printf err >&2; exit 7"),
            d.path(),
            &env,
            Instant::now() + Duration::from_secs(5),
            64,
            Some(&[0, 255, 10]),
        )
        .await
        .unwrap();
        assert_eq!(output.stdout, [0, 255, 10]);
        assert_eq!(output.stderr, b"err");
        assert_eq!(output.status.code(), Some(7));
        assert!(matches!(
            run(
                Path::new("/bin/sh"),
                &invoke("printf 123456789"),
                d.path(),
                &env,
                Instant::now() + Duration::from_secs(5),
                8
            )
            .await,
            Err(Error::Invalid("installed_child_output_limit"))
        ));
        assert!(matches!(
            run(
                Path::new("/bin/sh"),
                &invoke("while :; do :; done"),
                d.path(),
                &env,
                Instant::now() + Duration::from_millis(50),
                8
            )
            .await,
            Err(Error::Timeout)
        ));
    }
    #[test]
    fn runtime_environment_is_explicit_and_utility_path_excludes_build_tools() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("utilities");
        utilities(&path).unwrap();
        for forbidden in ["python", "python3", "uv", "cargo", "rustc", "node"] {
            assert!(!path.join(forbidden).exists());
        }
        let env = Environment {
            home: d.path().into(),
            temporary: d.path().into(),
            utilities: path,
        };
        let cmd = command(Path::new("/bin/sh"), &[], d.path(), &env);
        let vars = cmd
            .as_std()
            .get_envs()
            .filter(|(_, v)| v.is_some())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            vars,
            ["HOME", "LANG", "PATH", "TMPDIR"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }
}
