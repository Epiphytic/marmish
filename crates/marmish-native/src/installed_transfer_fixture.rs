//! Closed filesystem fixtures. Artifact paths never choose a local execution root.
use super::{records, streaming_fixture};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
fn bad() -> Error {
    Error::Invalid("installed_transfer_fixture")
}
pub(super) const MTIME: u64 = 1_700_000_000;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Item {
    pub path_hex: String,
    pub kind: String,
    pub bytes: u64,
    pub sha256: Option<String>,
    pub mode: u32,
    pub mtime: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub schema: u8,
    pub nonce: String,
    pub client: String,
    pub case: String,
    pub executable_sha256: String,
    pub items: Vec<Item>,
    pub checkpoint: Option<super::copy_checkpoint::Proof>,
}
pub(super) fn arguments(
    action: &str,
    nonce: &str,
    client: &str,
    case: &str,
) -> Result<Vec<OsString>> {
    if !matches!(action, "init" | "observe" | "absent")
        || !records::hexadecimal(nonce, 64)
        || !records::ROLES.contains(&client)
        || !matches!(case, "push" | "pull" | "put" | "get")
    {
        return Err(bad());
    }
    Ok(["transfer-v1", action, nonce, client, case]
        .into_iter()
        .map(OsString::from)
        .collect())
}
pub(super) fn target(nonce: &str, case: &str) -> Result<PathBuf> {
    arguments("observe", nonce, records::ROLES[0], case)?;
    Ok(PathBuf::from("transfer-v1").join(format!("{case}-{nonce}")))
}
pub(super) fn receipt_path(executable: &Path, nonce: &str, case: &str) -> Result<PathBuf> {
    arguments("observe", nonce, records::ROLES[0], case)?;
    Ok(executable
        .parent()
        .ok_or_else(bad)?
        .join("receipts")
        .join(format!("{case}-{nonce}.json")))
}
fn files(case: &str) -> Result<Vec<(String, usize)>> {
    if matches!(case, "put" | "get") {
        return Ok(vec![("data".into(), 65536)]);
    }
    if !matches!(case, "push" | "pull") {
        return Err(bad());
    }
    let names = [
        "empty",
        "nested/a",
        "nested/b",
        "space name",
        "雪",
        "line\nbreak",
        "-dash",
        "quote'\"",
        "$() ; [*]",
        "f09",
        "f10",
        "f11",
        "f12",
        "f13",
        "f14",
        "last",
    ];
    Ok(names
        .into_iter()
        .enumerate()
        .map(|(i, n)| {
            (
                n.to_owned(),
                if i == 0 {
                    0
                } else if i == 15 {
                    4096
                } else {
                    2048
                },
            )
        })
        .collect())
}
pub(super) fn expected(nonce: &str, case: &str) -> Result<Vec<Item>> {
    let files = files(case)?;
    let mut items = Vec::new();
    for (name, size) in files {
        let bytes = streaming_fixture::bytes(nonce, 91, size)?;
        items.push(Item {
            path_hex: hex::encode(name.as_bytes()),
            kind: "file".into(),
            bytes: size as u64,
            sha256: Some(records::digest(&bytes)),
            mode: 0o600,
            mtime: MTIME,
        });
    }
    if matches!(case, "push" | "pull") {
        items.push(Item {
            path_hex: hex::encode(b"nested"),
            kind: "directory".into(),
            bytes: 0,
            sha256: None,
            mode: 0o700,
            mtime: MTIME,
        });
    }
    items.sort_by(|a, b| a.path_hex.cmp(&b.path_hex));
    Ok(items)
}
pub(super) fn create(root: &Path, nonce: &str, case: &str) -> Result<()> {
    if root.try_exists()? {
        return Err(bad());
    }
    fs::DirBuilder::new().mode(0o700).create(root)?;
    if matches!(case, "push" | "pull") {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join("nested"))?;
    }
    for (name, size) in files(case)? {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root.join(name))?;
        file.write_all(&streaming_fixture::bytes(nonce, 91, size)?)?;
        file.sync_all()?;
        file.set_modified(UNIX_EPOCH + Duration::from_secs(MTIME))?;
    }
    if root.join("nested").is_dir() {
        fs::File::open(root.join("nested"))?
            .set_modified(UNIX_EPOCH + Duration::from_secs(MTIME))?;
    }
    Ok(())
}
pub(super) fn inventory(root: &Path) -> Result<Vec<Item>> {
    inventory_except(root, None)
}
/// Only the checkpoint observer supplies an exact, already validated namespace.
/// Unknown or additional private directories remain visible and fail fixtures.
pub(super) fn inventory_except(root: &Path, excluded: Option<&str>) -> Result<Vec<Item>> {
    fn walk(
        root: &Path,
        relative: &Path,
        items: &mut Vec<Item>,
        depth: usize,
        excluded: Option<&str>,
    ) -> Result<()> {
        if depth > 4 {
            return Err(bad());
        }
        let path = root.join(relative);
        crate::identity::check_dir(&path)?;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if depth == 0
                && excluded.is_some_and(|name| entry.file_name() == std::ffi::OsStr::new(name))
            {
                continue;
            }
            let relative = relative.join(entry.file_name());
            let path = root.join(&relative);
            let m = fs::symlink_metadata(&path)?;
            if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 || items.len() >= 32 {
                return Err(bad());
            }
            use std::os::unix::ffi::OsStrExt;
            let (kind, bytes, sha256) = if m.is_dir() {
                ("directory", 0, None)
            } else if m.is_file() {
                (
                    "file",
                    m.len(),
                    Some(records::digest(&records::read_bytes(
                        &path,
                        4 * 1024 * 1024,
                    )?)),
                )
            } else {
                return Err(bad());
            };
            items.push(Item {
                path_hex: hex::encode(relative.as_os_str().as_bytes()),
                kind: kind.into(),
                bytes,
                sha256,
                mode: m.mode() & 0o777,
                mtime: u64::try_from(m.mtime()).map_err(|_| bad())?,
            });
            if m.is_dir() {
                walk(root, &relative, items, depth + 1, excluded)?;
            }
        }
        Ok(())
    }
    let mut items = Vec::new();
    walk(root, Path::new(""), &mut items, 0, excluded)?;
    items.sort_by(|a, b| a.path_hex.cmp(&b.path_hex));
    Ok(items)
}
pub(super) fn checkpoint_arguments(
    nonce: &str,
    client: &str,
    launch: &marmish_stream::copy_protocol::Launch,
    stream: [u8; 16],
) -> Result<Vec<OsString>> {
    use std::os::unix::ffi::OsStrExt;
    launch.validate().map_err(|_| bad())?;
    let target = target(nonce, "put")?.join("data");
    if launch.direction != marmish_stream::copy_protocol::Direction::Put
        || launch.destination != target.as_os_str().as_bytes()
    {
        return Err(bad());
    }
    let bytes = records::canonical(launch)?;
    if bytes.len() > 4096 {
        return Err(bad());
    }
    let mut args = arguments("observe", nonce, client, "put")?;
    args.extend([hex::encode(bytes).into(), hex::encode(stream).into()]);
    Ok(args)
}
pub(super) fn execute(args: &[OsString]) -> Result<u8> {
    if args.len() != 5 && args.len() != 7 {
        return Err(bad());
    }
    let fields = args
        .iter()
        .map(|a| a.to_str().ok_or_else(bad))
        .collect::<Result<Vec<_>>>()?;
    let [_, action, nonce, client, case] = &fields[..5] else {
        return Err(bad());
    };
    let checkpoint = if args.len() == 7 {
        if *action != "observe" || *case != "put" || fields[5].len() > 8192 {
            return Err(bad());
        }
        let launch: marmish_stream::copy_protocol::Launch =
            serde_json::from_slice(&hex::decode(fields[5]).map_err(|_| bad())?)
                .map_err(|_| bad())?;
        let stream: [u8; 16] = hex::decode(fields[6])
            .map_err(|_| bad())?
            .try_into()
            .map_err(|_| bad())?;
        if args != checkpoint_arguments(nonce, client, &launch, stream)? {
            return Err(bad());
        }
        Some((launch, stream))
    } else {
        if args != arguments(action, nonce, client, case)?
            || (*action == "observe" && *case == "put")
        {
            return Err(bad());
        }
        None
    };
    let executable = std::env::current_exe()?.canonicalize()?;
    let workspace = executable.parent().and_then(Path::parent).ok_or_else(bad)?;
    let parent = workspace.join("transfer-v1");
    match fs::DirBuilder::new().mode(0o700).create(&parent) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    crate::identity::check_dir(&parent)?;
    let root = workspace.join(target(nonce, case)?);
    match *action {
        "init" => {
            create(&root, nonce, case)?;
        }
        "absent" => {
            match fs::symlink_metadata(&root) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(bad()),
            }
            if *case == "put" {
                fs::DirBuilder::new().mode(0o700).create(&root)?;
            }
            return Ok(0);
        }
        "observe" => {}
        _ => return Err(bad()),
    }
    let (checkpoint, items) = if let Some((launch, stream)) = checkpoint {
        let expected_items = expected(nonce, case)?;
        let digest: [u8; 32] = hex::decode(expected_items[0].sha256.as_ref().ok_or_else(bad)?)
            .map_err(|_| bad())?
            .try_into()
            .map_err(|_| bad())?;
        let (proof, items) = super::copy_checkpoint::inventory(super::copy_checkpoint::Expected {
            parent: &root.canonicalize()?,
            launch: &launch,
            stream,
            size: expected_items[0].bytes,
            sha256: digest,
            committed: true,
        })?;
        (Some(proof), items)
    } else {
        (None, inventory(&root)?)
    };
    if items != expected(nonce, case)? {
        return Err(bad());
    }
    let receipt = Receipt {
        schema: 1,
        nonce: (*nonce).into(),
        client: (*client).into(),
        case: (*case).into(),
        executable_sha256: records::digest(&records::read_bytes(&executable, 268435456)?),
        items,
        checkpoint,
    };
    let bytes = records::canonical(&receipt)?;
    std::io::stdout().write_all(&bytes)?;
    if *action == "observe" {
        records::write(&receipt_path(&executable, nonce, case)?, &receipt)?;
    }
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_literal_tree_matches_inventory_and_rejects_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("tree");
        let nonce = "ab".repeat(32);
        create(&root, &nonce, "push").unwrap();
        let items = inventory(&root).unwrap();
        assert_eq!(items, expected(&nonce, "push").unwrap());
        assert_eq!(items.iter().filter(|i| i.kind == "file").count(), 16);
        assert_eq!(items.iter().map(|i| i.bytes).sum::<u64>(), 32768);
        fs::remove_file(root.join("empty")).unwrap();
        std::os::unix::fs::symlink("../outside", root.join("empty")).unwrap();
        assert!(inventory(&root).is_err());
        assert!(arguments("observe", &nonce, records::ROLES[0], "../escape").is_err());
    }
}
