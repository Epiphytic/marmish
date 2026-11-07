//! Deterministic byte fixture; isolated build tooling, never a product worker.
use super::records;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{Read, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
const INPUT: usize = 16 * 1024;
const STREAM: usize = 128 * 1024;
fn bad() -> Error {
    Error::Invalid("installed_fixture")
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub schema: u8,
    pub nonce: String,
    pub vector: u8,
    pub executable_sha256: String,
    pub argv_sha256: String,
    pub stdin_bytes: usize,
    pub stdin_sha256: String,
    pub stdout_bytes: usize,
    pub stdout_sha256: String,
    pub stderr_bytes: usize,
    pub stderr_sha256: String,
    pub exit: u8,
}
pub(super) fn arguments(nonce: &str, vector: u8) -> Result<Vec<OsString>> {
    if !records::hexadecimal(nonce, 64) || vector > 2 {
        return Err(bad());
    }
    Ok(vec![
        nonce.into(),
        vector.to_string().into(),
        OsString::from(""),
        OsString::from("quoted ' \" \\ $() ; * ?"),
        OsString::from("line\n雪"),
        OsString::from_vec(vec![b'e', 0xff, b'd']),
    ])
}
pub(super) fn bytes(nonce: &str, vector: u8) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let _ = arguments(nonce, vector)?;
    let key = hex::decode(nonce).map_err(|_| bad())?;
    let input = (0..INPUT)
        .map(|i| {
            (i as u8)
                .wrapping_add(key[(i / 256) % 32])
                .wrapping_add(vector)
        })
        .collect();
    let stream = |label: u8| {
        (0..STREAM)
            .map(|i| {
                (i as u8)
                    .wrapping_mul(17)
                    .wrapping_add(key[(i / 256) % 32])
                    .wrapping_add(label)
                    .wrapping_add(vector)
            })
            .collect()
    };
    Ok((input, stream(23), stream(149)))
}
pub(super) fn receipt_path(executable: &Path, nonce: &str) -> Result<PathBuf> {
    if !records::hexadecimal(nonce, 64) || !executable.is_absolute() {
        return Err(bad());
    }
    Ok(executable
        .parent()
        .ok_or_else(bad)?
        .join("receipts")
        .join(format!("{nonce}.receipt")))
}
pub(super) fn argv_digest(executable: &Path, args: &[OsString]) -> Result<String> {
    let mut h = Sha256::new();
    for arg in std::iter::once(executable.as_os_str()).chain(args.iter().map(OsString::as_os_str)) {
        let b = arg.as_bytes();
        let n = u32::try_from(b.len()).map_err(|_| bad())?;
        h.update(n.to_be_bytes());
        h.update(b);
    }
    Ok(hex::encode(h.finalize()))
}
pub(super) fn expected_receipt(
    executable: &Path,
    nonce: &str,
    vector: u8,
    executable_sha256: &str,
) -> Result<Receipt> {
    let args = arguments(nonce, vector)?;
    let (input, stdout, stderr) = bytes(nonce, vector)?;
    Ok(Receipt {
        schema: 1,
        nonce: nonce.into(),
        vector,
        executable_sha256: executable_sha256.into(),
        argv_sha256: argv_digest(executable, &args)?,
        stdin_bytes: input.len(),
        stdin_sha256: records::digest(&input),
        stdout_bytes: stdout.len(),
        stdout_sha256: records::digest(&stdout),
        stderr_bytes: stderr.len(),
        stderr_sha256: records::digest(&stderr),
        exit: 7,
    })
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RecoveryReceipt {
    pub schema: u8,
    pub mode: String,
    pub nonce: String,
    pub executable_sha256: String,
    pub argv_sha256: String,
    pub stdin_bytes: usize,
    pub stdin_sha256: String,
    pub stdout_bytes: usize,
    pub stdout_sha256: String,
    pub stderr_bytes: usize,
    pub stderr_sha256: String,
    pub marker_sha256: String,
    pub exit: u8,
}
pub(super) fn recovery_arguments(nonce: &str) -> Result<Vec<OsString>> {
    let mut args = arguments(nonce, 0)?;
    args[1] = "recovery".into();
    Ok(args)
}
pub(super) fn expected_recovery_receipt(
    executable: &Path,
    nonce: &str,
    hash: &str,
) -> Result<RecoveryReceipt> {
    let (input, stdout, stderr) = bytes(nonce, 0)?;
    Ok(RecoveryReceipt {
        schema: 1,
        mode: "recovery".into(),
        nonce: nonce.into(),
        executable_sha256: hash.into(),
        argv_sha256: argv_digest(executable, &recovery_arguments(nonce)?)?,
        stdin_bytes: input.len(),
        stdin_sha256: records::digest(&input),
        stdout_bytes: stdout.len(),
        stdout_sha256: records::digest(&stdout),
        stderr_bytes: stderr.len(),
        stderr_sha256: records::digest(&stderr),
        marker_sha256: records::digest(b"run\n"),
        exit: 7,
    })
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct MarkerIdentity {
    pub device: u64,
    pub inode: u64,
}
fn file_identity(m: &fs::Metadata) -> MarkerIdentity {
    MarkerIdentity {
        device: m.dev(),
        inode: m.ino(),
    }
}
fn owned_regular(m: &fs::Metadata, max_links: u64) -> bool {
    m.is_file()
        && m.uid() == unsafe { libc::geteuid() }
        && m.mode() & 0o777 == 0o600
        && m.nlink() > 0
        && m.nlink() <= max_links
}
fn private_directory(path: &Path) -> Result<File> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
        return Err(bad());
    }
    Ok(file)
}
fn directory_unchanged(path: &Path, held: &File) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o700
        || file_identity(&m) != file_identity(&held.metadata()?)
    {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn recovery_paths(executable: &Path, nonce: &str) -> Result<(PathBuf, PathBuf)> {
    let _ = recovery_arguments(nonce)?;
    if !executable.is_absolute() {
        return Err(bad());
    }
    let dir = executable.parent().ok_or_else(bad)?.join("recovery");
    Ok((
        dir.join(format!("{nonce}.started")),
        dir.join(format!("{nonce}.release")),
    ))
}
fn absent(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e.into()),
    }
}
fn read_fixed(path: &Path, expected: &[u8], max_links: u64) -> Result<MarkerIdentity> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let before = file.metadata()?;
    if !owned_regular(&before, max_links) || before.len() != expected.len() as u64 {
        return Err(bad());
    }
    let mut content = Vec::new();
    (&mut file)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut content)?;
    let path_meta = fs::symlink_metadata(path)?;
    if content != expected
        || file_identity(&before) != file_identity(&path_meta)
        || !owned_regular(&path_meta, max_links)
    {
        return Err(bad());
    }
    Ok(file_identity(&before))
}
pub(super) fn recovery_marker(executable: &Path, nonce: &str) -> Result<MarkerIdentity> {
    let (marker, gate) = recovery_paths(executable, nonce)?;
    if !absent(&gate)? {
        return Err(bad());
    }
    read_fixed(&marker, b"run\n", 1)
}
pub(super) fn hold_recovery_marker(
    executable: &Path,
    nonce: &str,
    identity: MarkerIdentity,
) -> Result<File> {
    let (marker, _) = recovery_paths(executable, nonce)?;
    let held = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&marker)?;
    if file_identity(&held.metadata()?) != identity {
        return Err(bad());
    }
    verify_recovery_marker(executable, nonce, identity)?;
    Ok(held)
}
pub(super) fn verify_recovery_marker(
    executable: &Path,
    nonce: &str,
    identity: MarkerIdentity,
) -> Result<()> {
    let (marker, _) = recovery_paths(executable, nonce)?;
    if read_fixed(&marker, b"run\n", 1)? != identity {
        return Err(bad());
    }
    Ok(())
}
pub(super) fn release_recovery(executable: &Path, nonce: &str) -> Result<()> {
    let (marker, gate) = recovery_paths(executable, nonce)?;
    let parent = gate.parent().ok_or_else(bad)?;
    let held = private_directory(parent)?;
    read_fixed(&marker, b"run\n", 1)?;
    directory_unchanged(parent, &held)?;
    if !absent(&gate)? {
        read_fixed(&gate, b"release\n", 2)?;
        return Ok(());
    }
    records::write_raw(&gate, b"release\n")?;
    directory_unchanged(parent, &held)?;
    read_fixed(&gate, b"release\n", 2)?;
    Ok(())
}
fn execute_recovery(executable: &Path, nonce: &str, supplied: &[OsString]) -> Result<u8> {
    if supplied != recovery_arguments(nonce)? {
        return Err(bad());
    }
    let (input, stdout, stderr) = bytes(nonce, 0)?;
    let mut actual = Vec::new();
    std::io::stdin()
        .take((INPUT + 1) as u64)
        .read_to_end(&mut actual)?;
    if actual != input {
        return Err(bad());
    }
    let (marker, gate) = recovery_paths(executable, nonce)?;
    let directory = gate.parent().ok_or_else(bad)?;
    let held = private_directory(directory)?;
    if !absent(&gate)? {
        return Err(bad());
    }
    let mut started = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&marker)?;
    started.write_all(b"run\n")?;
    started.sync_all()?;
    let identity = read_fixed(&marker, b"run\n", 1)?;
    directory_unchanged(directory, &held)?;
    for (stream, bytes) in [(0, &stdout), (1, &stderr)] {
        if stream == 0 {
            let mut o = std::io::stdout().lock();
            o.write_all(&bytes[..65536])?;
            o.flush()?;
        } else {
            let mut o = std::io::stderr().lock();
            o.write_all(&bytes[..65536])?;
            o.flush()?;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(900);
    loop {
        directory_unchanged(directory, &held)?;
        if read_fixed(&marker, b"run\n", 1)? != identity {
            return Err(bad());
        }
        if !absent(&gate)? {
            read_fixed(&gate, b"release\n", 2)?;
            break;
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    {
        let mut o = std::io::stdout().lock();
        o.write_all(&stdout[65536..])?;
        o.flush()?;
    }
    {
        let mut o = std::io::stderr().lock();
        o.write_all(&stderr[65536..])?;
        o.flush()?;
    }
    directory_unchanged(directory, &held)?;
    if read_fixed(&marker, b"run\n", 1)? != identity {
        return Err(bad());
    }
    let executable_bytes = crate::native_manifest::read_bounded(executable, 256 * 1024 * 1024)?;
    let receipt =
        expected_recovery_receipt(executable, nonce, &records::digest(&executable_bytes))?;
    records::write(&receipt_path(executable, nonce)?, &receipt)?;
    Ok(7)
}
fn execute() -> Result<u8> {
    let supplied: Vec<_> = std::env::args_os().collect();
    if supplied
        .get(1)
        .is_some_and(|a| a == "edge-v1" || a == "--server")
    {
        return super::edge_fixture::execute(&supplied[1..]);
    }
    if supplied.get(1).is_some_and(|a| a == "transfer-v1") {
        return super::transfer_fixture::execute(&supplied[1..]);
    }
    if supplied.get(1).is_some_and(|a| a == "stream-v1") {
        return super::streaming_fixture::execute(&supplied[1..]);
    }
    if supplied.len() != 7 {
        return Err(bad());
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    if supplied[0].as_bytes() != executable.as_os_str().as_bytes() {
        return Err(bad());
    }
    let nonce = supplied[1].to_str().ok_or_else(bad)?;
    if supplied[2] == "recovery" {
        return execute_recovery(&executable, nonce, &supplied[1..]);
    }
    let vector = match supplied[2].to_str() {
        Some("0") => 0,
        Some("1") => 1,
        Some("2") => 2,
        _ => return Err(bad()),
    };
    let args = arguments(nonce, vector)?;
    if supplied[1..] != args {
        return Err(bad());
    }
    let (input, stdout, stderr) = bytes(nonce, vector)?;
    let mut actual = Vec::new();
    std::io::stdin()
        .take((INPUT + 1) as u64)
        .read_to_end(&mut actual)?;
    if actual != input {
        return Err(bad());
    }
    {
        let mut out = std::io::stdout().lock();
        out.write_all(&stdout)?;
        out.flush()?;
    }
    {
        let mut out = std::io::stderr().lock();
        out.write_all(&stderr)?;
        out.flush()?;
    }
    let executable_bytes = crate::native_manifest::read_bounded(&executable, 256 * 1024 * 1024)?;
    let receipt = expected_receipt(
        &executable,
        nonce,
        vector,
        &records::digest(&executable_bytes),
    )?;
    records::write(&receipt_path(&executable, nonce)?, &receipt)?;
    Ok(7)
}
pub(crate) fn entry() -> u8 {
    if std::env::args_os().count() == 2
        && std::env::args_os()
            .nth(1)
            .is_some_and(|a| a == "--release-info")
    {
        return super::release_info();
    }
    match execute() {
        Ok(exit) => exit,
        Err(_) => {
            eprintln!("marmish-installed-fixture: invalid_or_failed");
            125
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn recovery_fixture() -> (tempfile::TempDir, String) {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(root.path().join("recovery")).unwrap();
        fs::set_permissions(
            root.path().join("recovery"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        (root, "1".repeat(64))
    }
    #[test]
    fn recovery_gate_is_fixed_owned_atomic_and_marker_is_single_execution() {
        let (root, nonce) = recovery_fixture();
        let exe = root.path().join("main");
        let (marker, gate) = recovery_paths(&exe, &nonce).unwrap();
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&marker)
            .unwrap();
        f.write_all(b"run\n").unwrap();
        f.sync_all().unwrap();
        let identity = recovery_marker(&exe, &nonce).unwrap();
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)
                .is_err()
        );
        assert!(!gate.exists());
        release_recovery(&exe, &nonce).unwrap();
        assert_eq!(fs::read(&gate).unwrap(), b"release\n");
        release_recovery(&exe, &nonce).unwrap();
        assert!(recovery_marker(&exe, &nonce).is_err());
        verify_recovery_marker(&exe, &nonce, identity).unwrap();
        assert_eq!(fs::read_dir(gate.parent().unwrap()).unwrap().count(), 2);
        let original = expected_receipt(&exe, &nonce, 0, &"a".repeat(64)).unwrap();
        let gated = expected_recovery_receipt(&exe, &nonce, &"a".repeat(64)).unwrap();
        assert_ne!(original.argv_sha256, gated.argv_sha256);
        assert_eq!(original.stdout_sha256, gated.stdout_sha256);
    }
    #[test]
    fn recovery_refuses_unsafe_markers_gates_and_changed_directory() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (root, nonce) = recovery_fixture();
        let exe = root.path().join("main");
        let (marker, gate) = recovery_paths(&exe, &nonce).unwrap();
        fs::write(&marker, b"run\n").unwrap();
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
        let original = recovery_marker(&exe, &nonce).unwrap();
        let held = private_directory(marker.parent().unwrap()).unwrap();
        let link = root.path().join("link");
        fs::hard_link(&marker, &link).unwrap();
        assert!(recovery_marker(&exe, &nonce).is_err());
        fs::remove_file(link).unwrap();
        symlink(&marker, &gate).unwrap();
        assert!(release_recovery(&exe, &nonce).is_err());
        fs::remove_file(&gate).unwrap();
        let c = std::ffi::CString::new(gate.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(release_recovery(&exe, &nonce).is_err());
        fs::remove_file(&gate).unwrap();
        // Keep the original inode alive; unlink/recreate may legitimately recycle it.
        let original_file = hold_recovery_marker(&exe, &nonce, original).unwrap();
        fs::remove_file(&marker).unwrap();
        fs::write(&marker, b"run\n").unwrap();
        fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(verify_recovery_marker(&exe, &nonce, original).is_err());
        drop(original_file);
        let old = root.path().join("old");
        fs::rename(marker.parent().unwrap(), &old).unwrap();
        fs::create_dir(root.path().join("recovery")).unwrap();
        fs::set_permissions(
            root.path().join("recovery"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert!(directory_unchanged(&root.path().join("recovery"), &held).is_err());
    }
    #[test]
    fn fixed_vectors_have_every_byte_and_distinct_streams() {
        let n = "12".repeat(32);
        for v in 0..3 {
            let (i, o, e) = bytes(&n, v).unwrap();
            assert_eq!((i.len(), o.len(), e.len()), (16384, 131072, 131072));
            assert_eq!(
                i.iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                256
            );
            assert_ne!(o, e);
            assert!(
                arguments(&n, v)
                    .unwrap()
                    .last()
                    .unwrap()
                    .as_bytes()
                    .contains(&255)
            );
        }
    }
    #[test]
    fn nonce_cannot_control_receipt_path() {
        assert!(receipt_path(Path::new("/tmp/main"), "../../x").is_err());
        assert_eq!(
            receipt_path(Path::new("/tmp/f/main"), &"1".repeat(64)).unwrap(),
            PathBuf::from(format!("/tmp/f/receipts/{}.receipt", "1".repeat(64)))
        );
    }
}
