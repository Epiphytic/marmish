//! Build-only gated children. No installed production entrypoint exposes these.
use super::{records, streaming_records::RsyncPin};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn bad() -> Error {
    Error::Invalid("installed_edge_fixture")
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RsyncConfig {
    pub schema: u8,
    pub nonce: String,
    pub mode: String,
    pub rsync: RsyncPin,
}
fn workspace() -> Result<(PathBuf, PathBuf)> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let root = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(bad)?
        .into();
    Ok((executable, root))
}
pub(super) fn marker(root: &Path, nonce: &str) -> Result<PathBuf> {
    if !records::hexadecimal(nonce, 64) {
        return Err(bad());
    }
    Ok(root
        .join("edge-gates")
        .join(format!("{nonce}.started.json")))
}
fn begin(root: &Path, nonce: &str, args: &[OsString]) -> Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    records::write(
        &marker(root, nonce)?,
        &serde_json::json!({"schema":1,"nonce":nonce,
        "executable_sha256":records::digest(&records::read_bytes(&executable,268435456)?),
        "argv_sha256":super::fixture::argv_digest(&executable,args)?}),
    )
}
fn released(root: &Path, nonce: &str) -> Result<bool> {
    let path = root.join("edge-gates").join(format!("{nonce}.release"));
    match path.try_exists()? {
        false => Ok(false),
        true => {
            if records::read_bytes(&path, 8)? == b"release\n" {
                Ok(true)
            } else {
                Err(bad())
            }
        }
    }
}
fn wait(root: &Path, nonce: &str) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(1800);
    loop {
        if released(root, nonce)? {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(Error::Timeout);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
pub(super) fn execute(args: &[OsString]) -> Result<u8> {
    let (_executable, root) = workspace()?;
    if args.first().is_some_and(|a| a == "--server") {
        let config: RsyncConfig = records::read(&root.join("edge-gates/rsync.json"), 65536)?;
        if config.schema != 1 || !matches!(config.mode.as_str(), "hold" | "fail") {
            return Err(bad());
        }
        config.rsync.validate()?;
        super::streaming_prerequisites::verify(&config.rsync)?;
        begin(&root, &config.nonce, args)?;
        if config.mode == "fail" {
            std::io::stderr().write_all(b"installed rsync fixture failure\n")?;
            return Ok(23);
        }
        wait(&root, &config.nonce)?;
        use std::os::unix::process::CommandExt;
        let error = std::process::Command::new(&config.rsync.path)
            .args(args)
            .exec();
        return Err(error.into());
    }
    if args.len() != 4 || args[0] != "edge-v1" {
        return Err(bad());
    }
    let mode = args[1].to_str().ok_or_else(bad)?;
    let nonce = args[2].to_str().ok_or_else(bad)?;
    let role = args[3].to_str().ok_or_else(bad)?;
    if !matches!(mode, "hold" | "flood") || !records::ROLES.contains(&role) {
        return Err(bad());
    }
    // All state stays in the job-owned fixture workspace; no user path argument.
    crate::identity::check_dir(&root.join("edge-gates"))?;
    begin(&root, nonce, args)?;
    if mode == "hold" {
        wait(&root, nonce)?;
        return Ok(0);
    }
    let block: Vec<u8> = (0..8192).map(|n| n as u8).collect();
    let mut out = std::io::stdout().lock();
    loop {
        if released(&root, nonce)? {
            return Ok(0);
        }
        out.write_all(&block)?;
    }
}
