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
    let expected = serde_json::to_vec(&metadata()?).map_err(|_| bad())?;
    let mut child = Command::new(path)
        .arg("release-info")
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
        let input = manifest::safe_file(source, a.bytes)?;
        let path = output.join(parsed.asset_name(a.name));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        if std::io::copy(&mut input.take(a.bytes + 1), &mut file)? != a.bytes {
            return Err(bad());
        }
        file.sync_all()?;
        file.set_permissions(fs::Permissions::from_mode(a.mode))?;
        file.sync_all()?;
        manifest::verify_file(&path, a, false)?;
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
    fn input(root: &Path) -> PathBuf {
        let directory = root.join("input");
        fs::create_dir(&directory).unwrap();
        let metadata = serde_json::to_string(&metadata().unwrap()).unwrap();
        for name in &NAMES[..3] {
            create(
                &directory.join(name),
                format!("#!/bin/sh\nprintf '%s\\n' '{metadata}'\n").as_bytes(),
                0o555,
            )
            .unwrap();
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
    fn packaging_probes_original_names_and_refuses_reuse_or_changed_bytes() {
        let root = tempfile::tempdir().unwrap();
        let source = input(root.path());
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
        let source = input(root.path());
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
