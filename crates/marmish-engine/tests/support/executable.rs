use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// Keep this owner last in fixtures so detached workers retain their installed
// executable until engines, child processes and mutable state have been dropped.
pub struct InstalledWorker {
    pub path: PathBuf,
    _directory: tempfile::TempDir,
}

pub fn private_tempdir() -> tempfile::TempDir {
    private_tempdir_in(&std::env::temp_dir())
}

// Resolve only the existing fixture parent, never a database or private leaf.
pub fn private_tempdir_in(parent: &Path) -> tempfile::TempDir {
    let directory = tempfile::tempdir_in(parent.canonicalize().unwrap()).unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

// Cargo artifact sources must be unambiguous even when native and engine bins
// are built in the same target directory. Installed/fault fixtures use the
// generic copier below because their source names are intentionally arbitrary.
#[allow(dead_code)] // Fault fixtures include this module but use arbitrary-name sources.
pub fn stage_cargo_worker(source: &Path) -> InstalledWorker {
    assert_eq!(
        source.file_name(),
        Some(std::ffi::OsStr::new("marmish-engine-worker"))
    );
    stage_worker(source)
}

pub fn stage_worker(source: &Path) -> InstalledWorker {
    let directory = private_tempdir();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("marmish-worker");
    fs::copy(source, &path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    InstalledWorker {
        path,
        _directory: directory,
    }
}
