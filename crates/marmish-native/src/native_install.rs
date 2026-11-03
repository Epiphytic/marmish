//! Owned, recoverable native installation. Identity and provider state is never opened.
use crate::native_manifest::{self as manifest, AssetNaming, Manifest, NAMES};
use crate::{Error, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

#[path = "native_install_fs.rs"]
mod anchored;
use anchored::Anchors;
#[path = "native_install_legacy.rs"]
mod legacy;

const RECEIPT: &str = ".marmish-native-install.json";
const TRANSACTION: &str = ".transaction.json";
const VERSION_RECEIPT: &str = ".version-receipt.json";
const LAUNCHERS: [&str; 2] = ["marmish", "marmishd"];
pub(crate) struct InstallOptions {
    pub action: InstallAction,
    pub prefix: PathBuf,
    pub bin_dir: PathBuf,
    pub state_dir: Option<PathBuf>,
    pub debug: bool,
}
pub(crate) enum InstallAction {
    Install {
        source_dir: PathBuf,
        manifest: PathBuf,
        manifest_sha256: [u8; 32],
    },
    Select {
        version: String,
    },
    Status,
    Uninstall,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Operation {
    Install,
    Select,
    Status,
    Uninstall,
}
#[derive(Serialize)]
pub(crate) struct InstallReport {
    pub schema: u8,
    pub operation: Operation,
    pub version: Option<String>,
    pub target: Option<String>,
    pub active: bool,
    pub generation: u64,
    pub prefix: PathBuf,
    pub bin_dir: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub format: u8,
    pub install_id: String,
    pub prefix: PathBuf,
    pub bin_dir: PathBuf,
    pub generation: u64,
    pub active: Option<String>,
    pub launchers: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VersionReceipt {
    pub format: u8,
    pub install_id: String,
    pub txid: String,
    pub id: String,
    pub manifest_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    format: u8,
    install_id: String,
    txid: String,
    operation: String,
    previous_generation: u64,
    next_generation: u64,
    previous_active: Option<String>,
    next_active: Option<String>,
    previous_launchers: Vec<String>,
    next_launchers: Vec<String>,
    manifest_sha256: Option<String>,
    staging: Option<String>,
    created_launchers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy: Option<legacy::Legacy>,
}
#[cfg(test)]
thread_local! { static FAULT: std::cell::RefCell<Option<&'static str>> = const { std::cell::RefCell::new(None) }; }
fn fault(stage: &'static str) -> Result<()> {
    #[cfg(test)]
    if FAULT.with(|f| *f.borrow() == Some(stage)) {
        return Err(invalid("interrupted"));
    }
    let _ = stage;
    Ok(())
}
fn invalid(label: &'static str) -> Error {
    Error::Invalid(label)
}
fn random() -> Result<String> {
    let mut v = [0; 16];
    getrandom::fill(&mut v).map_err(|_| invalid("install_random"))?;
    Ok(hex::encode(v))
}
fn hex16(v: &str) -> bool {
    crate::foundation::valid_hex(v, 32)
}
pub(crate) fn valid_id(id: &str) -> bool {
    manifest::TARGETS.iter().any(|target| {
        id.strip_suffix(&format!("-{target}"))
            .is_some_and(manifest::version)
    })
}
fn names(v: &[String]) -> bool {
    v.is_empty() || v == ["marmish", "marmishd"]
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn sync(ctx: &Anchors, dir: &Path) -> Result<()> {
    ctx.sync(dir)
}
pub(crate) fn check_owned_dir(path: &Path, mode: Option<u32>) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o022 != 0
        || mode.is_some_and(|v| m.mode() & 0o777 != v)
    {
        return Err(invalid("unsafe_install_path"));
    }
    Ok(())
}
pub(crate) fn checked_metadata(path: &Path, mode: u32, max: u64) -> Result<Vec<u8>> {
    let f = manifest::safe_file(path, max)?;
    let m = f.metadata()?;
    if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != mode {
        return Err(invalid("unsafe_install_path"));
    }
    let mut bytes = Vec::new();
    f.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(invalid("install_metadata"));
    }
    Ok(bytes)
}
fn json<T: DeserializeOwned>(path: &Path, mode: u32, max: u64) -> Result<T> {
    serde_json::from_slice(&checked_metadata(path, mode, max)?)
        .map_err(|_| invalid("install_recovery_required"))
}
pub(crate) fn read_receipt(prefix: &Path) -> Result<Receipt> {
    let r: Receipt = json(&prefix.join(RECEIPT), 0o600, 8192)?;
    if r.format != 1
        || !hex16(&r.install_id)
        || r.prefix != prefix
        || !r.bin_dir.is_absolute()
        || !names(&r.launchers)
        || r.active.as_ref().is_some_and(|v| !valid_id(v))
        || (r.active.is_none() != r.launchers.is_empty())
    {
        return Err(invalid("install_recovery_required"));
    }
    Ok(r)
}
pub(crate) fn read_version(
    prefix: &Path,
    id: &str,
    install_id: &str,
) -> Result<(Manifest, VersionReceipt)> {
    if !valid_id(id) {
        return Err(invalid("version_conflict"));
    }
    let dir = prefix.join("versions").join(id);
    check_owned_dir(&dir, None)?;
    if ![0o700, 0o755].contains(&(fs::symlink_metadata(&dir)?.mode() & 0o777)) {
        return Err(invalid("unsafe_install_path"));
    }
    let v: VersionReceipt = json(&dir.join(VERSION_RECEIPT), 0o444, 4096)?;
    if v.format != 1 || v.install_id != install_id || v.id != id || !hex16(&v.txid) {
        return Err(invalid("version_conflict"));
    }
    let bytes = checked_metadata(&dir.join("manifest"), 0o444, 4096)?;
    if manifest::hex32(&v.manifest_sha256)? != <[u8; 32]>::from(Sha256::digest(&bytes)) {
        return Err(invalid("version_conflict"));
    }
    let m = Manifest::parse(&bytes)?;
    if m.id() != id {
        return Err(invalid("version_conflict"));
    }
    Ok((m, v))
}
fn atomic<T: Serialize>(ctx: &Anchors, root: &Path, name: &str, v: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec(v).map_err(|_| invalid("install_metadata"))?;
    bytes.push(b'\n');
    if bytes.len() > 8192 {
        return Err(invalid("install_metadata"));
    }
    let temp = root.join(format!("{name}.tmp-{}", random()?));
    let mut f = ctx.open(&temp, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    ctx.rename(&temp, &root.join(name), false)?;
    sync(ctx, root)
}
/// Validate every existing ancestor before creating anything; symlinks and '..' fail.
fn path(ctx: &Anchors, path: &Path, create: bool) -> Result<()> {
    if !path.is_absolute() {
        return Err(invalid("unsafe_install_path"));
    }
    let mut current = PathBuf::from("/");
    for part in path.components().skip(1) {
        let Component::Normal(part) = part else {
            return Err(invalid("unsafe_install_path"));
        };
        current.push(part);
        if !exists(&current)? {
            if !create {
                continue;
            }
            ctx.mkdir(&current, 0o755)?;
            sync(ctx, current.parent().ok_or(invalid("unsafe_install_path"))?)?;
        }
        if exists(&current)? {
            marmish_engine::validate_executable_ancestor(&current)
                .map_err(|_| invalid("unsafe_install_path"))?;
        }
    }
    Ok(())
}
fn contained(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}
fn validate_options(ctx: &Anchors, o: &InstallOptions) -> Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        return Err(invalid("install_root_forbidden"));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or(invalid("unsafe_install_path"))?;
    path(ctx, &home, false)?;
    if !home.is_dir() {
        return Err(invalid("unsafe_install_path"));
    }
    // HOME itself may not use the sticky-system ancestor exception.
    let hm = fs::symlink_metadata(&home)?;
    if hm.mode() & 0o022 != 0 {
        return Err(invalid("unsafe_install_path"));
    }
    path(ctx, &o.prefix, false)?;
    path(ctx, &o.bin_dir, false)?;
    let default_state = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Marmish/v2")
    } else {
        match std::env::var_os("XDG_DATA_HOME") {
            Some(p) if Path::new(&p).is_absolute() => PathBuf::from(p).join("marmish/v2"),
            _ => home.join(".local/share/marmish/v2"),
        }
    };
    for forbidden in [
        Some(home.join(".local/share/marmish")),
        Some(default_state),
        o.state_dir.clone(),
    ]
    .into_iter()
    .flatten()
    {
        path(ctx, &forbidden, false)?;
        if contained(&o.prefix, &forbidden) || contained(&o.bin_dir, &forbidden) {
            return Err(invalid("unsafe_install_path"));
        }
    }
    if contained(&o.prefix, &o.bin_dir) {
        return Err(invalid("unsafe_install_path"));
    }
    Ok(())
}
fn launcher_conflict(path: &Path) -> Error {
    eprintln!(
        "marmish installer: conflicting launcher {}; preserve or remove this independently reviewed program, or select a different --bin-dir",
        serde_json::to_string(path).unwrap_or_else(|_| "\"unrepresentable path\"".into())
    );
    invalid("unrelated_launcher")
}
fn link(path: &Path, target: &Path, allow_missing: bool) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && allow_missing => Ok(false),
        Err(e) => Err(e.into()),
        Ok(m) => {
            if !m.file_type().is_symlink()
                || m.uid() != unsafe { libc::geteuid() }
                || fs::read_link(path)? != target
            {
                return Err(launcher_conflict(path));
            }
            Ok(true)
        }
    }
}
fn launcher_target(r: &Receipt, name: &str) -> PathBuf {
    r.prefix.join("current").join(name)
}
fn validate_links(r: &Receipt, allow_missing: bool) -> Result<()> {
    for name in LAUNCHERS {
        let p = r.bin_dir.join(name);
        if r.launchers.iter().any(|n| n == name) {
            link(&p, &launcher_target(r, name), allow_missing)?;
        } else if exists(&p)? {
            return Err(launcher_conflict(&p));
        }
    }
    Ok(())
}
fn selector(prefix: &Path, allowed: &[Option<String>]) -> Result<()> {
    let current = prefix.join("current");
    if !exists(&current)? {
        return Ok(());
    }
    let target = fs::read_link(&current).map_err(|_| invalid("install_recovery_required"))?;
    if !allowed
        .iter()
        .flatten()
        .any(|id| target == Path::new("versions").join(id))
    {
        return Err(invalid("install_recovery_required"));
    }
    let meta = fs::symlink_metadata(current)?;
    if meta.uid() != unsafe { libc::geteuid() } {
        return Err(invalid("install_recovery_required"));
    }
    Ok(())
}
fn set_selector(ctx: &Anchors, r: &Receipt, tx: &Transaction, id: Option<&str>) -> Result<()> {
    selector(
        &r.prefix,
        &[tx.previous_active.clone(), tx.next_active.clone()],
    )?;
    let temp = r.prefix.join(format!(".selector-{}", tx.txid));
    if exists(&temp)? {
        let expected = tx
            .next_active
            .as_ref()
            .ok_or(invalid("install_recovery_required"))?;
        if !link(&temp, &Path::new("versions").join(expected), false)? {
            return Err(invalid("install_recovery_required"));
        }
        ctx.unlink(&temp, false)?;
    }
    match id {
        Some(id) => {
            ctx.symlink(&Path::new("versions").join(id), &temp)?;
            ctx.rename(&temp, &r.prefix.join("current"), false)?;
        }
        None => {
            if exists(&r.prefix.join("current"))? {
                ctx.unlink(&r.prefix.join("current"), false)?;
            }
        }
    }
    sync(ctx, &r.prefix)
}
fn reverify(r: &Receipt, id: &str) -> Result<Manifest> {
    let (m, v) = read_version(&r.prefix, id, &r.install_id)?;
    manifest::verify_directory(
        &r.prefix.join("versions").join(id),
        AssetNaming::InstalledFixedNames,
        manifest::hex32(&v.manifest_sha256)?,
    )?;
    for name in &NAMES[..3] {
        marmish_engine::validate_executable_path(&r.prefix.join("versions").join(id).join(name))
            .map_err(|_| invalid("unsafe_install_path"))?;
    }
    Ok(m)
}
fn cleanup_staging(ctx: &Anchors, prefix: &Path, tx: &Transaction) -> Result<()> {
    let Some(name) = &tx.staging else {
        return Ok(());
    };
    let dir = prefix.join(name);
    if !exists(&dir)? {
        return Ok(());
    }
    check_owned_dir(&dir, Some(0o700))?;
    let allowed = [
        "marmish",
        "marmishd",
        "marmish-worker",
        "THIRD-PARTY-LICENSES",
        "manifest",
        VERSION_RECEIPT,
    ];
    let entries: Vec<_> = fs::read_dir(&dir)?.collect::<std::io::Result<_>>()?;
    for entry in &entries {
        let name = entry.file_name();
        if !allowed.iter().any(|a| name == *a) {
            return Err(invalid("install_recovery_required"));
        }
        let m = fs::symlink_metadata(entry.path())?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.nlink() != 1
            || ![0o600, 0o555, 0o444].contains(&(m.mode() & 0o777))
        {
            return Err(invalid("install_recovery_required"));
        }
    }
    for entry in entries {
        ctx.unlink(&entry.path(), false)?;
    }
    ctx.unlink(&dir, true)?;
    sync(ctx, prefix)
}
fn validate_transaction(r: &Receipt, t: &Transaction) -> Result<()> {
    legacy::validate_transaction(t)?;
    if t.format != 1
        || t.install_id != r.install_id
        || !hex16(&t.txid)
        || !["install", "select", "uninstall"].contains(&t.operation.as_str())
        || t.previous_generation.checked_add(1) != Some(t.next_generation)
        || ![t.previous_generation, t.next_generation].contains(&r.generation)
        || !names(&t.previous_launchers)
        || !names(&t.next_launchers)
        || t.created_launchers
            .iter()
            .any(|n| !LAUNCHERS.contains(&n.as_str()))
        || t.created_launchers.windows(2).any(|w| w[0] >= w[1])
        || t.created_launchers
            .iter()
            .any(|n| !t.next_launchers.contains(n))
        || (t.previous_active.is_none() != t.previous_launchers.is_empty())
        || (t.operation == "install") != t.staging.is_some()
        || t.previous_active.as_ref().is_some_and(|id| !valid_id(id))
        || t.next_active.as_ref().is_some_and(|id| !valid_id(id))
        || t.staging
            .as_ref()
            .is_some_and(|s| s != &format!(".staging-{}", t.txid))
    {
        return Err(invalid("install_recovery_required"));
    }
    if t.operation == "uninstall" {
        if t.next_active.is_some()
            || !t.next_launchers.is_empty()
            || t.manifest_sha256.is_some()
            || t.staging.is_some()
        {
            return Err(invalid("install_recovery_required"));
        }
    } else if t.next_active.is_none()
        || t.next_launchers != LAUNCHERS
        || t.manifest_sha256
            .as_ref()
            .is_none_or(|s| manifest::hex32(s).is_err())
    {
        return Err(invalid("install_recovery_required"));
    }
    let (active, launchers) = if r.generation == t.previous_generation {
        (&t.previous_active, &t.previous_launchers)
    } else {
        (&t.next_active, &t.next_launchers)
    };
    if &r.active != active || &r.launchers != launchers {
        return Err(invalid("install_recovery_required"));
    }
    Ok(())
}
fn recover(ctx: &Anchors, r: &Receipt) -> Result<bool> {
    let p = r.prefix.join(TRANSACTION);
    if !exists(&p)? {
        selector(&r.prefix, std::slice::from_ref(&r.active))?;
        return Ok(false);
    }
    let tx: Transaction = json(&p, 0o600, 8192)?;
    validate_transaction(r, &tx)?;
    let previous = r.generation == tx.previous_generation;
    let desired = if previous {
        &tx.previous_active
    } else {
        &tx.next_active
    };
    if let Some(id) = desired {
        reverify(r, id)?;
    }
    // Validate the entire pair before any recovery mutation. Legacy wrappers
    // must never reach the native-created-link unlink branch.
    legacy::validate_recovery(r, &tx, previous)?;
    for name in LAUNCHERS {
        if tx.legacy.as_ref().is_some_and(|v| v.contains(name)) {
            continue;
        }
        let path = r.bin_dir.join(name);
        if exists(&path)? {
            link(&path, &launcher_target(r, name), false)?;
        }
    }
    set_selector(ctx, r, &tx, desired.as_deref())?;
    let desired_names = if previous {
        &tx.previous_launchers
    } else {
        &tx.next_launchers
    };
    for name in LAUNCHERS {
        if tx.legacy.as_ref().is_some_and(|v| v.contains(name)) {
            continue;
        }
        let p = r.bin_dir.join(name);
        if desired_names.iter().any(|n| n == name) {
            if !exists(&p)? {
                ctx.symlink(&launcher_target(r, name), &p)?;
            }
        } else if (tx.created_launchers.iter().any(|n| n == name) || !previous) && exists(&p)? {
            ctx.unlink(&p, false)?;
        }
    }
    legacy::recover(ctx, r, &tx, previous)?;
    sync(ctx, &r.bin_dir)?;
    cleanup_staging(ctx, &r.prefix, &tx)?;
    ctx.unlink(&p, false)?;
    sync(ctx, &r.prefix)?;
    Ok(previous && tx.legacy.is_some())
}
fn metadata_temps(ctx: &Anchors, prefix: &Path, initial: bool) -> Result<()> {
    for entry in fs::read_dir(prefix)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let receipt = name.strip_prefix(&format!("{RECEIPT}.tmp-"));
        let tx = name.strip_prefix(&format!("{TRANSACTION}.tmp-"));
        if let Some(suffix) = receipt.or(if initial { None } else { tx }) {
            if !hex16(suffix) {
                return Err(invalid("install_recovery_required"));
            }
            checked_metadata(&entry.path(), 0o600, 8192)?;
            ctx.unlink(&entry.path(), false)?;
        }
    }
    sync(ctx, prefix)
}
fn initial_receipt(ctx: &Anchors, o: &InstallOptions) -> Result<Receipt> {
    let entries: Vec<_> = fs::read_dir(&o.prefix)?.collect::<std::io::Result<_>>()?;
    let mut temps = 0;
    for e in entries {
        let n = e.file_name();
        if n == ".lock" {
            continue;
        }
        let Some(s) = n
            .to_str()
            .and_then(|n| n.strip_prefix(&format!("{RECEIPT}.tmp-")))
        else {
            return Err(invalid("unrelated_install_prefix"));
        };
        if !hex16(s) {
            return Err(invalid("unrelated_install_prefix"));
        }
        checked_metadata(&e.path(), 0o600, 8192)?;
        temps += 1;
        if temps > 1 {
            return Err(invalid("unrelated_install_prefix"));
        }
    }
    metadata_temps(ctx, &o.prefix, true)?;
    let r = Receipt {
        format: 1,
        install_id: random()?,
        prefix: o.prefix.clone(),
        bin_dir: o.bin_dir.clone(),
        generation: 0,
        active: None,
        launchers: vec![],
    };
    atomic(ctx, &o.prefix, RECEIPT, &r)?;
    Ok(r)
}
fn stage(
    ctx: &Anchors,
    r: &Receipt,
    t: &Transaction,
    source: &Path,
    manifest_path: &Path,
    m: &Manifest,
) -> Result<()> {
    let finaldir = r.prefix.join("versions").join(m.id());
    if exists(&finaldir)? {
        let existing = reverify(r, &m.id())?;
        if &existing != m {
            return Err(invalid("version_conflict"));
        }
        ctx.chmod_dir(&finaldir, 0o755)?;
        sync(ctx, &finaldir)?;
        sync(ctx, &r.prefix.join("versions"))?;
        return Ok(());
    }
    let staging = r
        .prefix
        .join(t.staging.as_ref().ok_or(invalid("install_metadata"))?);
    ctx.mkdir(&staging, 0o700)?;
    sync(ctx, &r.prefix)?;
    for a in &m.files {
        let from = source.join(m.asset_name(a.name));
        let input = manifest::safe_file(&from, a.bytes)?;
        let mut output = ctx.open(
            &staging.join(a.name),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        if std::io::copy(&mut input.take(a.bytes + 1), &mut output)? != a.bytes {
            return Err(invalid("manifest_asset"));
        }
        output.sync_all()?;
        fault("copy")?;
        output.set_permissions(fs::Permissions::from_mode(a.mode))?;
        output.sync_all()?;
        manifest::verify_file(&staging.join(a.name), a, true)?;
    }
    let bytes = manifest::read_bounded(manifest_path, 4096)?;
    if Some(hex::encode(Sha256::digest(&bytes))) != t.manifest_sha256 {
        return Err(invalid("install_manifest"));
    }
    let mut mf = ctx.open(
        &staging.join("manifest"),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    mf.write_all(&bytes)?;
    mf.sync_all()?;
    mf.set_permissions(fs::Permissions::from_mode(0o444))?;
    mf.sync_all()?;
    let receipt = VersionReceipt {
        format: 1,
        install_id: r.install_id.clone(),
        txid: t.txid.clone(),
        id: m.id(),
        manifest_sha256: t
            .manifest_sha256
            .clone()
            .ok_or(invalid("install_metadata"))?,
    };
    let mut vf = ctx.open(
        &staging.join(VERSION_RECEIPT),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    serde_json::to_writer(&mut vf, &receipt).map_err(|_| invalid("install_metadata"))?;
    vf.sync_all()?;
    vf.set_permissions(fs::Permissions::from_mode(0o444))?;
    vf.sync_all()?;
    for name in &NAMES[..3] {
        marmish_engine::validate_executable_path(&staging.join(name))
            .map_err(|_| invalid("unsafe_install_path"))?;
    }
    sync(ctx, &staging)?;
    ctx.rename(&staging, &finaldir, true)?;
    fault("version")?;
    ctx.chmod_dir(&finaldir, 0o755)?;
    sync(ctx, &finaldir)?;
    sync(ctx, &r.prefix.join("versions"))?;
    reverify(r, &m.id())?;
    Ok(())
}
pub(crate) fn run(o: InstallOptions) -> Result<InstallReport> {
    let anchors = Anchors::default();
    let ctx = &anchors;
    validate_options(ctx, &o)?;
    if o.debug {
        eprintln!("marmish installer debug: stage=validation");
    }
    // Hash all inputs before installation mutation.
    let incoming = match &o.action {
        InstallAction::Install {
            source_dir,
            manifest,
            manifest_sha256,
        } => {
            let m = manifest::verify_downloads(
                source_dir,
                manifest,
                AssetNaming::VersionedDownloads,
                *manifest_sha256,
            )?;
            let compiled = manifest::compiled_release_metadata();
            if m.version != compiled.version
                || m.target != compiled.target
                || m.commit != compiled.commit
                || m.formats != compiled.formats
            {
                return Err(invalid("install_provenance"));
            }
            Some(m)
        }
        _ => None,
    };
    path(ctx, &o.prefix, true)?;
    check_owned_dir(&o.prefix, None)?;
    path(ctx, &o.bin_dir, true)?;
    check_owned_dir(&o.bin_dir, None)?;
    let _prefix = ctx.directory(&o.prefix)?;
    let _bin = ctx.directory(&o.bin_dir)?;
    let lockpath = o.prefix.join(".lock");
    let lock = ctx.open(&lockpath, libc::O_RDWR | libc::O_CREAT, 0o600)?;
    let lm = lock.metadata()?;
    if !lm.is_file()
        || lm.nlink() != 1
        || lm.uid() != unsafe { libc::geteuid() }
        || lm.mode() & 0o777 != 0o600
    {
        return Err(invalid("unsafe_install_path"));
    }
    lock.try_lock_exclusive()
        .map_err(|_| invalid("install_busy"))?;
    let mut receipt = if exists(&o.prefix.join(RECEIPT))? {
        read_receipt(&o.prefix)?
    } else {
        initial_receipt(ctx, &o)?
    };
    if receipt.bin_dir != o.bin_dir {
        return Err(invalid("install_bin_binding"));
    }
    if o.debug {
        eprintln!("marmish installer debug: stage=recovery");
    }
    let restored_legacy = recover(ctx, &receipt)?;
    metadata_temps(ctx, &o.prefix, false)?;
    let operation = match o.action {
        InstallAction::Install { .. } => Operation::Install,
        InstallAction::Select { .. } => Operation::Select,
        InstallAction::Status => Operation::Status,
        InstallAction::Uninstall => Operation::Uninstall,
    };
    if !matches!(o.action, InstallAction::Status) {
        let legacy_plan = legacy::discover(ctx, &o, &receipt)?;
        if legacy_plan.is_none() {
            validate_links(&receipt, true)?;
        }
        let (next, hash) = match &o.action {
            InstallAction::Install {
                manifest_sha256, ..
            } => (
                incoming.as_ref().map(Manifest::id),
                Some(hex::encode(manifest_sha256)),
            ),
            InstallAction::Select { version } => {
                if !manifest::version(version) {
                    return Err(invalid("version_conflict"));
                }
                let id = format!("{version}-{}", env!("MARMISH_BUILD_TARGET"));
                let (_, v) = read_version(&receipt.prefix, &id, &receipt.install_id)?;
                reverify(&receipt, &id)?;
                (Some(id), Some(v.manifest_sha256))
            }
            InstallAction::Uninstall => (None, None),
            InstallAction::Status => unreachable!(),
        };
        let txid = random()?;
        let next_launchers: Vec<String> = if next.is_some() {
            LAUNCHERS.iter().map(|s| (*s).into()).collect()
        } else {
            vec![]
        };
        let mut created = Vec::new();
        for name in &next_launchers {
            if !exists(&receipt.bin_dir.join(name))?
                || legacy_plan
                    .as_ref()
                    .is_some_and(|p| p.record.contains(name))
            {
                created.push(name.clone());
            }
        }
        let tx = Transaction {
            format: 1,
            install_id: receipt.install_id.clone(),
            txid: txid.clone(),
            operation: match operation {
                Operation::Install => "install",
                Operation::Select => "select",
                _ => "uninstall",
            }
            .into(),
            previous_generation: receipt.generation,
            next_generation: receipt
                .generation
                .checked_add(1)
                .ok_or(invalid("install_generation"))?,
            previous_active: receipt.active.clone(),
            next_active: next.clone(),
            previous_launchers: receipt.launchers.clone(),
            next_launchers: next_launchers.clone(),
            manifest_sha256: hash,
            staging: incoming.as_ref().map(|_| format!(".staging-{txid}")),
            created_launchers: created,
            legacy: legacy_plan.as_ref().map(|p| p.record.clone()),
        };
        legacy::validate_destinations(&receipt, &tx)?;
        atomic(ctx, &receipt.prefix, TRANSACTION, &tx)?;
        fault("transaction")?;
        legacy::prepare(ctx, &receipt, &tx)?;
        let versions = receipt.prefix.join("versions");
        if !exists(&versions)? {
            ctx.mkdir(&versions, 0o755)?;
            ctx.chmod_dir(&versions, 0o755)?;
            sync(ctx, &receipt.prefix)?;
        }
        check_owned_dir(&versions, Some(0o755))?;
        if let (
            Some(m),
            InstallAction::Install {
                source_dir,
                manifest,
                ..
            },
        ) = (&incoming, &o.action)
        {
            stage(ctx, &receipt, &tx, source_dir, manifest, m)?;
        }
        for name in &tx.created_launchers {
            ctx.symlink(
                &launcher_target(&receipt, name),
                &receipt.bin_dir.join(name),
            )?;
        }
        sync(ctx, &receipt.bin_dir)?;
        set_selector(ctx, &receipt, &tx, next.as_deref())?;
        fault("selector")?;
        if matches!(o.action, InstallAction::Uninstall) {
            for name in &receipt.launchers {
                let p = receipt.bin_dir.join(name);
                if exists(&p)? {
                    link(&p, &launcher_target(&receipt, name), false)?;
                    ctx.unlink(&p, false)?;
                }
            }
            sync(ctx, &receipt.bin_dir)?;
        }
        receipt.generation = tx.next_generation;
        receipt.active = next;
        receipt.launchers = next_launchers;
        atomic(ctx, &receipt.prefix, RECEIPT, &receipt)?;
        fault("commit")?;
        recover(ctx, &receipt)?;
    }
    // This Status just restored exact wrappers from a durable legacy transaction.
    // They intentionally are not native receipt-owned links.
    if !(restored_legacy && matches!(o.action, InstallAction::Status)) {
        validate_links(&receipt, false)?;
    }
    selector(&receipt.prefix, std::slice::from_ref(&receipt.active))?;
    if receipt.active.is_some() && !exists(&receipt.prefix.join("current"))? {
        return Err(invalid("install_recovery_required"));
    }
    if o.debug {
        eprintln!("marmish installer debug: stage=complete");
    }
    let active_manifest = receipt
        .active
        .as_ref()
        .map(|id| read_version(&receipt.prefix, id, &receipt.install_id).map(|v| v.0))
        .transpose()?;
    Ok(InstallReport {
        schema: 1,
        operation,
        version: active_manifest.as_ref().map(|m| m.version.clone()),
        target: active_manifest.map(|m| m.target),
        active: receipt.active.is_some(),
        generation: receipt.generation,
        prefix: receipt.prefix,
        bin_dir: receipt.bin_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("native_install_legacy_tests.rs");
    struct Fixture {
        root: tempfile::TempDir,
        source: PathBuf,
        manifest: PathBuf,
        hash: [u8; 32],
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("marmish-install-test-")
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir_in(std::env::var_os("HOME").unwrap())
                .unwrap();
            let source = root.path().join("downloads");
            fs::create_dir(&source).unwrap();
            let meta = manifest::compiled_release_metadata();
            let mut text = format!(
                "marmish-manifest 1\nversion {}\ntarget {}\ncommit {}\nsigning {} notarized=no\nminimum {}\nstate profile={} journal={} engine={} config={} commands={}\n",
                meta.version,
                meta.target,
                meta.commit,
                if cfg!(target_os = "macos") {
                    "adhoc"
                } else {
                    "none"
                },
                if cfg!(target_os = "macos") {
                    "macos=15.0"
                } else {
                    "glibc=2.39"
                },
                meta.formats[0],
                meta.formats[1],
                meta.formats[2],
                meta.formats[3],
                meta.formats[4]
            );
            for (n, name) in NAMES.iter().enumerate() {
                let body = format!("fixture-{name}\n");
                fs::write(
                    source.join(format!("{name}-{}-{}", meta.version, meta.target)),
                    &body,
                )
                .unwrap();
                text.push_str(&format!(
                    "file {} {} {} {name}\n",
                    hex::encode(Sha256::digest(body.as_bytes())),
                    body.len(),
                    if n == 3 { "0444" } else { "0555" }
                ));
            }
            text.push_str("end\n");
            let manifest =
                source.join(format!("marmish-{}-{}.manifest", meta.version, meta.target));
            fs::write(&manifest, &text).unwrap();
            let hash = Sha256::digest(text.as_bytes()).into();
            Self {
                root,
                source,
                manifest,
                hash,
            }
        }
        fn options(&self, action: InstallAction) -> InstallOptions {
            InstallOptions {
                action,
                prefix: self.root.path().join("prefix"),
                bin_dir: self.root.path().join("bin"),
                state_dir: Some(self.root.path().join("state")),
                debug: false,
            }
        }
        fn install(&self) -> Result<InstallReport> {
            run(self.options(InstallAction::Install {
                source_dir: self.source.clone(),
                manifest: self.manifest.clone(),
                manifest_sha256: self.hash,
            }))
        }
        fn prefix(&self) -> PathBuf {
            self.root.path().join("prefix")
        }
    }
    #[test]
    fn install_select_uninstall_preserves_versions_and_unrelated_state() {
        let f = Fixture::new();
        assert!(f.install().unwrap().active);
        let receipt = read_receipt(&f.prefix()).unwrap();
        let id = receipt.active.unwrap();
        assert!(f.prefix().join("versions").join(&id).is_dir());
        assert!(
            run(f.options(InstallAction::Select {
                version: env!("CARGO_PKG_VERSION").into()
            }))
            .unwrap()
            .active
        );
        assert!(!run(f.options(InstallAction::Uninstall)).unwrap().active);
        assert!(f.prefix().join("versions").join(id).is_dir());
        assert!(!f.root.path().join("bin/marmish").exists());
        assert!(f.install().unwrap().active);
    }
    #[test]
    fn every_commit_boundary_recovers_without_adopting_version() {
        for checkpoint in ["transaction", "copy", "version", "selector", "commit"] {
            let f = Fixture::new();
            FAULT.with(|v| *v.borrow_mut() = Some(checkpoint));
            assert!(f.install().is_err(), "{checkpoint}");
            FAULT.with(|v| *v.borrow_mut() = None);
            let report = run(f.options(InstallAction::Status)).unwrap();
            assert_eq!(report.active, checkpoint == "commit", "{checkpoint}");
            assert!(f.install().unwrap().active, "{checkpoint}");
        }
    }
    #[test]
    fn launchers_and_linked_inputs_never_replaced() {
        let f = Fixture::new();
        fs::create_dir(f.root.path().join("bin")).unwrap();
        let existing = f.root.path().join("bin/marmish");
        fs::write(&existing, b"unrelated").unwrap();
        assert!(f.install().is_err());
        assert_eq!(fs::read(existing).unwrap(), b"unrelated");
        let g = Fixture::new();
        let asset = g.source.join(format!(
            "marmish-{}-{}",
            env!("CARGO_PKG_VERSION"),
            env!("MARMISH_BUILD_TARGET")
        ));
        fs::hard_link(&asset, g.root.path().join("extra")).unwrap();
        assert!(g.install().is_err());
        assert!(!g.prefix().exists());
    }
    #[test]
    fn unexpected_staging_entry_and_malformed_transaction_are_preserved() {
        let f = Fixture::new();
        FAULT.with(|v| *v.borrow_mut() = Some("copy"));
        assert!(f.install().is_err());
        FAULT.with(|v| *v.borrow_mut() = None);
        let tx: Transaction = json(&f.prefix().join(TRANSACTION), 0o600, 8192).unwrap();
        let unexpected = f.prefix().join(tx.staging.unwrap()).join("unrelated");
        fs::write(&unexpected, b"retain").unwrap();
        assert!(run(f.options(InstallAction::Status)).is_err());
        assert_eq!(fs::read(unexpected).unwrap(), b"retain");
    }
    #[test]
    fn inherited_umask077_installs_in_separate_process() {
        const CHILD: &str = "MARMISH_INSTALL_UMASK_CHILD";
        if std::env::var_os(CHILD).is_some() {
            unsafe {
                libc::umask(0o077);
            }
            let fixture = Fixture::new();
            assert!(fixture.install().unwrap().active);
            check_owned_dir(&fixture.prefix().join("versions"), Some(0o755)).unwrap();
            return;
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_install::tests::inherited_umask077_installs_in_separate_process",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[test]
    fn malformed_transaction_preserves_evidence() {
        let f = Fixture::new();
        FAULT.with(|v| *v.borrow_mut() = Some("transaction"));
        assert!(f.install().is_err());
        FAULT.with(|v| *v.borrow_mut() = None);
        let path = f.prefix().join(TRANSACTION);
        fs::write(&path, b"{not valid JSON").unwrap();
        assert!(run(f.options(InstallAction::Status)).is_err());
        assert_eq!(fs::read(path).unwrap(), b"{not valid JSON");
    }
    #[test]
    fn bootstrap_and_rust_share_manifest_vectors_with_utility_only_path() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let metadata = manifest::compiled_release_metadata();
        let probe = serde_json::to_string(&metadata).unwrap();
        let body = format!(
            "#!/bin/sh\ncase \"$1\" in\nrelease-info) printf '%s\\n' '{probe}';;\ninstall) printf done > \"$MARMISH_BOOTSTRAP_MARKER\";;\n*) exit 1;;\nesac\n"
        );
        let cli = f
            .source
            .join(format!("marmish-{}-{}", metadata.version, metadata.target));
        fs::write(&cli, &body).unwrap();
        let original = fs::read_to_string(&f.manifest).unwrap();
        let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();
        lines[7] = format!(
            "file {} {} 0555 marmish",
            hex::encode(Sha256::digest(body.as_bytes())),
            body.len()
        );
        let valid = lines.join("\n") + "\n";
        let utilities = f.root.path().join("utilities");
        fs::create_dir(&utilities).unwrap();
        for utility in [
            "curl",
            "uname",
            "id",
            "mktemp",
            "chmod",
            "mkdir",
            "rm",
            "rmdir",
            "mv",
            "cat",
            "wc",
            "tr",
            "awk",
            "sha256sum",
            "shasum",
            "sysctl",
        ] {
            if let Some(path) = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
                .iter()
                .map(|dir| Path::new(dir).join(utility))
                .find(|p| p.is_file())
            {
                symlink(path, utilities.join(utility)).unwrap();
            }
        }
        let mut vectors = vec![(valid.clone(), true)];
        vectors.extend([
            (valid.trim_end_matches('\n').to_owned(), false),
            (valid.clone() + "unknown\n", false),
            (valid.replace("state profile=2", "state profile=02"), false),
            (
                valid.replace(
                    &format!("journal={}", crate::journal::JOURNAL_FORMAT),
                    "journal=4294967296",
                ),
                false,
            ),
            (valid.replace("0555 marmish\n", "0755 marmish\n"), false),
            (valid.replace("commit ", "commit  "), false),
            (valid.replace("minimum ", "minimum \u{007f}"), false),
            (
                valid
                    .replace("glibc=2.39", "glibc=18446744073709551616.0")
                    .replace("macos=15.0", "macos=18446744073709551616.0"),
                false,
            ),
        ]);
        for (index, (text, expected)) in vectors.into_iter().enumerate() {
            assert_eq!(
                Manifest::parse(text.as_bytes()).is_ok(),
                expected,
                "Rust vector {index}"
            );
            fs::write(&f.manifest, &text).unwrap();
            let hash = hex::encode(Sha256::digest(text.as_bytes()));
            let template = include_str!("../../../scripts/install-native.sh")
                .replace("@MARMISH_VERSION@", metadata.version)
                .replace("@MARMISH_SHA_AMD64@", &hash)
                .replace("@MARMISH_SHA_ARM64@", &hash)
                .replace("@MARMISH_SHA_MACOS@", &hash);
            let script = f.root.path().join("bootstrap.sh");
            fs::write(&script, template).unwrap();
            let marker = f.root.path().join(format!("marker-{index}"));
            let output = std::process::Command::new("/bin/sh")
                .arg(&script)
                .args(["--asset-dir"])
                .arg(&f.source)
                .args(["--prefix"])
                .arg(f.prefix())
                .args(["--bin-dir"])
                .arg(f.root.path().join("bin"))
                .env("PATH", &utilities)
                .env("TMPDIR", f.root.path())
                .env("MARMISH_BOOTSTRAP_MARKER", &marker)
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            assert_eq!(
                output.status.success(),
                expected,
                "shell vector {index}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                marker.exists(),
                expected,
                "install invoked for vector {index}"
            );
        }
    }
}
