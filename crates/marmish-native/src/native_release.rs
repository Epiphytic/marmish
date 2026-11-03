//! Build-time release assembly. Never linked into an installer's trust decision.
use crate::native_manifest::{self as manifest, AssetNaming, Manifest, NAMES, TARGETS};
use crate::{Error, Result};
use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "marmish-release-tool")]
struct Options {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Package the three final signed executables and a complete license inventory.
    Package {
        #[arg(long)]
        input_dir: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long)]
        licenses: PathBuf,
        #[arg(long)]
        signing: String,
        #[arg(long)]
        minimum: String,
        #[arg(long)]
        notarized: bool,
    },
    /// Package build-only installed-test tools separately from install assets.
    PackageTooling {
        #[arg(long)]
        input_dir: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long)]
        harness_sha256: String,
    },
    /// Render one independently usable bootstrap with pins for all three targets.
    Render {
        #[arg(long)]
        assets_dir: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}
fn bad() -> Error {
    Error::Invalid("release_assets")
}
fn metadata() -> Result<manifest::ReleaseMetadata> {
    let m = manifest::compiled_release_metadata();
    if !manifest::version(m.version) || m.commit == "0000000000000000000000000000000000000000" {
        return Err(bad());
    }
    Ok(m)
}
fn probe(path: &Path) -> Result<()> {
    probe_argument(path, "release-info")
}
fn probe_argument(path: &Path, argument: &str) -> Result<()> {
    let expected = serde_json::to_vec(&metadata()?).map_err(|_| bad())?;
    let mut child = Command::new(path)
        .arg(argument)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| {
        let mut stdout = child.stdout.take().ok_or_else(bad)?;
        let fd = stdout.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(bad());
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut bytes = Vec::new();
        let mut eof = false;
        loop {
            let mut buffer = [0; 1024];
            match stdout.read(&mut buffer) {
                Ok(0) => eof = true,
                Ok(n) => {
                    bytes.extend_from_slice(&buffer[..n]);
                    if bytes.len() > 4096 {
                        return Err(bad());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            if eof && let Some(status) = child.try_wait()? {
                if !status.success() || bytes.strip_suffix(b"\n") != Some(expected.as_slice()) {
                    return Err(bad());
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if child.try_wait()?.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
fn asset(path: &Path, max: u64) -> Result<(u64, [u8; 32])> {
    let mut input = manifest::safe_file(path, max)?;
    let bytes = input.metadata()?.len();
    if bytes == 0 {
        return Err(bad());
    }
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > max {
            return Err(bad());
        }
        hash.update(&buffer[..n]);
    }
    if count != bytes {
        return Err(bad());
    }
    Ok((bytes, hash.finalize().into()))
}
fn create(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()?;
    Ok(())
}
fn copy_asset(source: &Path, path: &Path, a: &manifest::Asset) -> Result<()> {
    let input = manifest::safe_file(source, a.bytes)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    if std::io::copy(&mut input.take(a.bytes + 1), &mut file)? != a.bytes {
        return Err(bad());
    }
    file.sync_all()?;
    file.set_permissions(fs::Permissions::from_mode(a.mode))?;
    file.sync_all()?;
    manifest::verify_file(path, a, false)
}
const TOOL_NAMES: [&str; 2] = ["marmish-installed-check", "marmish-installed-fixture"];
fn package_tooling(input: &Path, output: &Path, harness: &str) -> Result<()> {
    let m = metadata()?;
    if harness.len() != 64
        || !harness
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(bad());
    }
    let role = match m.target {
        "x86_64-unknown-linux-gnu" => "linux-amd64",
        "aarch64-unknown-linux-gnu" => "linux-arm64",
        "aarch64-apple-darwin" => "macos-arm64",
        _ => return Err(bad()),
    };
    let mut files = serde_json::Map::new();
    let mut assets = Vec::new();
    for name in TOOL_NAMES {
        let source = input.join(name);
        let (bytes, sha256) = asset(&source, 256 * 1024 * 1024)?;
        probe_argument(&source, "--release-info")?;
        files.insert(
            name.into(),
            serde_json::json!({"sha256": hex::encode(sha256), "bytes": bytes}),
        );
        assets.push(manifest::Asset {
            name,
            bytes,
            sha256,
            mode: 0o555,
        });
    }
    let record = serde_json::json!({
        "schema": 1, "source_commit": m.commit, "target": m.target,
        "harness_sha256": harness, "files": files
    });
    let bytes = serde_json::to_vec(&record).map_err(|_| bad())?;
    fs::create_dir(output)?;
    for a in assets {
        copy_asset(&input.join(a.name), &output.join(a.name), &a)?;
    }
    create(&output.join(format!("{role}-tooling.json")), &bytes, 0o444)?;
    File::open(output)?.sync_all()?;
    Ok(())
}
fn package(
    input: &Path,
    output: &Path,
    licenses: &Path,
    signing: &str,
    minimum: &str,
    notarized: bool,
) -> Result<()> {
    let m = metadata()?;
    let mut text = format!(
        "marmish-manifest 1\nversion {}\ntarget {}\ncommit {}\nsigning {signing} notarized={}\nminimum {minimum}\nstate profile={} journal={} engine={} config={} commands={}\n",
        m.version,
        m.target,
        m.commit,
        if notarized { "yes" } else { "no" },
        m.formats[0],
        m.formats[1],
        m.formats[2],
        m.formats[3],
        m.formats[4]
    );
    let mut sources = Vec::new();
    for (index, name) in NAMES.iter().enumerate() {
        let source = if index == 3 {
            licenses.into()
        } else {
            input.join(name)
        };
        if index < 3 {
            probe(&source)?;
        }
        let (bytes, hash) = asset(
            &source,
            if index == 3 {
                16 * 1024 * 1024
            } else {
                256 * 1024 * 1024
            },
        )?;
        text.push_str(&format!(
            "file {} {bytes} {} {name}\n",
            hex::encode(hash),
            if index == 3 { "0444" } else { "0555" }
        ));
        sources.push(source);
    }
    text.push_str("end\n");
    let parsed = Manifest::parse(text.as_bytes())?;
    // Never overwrite an earlier candidate or its evidence.
    fs::create_dir(output)?;
    for (source, a) in sources.iter().zip(&parsed.files) {
        copy_asset(source, &output.join(parsed.asset_name(a.name)), a)?;
    }
    let path = output.join(format!("marmish-{}-{}.manifest", m.version, m.target));
    create(&path, text.as_bytes(), 0o444)?;
    manifest::verify_downloads(
        output,
        &path,
        AssetNaming::VersionedDownloads,
        Sha256::digest(text.as_bytes()).into(),
    )?;
    File::open(output)?.sync_all()?;
    Ok(())
}
fn render(assets: &Path, output: &Path) -> Result<()> {
    let m = metadata()?;
    let mut script =
        include_str!("../../../scripts/install-native.sh").replace("@MARMISH_VERSION@", m.version);
    let pins = [
        "@MARMISH_SHA_AMD64@",
        "@MARMISH_SHA_ARM64@",
        "@MARMISH_SHA_MACOS@",
    ];
    for (target, pin) in TARGETS.iter().zip(pins) {
        let path = assets.join(format!("marmish-{}-{target}.manifest", m.version));
        let bytes = manifest::read_bounded(&path, 4096)?;
        let parsed = Manifest::parse(&bytes)?;
        if parsed.version != m.version
            || parsed.target != *target
            || parsed.commit != m.commit
            || parsed.formats != m.formats
        {
            return Err(bad());
        }
        let hash = Sha256::digest(&bytes).into();
        manifest::verify_downloads(assets, &path, AssetNaming::VersionedDownloads, hash)?;
        script = script.replace(pin, &hex::encode(hash));
    }
    if script.contains("@MARMISH_") {
        return Err(bad());
    }
    create(output, script.as_bytes(), 0o555)
}
pub fn entry() -> u8 {
    let options = Options::parse();
    let result = match options.action {
        Action::Package {
            input_dir,
            output_dir,
            licenses,
            signing,
            minimum,
            notarized,
        } => package(
            &input_dir,
            &output_dir,
            &licenses,
            &signing,
            &minimum,
            notarized,
        ),
        Action::Render { assets_dir, output } => render(&assets_dir, &output),
        Action::PackageTooling {
            input_dir,
            output_dir,
            harness_sha256,
        } => package_tooling(&input_dir, &output_dir, &harness_sha256),
    };
    match result {
        Ok(()) => 0,
        Err(_) => {
            eprintln!("marmish release: release_assets_failed");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(root: &Path, names: &[&str], argument: &str) -> PathBuf {
        let directory = root.join("input");
        fs::create_dir(&directory).unwrap();
        let metadata = serde_json::to_string(&metadata().unwrap()).unwrap();
        for name in names {
            create(&directory.join(name), format!(
                "#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = {argument} ] || exit 9\nprintf '%s\\n' '{metadata}'\n"
            ).as_bytes(), 0o555).unwrap();
        }
        directory
    }
    fn platform() -> (&'static str, &'static str) {
        if cfg!(target_os = "macos") {
            ("adhoc", "macos=15.0")
        } else {
            ("none", "glibc=2.39")
        }
    }
    #[test]
    fn tooling_record_binds_only_two_probed_files_and_exact_source() {
        let root = tempfile::tempdir().unwrap();
        let source = input(root.path(), &TOOL_NAMES, "--release-info");
        let output = root.path().join("output");
        let harness = "a".repeat(64);
        package_tooling(&source, &output, &harness).unwrap();
        let entries: Vec<_> = fs::read_dir(&output).unwrap().collect();
        let mut names: Vec<_> = entries
            .iter()
            .map(|e| e.as_ref().unwrap().file_name())
            .collect();
        names.sort();
        let role = match metadata().unwrap().target {
            "x86_64-unknown-linux-gnu" => "linux-amd64",
            "aarch64-unknown-linux-gnu" => "linux-arm64",
            "aarch64-apple-darwin" => "macos-arm64",
            _ => panic!("unsupported target"),
        };
        let mut expected: Vec<_> = TOOL_NAMES.iter().map(std::ffi::OsString::from).collect();
        expected.push(format!("{role}-tooling.json").into());
        expected.sort();
        assert_eq!(names, expected);
        let record = entries
            .into_iter()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "json"))
            .unwrap();
        assert_eq!(
            fs::metadata(&record).unwrap().permissions().mode() & 0o777,
            0o444
        );
        let record_bytes = fs::read(record).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&record_bytes).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 5);
        assert_eq!(value["schema"], 1);
        assert_eq!(value["source_commit"], metadata().unwrap().commit);
        assert_eq!(value["target"], metadata().unwrap().target);
        assert_eq!(value["harness_sha256"], harness);
        assert_eq!(value["files"].as_object().unwrap().len(), 2);
        for name in TOOL_NAMES {
            assert_eq!(
                fs::metadata(output.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o555
            );
            let (bytes, hash) = asset(&output.join(name), 256 * 1024 * 1024).unwrap();
            assert_eq!(value["files"][name]["bytes"], bytes);
            assert_eq!(value["files"][name]["sha256"], hex::encode(hash));
        }
        assert!(package_tooling(&source, &output, &harness).is_err());
    }
    #[test]
    fn tooling_rejects_bad_harness_or_probe_before_output_mutation() {
        let root = tempfile::tempdir().unwrap();
        let source = input(root.path(), &TOOL_NAMES, "--release-info");
        let output = root.path().join("output");
        for harness in ["a".repeat(63), "A".repeat(64), "g".repeat(64)] {
            assert!(package_tooling(&source, &output, &harness).is_err());
            assert!(!output.exists());
        }
        let tool = source.join(TOOL_NAMES[1]);
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&tool, b"#!/bin/sh\nprintf wrong\\n\n").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o555)).unwrap();
        assert!(package_tooling(&source, &output, &"a".repeat(64)).is_err());
        assert!(!output.exists());
    }
    #[test]
    fn packaging_probes_original_names_and_refuses_reuse_or_changed_bytes() {
        let root = tempfile::tempdir().unwrap();
        let source = input(root.path(), &NAMES[..3], "release-info");
        let licenses = root.path().join("licenses");
        create(&licenses, b"Original dependency notices\n", 0o444).unwrap();
        let output = root.path().join("assets");
        let (signing, minimum) = platform();
        package(&source, &output, &licenses, signing, minimum, false).unwrap();
        assert!(package(&source, &output, &licenses, signing, minimum, false).is_err());
        let m = metadata().unwrap();
        let manifest_path = output.join(format!("marmish-{}-{}.manifest", m.version, m.target));
        let bytes = fs::read(&manifest_path).unwrap();
        let parsed = Manifest::parse(&bytes).unwrap();
        let binary = output.join(parsed.asset_name("marmish"));
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&binary, b"changed").unwrap();
        assert!(
            manifest::verify_downloads(
                &output,
                &manifest_path,
                AssetNaming::VersionedDownloads,
                Sha256::digest(bytes).into()
            )
            .is_err()
        );
    }
    #[test]
    fn malformed_release_probe_is_finite_and_never_creates_output() {
        let root = tempfile::tempdir().unwrap();
        let source = input(root.path(), &NAMES[..3], "release-info");
        let cli = source.join("marmish");
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&cli, b"#!/bin/sh\nprintf wrong\\n\n").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o555)).unwrap();
        let licenses = root.path().join("licenses");
        create(&licenses, b"Notices\n", 0o444).unwrap();
        let output = root.path().join("assets");
        let (signing, minimum) = platform();
        assert!(package(&source, &output, &licenses, signing, minimum, false).is_err());
        assert!(!output.exists());
    }
    #[test]
    fn rendering_requires_complete_matching_target_provenance() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("install.sh");
        assert!(render(root.path(), &output).is_err());
        let m = metadata().unwrap();
        for target in TARGETS {
            let (signing, minimum) = if target.ends_with("apple-darwin") {
                ("adhoc", "macos=15.0")
            } else {
                ("none", "glibc=2.39")
            };
            let mut text = format!(
                "marmish-manifest 1\nversion {}\ntarget {target}\ncommit {}\nsigning {signing} notarized=no\nminimum {minimum}\nstate profile={} journal={} engine={} config={} commands={}\n",
                m.version,
                m.commit,
                m.formats[0],
                m.formats[1],
                m.formats[2],
                m.formats[3],
                m.formats[4]
            );
            for (index, name) in NAMES.iter().enumerate() {
                let content = format!("fixture-{target}-{name}\n");
                text.push_str(&format!(
                    "file {} {} {} {name}\n",
                    hex::encode(Sha256::digest(content.as_bytes())),
                    content.len(),
                    if index == 3 { "0444" } else { "0555" }
                ));
                create(
                    &root.path().join(format!("{name}-{}-{target}", m.version)),
                    content.as_bytes(),
                    if index == 3 { 0o444 } else { 0o555 },
                )
                .unwrap();
            }
            text.push_str("end\n");
            create(
                &root
                    .path()
                    .join(format!("marmish-{}-{target}.manifest", m.version)),
                text.as_bytes(),
                0o444,
            )
            .unwrap();
        }
        render(root.path(), &output).unwrap();
        assert!(!fs::read_to_string(&output).unwrap().contains("@MARMISH_"));
        assert!(render(root.path(), &output).is_err());
        fs::remove_file(&output).unwrap();
        let manifest_path = root
            .path()
            .join(format!("marmish-{}-{}.manifest", m.version, TARGETS[0]));
        let bytes = fs::read_to_string(&manifest_path)
            .unwrap()
            .replace(m.commit, &"f".repeat(40));
        fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(manifest_path, bytes).unwrap();
        assert!(render(root.path(), &output).is_err());
        assert!(!output.exists());
    }
}
