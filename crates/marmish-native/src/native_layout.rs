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
        // One signed credential-helper identity for all same-version frontends.
        let helper = executable(&parent.join("marmish"))?;
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
        let helper = if name == downloaded {
            frontend
        } else {
            let expected = parent.join("marmish");
            let helper = executable(&expected)?;
            if helper != expected {
                return Err(bad());
            }
            helper
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn fixture() -> tempfile::TempDir {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let root = tempfile::Builder::new()
            .prefix("marmish-layout-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        for name in ["marmish", "marmishd", "marmish-worker"] {
            fs::write(root.path().join(name), name).unwrap();
            fs::set_permissions(root.path().join(name), fs::Permissions::from_mode(0o500)).unwrap();
        }
        root
    }

    #[test]
    fn ordinary_frontends_share_the_same_canonical_helper() {
        let root = fixture();
        for name in ["marmish", "marmishd", "marmish-worker"] {
            let layout = resolve(&root.path().join(name)).unwrap();
            assert_eq!(layout.helper, root.path().join("marmish"));
            assert_eq!(layout.worker, root.path().join("marmish-worker"));
        }
    }

    #[test]
    fn missing_symlinked_or_writable_helper_fails_closed() {
        let root = fixture();
        let helper = root.path().join("marmish");
        fs::remove_file(&helper).unwrap();
        for name in ["marmishd", "marmish-worker"] {
            assert!(resolve(&root.path().join(name)).is_err());
        }
        symlink(root.path().join("marmishd"), &helper).unwrap();
        assert!(resolve(&root.path().join("marmishd")).is_err());
        fs::remove_file(&helper).unwrap();
        fs::write(&helper, b"unsafe").unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(resolve(&root.path().join("marmishd")).is_err());
    }

    #[test]
    fn standalone_download_retains_its_exact_exception_even_beside_siblings() {
        let root = fixture();
        let download = root.path().join(format!(
            "marmish-{}-{}",
            env!("CARGO_PKG_VERSION"),
            env!("MARMISH_BUILD_TARGET")
        ));
        fs::write(&download, b"download").unwrap();
        fs::set_permissions(&download, fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(resolve(&download).unwrap().helper, download);
        fs::remove_file(root.path().join("marmish")).unwrap();
        fs::remove_file(root.path().join("marmish-worker")).unwrap();
        assert_eq!(resolve(&download).unwrap().helper, download);
        assert!(resolve(&root.path().join("marmishd")).is_err());
    }

    #[test]
    fn resolved_helper_does_not_follow_later_launcher_retarget() {
        let first = fixture();
        let second = fixture();
        let launcher = first.path().join("launcher");
        symlink(first.path().join("marmishd"), &launcher).unwrap();
        let held = resolve(&launcher).unwrap();
        fs::remove_file(&launcher).unwrap();
        symlink(second.path().join("marmishd"), &launcher).unwrap();
        assert_eq!(held.helper, first.path().join("marmish"));
        assert_eq!(
            resolve(&launcher).unwrap().helper,
            second.path().join("marmish")
        );
    }
}
