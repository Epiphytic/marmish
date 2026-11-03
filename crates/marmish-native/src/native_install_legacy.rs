//! Exact v0.1 installer ownership recognition; never executes an old launcher.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Legacy {
    pub(super) root: PathBuf,
    pub(super) version: String,
    pub(super) target: String,
    pub(super) launchers: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    format: u8,
    version: String,
    target: String,
    launcher_dir: PathBuf,
}
pub(super) struct Plan {
    pub record: Legacy,
    _lock: fs::File,
}
fn version(v: &str) -> bool {
    v.strip_prefix("0.1.0-experimental.").is_some_and(|n| {
        !n.is_empty() && (n == "0" || !n.starts_with('0')) && n.bytes().all(|b| b.is_ascii_digit())
    })
}
impl Legacy {
    pub fn contains(&self, name: &str) -> bool {
        self.launchers.iter().any(|n| n == name)
    }
    pub(super) fn validate(&self) -> Result<()> {
        let root = self
            .root
            .to_str()
            .ok_or(invalid("legacy_install_unrecognized"))?;
        if root.len() > 2048
            || !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
            || self.version.len() > 64
            || !version(&self.version)
            || self.target.len() > 64
            || self.target != env!("MARMISH_BUILD_TARGET")
            || self.launchers.is_empty()
            || self
                .launchers
                .iter()
                .any(|n| !LAUNCHERS.contains(&n.as_str()))
            || self.launchers.windows(2).any(|w| w[0] >= w[1])
        {
            return Err(invalid("legacy_install_unrecognized"));
        }
        Ok(())
    }
    pub(super) fn version_root(&self) -> PathBuf {
        self.root
            .join("versions")
            .join(format!("marmish-{}-{}", self.version, self.target))
    }
    pub(super) fn wrapper(&self, name: &str) -> Result<Vec<u8>> {
        let target = self.version_root().join("bin").join(name);
        let target = target
            .to_str()
            .ok_or(invalid("legacy_install_unrecognized"))?;
        // Python shlex.quote's exact ASCII-safe set, including its apostrophe escape.
        let quoted = if target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
        {
            target.to_owned()
        } else {
            format!("'{}'", target.replace('\'', "'\"'\"'"))
        };
        Ok(format!("#!/bin/sh\n# marmish installer format 1\nexec {quoted} \"$@\"\n").into_bytes())
    }
    pub(super) fn backup(&self, r: &Receipt, tx: &Transaction, name: &str) -> PathBuf {
        r.bin_dir
            .join(format!(".marmish-legacy-{}-{name}", tx.txid))
    }
}
fn wrapper(path: &Path, expected: &[u8]) -> Result<(u64, u64)> {
    let f = manifest::safe_file(path, expected.len() as u64 + 1)?;
    let m = f.metadata()?;
    if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o755 {
        return Err(invalid("legacy_install_unrecognized"));
    }
    let mut bytes = Vec::new();
    f.take(expected.len() as u64 + 1).read_to_end(&mut bytes)?;
    if bytes != expected {
        return Err(invalid("legacy_install_unrecognized"));
    }
    Ok((m.dev(), m.ino()))
}
fn unchanged(path: &Path, identity: (u64, u64)) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || (m.dev(), m.ino()) != identity {
        return Err(invalid("legacy_install_unrecognized"));
    }
    Ok(())
}
fn recognize(ctx: &Anchors, o: &InstallOptions, root: PathBuf) -> Result<Option<Plan>> {
    let marker_path = root.join(".marmish-install.json");
    if !exists(&marker_path)? {
        return Ok(None);
    }
    path(ctx, &root, false)?;
    check_owned_dir(&root, None)?;
    if fs::symlink_metadata(&root)?.mode() & 0o077 != 0 {
        return Err(invalid("legacy_install_unrecognized"));
    }
    let marker: Marker = json(&marker_path, 0o600, 16384)?;
    if marker.launcher_dir != o.bin_dir {
        return Ok(None);
    }
    if contained(&root, &o.prefix) {
        return Err(invalid("unsafe_install_path"));
    }
    let mut record = Legacy {
        root,
        version: marker.version,
        target: marker.target,
        launchers: vec!["marmish".into()],
    };
    record.validate()?;
    if marker.format != 1 {
        return Err(invalid("legacy_install_unrecognized"));
    }
    let lock = ctx.open(&record.root.join(".marmish-install.lock"), libc::O_RDWR, 0)?;
    let m = lock.metadata()?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
    {
        return Err(invalid("legacy_install_unrecognized"));
    }
    lock.try_lock_exclusive()
        .map_err(|_| invalid("install_busy"))?;
    // Re-read after taking the old installer's lock, before observing launchers.
    let again: Marker = json(&marker_path, 0o600, 16384)?;
    if again.format != 1
        || again.version != record.version
        || again.target != record.target
        || again.launcher_dir != o.bin_dir
    {
        return Err(invalid("legacy_install_unrecognized"));
    }
    for dir in [
        record.root.join("versions"),
        record.version_root(),
        record.version_root().join("bin"),
    ] {
        path(ctx, &dir, false)?;
        check_owned_dir(&dir, None)?;
    }
    let current = record.root.join("current");
    let cm = fs::symlink_metadata(&current)?;
    if !cm.file_type().is_symlink()
        || cm.uid() != unsafe { libc::geteuid() }
        || fs::read_link(current)?
            != Path::new("versions").join(format!("marmish-{}-{}", record.version, record.target))
    {
        return Err(invalid("legacy_install_unrecognized"));
    }
    record.launchers.clear();
    for name in LAUNCHERS {
        let p = o.bin_dir.join(name);
        if exists(&p)? {
            wrapper(&p, &record.wrapper(name)?).map_err(|_| launcher_conflict(&p))?;
            record.launchers.push(name.into());
        }
    }
    if record.launchers.is_empty() {
        return Ok(None);
    }
    Ok(Some(Plan {
        record,
        _lock: lock,
    }))
}
pub(super) fn discover(ctx: &Anchors, o: &InstallOptions, r: &Receipt) -> Result<Option<Plan>> {
    if r.active.is_some()
        || !r.launchers.is_empty()
        || !matches!(o.action, InstallAction::Install { .. })
    {
        return Ok(None);
    }
    if !LAUNCHERS
        .iter()
        .any(|n| exists(&o.bin_dir.join(n)).unwrap_or(true))
    {
        return Ok(None);
    }
    let home = PathBuf::from(std::env::var_os("HOME").ok_or(invalid("unsafe_install_path"))?);
    let mut roots = vec![home.join(".local/share/marmish")];
    if o.bin_dir.file_name().is_some_and(|n| n == "bin") {
        let root = o
            .bin_dir
            .parent()
            .ok_or(invalid("unsafe_install_path"))?
            .to_owned();
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    let mut found = None;
    for root in roots {
        if let Some(plan) = recognize(ctx, o, root)? {
            if found.is_some() {
                return Err(invalid("legacy_install_unrecognized"));
            }
            found = Some(plan);
        }
    }
    Ok(found)
}
pub(super) fn validate_transaction(tx: &Transaction) -> Result<()> {
    if let Some(v) = &tx.legacy {
        v.validate()?;
        if tx.operation != "install"
            || tx.previous_active.is_some()
            || !tx.previous_launchers.is_empty()
            || v.launchers
                .iter()
                .any(|n| !tx.created_launchers.contains(n))
        {
            return Err(invalid("install_recovery_required"));
        }
    }
    Ok(())
}
pub(super) fn validate_destinations(r: &Receipt, tx: &Transaction) -> Result<()> {
    if let Some(v) = &tx.legacy {
        for name in &v.launchers {
            if exists(&v.backup(r, tx, name))? {
                return Err(invalid("install_recovery_required"));
            }
        }
    }
    Ok(())
}
pub(super) fn prepare(ctx: &Anchors, r: &Receipt, tx: &Transaction) -> Result<()> {
    let Some(v) = &tx.legacy else { return Ok(()) };
    // Prevalidate the entire pair before the first move.
    for name in &v.launchers {
        wrapper(&r.bin_dir.join(name), &v.wrapper(name)?)?;
        if exists(&v.backup(r, tx, name))? {
            return Err(invalid("install_recovery_required"));
        }
    }
    for name in &v.launchers {
        let p = r.bin_dir.join(name);
        let identity = wrapper(&p, &v.wrapper(name)?)?;
        unchanged(&p, identity)?;
        ctx.rename(&p, &v.backup(r, tx, name), true)?;
        sync(ctx, &r.bin_dir)?;
        fault(if name == "marmish" {
            "legacy_marmish"
        } else {
            "legacy_marmishd"
        })?;
    }
    Ok(())
}
pub(super) fn validate_recovery(r: &Receipt, tx: &Transaction, previous: bool) -> Result<()> {
    let Some(v) = &tx.legacy else { return Ok(()) };
    for name in &v.launchers {
        let p = r.bin_dir.join(name);
        let backup = v.backup(r, tx, name);
        if exists(&backup)? {
            wrapper(&backup, &v.wrapper(name)?)?;
            if exists(&p)? {
                link(&p, &launcher_target(r, name), false)?;
            }
        } else if previous {
            wrapper(&p, &v.wrapper(name)?)?;
        } else {
            return Err(invalid("install_recovery_required"));
        }
    }
    Ok(())
}
pub(super) fn recover(ctx: &Anchors, r: &Receipt, tx: &Transaction, previous: bool) -> Result<()> {
    let Some(v) = &tx.legacy else { return Ok(()) };
    for name in &v.launchers {
        let p = r.bin_dir.join(name);
        let backup = v.backup(r, tx, name);
        if previous && exists(&backup)? {
            if exists(&p)? {
                link(&p, &launcher_target(r, name), false)?;
                ctx.unlink(&p, false)?;
            }
            let identity = wrapper(&backup, &v.wrapper(name)?)?;
            unchanged(&backup, identity)?;
            ctx.rename(&backup, &p, true)?;
        } else if !previous {
            if !exists(&p)? {
                ctx.symlink(&launcher_target(r, name), &p)?;
            }
            eprintln!(
                "marmish installer: retained previous launcher {}",
                serde_json::to_string(&backup).map_err(|_| invalid("install_metadata"))?
            );
        }
        sync(ctx, &r.bin_dir)?;
    }
    Ok(())
}
