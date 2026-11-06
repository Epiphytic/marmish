//! Local setup only: no relay activation, execution worker, or service startup.
use crate::command_config::CommandConfig;
use crate::command_lifecycle::{OwnedIdentity, stop_owned};
use crate::identity::{BackendChoice, ProfileLease};
use crate::mdk_adapter::MdkOwner;
use crate::transport::{ArtiDirs, RelayConnector, default_relays};
use crate::{Error, Result};
use nostr::nips::nip19::ToBech32;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::{
    fs::{DirBuilderExt, OpenOptionsExt},
    io::AsRawFd,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SetupRole {
    Client,
    Server,
    Both,
}
impl SetupRole {
    fn text(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
            Self::Both => "both",
        }
    }
}
pub(crate) struct SetupOptions {
    pub state_dir: PathBuf,
    pub identity: String,
    pub backend: BackendChoice,
    pub helper: PathBuf,
    pub interactive: bool,
    pub role: Option<SetupRole>,
    pub relays: Option<Vec<String>>,
    pub tor: Option<String>,
    pub grants: Vec<String>,
    pub revoke: Vec<String>,
    pub trust: Vec<String>,
    pub untrust: Vec<String>,
    pub update: bool,
    pub no_grants: bool,
    pub debug: bool,
}
#[derive(Serialize)]
pub(crate) struct SetupReport {
    pub schema: u8,
    pub identity: String,
    pub npub: String,
    pub backend: String,
    pub role: SetupRole,
    pub inbound_enabled: bool,
    pub config_changed: bool,
    pub identity_created: bool,
    pub owner_opened: bool,
    pub engine_stop_complete: bool,
    pub mdk_stop_complete: bool,
    pub runtime_cleanup_complete: bool,
}
fn list(values: &[String]) -> Result<BTreeSet<String>> {
    if values.len() > 4096 {
        return Err(Error::Invalid("invalid_grant"));
    }
    values
        .iter()
        .map(|s| {
            if !s.starts_with("npub1") {
                return Err(Error::Invalid("invalid_grant"));
            }
            crate::command_config::account(s)
                .map(hex::encode)
                .map_err(|_| Error::Invalid("invalid_grant"))
        })
        .collect()
}
fn update_set(value: &mut Value, field: &str, add: &[String], remove: &[String]) -> Result<()> {
    let mut set: BTreeSet<_> = value[field]
        .as_array()
        .ok_or(Error::Invalid("config_invalid"))?
        .iter()
        .map(|s| {
            s.as_str()
                .ok_or(Error::Invalid("config_invalid"))
                .and_then(crate::command_config::account)
                .map(hex::encode)
        })
        .collect::<Result<_>>()?;
    let before = set.clone();
    let add = list(add)?;
    let remove = list(remove)?;
    if !add.is_disjoint(&remove) {
        return Err(Error::Invalid("invalid_grant"));
    }
    set.extend(add);
    for key in remove {
        set.remove(&key);
    }
    if set != before {
        value[field] = json!(set);
    }
    Ok(())
}
fn proposed(mut old: Option<Value>, o: &SetupOptions) -> Result<(Value, bool, SetupRole)> {
    let existing = old.is_some();
    let role = o
        .role
        .unwrap_or_else(|| match old.as_ref().and_then(|v| v["role"].as_str()) {
            Some("client") => SetupRole::Client,
            Some("server") => SetupRole::Server,
            _ => SetupRole::Both,
        });
    if role == SetupRole::Client && !o.grants.is_empty() || o.no_grants && !o.grants.is_empty() {
        return Err(Error::Invalid("invalid_grant"));
    }
    if !existing
        && !o.interactive
        && role != SetupRole::Client
        && o.grants.is_empty()
        && !o.no_grants
    {
        return Err(Error::Invalid("grant_choice_required"));
    }
    let before = old.clone();
    let mut value=old.take().unwrap_or_else(||json!({"version":crate::command_config::CONFIG_FORMAT,"role":role.text(),"relays":default_relays(),"tor":"auto","trusted_remotes":[],"execution_clients":[],"command_policy":{"version":1},"cwd":null,"limits":{},"aliases":{}}));
    if let Some(r) = o.role {
        value["role"] = json!(r.text());
    }
    if let Some(r) = &o.relays {
        value["relays"] = json!(r);
    }
    if let Some(t) = &o.tor {
        value["tor"] = json!(t);
    }
    update_set(&mut value, "execution_clients", &o.grants, &o.revoke)?;
    update_set(&mut value, "trusted_remotes", &o.trust, &o.untrust)?;
    if o.no_grants
        && !value["execution_clients"]
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return Err(Error::Invalid("explicit_revoke_required"));
    }
    CommandConfig::from_bytes(
        &serde_json::to_vec(&value).map_err(|_| Error::Invalid("config_invalid"))?,
    )?;
    let changed = before.as_ref() != Some(&value);
    if existing && changed && !o.update {
        return Err(Error::Invalid("config_exists"));
    }
    Ok((value, changed, role))
}
struct Tty {
    file: File,
}
impl Tty {
    fn open() -> Result<Self> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_CLOEXEC)
            .open("/dev/tty")
            .map_err(|_| Error::Invalid("no_tty"))?;
        let fd = f.as_raw_fd();
        if unsafe { libc::isatty(fd) } != 1
            || unsafe { libc::tcgetpgrp(fd) } != unsafe { libc::getpgrp() }
        {
            return Err(Error::Invalid("no_tty"));
        }
        Ok(Self { file: f })
    }
    fn line(&mut self, prompt: &str) -> Result<String> {
        self.file.write_all(prompt.as_bytes())?;
        self.file.flush()?;
        let mut v = Vec::new();
        let mut byte = [0];
        loop {
            if self.file.read(&mut byte)? == 0 {
                return Err(Error::Invalid("interrupted"));
            }
            if byte[0] == b'\n' {
                break;
            }
            if v.len() >= 65536 {
                return Err(Error::Invalid("setup_input"));
            }
            v.push(byte[0]);
        }
        String::from_utf8(v)
            .map(|s| s.trim().to_owned())
            .map_err(|_| Error::Invalid("setup_input"))
    }
}
fn choices(o: &mut SetupOptions) -> Result<()> {
    if !o.interactive {
        return Ok(());
    }
    let mut tty = Tty::open()?;
    if o.role.is_none() {
        o.role = Some(
            match tty.line("Role [c]lient/[s]erver/[b]oth [b]: ")?.as_str() {
                "" | "b" | "both" => SetupRole::Both,
                "c" | "client" => SetupRole::Client,
                "s" | "server" => SetupRole::Server,
                _ => return Err(Error::Invalid("setup_role")),
            },
        );
    }
    if o.relays.is_none() {
        tty.file.write_all(
            b"Enter keeps existing relay settings, or uses White Noise EU/US for a new profile.\nDefault Tor auto connects to public relays directly; choose always or --tor always for Tor.\n",
        )?;
        let text = tty.line("Explicit relay URL(s), separated by spaces: ")?;
        if !text.is_empty() {
            o.relays = Some(text.split_whitespace().map(str::to_owned).collect());
        }
    }
    if o.tor.is_none() {
        o.tor = Some(
            match tty
                .line("Tor auto/always/off [auto, experimental]: ")?
                .as_str()
            {
                "" | "auto" => "auto",
                "always" => "always",
                "off" => "off",
                _ => return Err(Error::Invalid("route_mode")),
            }
            .into(),
        );
    }
    if o.role != Some(SetupRole::Client) && o.grants.is_empty() && !o.no_grants {
        let text = tty.line("Npub(s) allowed to execute here (empty disables inbound): ")?;
        o.grants = text.split_whitespace().map(str::to_owned).collect();
    }
    if o.role != Some(SetupRole::Server) && o.trust.is_empty() {
        let text = tty.line("Trusted outbound server npub(s), independently (empty allowed): ")?;
        o.trust = text.split_whitespace().map(str::to_owned).collect();
    }
    Ok(())
}
fn config_read(profile: &ProfileLease) -> Result<Option<Value>> {
    let path = profile.root().join("config.json");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(_) => {
            let mut bytes = Vec::new();
            crate::identity::open_private(&path, false)?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            CommandConfig::from_bytes(&bytes).map_err(|error| match error {
                Error::Invalid("config_version") => Error::Storage("incompatible_state"),
                other => other,
            })?;
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| Error::Invalid("config_invalid"))
        }
    }
}
#[cfg(test)]
thread_local! { static FAIL_COMMIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
fn commit(root: &Path, value: &Value) -> Result<()> {
    #[cfg(test)]
    if FAIL_COMMIT.with(|v| v.replace(false)) {
        return Err(Error::Storage("setup_commit_injected"));
    }
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|_| Error::Identity("random"))?;
    let temp = root.join(format!(".config-{}", hex::encode(random)));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temp)?;
        serde_json::to_writer(&mut file, value).map_err(|_| Error::Invalid("config_invalid"))?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp, root.join("config.json"))?;
        File::open(root)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub(crate) async fn run(mut o: SetupOptions) -> Result<SetupReport> {
    if o.debug {
        eprintln!("marmish setup debug: stage=choices");
    }
    choices(&mut o)?;
    // Validate choices before any credential preparation. Existing config is read under lease.
    let state = o.state_dir.clone();
    let identity = o.identity.clone();
    let mut profile = tokio::task::spawn_blocking(move || {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&state)?;
        ProfileLease::acquire(&state, &identity)
    })
    .await
    .map_err(|_| Error::Identity("initialization_worker"))??;
    let (config, changed, role) = proposed(config_read(&profile)?, &o).inspect_err(|error| {
        if matches!(error, Error::Invalid("config_exists")) {
            eprintln!(
                "marmish setup: existing configuration at {:?} differs from the requested options; rerun setup with --update to apply them (existing identity and data are kept).",
                profile.root().join("config.json")
            );
        }
    })?;
    let normalized = CommandConfig::from_bytes(
        &serde_json::to_vec(&config).map_err(|_| Error::Invalid("config_invalid"))?,
    )?;
    let snapshot = profile.setup_snapshot();
    let identity_created = snapshot.public_key.is_none();
    if snapshot
        .backend
        .is_some_and(|b| o.backend != BackendChoice::Auto && o.backend != b)
    {
        return Err(Error::Identity("backend_mismatch"));
    }
    let (key, backend, stop, opened) = if snapshot.ready {
        if changed {
            commit(profile.root(), &config)?;
        }
        (
            snapshot
                .public_key
                .ok_or(Error::Identity("invalid_manifest"))?,
            snapshot
                .backend
                .ok_or(Error::Identity("invalid_manifest"))?,
            None,
            false,
        )
    } else {
        let choice = o.backend;
        let helper = o.helper;
        let (mut profile, material) = tokio::task::spawn_blocking(move || {
            let material = profile.prepare(
                choice,
                &helper,
                std::time::Instant::now() + Duration::from_secs(30),
            )?;
            Ok::<_, Error>((profile, material))
        })
        .await
        .map_err(|_| Error::Identity("initialization_worker"))??;
        crate::native_preflight::check(&profile, &material)?;
        let key = material.public_key();
        let connector = RelayConnector::new(
            normalized.relay_policy().clone(),
            ArtiDirs {
                state: profile.arti_state().into(),
                cache: profile.arti_cache().into(),
            },
        )?;
        let backend = profile
            .setup_snapshot()
            .backend
            .ok_or(Error::Identity("invalid_manifest"))?;
        let owner = MdkOwner::open(&profile, &material, connector, vec![])?;
        let action = (|| {
            profile.mark_ready(owner.opened_binding())?;
            if changed {
                commit(profile.root(), &config)?;
            }
            Ok::<_, Error>(())
        })();
        let observed = stop_owned(
            OwnedIdentity { owner, profile },
            tokio::time::Instant::now() + Duration::from_secs(30),
        )
        .await;
        let report = if let Some(pending) = observed.pending {
            pending.finish().await
        } else {
            observed.report
        };
        action?;
        if !report.engine_stop_complete
            || !report.stream_stop_complete
            || !report.mdk_stop_complete
            || !report.runtime_cleanup_complete
        {
            return Err(Error::Storage("setup_stop_incomplete"));
        }
        (key, backend, Some(report), true)
    };
    if o.debug {
        eprintln!("marmish setup debug: stage=complete");
    }
    Ok(SetupReport {
        schema: 1,
        identity: o.identity,
        npub: key.to_bech32().map_err(|_| Error::Identity("public_key"))?,
        backend: if backend == BackendChoice::Keychain {
            "keychain"
        } else {
            "file"
        }
        .into(),
        role,
        inbound_enabled: role != SetupRole::Client && !normalized.execution_clients().is_empty(),
        config_changed: changed,
        identity_created,
        owner_opened: opened,
        engine_stop_complete: stop.is_none_or(|s| s.engine_stop_complete),
        mdk_stop_complete: stop.is_none_or(|s| s.mdk_stop_complete),
        runtime_cleanup_complete: stop.is_none_or(|s| s.runtime_cleanup_complete),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> SetupOptions {
        SetupOptions {
            state_dir: PathBuf::new(),
            identity: "default".into(),
            backend: BackendChoice::File,
            helper: PathBuf::new(),
            interactive: false,
            role: Some(SetupRole::Both),
            relays: Some(vec!["wss://relay.example/".into()]),
            tor: Some("off".into()),
            grants: vec![],
            revoke: vec![],
            trust: vec![],
            untrust: vec![],
            update: false,
            no_grants: true,
            debug: false,
        }
    }
    #[test]
    fn both_without_grants_is_disabled_and_new_profiles_default_to_white_noise() {
        let o = options();
        let (v, _, role) = proposed(None, &o).unwrap();
        assert_eq!(role, SetupRole::Both);
        assert!(v["execution_clients"].as_array().unwrap().is_empty());
        let mut o = options();
        o.relays = None;
        o.tor = None;
        let (defaulted, _, _) = proposed(None, &o).unwrap();
        assert_eq!(defaulted["relays"], json!(default_relays()));
        assert_eq!(defaulted["tor"], "auto");
        assert!(
            defaulted["execution_clients"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(v["relays"], json!(["wss://relay.example/"]));
    }
    #[test]
    fn omitted_setup_relays_preserve_existing_authority() {
        let (original, _, _) = proposed(None, &options()).unwrap();
        let mut omitted = options();
        omitted.relays = None;
        let (repeated, changed, _) = proposed(Some(original.clone()), &omitted).unwrap();
        assert!(!changed);
        assert_eq!(repeated, original);
        omitted.update = true;
        omitted.role = Some(SetupRole::Client);
        let (updated, changed, _) = proposed(Some(original.clone()), &omitted).unwrap();
        assert!(changed);
        assert_eq!(updated["relays"], original["relays"]);
    }
    #[test]
    fn explicit_empty_or_invalid_setup_relays_never_use_defaults() {
        for relays in [
            vec![],
            vec!["not-a-relay".into()],
            vec!["ws://relay.example/".into()],
        ] {
            let mut o = options();
            o.relays = Some(relays);
            assert!(proposed(None, &o).is_err());
        }
    }
    #[test]
    fn existing_restrictions_preserved_and_update_explicit() {
        let mut o = options();
        let (v, _, _) = proposed(None, &o).unwrap();
        assert!(!proposed(Some(v.clone()), &o).unwrap().1);
        o.role = Some(SetupRole::Client);
        assert!(proposed(Some(v.clone()), &o).is_err());
        o.update = true;
        let (new, _, _) = proposed(Some(v.clone()), &o).unwrap();
        for field in ["limits", "cwd", "aliases", "command_policy"] {
            assert_eq!(new[field], v[field]);
        }
    }
    fn private_state() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        tempfile::Builder::new()
            .prefix("marmish-setup-test-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(std::env::var_os("HOME").unwrap())
            .unwrap()
    }
    #[tokio::test(flavor = "current_thread")]
    async fn actual_file_setup_is_local_repeatable_and_releases_lease() {
        let root = private_state();
        let mut first = options();
        first.state_dir = root.path().join("state");
        first.relays = None;
        let report = run(first).await.unwrap();
        assert!(report.identity_created && report.owner_opened);
        assert!(!report.inbound_enabled);
        assert!(
            report.engine_stop_complete
                && report.mdk_stop_complete
                && report.runtime_cleanup_complete
        );
        let mut second = options();
        second.state_dir = root.path().join("state");
        second.relays = None;
        let repeated = run(second).await.unwrap();
        assert_eq!(repeated.npub, report.npub);
        assert!(!repeated.identity_created && !repeated.owner_opened && !repeated.config_changed);
        let profile = ProfileLease::acquire(&root.path().join("state"), "default").unwrap();
        assert!(profile.setup_snapshot().ready);
        let config = config_read(&profile).unwrap().unwrap();
        assert_eq!(config["relays"], json!(default_relays()));
        assert!(!profile.root().join("commands").exists());
    }
    #[tokio::test(flavor = "current_thread")]
    async fn config_commit_failure_stops_owner_before_retry() {
        let root = private_state();
        let state = root.path().join("state");
        let mut first = options();
        first.state_dir = state.clone();
        FAIL_COMMIT.with(|v| v.set(true));
        assert!(matches!(
            run(first).await,
            Err(Error::Storage("setup_commit_injected"))
        ));
        let profile = ProfileLease::acquire(&state, "default").unwrap();
        let snapshot = profile.setup_snapshot();
        assert!(snapshot.ready);
        assert!(!profile.root().join("config.json").exists());
        let key = snapshot.public_key.unwrap();
        drop(profile);
        let mut second = options();
        second.state_dir = state;
        let report = run(second).await.unwrap();
        assert_eq!(report.npub, key.to_bech32().unwrap());
        assert!(!report.owner_opened && !report.identity_created);
        assert!(report.config_changed);
    }
    #[test]
    fn repeated_semantic_principals_preserve_npub_spelling_and_order() {
        let mut o = options();
        o.no_grants = false;
        let (mut value, _, _) = proposed(None, &options()).unwrap();
        let a = nostr::key::Keys::generate()
            .public_key()
            .to_bech32()
            .unwrap();
        let b = nostr::key::Keys::generate()
            .public_key()
            .to_bech32()
            .unwrap();
        value["execution_clients"] = json!([b, a]);
        value["trusted_remotes"] = json!([a, b]);
        let (repeated, changed, _) = proposed(Some(value.clone()), &o).unwrap();
        assert!(!changed);
        assert_eq!(repeated, value);
        o.grants = vec![a.clone()];
        o.trust = vec![b.clone()];
        let (repeated, changed, _) = proposed(Some(value.clone()), &o).unwrap();
        assert!(!changed);
        assert_eq!(repeated, value);
    }
    #[tokio::test(flavor = "current_thread")]
    async fn future_config_is_incompatible_without_identity_or_config_mutation() {
        let root = private_state();
        let mut options = options();
        options.state_dir = root.path().join("state");
        let profile = ProfileLease::acquire(&options.state_dir, "default").unwrap();
        let mut value = proposed(None, &options).unwrap().0;
        value["version"] = json!(2);
        commit(profile.root(), &value).unwrap();
        let path = profile.root().join("config.json");
        let before = fs::read(&path).unwrap();
        let names = fs::read_dir(profile.root())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<BTreeSet<_>>();
        assert!(profile.setup_snapshot().public_key.is_none());
        drop(profile);
        assert!(matches!(
            run(options).await,
            Err(Error::Storage("incompatible_state"))
        ));
        assert_eq!(fs::read(path).unwrap(), before);
        let profile = ProfileLease::acquire(&root.path().join("state"), "default").unwrap();
        assert!(profile.setup_snapshot().public_key.is_none());
        assert_eq!(
            fs::read_dir(profile.root())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<BTreeSet<_>>(),
            names
        );
    }
}
