//! Exact release grammar. Download names never become arbitrary paths or URLs.
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub(crate) const NAMES: [&str; 4] = [
    "marmish",
    "marmishd",
    "marmish-worker",
    "THIRD-PARTY-LICENSES",
];
pub(crate) const TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asset {
    pub name: &'static str,
    pub sha256: [u8; 32],
    pub bytes: u64,
    pub mode: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub version: String,
    pub target: String,
    pub commit: String,
    pub signing: String,
    pub notarized: bool,
    pub minimum: String,
    pub formats: [u32; 5],
    pub files: Vec<Asset>,
}
#[derive(Clone, Copy)]
pub(crate) enum AssetNaming {
    VersionedDownloads,
    InstalledFixedNames,
}
#[derive(Serialize)]
pub(crate) struct ReleaseMetadata {
    pub schema: u8,
    pub version: &'static str,
    pub target: &'static str,
    pub commit: &'static str,
    pub formats: [u32; 5],
}
pub(crate) fn compiled_release_metadata() -> ReleaseMetadata {
    ReleaseMetadata {
        schema: 1,
        version: env!("CARGO_PKG_VERSION"),
        target: env!("MARMISH_BUILD_TARGET"),
        commit: env!("MARMISH_BUILD_COMMIT"),
        formats: [
            u32::from(crate::identity::PROFILE_FORMAT),
            crate::journal::JOURNAL_FORMAT as u32,
            marmish_engine::ENGINE_FORMAT as u32,
            crate::command_config::CONFIG_FORMAT,
            crate::command_wire::COMMAND_FORMAT,
        ],
    }
}
pub(crate) fn version(value: &str) -> bool {
    let Some(rest) = value
        .strip_prefix("0.2.")
        .or_else(|| value.strip_prefix("0.3."))
    else {
        return false;
    };
    match rest.split_once("-rc.") {
        Some((patch, rc)) => decimal(patch, false).is_some() && decimal(rc, true).is_some(),
        None => decimal(rest, false).is_some(),
    }
}
fn decimal(value: &str, positive: bool) -> Option<u64> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|v| v.is_ascii_digit())
    {
        return None;
    }
    let n = value.parse::<u64>().ok()?;
    (!positive || n > 0).then_some(n)
}
pub(crate) fn hex32(value: &str) -> Result<[u8; 32]> {
    if !crate::foundation::valid_hex(value, 64) {
        return Err(Error::Invalid("install_manifest"));
    }
    let mut out = [0; 32];
    hex::decode_to_slice(value, &mut out).map_err(|_| Error::Invalid("install_manifest"))?;
    Ok(out)
}
impl Manifest {
    pub(crate) fn id(&self) -> String {
        format!("{}-{}", self.version, self.target)
    }
    pub(crate) fn asset_name(&self, name: &str) -> String {
        format!("{name}-{}-{}", self.version, self.target)
    }
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let bad = || Error::Invalid("install_manifest");
        if bytes.len() > 4096
            || !bytes.is_ascii()
            || !bytes.ends_with(b"\n")
            || bytes.contains(&b'\r')
            || bytes.contains(&b'\t')
        {
            return Err(bad());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| bad())?;
        let lines: Vec<_> = text
            .strip_suffix('\n')
            .ok_or_else(bad)?
            .split('\n')
            .collect();
        if lines.len() != 12
            || lines
                .iter()
                .any(|l| l.is_empty() || l.starts_with(' ') || l.ends_with(' ') || l.contains("  "))
            || lines[0] != "marmish-manifest 1"
            || lines[11] != "end"
        {
            return Err(bad());
        }
        let take = |n: usize, p: &str| lines[n].strip_prefix(p).ok_or_else(bad);
        let v = take(1, "version ")?;
        if !version(v) {
            return Err(bad());
        }
        let target = take(2, "target ")?;
        if !TARGETS.contains(&target) {
            return Err(bad());
        }
        let commit = take(3, "commit ")?;
        if !crate::foundation::valid_hex(commit, 40) {
            return Err(bad());
        }
        let signing: Vec<_> = take(4, "signing ")?.split(' ').collect();
        if signing.len() != 2 {
            return Err(bad());
        }
        let notarized = match signing[1] {
            "notarized=yes" => true,
            "notarized=no" => false,
            _ => return Err(bad()),
        };
        let minimum = take(5, "minimum ")?;
        let (family, nums) = minimum.split_once('=').ok_or_else(bad)?;
        let (major, minor) = nums.split_once('.').ok_or_else(bad)?;
        if decimal(major, false).is_none() || decimal(minor, false).is_none() {
            return Err(bad());
        }
        if target.ends_with("linux-gnu") {
            if family != "glibc" || signing[0] != "none" || notarized {
                return Err(bad());
            }
        } else if family != "macos"
            || !["adhoc", "developer-id"].contains(&signing[0])
            || (signing[0] == "adhoc" && notarized)
        {
            return Err(bad());
        }
        let state: Vec<_> = take(6, "state ")?.split(' ').collect();
        let mut formats = [0; 5];
        if state.len() != 5 {
            return Err(bad());
        }
        for (i, key) in ["profile=", "journal=", "engine=", "config=", "commands="]
            .iter()
            .enumerate()
        {
            formats[i] = decimal(state[i].strip_prefix(key).ok_or_else(bad)?, true)
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(bad)?;
        }
        let mut files = Vec::new();
        let mut total = 0;
        for (i, name) in NAMES.iter().enumerate() {
            let parts: Vec<_> = lines[7 + i].split(' ').collect();
            let mode = if i == 3 { 0o444 } else { 0o555 };
            if parts.len() != 5
                || parts[0] != "file"
                || parts[4] != *name
                || parts[3] != if i == 3 { "0444" } else { "0555" }
            {
                return Err(bad());
            }
            let bytes = decimal(parts[2], true).ok_or_else(bad)?;
            if bytes
                > if i == 3 {
                    16 * 1024 * 1024
                } else {
                    256 * 1024 * 1024
                }
            {
                return Err(bad());
            }
            total += bytes;
            files.push(Asset {
                name,
                sha256: hex32(parts[1])?,
                bytes,
                mode,
            });
        }
        if total > 784 * 1024 * 1024 {
            return Err(bad());
        }
        Ok(Self {
            version: v.into(),
            target: target.into(),
            commit: commit.into(),
            signing: signing[0].into(),
            notarized,
            minimum: minimum.into(),
            formats,
            files,
        })
    }
}
pub(crate) fn safe_file(path: &Path, max: u64) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let m = f.metadata()?;
    if !m.is_file() || m.nlink() != 1 || m.len() > max {
        return Err(Error::Invalid("unsafe_install_path"));
    }
    Ok(f)
}
pub(crate) fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    safe_file(path, max)?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(Error::Invalid("install_manifest"));
    }
    Ok(bytes)
}
pub(crate) fn verify_file(path: &Path, asset: &Asset, installed: bool) -> Result<()> {
    let mut file = safe_file(path, asset.bytes)?;
    let m = file.metadata()?;
    if m.len() != asset.bytes
        || (installed && (m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != asset.mode))
    {
        return Err(Error::Invalid("install_asset"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if <[u8; 32]>::from(hash.finalize()) != asset.sha256 {
        return Err(Error::Invalid("install_digest"));
    }
    Ok(())
}
pub(crate) fn verify_directory(
    dir: &Path,
    naming: AssetNaming,
    expected: [u8; 32],
) -> Result<Manifest> {
    let manifest_path = match naming {
        AssetNaming::InstalledFixedNames => dir.join("manifest"),
        AssetNaming::VersionedDownloads => {
            let entries = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
            let mut candidates = entries.into_iter().filter(|entry| {
                entry.file_name().to_str().is_some_and(|n| {
                    (n.starts_with("marmish-0.2.") || n.starts_with("marmish-0.3."))
                        && n.ends_with(".manifest")
                })
            });
            let first = candidates
                .next()
                .ok_or(Error::Invalid("install_manifest_path"))?;
            if candidates.next().is_some() {
                return Err(Error::Invalid("install_manifest_path"));
            }
            first.path()
        }
    };
    verify_downloads(dir, &manifest_path, naming, expected)
}
pub(crate) fn verify_downloads(
    dir: &Path,
    path: &Path,
    naming: AssetNaming,
    expected: [u8; 32],
) -> Result<Manifest> {
    let bytes = read_bounded(path, 4096)?;
    if <[u8; 32]>::from(Sha256::digest(&bytes)) != expected {
        return Err(Error::Invalid("install_digest"));
    }
    let manifest = Manifest::parse(&bytes)?;
    if matches!(naming, AssetNaming::VersionedDownloads)
        && path != dir.join(format!("{}.manifest", manifest.asset_name("marmish")))
    {
        return Err(Error::Invalid("install_manifest_path"));
    }
    for asset in &manifest.files {
        let name = match naming {
            AssetNaming::VersionedDownloads => manifest.asset_name(asset.name),
            AssetNaming::InstalledFixedNames => asset.name.into(),
        };
        verify_file(
            &dir.join(name),
            asset,
            matches!(naming, AssetNaming::InstalledFixedNames),
        )?;
    }
    Ok(manifest)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> String {
        format!(
            "marmish-manifest 1\nversion 0.2.0\ntarget {}\ncommit {}\nsigning none notarized=no\nminimum glibc=2.39\nstate profile=2 journal=5 engine=1 config=1 commands=1\nfile {} 1 0555 marmish\nfile {} 1 0555 marmishd\nfile {} 1 0555 marmish-worker\nfile {} 1 0444 THIRD-PARTY-LICENSES\nend\n",
            TARGETS[0],
            "a".repeat(40),
            "b".repeat(64),
            "b".repeat(64),
            "b".repeat(64),
            "b".repeat(64)
        )
    }
    #[test]
    fn strict_vectors() {
        let s = sample();
        assert!(Manifest::parse(s.as_bytes()).is_ok());
        for (a, b) in [
            ("0.2.0", "0.1.9"),
            ("0.2.0", "0.2.01"),
            ("0.2.0", "0.2.0-rc.0"),
            ("journal=5", "journal=0"),
            ("1 0555", "01 0555"),
            ("end\n", "end\n\n"),
            ("glibc=2.39", "macos=15.0"),
        ] {
            assert!(Manifest::parse(s.replace(a, b).as_bytes()).is_err(), "{b}");
        }
        assert!(Manifest::parse(s.trim_end().as_bytes()).is_err());
        assert!(version("0.2.0-rc.1"));
        assert!(!version("0.2.0-rc.01"));
    }

    #[test]
    fn shared_release_version_vectors() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../../tools/release-version-vectors.json"))
                .unwrap();
        for (kind, expected) in [("valid", true), ("invalid", false)] {
            for value in vectors[kind].as_array().unwrap() {
                let value = value.as_str().unwrap();
                assert_eq!(version(value), expected, "{value:?}");
                assert_eq!(
                    Manifest::parse(
                        sample()
                            .replace("version 0.2.0\n", &format!("version {value}\n"))
                            .as_bytes()
                    )
                    .is_ok(),
                    expected,
                    "manifest {value:?}"
                );
            }
        }
    }

    #[test]
    fn versioned_directory_discovery_supports_both_families_and_rejects_ambiguity() {
        for version in ["0.2.3", "0.3.0", "0.3.1"] {
            let root = tempfile::tempdir().unwrap();
            let digest = hex::encode(Sha256::digest(b"x"));
            let text = sample()
                .replace("0.2.0", version)
                .replace(&"b".repeat(64), &digest);
            let parsed = Manifest::parse(text.as_bytes()).unwrap();
            for name in NAMES {
                fs::write(root.path().join(parsed.asset_name(name)), b"x").unwrap();
            }
            let path = root
                .path()
                .join(format!("marmish-{version}-{}.manifest", TARGETS[0]));
            fs::write(&path, &text).unwrap();
            let pin = Sha256::digest(text.as_bytes()).into();
            assert_eq!(
                verify_directory(root.path(), AssetNaming::VersionedDownloads, pin)
                    .unwrap()
                    .version,
                version
            );
            // A matching malformed name must remain visible to discovery so
            // adding a second candidate cannot silently bypass ambiguity.
            fs::write(root.path().join("marmish-0.3.invalid.manifest"), b"bad").unwrap();
            assert!(verify_directory(root.path(), AssetNaming::VersionedDownloads, pin).is_err());
            fs::remove_file(&path).unwrap();
            assert!(verify_directory(root.path(), AssetNaming::VersionedDownloads, pin).is_err());
        }
    }
}
