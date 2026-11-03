//! Install and initialize through the actual candidate launchers before rendezvous.
use super::{
    process::{self, Environment},
    records::{self, Context, FilePin, IdentityReady, Installation, ROLES, TARGETS},
};
use crate::{Error, Result};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::PathBuf,
    time::Duration,
};
use tokio::time::Instant;
pub(super) struct Prepare {
    pub cohort: String,
    pub context: PathBuf,
    pub assets: PathBuf,
    pub bootstrap: PathBuf,
    pub workspace: PathBuf,
    pub role: String,
    pub output: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Setup {
    pub schema: u8,
    pub identity: String,
    pub npub: String,
    pub backend: String,
    pub role: String,
    pub inbound_enabled: bool,
    pub config_changed: bool,
    pub identity_created: bool,
    pub owner_opened: bool,
    pub engine_stop_complete: bool,
    pub mdk_stop_complete: bool,
    pub runtime_cleanup_complete: bool,
}
fn bad() -> Error {
    Error::Invalid("installed_prepare")
}
fn directory(p: &std::path::Path) -> Result<()> {
    fs::DirBuilder::new().mode(0o700).create(p)?;
    crate::identity::check_dir(p)
}
pub(super) async fn run(o: Prepare) -> Result<()> {
    let ctx: Context = records::read(&o.context, 65536)?;
    ctx.validate()?;
    ctx.require_cohort(&o.cohort)?;
    let index = ROLES.iter().position(|r| *r == o.role).ok_or_else(bad)?;
    if super::now()? >= ctx.expires_at
        || !o.workspace.is_absolute()
        || !o.assets.is_absolute()
        || !o.bootstrap.is_absolute()
    {
        return Err(bad());
    }
    records::verify_tools(&ctx, index)?;
    let expected = crate::native_manifest::hex32(ctx.manifests.get(&o.role).ok_or_else(bad)?)?;
    let manifest = crate::native_manifest::verify_directory(
        &o.assets,
        crate::native_manifest::AssetNaming::VersionedDownloads,
        expected,
    )?;
    if manifest.target != TARGETS[index]
        || manifest.commit != ctx.source_commit
        || manifest.version != ctx.version
    {
        return Err(bad());
    }
    if manifest.target != crate::native_manifest::compiled_release_metadata().target {
        return Err(bad());
    }
    let bootstrap = crate::native_manifest::read_bounded(&o.bootstrap, 128 * 1024)?;
    if records::digest(&bootstrap) != ctx.bootstrap_sha256 {
        return Err(bad());
    }
    let meta = fs::symlink_metadata(&o.bootstrap)?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        return Err(bad());
    }
    if o.workspace.parent().ok_or_else(bad)?.canonicalize()?
        != o.workspace.parent().ok_or_else(bad)?
    {
        return Err(bad());
    }
    directory(&o.workspace)?;
    if o.output.parent() != Some(o.workspace.as_path()) {
        return Err(bad());
    }
    directory(&o.output)?;
    let private_bootstrap = o.workspace.join("bootstrap.sh");
    {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&private_bootstrap)?;
        f.write_all(&bootstrap)?;
        f.sync_all()?;
    }
    let environment = Environment {
        home: o.workspace.join("home"),
        temporary: o.workspace.join("tmp"),
        utilities: o.workspace.join("utilities"),
    };
    process::utilities(&environment.utilities)?;
    directory(&environment.home)?;
    directory(&environment.temporary)?;
    let prefix = o.workspace.join("install");
    let bin = o.workspace.join("bin");
    let state = o.workspace.join("profiles");
    // The installer creates its exact private lifetime roots. No PATH or global installation.
    let args = process::args(&[
        private_bootstrap.as_os_str(),
        OsStr::new("--asset-dir"),
        o.assets.as_os_str(),
        OsStr::new("--prefix"),
        prefix.as_os_str(),
        OsStr::new("--bin-dir"),
        bin.as_os_str(),
        OsStr::new("--state-dir"),
        state.as_os_str(),
        OsStr::new("--key-storage"),
        OsStr::new("file"),
    ]);
    let cutoff =
        Instant::now() + Duration::from_secs(300.min(ctx.expires_at.saturating_sub(super::now()?)));
    let install = process::run(
        std::path::Path::new("/bin/sh"),
        &args,
        &o.workspace,
        &environment,
        cutoff,
        65536,
    )
    .await?;
    if !install.status.success() {
        return Err(bad());
    }
    let launcher = bin.join("marmish");
    let executable = launcher.canonicalize()?;
    let version_root = executable.parent().ok_or_else(bad)?;
    if !version_root.starts_with(prefix.join("versions")) {
        return Err(bad());
    }
    let installed = crate::native_manifest::verify_directory(
        version_root,
        crate::native_manifest::AssetNaming::InstalledFixedNames,
        expected,
    )?;
    if installed != manifest {
        return Err(bad());
    }
    let mut accounts = Vec::new();
    for (name, role) in [("client", "client"), ("server", "server")] {
        let mut args = vec![
            OsString::from("--state-dir"),
            state.clone().into_os_string(),
            OsString::from("--identity"),
            OsString::from(name),
            OsString::from("--key-storage"),
            OsString::from("file"),
            OsString::from("--tor"),
            OsString::from("off"),
        ];
        for relay in &ctx.relays {
            args.push("--relay".into());
            args.push(relay.into());
        }
        args.extend([
            "setup".into(),
            "--role".into(),
            role.into(),
            "--no-grants".into(),
        ]);
        let output =
            process::run(&launcher, &args, &o.workspace, &environment, cutoff, 65536).await?;
        if !output.status.success() {
            return Err(bad());
        }
        let setup: Setup = serde_json::from_slice(&output.stdout).map_err(|_| bad())?;
        if setup.schema != 1
            || setup.identity != name
            || setup.backend != "file"
            || setup.role != role
            || setup.inbound_enabled
            || !setup.config_changed
            || !setup.identity_created
            || !setup.owner_opened
            || !setup.engine_stop_complete
            || !setup.mdk_stop_complete
            || !setup.runtime_cleanup_complete
        {
            return Err(bad());
        }
        accounts.push(hex::encode(crate::command_config::account(&setup.npub)?));
        if state.join(name).join("commands").try_exists()? {
            return Err(bad());
        }
    }
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce).map_err(|_| Error::Identity("random"))?;
    let record = IdentityReady {
        schema: 1,
        kind: "identity-ready".into(),
        context_sha256: ctx.digest()?,
        role: o.role.clone(),
        nonce: hex::encode(nonce),
        client_account: accounts.remove(0),
        server_account: accounts.remove(0),
        installation: Installation {
            version: manifest.version,
            target: manifest.target,
            source_commit: manifest.commit,
            manifest_sha256: hex::encode(expected),
            bootstrap_sha256: records::digest(&bootstrap),
            files: manifest
                .files
                .into_iter()
                .map(|a| {
                    (
                        a.name.to_owned(),
                        FilePin {
                            sha256: hex::encode(a.sha256),
                            bytes: a.bytes,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        },
        identity_ready_at: super::now()?,
    };
    record.validate(&ctx, &o.role)?;
    records::write(
        &o.output.join(format!("{}-identity-ready.json", o.role)),
        &record,
    )
}
