use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// Keep this owner last in fixtures so detached workers retain their installed
// executable until engines, child processes and mutable state have been dropped.
pub struct InstalledWorker {
    pub path: PathBuf,
    _directory: tempfile::TempDir,
}

pub fn stage_worker(source: &Path) -> InstalledWorker {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
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
