//! Resolve canonical executable lifetime once, before any provider/profile access.
use crate::native_install;
use crate::native_manifest::{self, NAMES};
use crate::{Error, Result};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub(crate) struct ExecutableLayout {
    pub helper: PathBuf,
    pub worker: PathBuf,
}
fn bad() -> Error {
    Error::Invalid("worker_installation")
}
fn exists(p: &Path) -> Result<bool> {
    match fs::symlink_metadata(p) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn executable(p: &Path) -> Result<PathBuf> {
    marmish_engine::validate_executable_path(p).map_err(|_| bad())
}
pub(crate) fn resolve(current: &Path) -> Result<ExecutableLayout> {
    let current = current.canonicalize()?;
    let parent = current.parent().ok_or_else(bad)?;
    let name = current
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(bad)?;
    let claimed =
        exists(&parent.join("manifest"))? || exists(&parent.join(".version-receipt.json"))?;
    if claimed {
        if !NAMES[..3].contains(&name) {
            return Err(bad());
        }
        let versions = parent
            .parent()
            .filter(|p| p.file_name().is_some_and(|s| s == "versions"))
            .ok_or_else(bad)?;
        let prefix = versions.parent().ok_or_else(bad)?;
        let receipt = native_install::read_receipt(prefix)?;
        let id = parent
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(bad)?;
        let (manifest, _) = native_install::read_version(prefix, id, &receipt.install_id)?;
        let compiled = native_manifest::compiled_release_metadata();
        if manifest.version != compiled.version
            || manifest.target != compiled.target
            || manifest.commit != compiled.commit
            || manifest.formats != compiled.formats
        {
            return Err(bad());
        }
        for a in &manifest.files {
            let p = parent.join(a.name);
            let f = native_manifest::safe_file(&p, a.bytes)?;
            let m = f.metadata()?;
            if m.uid() != unsafe { libc::geteuid() }
                || m.len() != a.bytes
                || m.mode() & 0o777 != a.mode
            {
                return Err(bad());
            }
        }
        executable(&current)?;
        let helper = executable(&parent.join(if name == "marmish-worker" {
            "marmish"
        } else {
            name
        }))?;
        let worker = executable(&parent.join("marmish-worker"))?;
        Ok(ExecutableLayout { helper, worker })
    } else {
        // Cargo/debug/release siblings and the exact downloaded release CLI are recognized.
        let downloaded = format!(
            "marmish-{}-{}",
            env!("CARGO_PKG_VERSION"),
            env!("MARMISH_BUILD_TARGET")
        );
        if !["marmish", "marmishd", "marmish-worker"].contains(&name) && name != downloaded {
            return Err(bad());
        }
        let frontend = executable(&current)?;
        let helper = if name == "marmish-worker" {
            executable(&parent.join("marmish"))?
        } else {
            frontend.clone()
        };
        // Downloaded installer probes intentionally do not contain a chmod'd sibling worker.
        let worker = parent.join("marmish-worker");
        let worker = if name == downloaded {
            worker
        } else {
            executable(&worker)?
        };
        Ok(ExecutableLayout { helper, worker })
    }
}
