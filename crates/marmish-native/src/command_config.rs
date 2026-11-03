//! Private configuration normalization and pure executable-policy preflight.
//! Loading alone does not activate an engine or authorize command execution.
use crate::command_wire::Account;
use crate::transport::{RelayPolicy, RouteMode, TransportLimits};
use crate::{Error, Result};
use nostr::key::PublicKey;
use nostr::nips::nip19::FromBech32;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CStr;
use std::fmt;
use std::io::Read;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) const CONFIG_FORMAT: u32 = 1;
const MAX_CONFIG: u64 = 1024 * 1024;
const DAY: u64 = 86400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    Client,
    Server,
    Both,
}

/// The transport parser supplies the canonical immutable set, without DNS.
#[derive(Clone, Debug)]
pub(crate) struct RelaySet(RelayPolicy);
impl RelaySet {
    pub(crate) fn urls(&self) -> &[nostr::types::Url] {
        self.0.urls()
    }
}
#[derive(Clone, Debug)]
pub(crate) struct AliasTarget {
    pub(crate) remote: Account,
    pub(crate) relays: RelaySet,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandLimits {
    pub(crate) active_per_principal: u32,
    pub(crate) active_global: u32,
    pub(crate) retained_per_principal: u32,
    pub(crate) retained_global: u32,
    pub(crate) engine_starting: u32,
    pub(crate) engine_tokens: u32,
    pub(crate) token_lifetime: Duration,
    pub(crate) payload_retention: Duration,
    pub(crate) tombstone_retention: Duration,
    pub(crate) stream_bytes: u64,
    pub(crate) spool_bytes: u64,
    pub(crate) max_commands: Option<NonZeroU64>,
    pub(crate) max_uptime: Option<Duration>,
}

pub(crate) struct CommandConfig {
    role: Role,
    relay_policy: RelayPolicy,
    trusted_remotes: BTreeSet<Account>,
    execution_clients: BTreeSet<Account>,
    aliases: BTreeMap<String, AliasTarget>,
    cwd: PathBuf,
    limits: CommandLimits,
    // Preserve ordered additive layers for the later gated engine wrappers.
    // Loading does not compile globs, check admitted principals, or authorize exec.
    policy: RawPolicy,
}
impl CommandConfig {
    pub(crate) fn load(profile_root: &Path) -> Result<Self> {
        crate::identity::check_dir(profile_root)?;
        let file = crate::identity::open_private(&profile_root.join("config.json"), false)?;
        if file.metadata()?.len() > MAX_CONFIG {
            return Err(Error::Invalid("config_size"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_CONFIG {
            return Err(Error::Invalid("config_size"));
        }
        Self::from_bytes(&bytes)
    }
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() as u64 > MAX_CONFIG {
            return Err(Error::Invalid("config_size"));
        }
        let raw: RawConfig =
            serde_json::from_slice(bytes).map_err(|_| Error::Invalid("config_schema"))?;
        Self::normalize(raw)
    }
    fn normalize(raw: RawConfig) -> Result<Self> {
        if raw.version != CONFIG_FORMAT {
            return Err(Error::Invalid("config_version"));
        }
        let mode: RouteMode = raw.tor.parse()?;
        let relay_policy = RelayPolicy::parse(&raw.relays, mode, TransportLimits::default())?;
        let trusted_remotes = accounts(raw.trusted_remotes)?;
        let execution_clients = accounts(raw.execution_clients)?;
        raw.command_policy.validate_bounds()?;
        let cwd = match raw.cwd {
            Some(value) => PathBuf::from(value),
            None => passwd_home()?,
        };
        if !cwd.is_absolute()
            || cwd.as_os_str().len() > 4095
            || cwd.as_os_str().as_encoded_bytes().contains(&0)
            || cwd.to_str().is_none()
        {
            return Err(Error::Invalid("config_cwd"));
        }
        let mut aliases = BTreeMap::new();
        for (name, target) in raw.aliases.0 {
            if !alias_name(&name) {
                return Err(Error::Invalid("config_alias"));
            }
            let policy = RelayPolicy::parse(&target.relays, mode, TransportLimits::default())?;
            if policy.urls() != relay_policy.urls() {
                return Err(Error::Invalid("config_alias_relays"));
            }
            aliases.insert(
                name,
                AliasTarget {
                    remote: account(&target.remote)?,
                    relays: RelaySet(policy),
                },
            );
        }
        Ok(Self {
            role: raw.role,
            relay_policy,
            trusted_remotes,
            execution_clients,
            aliases,
            cwd,
            limits: raw.limits.normalize()?,
            policy: raw.command_policy,
        })
    }
    fn command_policy(&self) -> Result<marmish_engine::CommandPolicy> {
        use marmish_engine::{CommandPolicy, KeyRestrictions, PrincipalId, Restrictions};
        let restrictions = |raw: &RawRestrictions| Restrictions {
            allow_exec: raw.allow_exec.clone(),
            deny_exec: raw.deny_exec.clone(),
        };
        let mut defaults = restrictions(&self.policy.defaults);
        if defaults.allow_exec.is_none() {
            defaults.allow_exec = Some(vec!["/**".into()]);
        }
        Ok(CommandPolicy {
            version: self.policy.version,
            defaults,
            keys: self
                .policy
                .keys
                .iter()
                .map(|entry| {
                    Ok(KeyRestrictions {
                        principal: PrincipalId(account(&entry.principal)?),
                        restrictions: restrictions(&entry.restrictions),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        })
    }
    pub(crate) fn engine_policy(
        &self,
        root: PathBuf,
        worker: PathBuf,
        server: Account,
        historical: &[Account],
    ) -> Result<Option<marmish_engine::EnginePolicy>> {
        use marmish_engine::{EngineLimits, EnginePolicy, PrincipalId, ServerId};
        if historical.len() > 4096 {
            return Err(Error::Invalid("config_principals"));
        }
        let principals: BTreeSet<_> = self
            .execution_clients
            .iter()
            .chain(historical)
            .copied()
            .collect();
        if principals.len() > 4096 {
            return Err(Error::Invalid("config_principals"));
        }
        if principals.is_empty() {
            return Ok(None);
        }
        let allowed_principals: Vec<_> = principals.into_iter().map(PrincipalId).collect();
        let commands = self.command_policy()?;
        commands
            .validate_for_principals(&allowed_principals)
            .map_err(|_| Error::Invalid("config_policy"))?;
        let limits = &self.limits;
        let policy = EnginePolicy {
            state_root: root,
            worker_executable: worker,
            server: ServerId(server),
            allowed_principals,
            allow_shell_command: false,
            command_policy: Some(commands),
            cwd: Some(self.cwd.clone()),
            limits: EngineLimits {
                active_jobs: limits.active_global,
                invocation_records: limits.retained_global,
                outstanding_tokens: limits.engine_tokens,
                starting_workers: limits.engine_starting,
                token_lifetime: limits.token_lifetime,
                spool_bytes_per_stream: limits.stream_bytes,
                aggregate_spool_bytes: limits.spool_bytes,
                output_retention: marmish_core::Retention::from_seconds(Some(
                    limits.payload_retention.as_secs(),
                ))
                .map_err(|_| Error::Invalid("config_limits"))?,
                tombstone_retention: limits.tombstone_retention,
                ..EngineLimits::default()
            },
        };
        policy
            .validate_limits()
            .map_err(|_| Error::Invalid("config_limits"))?;
        Ok(Some(policy))
    }
    /// Matching only: service independently enforces the current grant and Exec-only request.
    pub(crate) fn check_exec_restrictions(
        &self,
        principal: Account,
        argv: &[Vec<u8>],
    ) -> Result<()> {
        use marmish_engine::{CommandRequest, EngineLimits, EnginePolicy, PrincipalId, ServerId};
        let commands = self.command_policy()?;
        let admitted: Vec<_> = self
            .execution_clients
            .iter()
            .copied()
            .map(PrincipalId)
            .collect();
        // Startup validates the complete policy against current plus historical owners.
        // Matching does not re-authorize those owners or reject historical key layers.
        let policy = EnginePolicy {
            state_root: PathBuf::new(),
            worker_executable: PathBuf::new(),
            server: ServerId([0; 32]),
            allowed_principals: admitted,
            allow_shell_command: false,
            command_policy: Some(commands),
            cwd: None,
            limits: EngineLimits::default(),
        };
        policy
            .check_exec_restrictions(
                PrincipalId(principal),
                &CommandRequest::Exec {
                    argv: argv.to_vec(),
                },
            )
            .map(|_| ())
            .map_err(|_| Error::Invalid("command_denied"))
    }
    pub(crate) fn role(&self) -> Role {
        self.role
    }
    pub(crate) fn relay_policy(&self) -> &RelayPolicy {
        &self.relay_policy
    }
    pub(crate) fn trusted_remotes(&self) -> &BTreeSet<Account> {
        &self.trusted_remotes
    }
    pub(crate) fn execution_clients(&self) -> &BTreeSet<Account> {
        &self.execution_clients
    }
    pub(crate) fn aliases(&self) -> &BTreeMap<String, AliasTarget> {
        &self.aliases
    }
    #[cfg(test)]
    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }
    pub(crate) fn apply_limit_overrides(
        &mut self,
        overrides: crate::command_limits::LimitOverrides,
    ) -> Result<()> {
        let maximum = overrides.max_commands.or(self.limits.max_commands);
        let uptime = overrides
            .max_uptime
            .or(self.limits.max_uptime)
            .or_else(|| maximum.map(|_| crate::command_limits::MAX_UPTIME));
        if maximum.is_some_and(|n| n.get() > i64::MAX as u64)
            || uptime.is_some_and(|d| d.is_zero() || d > crate::command_limits::MAX_UPTIME)
            || (maximum.is_some()
                && (self.role == Role::Client || self.execution_clients.is_empty()))
        {
            return Err(Error::Invalid("command_limits"));
        }
        self.limits.max_commands = maximum;
        self.limits.max_uptime = uptime;
        Ok(())
    }
    pub(crate) fn limits(&self) -> &CommandLimits {
        &self.limits
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    role: Role,
    relays: Vec<String>,
    tor: String,
    trusted_remotes: Vec<String>,
    execution_clients: Vec<String>,
    command_policy: RawPolicy,
    #[serde(deserialize_with = "required_cwd")]
    cwd: Option<String>,
    limits: RawLimits,
    aliases: RawAliases,
}
// deserialize_with keeps this nullable field required in the containing object.
fn required_cwd<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<String>, D::Error> {
    Option::deserialize(d)
}
// Omission retains a field's default; explicit null is not its value syntax.
fn non_null_optional<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAlias {
    remote: String,
    relays: Vec<String>,
}
struct RawAliases(BTreeMap<String, RawAlias>);
impl<'de> Deserialize<'de> for RawAliases {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct AliasesVisitor;
        impl<'de> Visitor<'de> for AliasesVisitor {
            type Value = RawAliases;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("at most 256 distinct aliases")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut aliases = BTreeMap::new();
                while let Some(name) = map.next_key::<String>()? {
                    if aliases.len() >= 256 || aliases.contains_key(&name) {
                        return Err(de::Error::custom("alias count or duplicate"));
                    }
                    aliases.insert(name, map.next_value()?);
                }
                Ok(RawAliases(aliases))
            }
        }
        d.deserialize_map(AliasesVisitor)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    version: u32,
    #[serde(default)]
    defaults: RawRestrictions,
    #[serde(default)]
    keys: Vec<RawKeyRestrictions>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKeyRestrictions {
    principal: String,
    restrictions: RawRestrictions,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRestrictions {
    #[serde(default, deserialize_with = "non_null_optional")]
    allow_exec: Option<Vec<String>>,
    #[serde(default)]
    deny_exec: Vec<String>,
}
impl RawPolicy {
    fn validate_bounds(&self) -> Result<()> {
        if self.version != 1 || self.keys.len() > 128 {
            return Err(Error::Invalid("config_policy_bounds"));
        }
        for key in &self.keys {
            account(&key.principal)?;
        }
        let mut count = usize::from(self.defaults.allow_exec.is_none());
        for layer in
            std::iter::once(&self.defaults).chain(self.keys.iter().map(|k| &k.restrictions))
        {
            for pattern in layer.allow_exec.iter().flatten().chain(&layer.deny_exec) {
                count += 1;
                if count > 256 || pattern.len() > 1024 {
                    return Err(Error::Invalid("config_policy_bounds"));
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn account(value: &str) -> Result<Account> {
    // Account is a 32-byte identifier, not proof of curve validity/authentication.
    let key = if value.starts_with("npub1") {
        PublicKey::from_bech32(value)
    } else if crate::foundation::valid_hex(value, 64) {
        PublicKey::from_hex(value)
    } else {
        return Err(Error::Invalid("config_account"));
    };
    key.map(|k| k.to_bytes())
        .map_err(|_| Error::Invalid("config_account"))
}
fn accounts(values: Vec<String>) -> Result<BTreeSet<Account>> {
    if values.len() > 4096 {
        return Err(Error::Invalid("config_principals"));
    }
    let mut keys = BTreeSet::new();
    for value in values {
        if !keys.insert(account(&value)?) {
            return Err(Error::Invalid("config_principals"));
        }
    }
    Ok(keys)
}
fn alias_name(value: &str) -> bool {
    !value.is_empty()
        && !matches!(
            value,
            "init"
                | "setup"
                | "install"
                | "uninstall"
                | "install-status"
                | "release-info"
                | "keychain-authorize"
                | "daemon"
                | "status"
                | "commands"
                | "command-status"
                | "wait"
                | "output"
                | "cancel"
                | "open-conversation"
                | "send"
                | "received"
                | "foundation-check"
                | "command-check"
                | "command-evidence"
                | "command-cleanup-status"
                | "command-local-cancel"
                | "command-prepare-cleanup"
                | "help"
                | "_vault-helper"
        )
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn passwd_home() -> Result<PathBuf> {
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    let mut buffer = vec![0u8; 65536];
    let code = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            &mut passwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if code != 0 || found.is_null() || passwd.pw_dir.is_null() {
        return Err(Error::Invalid("config_passwd"));
    }
    let home = unsafe { CStr::from_ptr(passwd.pw_dir) }
        .to_str()
        .map_err(|_| Error::Invalid("config_cwd"))?;
    Ok(PathBuf::from(home))
}

/// JSON duration fields are integer seconds, normalized into Duration getters.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawLimits {
    active_per_principal: u32,
    active_global: u32,
    retained_per_principal: u32,
    retained_global: u32,
    engine_starting: u32,
    engine_tokens: u32,
    token_lifetime: u64,
    payload_retention: u64,
    tombstone_retention: u64,
    stream_bytes: u64,
    spool_bytes: u64,
    #[serde(deserialize_with = "non_null_optional")]
    max_commands: Option<NonZeroU64>,
    max_uptime: Option<u64>,
}
impl Default for RawLimits {
    fn default() -> Self {
        Self {
            active_per_principal: 4,
            active_global: 32,
            retained_per_principal: 1024,
            retained_global: 10000,
            engine_starting: 4,
            engine_tokens: 1024,
            token_lifetime: 120,
            payload_retention: DAY,
            tombstone_retention: 2 * DAY,
            stream_bytes: 8 * 1024 * 1024,
            spool_bytes: 512 * 1024 * 1024,
            max_commands: None,
            max_uptime: None,
        }
    }
}
impl RawLimits {
    fn normalize(self) -> Result<CommandLimits> {
        if !(1..=4).contains(&self.active_per_principal)
            || !(1..=32).contains(&self.active_global)
            || self.active_per_principal > self.active_global
            || !(1..=1024).contains(&self.retained_per_principal)
            || !(1..=10000).contains(&self.retained_global)
            || self.retained_per_principal > self.retained_global
            || self.active_global > self.retained_global
            || !(1..=4).contains(&self.engine_starting)
            || !(1..=1024).contains(&self.engine_tokens)
            || self.token_lifetime != 120
            || self.tombstone_retention != 2 * DAY
            || !(1..=7 * DAY).contains(&self.payload_retention)
            || !(1..=8 * 1024 * 1024).contains(&self.stream_bytes)
            || !(1..=512 * 1024 * 1024).contains(&self.spool_bytes)
            || self.max_uptime.is_some_and(|s| s == 0 || s > 1800)
        {
            return Err(Error::Invalid("config_limits"));
        }
        Ok(CommandLimits {
            active_per_principal: self.active_per_principal,
            active_global: self.active_global,
            retained_per_principal: self.retained_per_principal,
            retained_global: self.retained_global,
            engine_starting: self.engine_starting,
            engine_tokens: self.engine_tokens,
            token_lifetime: Duration::from_secs(self.token_lifetime),
            payload_retention: Duration::from_secs(self.payload_retention),
            tombstone_retention: Duration::from_secs(self.tombstone_retention),
            stream_bytes: self.stream_bytes,
            spool_bytes: self.spool_bytes,
            max_commands: self.max_commands,
            max_uptime: self.max_uptime.map(Duration::from_secs),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::nips::nip19::ToBech32;
    use serde_json::{Value, json};
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};

    fn document() -> Value {
        json!({"version":1,"role":"both","relays":["wss://relay.example/"],"tor":"auto",
            "trusted_remotes":[],"execution_clients":[],"command_policy":{"version":1},
            "cwd":null,"limits":{},"aliases":{}})
    }
    struct Fixture(tempfile::TempDir);
    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
            Self(dir)
        }
        fn write(&self, bytes: &[u8]) {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(self.path())
                .unwrap();
            file.write_all(bytes).unwrap();
        }
        fn path(&self) -> PathBuf {
            self.0.path().join("config.json")
        }
        fn load(&self, value: &Value) -> Result<CommandConfig> {
            self.write(&serde_json::to_vec(value).unwrap());
            CommandConfig::load(self.0.path())
        }
    }
    #[test]
    fn launch_overrides_merge_before_effective_limit_validation() {
        use crate::command_limits::{LimitOverrides, MAX_UPTIME};
        let fixture = Fixture::new();
        let mut value = document();
        value["execution_clients"] = json!(["11".repeat(32)]);
        let mut config = fixture.load(&value).unwrap();
        config
            .apply_limit_overrides(LimitOverrides {
                max_commands: NonZeroU64::new(3),
                max_uptime: None,
            })
            .unwrap();
        assert_eq!(config.limits().max_uptime, Some(MAX_UPTIME));
        config
            .apply_limit_overrides(LimitOverrides {
                max_commands: None,
                max_uptime: Some(Duration::from_secs(15)),
            })
            .unwrap();
        assert_eq!(config.limits().max_commands.unwrap().get(), 3);
        assert_eq!(config.limits().max_uptime, Some(Duration::from_secs(15)));
        let mut no_grants = fixture.load(&document()).unwrap();
        assert!(
            no_grants
                .apply_limit_overrides(LimitOverrides {
                    max_commands: NonZeroU64::new(1),
                    max_uptime: None
                })
                .is_err()
        );
        assert!(no_grants.limits().max_commands.is_none());
        value["role"] = json!("client");
        let mut client = fixture.load(&value).unwrap();
        assert!(
            client
                .apply_limit_overrides(LimitOverrides {
                    max_commands: NonZeroU64::new(1),
                    max_uptime: None
                })
                .is_err()
        );
        assert!(
            client
                .apply_limit_overrides(LimitOverrides {
                    max_commands: None,
                    max_uptime: Some(Duration::ZERO)
                })
                .is_err()
        );
    }
    #[test]
    fn required_fields_defaults_and_all_getters() {
        let fixture = Fixture::new();
        let config = fixture.load(&document()).unwrap();
        assert_eq!(config.role(), Role::Both);
        assert_eq!(
            config.relay_policy().urls()[0].as_str(),
            "wss://relay.example/"
        );
        assert!(config.trusted_remotes().is_empty());
        assert!(config.execution_clients().is_empty());
        assert!(config.aliases().is_empty());
        assert_eq!(config.cwd(), passwd_home().unwrap());
        let limits = config.limits();
        assert_eq!((limits.active_per_principal, limits.active_global), (4, 32));
        assert_eq!(
            (limits.retained_per_principal, limits.retained_global),
            (1024, 10000)
        );
        assert_eq!((limits.engine_starting, limits.engine_tokens), (4, 1024));
        assert_eq!(limits.token_lifetime, Duration::from_secs(120));
        assert_eq!(limits.payload_retention, Duration::from_secs(DAY));
        assert_eq!(limits.tombstone_retention, Duration::from_secs(2 * DAY));
        assert_eq!(
            (limits.stream_bytes, limits.spool_bytes),
            (8 * 1024 * 1024, 512 * 1024 * 1024)
        );
        assert!(limits.max_commands.is_none() && limits.max_uptime.is_none());
        for key in document().as_object().unwrap().keys() {
            let mut missing = document();
            missing.as_object_mut().unwrap().remove(key);
            assert!(fixture.load(&missing).is_err(), "missing {key}");
        }
    }
    #[test]
    fn duplicate_and_unknown_fields_fail_in_every_object() {
        let fixture = Fixture::new();
        let base = serde_json::to_string(&document()).unwrap();
        let cases = [
            base.replacen("\"version\":1", "\"version\":1,\"version\":1", 1),
            base.replacen("\"limits\":{}", "\"limits\":{\"active_global\":32,\"active_global\":32}", 1),
            base.replacen("\"aliases\":{}", "\"aliases\":{\"a\":{},\"a\":{}}", 1),
            base.replacen("\"aliases\":{}", "\"aliases\":{\"a\":{\"remote\":\"x\",\"remote\":\"x\",\"relays\":[]}}", 1),
            base.replacen("\"command_policy\":{\"version\":1}", "\"command_policy\":{\"version\":1,\"defaults\":{\"deny_exec\":[],\"deny_exec\":[]}}", 1),
            base.replacen("\"command_policy\":{\"version\":1}", "\"command_policy\":{\"version\":1,\"keys\":[{\"principal\":\"x\",\"principal\":\"x\",\"restrictions\":{}}]}", 1),
            base.replacen("\"limits\":{}", "\"limits\":{\"secret\":1}", 1),
            base.replacen("\"role\":\"both\"", "\"role\":\"both\",\"extra\":1", 1),
        ];
        for bytes in cases {
            fixture.write(bytes.as_bytes());
            assert!(CommandConfig::load(fixture.0.path()).is_err());
        }
    }
    #[test]
    fn account_whitelist_and_duplicates() {
        let key = PublicKey::from_hex(&"ff".repeat(32)).unwrap();
        let npub = key.to_bech32().unwrap();
        assert_eq!(account(&npub).unwrap(), [255; 32]);
        assert_eq!(account(&"ff".repeat(32)).unwrap(), [255; 32]);
        for invalid in [
            "FF".repeat(32),
            format!("nostr:{npub}"),
            npub.to_uppercase(),
            "nsec1not-a-public-key".into(),
            "nprofile1invalid".into(),
            "00".repeat(31),
        ] {
            assert!(account(&invalid).is_err());
        }
        assert!(accounts(vec![npub, "ff".repeat(32)]).is_err());
        assert!(accounts(vec!["00".repeat(32); 4097]).is_err());
        let values = (0..4096u64).map(|n| format!("{n:064x}")).collect();
        assert_eq!(accounts(values).unwrap().len(), 4096);
    }
    #[test]
    fn aliases_require_exact_canonical_set_and_valid_name() {
        let fixture = Fixture::new();
        let mut value = document();
        value["aliases"] =
            json!({"Remote_1":{"remote":"00".repeat(32),"relays":["wss://relay.example:443/"]}});
        let config = fixture.load(&value).unwrap();
        let target = &config.aliases()["Remote_1"];
        assert_eq!(target.remote, [0; 32]);
        assert_eq!(target.relays.urls(), config.relay_policy().urls());
        value["aliases"]["Remote_1"]["relays"] = json!(["wss://other.example/"]);
        assert!(fixture.load(&value).is_err());
        for name in [
            "",
            "1bad",
            "npub@host",
            "with space",
            "é",
            &"a".repeat(65),
            "keychain-authorize",
            "command-evidence",
            "command-cleanup-status",
            "command-local-cancel",
            "command-prepare-cleanup",
        ] {
            assert!(!alias_name(name));
        }
        let mut value = document();
        let aliases = (0..257)
            .map(|n| {
                (
                    format!("a{n}"),
                    json!({"remote":"00".repeat(32),"relays":["wss://relay.example/"]}),
                )
            })
            .collect();
        value["aliases"] = Value::Object(aliases);
        assert!(fixture.load(&value).is_err());
    }
    #[test]
    fn ordered_additive_layers_are_preserved_without_glob_validation() {
        let fixture = Fixture::new();
        let mut value = document();
        value["command_policy"] = json!({"version":1,"defaults":{"deny_exec":["not-a-glob-policy"]},"keys":[
            {"principal":"00".repeat(32),"restrictions":{"allow_exec":[]}},
            {"principal":"00".repeat(32),"restrictions":{"allow_exec":["invalid\\escape"]}}]});
        let config = fixture.load(&value).unwrap();
        assert!(config.policy.defaults.allow_exec.is_none());
        assert_eq!(config.policy.defaults.deny_exec, ["not-a-glob-policy"]);
        assert_eq!(config.policy.keys.len(), 2);
        assert_eq!(config.policy.keys[0].restrictions.allow_exec, Some(vec![]));
        assert_eq!(
            config.policy.keys[1].restrictions.allow_exec,
            Some(vec!["invalid\\escape".into()])
        );
    }
    #[test]
    fn policy_raw_bounds_include_future_default_allow() {
        let fixture = Fixture::new();
        let mut value = document();
        value["command_policy"]["defaults"] = json!({"deny_exec":vec!["/x";255]});
        assert!(fixture.load(&value).is_ok());
        value["command_policy"]["defaults"]["deny_exec"] = json!(vec!["/x"; 256]);
        assert!(fixture.load(&value).is_err());
        value["command_policy"]["defaults"]["allow_exec"] = json!([]);
        assert!(fixture.load(&value).is_ok());
        value["command_policy"]["defaults"] = json!({"allow_exec":["x".repeat(1025)]});
        assert!(fixture.load(&value).is_err());
        value["command_policy"] = json!({"version":1,"keys":vec![json!({"principal":"00".repeat(32),"restrictions":{}});129]});
        assert!(fixture.load(&value).is_err());
    }
    #[test]
    fn closed_limits_seconds_fixed_values_and_ceiling_relationships() {
        let fixture = Fixture::new();
        for (key, invalid) in [
            ("active_per_principal", 5),
            ("active_global", 33),
            ("retained_per_principal", 1025),
            ("retained_global", 10001),
            ("engine_starting", 5),
            ("engine_tokens", 1025),
            ("token_lifetime", 60),
            ("tombstone_retention", 1),
            ("payload_retention", 0),
            ("stream_bytes", 8388609),
            ("spool_bytes", 536870913),
            ("max_uptime", 1801),
            ("max_commands", 0),
        ] {
            let mut value = document();
            value["limits"][key] = json!(invalid);
            assert!(fixture.load(&value).is_err(), "{key}");
        }
        let mut value = document();
        value["limits"] = json!({"payload_retention":604800,"max_uptime":1800,"max_commands":3});
        let config = fixture.load(&value).unwrap();
        assert_eq!(
            config.limits().payload_retention,
            Duration::from_secs(604800)
        );
        assert_eq!(config.limits().max_uptime, Some(Duration::from_secs(1800)));
        assert_eq!(config.limits().max_commands.unwrap().get(), 3);
        value["limits"]["token_lifetime"] = json!({"secs":120,"nanos":0});
        assert!(fixture.load(&value).is_err());
        value["limits"] = json!({"retained_global":1});
        assert!(fixture.load(&value).is_err());
    }
    #[test]
    fn null_limits_and_policy_members_do_not_mean_omission() {
        let fixture = Fixture::new();
        for key in ["max_commands", "active_global", "token_lifetime"] {
            let mut value = document();
            value["limits"][key] = Value::Null;
            assert!(fixture.load(&value).is_err(), "{key}");
        }
        let mut value = document();
        value["limits"]["max_uptime"] = Value::Null;
        let config = fixture.load(&value).unwrap();
        assert_eq!(config.limits().max_commands, None);
        assert_eq!(config.limits().max_uptime, None);
        value["command_policy"]["defaults"]["allow_exec"] = Value::Null;
        assert!(fixture.load(&value).is_err());
        value["command_policy"] = json!({"version":1,"keys":[{
            "principal":"00".repeat(32),"restrictions":{"allow_exec":null}
        }]});
        assert!(fixture.load(&value).is_err());
    }
    #[test]
    fn engine_only_limit_relationships_are_not_structural_validation() {
        let fixture = Fixture::new();
        for limits in [
            json!({"active_per_principal":3,"active_global":3,"engine_starting":4}),
            json!({"active_per_principal":4,"retained_per_principal":1}),
            json!({"stream_bytes":8388608,"spool_bytes":1}),
        ] {
            let mut value = document();
            value["limits"] = limits;
            assert!(fixture.load(&value).is_ok());
        }
    }
    #[test]
    fn cwd_is_literal_absolute_or_passwd_home() {
        let fixture = Fixture::new();
        for cwd in [
            "relative",
            "~/bin",
            "$HOME",
            "/nul\0path",
            &format!("/{}", "a".repeat(4095)),
        ] {
            let mut value = document();
            value["cwd"] = json!(cwd);
            assert!(fixture.load(&value).is_err());
        }
        let mut value = document();
        value["cwd"] = json!("/literal-$HOME");
        assert_eq!(
            fixture.load(&value).unwrap().cwd(),
            Path::new("/literal-$HOME")
        );
    }
    #[test]
    fn relay_raw_count_and_tor_validation_are_existing_policy_checks() {
        let fixture = Fixture::new();
        for relays in [
            vec![],
            vec!["wss://relay.example/"; 17],
            vec!["ws://ordinary.example/"],
        ] {
            let mut value = document();
            value["relays"] = json!(relays);
            assert!(fixture.load(&value).is_err());
        }
        let mut value = document();
        value["tor"] = json!("unexpected");
        assert!(fixture.load(&value).is_err());
    }
    #[test]
    fn private_file_permissions_links_and_fifo_refused() {
        let fixture = Fixture::new();
        fixture.load(&document()).unwrap();
        fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(CommandConfig::load(fixture.0.path()).is_err());
        fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(fixture.0.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(CommandConfig::load(fixture.0.path()).is_err());
        fs::set_permissions(fixture.0.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::hard_link(fixture.path(), fixture.0.path().join("other")).unwrap();
        assert!(CommandConfig::load(fixture.0.path()).is_err());
        fs::remove_file(fixture.path()).unwrap();
        symlink(fixture.0.path().join("other"), fixture.path()).unwrap();
        assert!(CommandConfig::load(fixture.0.path()).is_err());
        fs::remove_file(fixture.path()).unwrap();
        let path = std::ffi::CString::new(fixture.path().as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(CommandConfig::load(fixture.0.path()).is_err());
    }
    #[test]
    fn oversized_input_and_trailing_json_are_rejected() {
        let fixture = Fixture::new();
        fixture.write(&vec![b' '; MAX_CONFIG as usize + 1]);
        assert!(matches!(
            CommandConfig::load(fixture.0.path()),
            Err(Error::Invalid("config_size"))
        ));
        let mut bytes = serde_json::to_vec(&document()).unwrap();
        bytes.extend_from_slice(b"{}");
        fixture.write(&bytes);
        assert!(CommandConfig::load(fixture.0.path()).is_err());
    }
    #[test]
    fn engine_conversion_is_role_independent_and_maps_reviewed_limits() {
        let fixture = Fixture::new();
        let mut value = document();
        value["role"] = json!("client");
        value["execution_clients"] = json!([]);
        let config = fixture.load(&value).unwrap();
        assert!(
            config
                .engine_policy("/state".into(), "/worker".into(), [9; 32], &[])
                .unwrap()
                .is_none()
        );
        let policy = config
            .engine_policy("/state".into(), "/worker".into(), [9; 32], &[[4; 32]])
            .unwrap()
            .unwrap();
        assert_eq!(
            policy.allowed_principals,
            vec![marmish_engine::PrincipalId([4; 32])]
        );
        assert!(!policy.allow_shell_command);
        assert_eq!(policy.limits.token_lifetime, Duration::from_secs(120));
        assert_eq!(policy.limits.cancel_grace, Duration::from_secs(5));
        assert_eq!(policy.limits.drain_grace, Duration::from_secs(2));
        assert_eq!(
            policy.command_policy.unwrap().defaults.allow_exec,
            Some(vec!["/**".into()])
        );
        assert!(
            config
                .engine_policy(
                    "/state".into(),
                    "/worker".into(),
                    [9; 32],
                    &vec![[4; 32]; 4097]
                )
                .is_err()
        );
        value["limits"] = json!({"spool_bytes":1});
        let config = fixture.load(&value).unwrap();
        assert!(
            config
                .engine_policy("/state".into(), "/worker".into(), [9; 32], &[[4; 32]])
                .is_err()
        );
    }
    #[test]
    fn policy_conversion_preserves_empty_additive_and_historical_layers() {
        let fixture = Fixture::new();
        let mut value = document();
        value["execution_clients"] = json!(["00".repeat(32)]);
        value["command_policy"] = json!({"version":1,"keys":[{
            "principal":"00".repeat(32),"restrictions":{"deny_exec":["/blocked/**"]}
        }]});
        let config = fixture.load(&value).unwrap();
        assert!(
            config
                .check_exec_restrictions([0; 32], &[b"/not-present/allowed".to_vec()])
                .is_ok()
        );
        assert!(
            config
                .check_exec_restrictions([0; 32], &[b"/blocked/file".to_vec()])
                .is_err()
        );
        value["command_policy"]["keys"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "principal":"11".repeat(32),"restrictions":{"deny_exec":["/historical/**"]}
            }));
        let config = fixture.load(&value).unwrap();
        assert!(
            config
                .engine_policy("/state".into(), "/worker".into(), [9; 32], &[[0x11; 32]])
                .is_ok()
        );
        assert!(
            config
                .check_exec_restrictions([0; 32], &[b"/not-present/allowed".to_vec()])
                .is_ok()
        );
        value["command_policy"]["defaults"]["allow_exec"] = json!([]);
        let config = fixture.load(&value).unwrap();
        assert!(
            config
                .check_exec_restrictions([0; 32], &[b"/not-present/allowed".to_vec()])
                .is_err()
        );
        value["command_policy"]["defaults"]["allow_exec"] = json!(["relative"]);
        let config = fixture.load(&value).unwrap();
        assert!(
            config
                .engine_policy("/state".into(), "/worker".into(), [9; 32], &[[0x11; 32]])
                .is_err()
        );
    }
}
