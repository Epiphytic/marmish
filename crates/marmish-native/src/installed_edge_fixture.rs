//! Build-only gated children. No installed production entrypoint exposes these.
use super::{records, streaming_records::RsyncPin};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::Write,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn bad() -> Error {
    Error::Invalid("installed_edge_fixture")
}
/// Prepare only the gate child of an already validated, job-owned workspace.
/// Existing contents and permissions are never repaired or replaced.
pub(super) fn prepare_gates(workspace: &Path) -> Result<()> {
    let gates = workspace.join("edge-gates");
    match std::fs::DirBuilder::new().mode(0o700).create(&gates) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    crate::identity::check_dir(&gates)
}

/// Preserve the closed marker contract without accepting extra keys or coercions.
pub(super) fn validate_marker(
    marker: &serde_json::Value,
    nonce: &str,
    image: &str,
    argv: &str,
) -> Result<()> {
    if marker
        != &serde_json::json!({
            "schema": 1,
            "nonce": nonce,
            "executable_sha256": image,
            "argv_sha256": argv,
        })
    {
        return Err(bad());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    };

    #[test]
    fn absent_gates_are_created_as_private_owned_directory() {
        let root = crate::journal::private_test_directory();
        prepare_gates(root.path()).unwrap();
        let metadata = fs::symlink_metadata(root.path().join("edge-gates")).unwrap();
        assert!(metadata.is_dir());
        assert_eq!(metadata.mode() & 0o777, 0o700);
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    }

    #[test]
    fn safe_existing_gates_and_contents_are_not_replaced() {
        let root = crate::journal::private_test_directory();
        prepare_gates(root.path()).unwrap();
        let gates = root.path().join("edge-gates");
        let marker = gates.join("retained.started.json");
        fs::write(&marker, b"immutable existing fixture marker\n").unwrap();
        let directory_before = fs::symlink_metadata(&gates).unwrap();
        let file_before = fs::symlink_metadata(&marker).unwrap();
        prepare_gates(root.path()).unwrap();
        let directory_after = fs::symlink_metadata(&gates).unwrap();
        let file_after = fs::symlink_metadata(&marker).unwrap();
        assert_eq!(directory_before.ino(), directory_after.ino());
        assert_eq!(directory_before.mode(), directory_after.mode());
        assert_eq!(file_before.ino(), file_after.ino());
        assert_eq!(file_before.mode(), file_after.mode());
        assert_eq!(
            fs::read(&marker).unwrap(),
            b"immutable existing fixture marker\n"
        );
    }

    #[test]
    fn symlink_gate_is_rejected_without_following_or_replacing_it() {
        let root = crate::journal::private_test_directory();
        let target = crate::journal::private_test_directory();
        fs::write(target.path().join("sentinel"), b"unchanged").unwrap();
        let gates = root.path().join("edge-gates");
        symlink(target.path(), &gates).unwrap();
        assert!(prepare_gates(root.path()).is_err());
        assert!(fs::symlink_metadata(gates).unwrap().is_symlink());
        assert_eq!(
            fs::read(target.path().join("sentinel")).unwrap(),
            b"unchanged"
        );
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 1);
    }

    #[test]
    fn regular_file_gate_is_rejected_without_truncation() {
        let root = crate::journal::private_test_directory();
        let gates = root.path().join("edge-gates");
        fs::write(&gates, b"keep this file").unwrap();
        assert!(prepare_gates(root.path()).is_err());
        assert_eq!(fs::read(gates).unwrap(), b"keep this file");
    }

    #[test]
    fn unsafe_gate_permissions_are_rejected_without_repair() {
        let root = crate::journal::private_test_directory();
        prepare_gates(root.path()).unwrap();
        let gates = root.path().join("edge-gates");
        for mode in [0o750, 0o755, 0o770, 0o777] {
            fs::set_permissions(&gates, fs::Permissions::from_mode(mode)).unwrap();
            assert!(prepare_gates(root.path()).is_err());
            assert_eq!(fs::symlink_metadata(&gates).unwrap().mode() & 0o777, mode);
        }
    }

    #[test]
    fn absent_workspace_is_not_recursively_created() {
        let root = crate::journal::private_test_directory();
        let missing = root.path().join("missing");
        assert!(prepare_gates(&missing).is_err());
        assert!(!missing.try_exists().unwrap());
    }

    fn marker_value() -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "nonce": "a".repeat(64),
            "executable_sha256": "b".repeat(64),
            "argv_sha256": "c".repeat(64),
        })
    }

    fn check_marker(value: &serde_json::Value) -> Result<()> {
        validate_marker(value, &"a".repeat(64), &"b".repeat(64), &"c".repeat(64))
    }

    #[test]
    fn marker_requires_all_exact_fields_and_rejects_unknown_fields() {
        let marker = marker_value();
        check_marker(&marker).unwrap();
        for field in ["schema", "nonce", "executable_sha256", "argv_sha256"] {
            let mut missing = marker.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(check_marker(&missing).is_err(), "missing {field}");
            for wrong in [
                serde_json::json!(null),
                serde_json::json!(true),
                serde_json::json!(2),
                serde_json::json!(1.0),
                serde_json::json!(""),
                serde_json::json!("d".repeat(64)),
                serde_json::json!([]),
                serde_json::json!({}),
            ] {
                let mut invalid = marker.clone();
                invalid[field] = wrong;
                assert!(check_marker(&invalid).is_err(), "invalid {field}");
            }
        }
        let mut extra = marker;
        extra["unrecognized"] = serde_json::json!(null);
        assert!(check_marker(&extra).is_err());
    }

    #[test]
    fn marker_rejects_nonobjects_and_each_wrong_expected_binding() {
        for value in [
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!(false),
            serde_json::json!(1),
            serde_json::json!("marker"),
        ] {
            assert!(check_marker(&value).is_err());
        }
        let marker = marker_value();
        let nonce = "a".repeat(64);
        let image = "b".repeat(64);
        let argv = "c".repeat(64);
        assert!(validate_marker(&marker, &"d".repeat(64), &image, &argv).is_err());
        assert!(validate_marker(&marker, &nonce, &"d".repeat(64), &argv).is_err());
        assert!(validate_marker(&marker, &nonce, &image, &"d".repeat(64)).is_err());
    }
}
