//! Pinned host tools used only by the feature-gated installed driver.
use super::{
    process::{self, Environment},
    records::{self, Context},
    streaming_records::RsyncPin,
};
use crate::{Error, Result};
use std::{ffi::OsString, path::Path};
use tokio::time::Instant;
fn bad() -> Error {
    Error::Invalid("installed_rsync_prerequisite")
}
pub(super) async fn capture(
    ctx: &Context,
    workspace: &Path,
    env: &Environment,
    deadline: Instant,
) -> Result<()> {
    if !matches!(ctx.cohort.as_str(), "rsync-matrix" | "edge-rsync") {
        return Ok(());
    }
    let original = std::env::var_os("MARMISH_TEST_RSYNC").ok_or_else(bad)?;
    let original = Path::new(&original);
    if !original.is_absolute() {
        return Err(bad());
    }
    let path = original.canonicalize()?;
    let metadata = std::fs::metadata(&path)?;
    use std::os::unix::fs::PermissionsExt;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(bad());
    }
    let bytes = host_bytes(&path)?;
    let output = process::run(
        &path,
        &[OsString::from("--version")],
        workspace,
        env,
        deadline,
        65536,
    )
    .await?;
    if !output.status.success() {
        return Err(bad());
    }
    let version = std::str::from_utf8(&output.stdout)
        .map_err(|_| bad())?
        .lines()
        .next()
        .ok_or_else(bad)?
        .to_owned();
    let pin = RsyncPin {
        path: path.to_str().ok_or_else(bad)?.into(),
        version,
        sha256: records::digest(&bytes),
        package: if cfg!(target_os = "macos") {
            Some(
                serde_json::from_str(
                    &std::env::var("MARMISH_TEST_RSYNC_PACKAGE").map_err(|_| bad())?,
                )
                .map_err(|_| bad())?,
            )
        } else {
            None
        },
        acquisition: if cfg!(target_os = "macos") {
            "homebrew"
        } else {
            "hosted-prerequisite"
        }
        .into(),
    };
    pin.validate()?;
    records::write(&workspace.join("rsync.json"), &pin)
}
pub(super) fn rsync(ctx: &Context, workspace: &Path) -> Result<Option<RsyncPin>> {
    if !matches!(ctx.cohort.as_str(), "rsync-matrix" | "edge-rsync") {
        return Ok(None);
    }
    let pin: RsyncPin = records::read(&workspace.join("rsync.json"), 16384)?;
    verify(&pin)?;
    Ok(Some(pin))
}

pub(super) fn verify(pin: &RsyncPin) -> Result<()> {
    pin.validate()?;
    let path = Path::new(&pin.path);
    if path.canonicalize()? != path || records::digest(&host_bytes(path)?) != pin.sha256 {
        return Err(bad());
    }
    Ok(())
}
fn host_bytes(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || ![0, unsafe { libc::geteuid() }].contains(&metadata.uid())
        || metadata.mode() & 0o022 != 0
        || metadata.mode() & 0o111 == 0
        || metadata.len() > 64 * 1024 * 1024
    {
        return Err(bad());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(bad());
    }
    Ok(bytes)
}
