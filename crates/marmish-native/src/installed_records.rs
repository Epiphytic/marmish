//! Closed public records for the build-only installed acceptance driver.
//! Records carry provenance and measurements; they never grant runtime authority.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub(super) const ROLES: [&str; 3] = ["linux-amd64", "linux-arm64", "macos-arm64"];
pub(super) const TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
];
pub(super) const RECORD_LIMIT: u64 = 16 * 1024 * 1024;
fn invalid() -> Error {
    Error::Invalid("installed_record")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Context {
    pub schema: u8,
    pub repository: String,
    pub run_id: u64,
    pub run_attempt: u64,
    pub candidate_run_id: u64,
    pub candidate_run_attempt: u64,
    pub nonce: String,
    pub bootstrap_sha256: String,
    #[serde(deserialize_with = "role_map")]
    pub tooling_manifests: BTreeMap<String, String>,
    pub tooling: [ToolingManifest; 3],
    pub source_commit: String,
    pub harness_sha256: String,
    pub cohort: String,
    pub roles: [String; 3],
    pub relays: Vec<String>,
    #[serde(deserialize_with = "role_map")]
    pub manifests: BTreeMap<String, String>,
    pub version: String,
    pub created_at: u64,
    pub expires_at: u64,
}
impl Context {
    pub(super) fn require_cohort(&self, expected: &str) -> Result<()> {
        self.cohort_lifetime()?;
        if self.cohort != expected {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn cohort_lifetime(&self) -> Result<u64> {
        match self.cohort.as_str() {
            "matrix" => Ok(2220),
            "version-lifecycle" => Ok(1800),
            "autoexit" | "uptime" | "benchmark" | "recovery" | "policy" => Ok(3300),
            name if super::streaming_records::is_streaming(name) => Ok(3300),
            _ => Err(invalid()),
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        if self.schema != 1
            || self.run_id == 0
            || self.run_attempt == 0
            || self.candidate_run_id == 0
            || self.candidate_run_attempt == 0
            || !hexadecimal(&self.nonce, 64)
            || !hexadecimal(&self.bootstrap_sha256, 64)
            || self.tooling_manifests.len() != 3
            || self.tooling_manifests.keys().map(String::as_str).ne(ROLES)
            || self.tooling_manifests.values().any(|v| !hexadecimal(v, 64))
            || self.repository != "Epiphytic/marmish"
            || !hexadecimal(&self.source_commit, 40)
            || !hexadecimal(&self.harness_sha256, 64)
            || self.roles.iter().map(String::as_str).ne(ROLES)
            || self.expires_at <= self.created_at
            || self.expires_at - self.created_at > self.cohort_lifetime()?
            || !crate::native_manifest::version(&self.version)
            || self.relays.is_empty()
            || self.relays.len() > 16
            || self.manifests.len() != 3
            || self.manifests.keys().map(String::as_str).ne(ROLES)
            || self.manifests.values().any(|v| !hexadecimal(v, 64))
        {
            return Err(invalid());
        }
        for (i, t) in self.tooling.iter().enumerate() {
            t.validate(ROLES[i], &self.source_commit, &self.harness_sha256)?;
        }
        let policy = crate::transport::RelayPolicy::parse(
            &self.relays,
            crate::transport::RouteMode::Off,
            crate::transport::TransportLimits::default(),
        )?;
        if policy
            .urls()
            .iter()
            .map(ToString::to_string)
            .ne(self.relays.iter().cloned())
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn digest(&self) -> Result<String> {
        Ok(digest(&canonical(self)?))
    }
    pub(super) fn prefix(&self) -> String {
        format!(
            "v02-installed-{}-{}-{}-{}",
            self.run_id,
            self.run_attempt,
            &self.source_commit[..12],
            self.cohort
        )
    }
}
pub(super) fn own_identity(workspace: &Path, ctx: &Context) -> Result<IdentityReady> {
    let target = crate::native_manifest::compiled_release_metadata().target;
    let index = TARGETS
        .iter()
        .position(|t| *t == target)
        .ok_or_else(invalid)?;
    let role = ROLES[index];
    let bytes = read_bytes(
        &workspace
            .join("output")
            .join(format!("{role}-identity-ready.json")),
        65536,
    )?;
    let identity: IdentityReady = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    identity.validate(ctx, role)?;
    if bytes != canonical(&identity)? {
        return Err(invalid());
    }
    Ok(identity)
}

/// Authenticate both build-only executables before any candidate setup side effect.
pub(super) fn verify_tools(ctx: &Context, index: usize) -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let tool = &ctx.tooling[index];
    for (path, pin) in [
        (&exe, &tool.files.driver),
        (
            &exe.parent()
                .ok_or_else(invalid)?
                .join("marmish-installed-fixture"),
            &tool.files.fixture,
        ),
    ] {
        let b = read_bytes(path, 268435456)?;
        if b.len() as u64 != pin.bytes || digest(&b) != pin.sha256 {
            return Err(invalid());
        }
    }
    Ok(exe)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FilePin {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Installation {
    pub version: String,
    pub target: String,
    pub source_commit: String,
    pub manifest_sha256: String,
    pub bootstrap_sha256: String,
    #[serde(deserialize_with = "file_map")]
    pub files: BTreeMap<String, FilePin>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdentityReady {
    pub schema: u8,
    pub kind: String,
    pub context_sha256: String,
    pub role: String,
    pub nonce: String,
    pub client_account: String,
    pub server_account: String,
    pub installation: Installation,
    pub identity_ready_at: u64,
}
impl IdentityReady {
    pub(super) fn validate(&self, ctx: &Context, role: &str) -> Result<()> {
        let index = ROLES.iter().position(|r| *r == role).ok_or_else(invalid)?;
        let pin = &self.installation;
        if self.schema != 1
            || self.kind != "identity-ready"
            || self.role != role
            || self.context_sha256 != ctx.digest()?
            || !hexadecimal(&self.nonce, 64)
            || !hexadecimal(&self.client_account, 64)
            || !hexadecimal(&self.server_account, 64)
            || self.client_account == self.server_account
            || pin.version != ctx.version
            || pin.target != TARGETS[index]
            || pin.source_commit != ctx.source_commit
            || ctx.manifests.get(role) != Some(&pin.manifest_sha256)
            || pin.bootstrap_sha256 != ctx.bootstrap_sha256
            || pin.files.len() != 4
            || pin.files.keys().map(String::as_str).ne([
                "THIRD-PARTY-LICENSES",
                "marmish",
                "marmish-worker",
                "marmishd",
            ])
            || pin
                .files
                .values()
                .any(|f| !hexadecimal(&f.sha256, 64) || f.bytes == 0 || f.bytes > 268435456)
            || self.identity_ready_at < ctx.created_at
            || self.identity_ready_at > ctx.created_at.saturating_add(900)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
pub(super) fn identities(dir: &Path, ctx: &Context) -> Result<BTreeMap<String, IdentityReady>> {
    identities_with_kinds(dir, ctx, &["identity-ready"])
}
pub(super) fn matrix_identities(
    dir: &Path,
    ctx: &Context,
) -> Result<BTreeMap<String, IdentityReady>> {
    identities_with_kinds(
        dir,
        ctx,
        &[
            "identity-ready",
            "server-ready",
            "client-result",
            "server-result",
        ],
    )
}
fn identities_with_kinds(
    dir: &Path,
    ctx: &Context,
    kinds: &[&str],
) -> Result<BTreeMap<String, IdentityReady>> {
    let mut result = BTreeMap::new();
    for role in ROLES {
        let value: IdentityReady = read(&dir.join(format!("{role}-identity-ready.json")), 65536)?;
        value.validate(ctx, role)?;
        result.insert(role.to_owned(), value);
    }
    let expected: std::collections::BTreeSet<_> = ROLES
        .iter()
        .flat_map(|r| kinds.iter().map(move |kind| format!("{r}-{kind}.json")))
        .collect();
    let actual: std::collections::BTreeSet<_> = fs::read_dir(dir)?
        .map(|e| e.map(|v| v.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|n| n.into_string().map_err(|_| invalid()))
        .collect::<Result<_>>()?;
    if actual != expected {
        return Err(invalid());
    }
    let mut accounts = std::collections::BTreeSet::new();
    if result
        .values()
        .any(|r| !accounts.insert(&r.client_account) || !accounts.insert(&r.server_account))
    {
        return Err(invalid());
    }
    Ok(result)
}

pub(super) fn hexadecimal(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(super) fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    // Value's default map representation is sorted; no preserve_order feature is enabled.
    let value = serde_json::to_value(value).map_err(|_| invalid())?;
    let mut bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
    bytes.push(b'\n');
    Ok(bytes)
}
pub(super) fn read<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Result<T> {
    serde_json::from_slice(&read_bytes(path, limit)?).map_err(|_| invalid())
}
pub(super) fn read_bytes(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & 0o022 != 0
        || m.len() > limit
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}
pub(super) fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_raw(path, &canonical(value)?)
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PublicationFaultStage {
    BeforeLink,
    AfterLinkErr,
}
#[cfg(test)]
thread_local! {
    static PUBLICATION_FAULT: std::cell::Cell<Option<(&'static str, PublicationFaultStage)>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(super) struct PublicationFault;
#[cfg(test)]
impl PublicationFault {
    pub(super) fn install(name: &'static str, stage: PublicationFaultStage) -> Self {
        PUBLICATION_FAULT.with(|f| {
            assert!(f.get().is_none());
            f.set(Some((name, stage)));
        });
        Self
    }
}
#[cfg(test)]
impl Drop for PublicationFault {
    fn drop(&mut self) {
        PUBLICATION_FAULT.with(|f| f.set(None));
    }
}
#[cfg(test)]
fn publication_fault(path: &Path, stage: PublicationFaultStage) -> Result<()> {
    PUBLICATION_FAULT.with(|f| {
        if f.get().is_some_and(|(name, expected)| {
            path.file_name().is_some_and(|n| n == name) && expected == stage
        }) {
            f.set(None);
            Err(Error::Io(std::io::Error::other(
                "injected publication failure",
            )))
        } else {
            Ok(())
        }
    })
}
/// Immutable publication can return an error after the final hard link is visible.
/// Callers must not infer absence from Err (directory sync and cleanup follow the link).
pub(super) fn write_raw(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > RECORD_LIMIT {
        return Err(invalid());
    }
    let parent = path.parent().ok_or_else(invalid)?;
    crate::identity::check_dir(parent)?;
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).map_err(|_| Error::Identity("random"))?;
    let temporary = parent.join(format!(".record-{}", hex::encode(nonce)));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        // Hard-link publication is atomic and refuses replacement of an immutable record.
        #[cfg(test)]
        publication_fault(path, PublicationFaultStage::BeforeLink)?;
        fs::hard_link(&temporary, path)?;
        fs::remove_file(&temporary)?;
        #[cfg(test)]
        publication_fault(path, PublicationFaultStage::AfterLinkErr)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn role_map<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct Roles;
    impl<'de> serde::de::Visitor<'de> for Roles {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("exact three distinct platform roles")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut a: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((k, v)) = a.next_entry::<String, String>()? {
                if !ROLES.contains(&k.as_str()) || result.insert(k, v).is_some() {
                    return Err(serde::de::Error::custom("installed_record"));
                }
            }
            if result.len() != 3 {
                return Err(serde::de::Error::custom("installed_record"));
            }
            Ok(result)
        }
    }
    d.deserialize_map(Roles)
}

fn file_map<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<String, FilePin>, D::Error> {
    struct Pins;
    impl<'de> serde::de::Visitor<'de> for Pins {
        type Value = BTreeMap<String, FilePin>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("four distinct installed asset pins")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut a: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((k, v)) = a.next_entry::<String, FilePin>()? {
                if !crate::native_manifest::NAMES.contains(&k.as_str())
                    || result.insert(k, v).is_some()
                {
                    return Err(serde::de::Error::custom("installed_record"));
                }
            }
            if result.len() != 4 {
                return Err(serde::de::Error::custom("installed_record"));
            }
            Ok(result)
        }
    }
    d.deserialize_map(Pins)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    pub(crate) fn ctx() -> Context {
        Context {
            schema: 1,
            repository: "Epiphytic/marmish".into(),
            run_id: 1,
            run_attempt: 1,
            candidate_run_id: 2,
            candidate_run_attempt: 1,
            nonce: "a".repeat(64),
            bootstrap_sha256: "b".repeat(64),
            tooling_manifests: ROLES
                .into_iter()
                .map(|r| (r.into(), "c".repeat(64)))
                .collect(),
            tooling: std::array::from_fn(|i| ToolingManifest {
                schema: 1,
                target: TARGETS[i].into(),
                source_commit: "1".repeat(40),
                harness_sha256: "2".repeat(64),
                files: ToolFiles {
                    driver: FilePin {
                        sha256: "d".repeat(64),
                        bytes: 1,
                    },
                    fixture: FilePin {
                        sha256: "e".repeat(64),
                        bytes: 1,
                    },
                },
            }),
            source_commit: "1".repeat(40),
            harness_sha256: "2".repeat(64),
            cohort: "matrix".into(),
            roles: ROLES.map(str::to_owned),
            relays: vec!["wss://relay.example.com/".into()],
            manifests: ROLES
                .into_iter()
                .map(|r| (r.to_owned(), "3".repeat(64)))
                .collect(),
            version: "0.2.0-rc.1".into(),
            created_at: 100,
            expires_at: 2320,
        }
    }
    #[test]
    fn tooling_producer_record_has_exact_pinned_two_file_schema() {
        let source = "1".repeat(40);
        let harness = "2".repeat(64);
        let producer = serde_json::json!({"schema":1,"target":TARGETS[2],"source_commit":source,"harness_sha256":harness,"files":{"marmish-installed-check":{"sha256":"a".repeat(64),"bytes":123},"marmish-installed-fixture":{"sha256":"b".repeat(64),"bytes":456}}});
        let parsed: ToolingManifest = serde_json::from_value(producer.clone()).unwrap();
        parsed.validate(ROLES[2], &source, &harness).unwrap();
        assert!(parsed.validate(ROLES[0], &source, &harness).is_err());
        assert!(
            parsed
                .validate(ROLES[2], &"3".repeat(40), &harness)
                .is_err()
        );
        let mut extra = producer.clone();
        extra["files"]["marmish"] = serde_json::json!({"sha256":"c".repeat(64),"bytes":1});
        assert!(serde_json::from_value::<ToolingManifest>(extra).is_err());
        let mut missing = producer;
        missing["files"]
            .as_object_mut()
            .unwrap()
            .remove("marmish-installed-fixture");
        assert!(serde_json::from_value::<ToolingManifest>(missing).is_err());
    }
    #[test]
    fn context_budget_roles_and_canonical_digest() {
        let mut c = ctx();
        c.validate().unwrap();
        let b = canonical(&c).unwrap();
        assert!(b.ends_with(b"\n"));
        assert_eq!(digest(&b), c.digest().unwrap());
        assert!(
            c.prefix()
                .starts_with("v02-installed-1-1-111111111111-matrix")
        );
        c.expires_at += 1;
        assert!(c.validate().is_err());
    }
    #[test]
    fn local_contexts_keep_fixed_original_budgets_and_entrypoint_equality() {
        for name in ["autoexit", "uptime", "benchmark", "recovery", "policy"] {
            let mut c = ctx();
            c.cohort = name.into();
            c.expires_at = c.created_at + 3300;
            c.validate().unwrap();
            c.require_cohort(name).unwrap();
            assert!(c.require_cohort("matrix").is_err());
            c.expires_at += 1;
            assert!(c.validate().is_err());
        }
        let mut c = ctx();
        c.cohort = "unknown".into();
        assert!(c.validate().is_err());
        assert!(c.require_cohort("unknown").is_err());
    }
    #[test]
    fn duplicate_or_unknown_context_fields_fail() {
        let c = canonical(&ctx()).unwrap();
        let s = String::from_utf8(c).unwrap();
        let duplicate = s.replacen("\"schema\":1", "\"schema\":1,\"schema\":1", 1);
        assert!(serde_json::from_str::<Context>(&duplicate).is_err());
        let role = s.replacen("\"linux-amd64\":", "\"extra\":", 1);
        assert!(serde_json::from_str::<Context>(&role).is_err());
        let duplicate = s.replacen(
            "\"linux-amd64\":",
            &format!("\"linux-amd64\":\"{}\",\"linux-amd64\":", "3".repeat(64)),
            1,
        );
        assert!(serde_json::from_str::<Context>(&duplicate).is_err());
    }
    #[test]
    fn immutable_record_publication_and_bounded_nofollow_read() {
        let d = tempfile::tempdir().unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let p = d.path().join("record.json");
        write(&p, &ctx()).unwrap();
        assert!(write(&p, &ctx()).is_err());
        let _: Context = read(&p, 65536).unwrap();
        assert!(read::<Context>(&p, 5).is_err());
        let l = d.path().join("link");
        std::os::unix::fs::symlink(&p, &l).unwrap();
        assert!(read::<Context>(&l, 65536).is_err());
        fs::hard_link(&p, d.path().join("hard")).unwrap();
        assert!(read::<Context>(&p, 65536).is_err());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolingManifest {
    pub schema: u8,
    pub target: String,
    pub source_commit: String,
    pub harness_sha256: String,
    pub files: ToolFiles,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolFiles {
    #[serde(rename = "marmish-installed-check")]
    pub driver: FilePin,
    #[serde(rename = "marmish-installed-fixture")]
    pub fixture: FilePin,
}
impl ToolingManifest {
    pub(super) fn validate(&self, role: &str, source: &str, harness: &str) -> Result<()> {
        let i = ROLES.iter().position(|r| *r == role).ok_or_else(invalid)?;
        if self.schema != 1
            || self.target != TARGETS[i]
            || self.source_commit != source
            || self.harness_sha256 != harness
            || [&self.files.driver, &self.files.fixture]
                .iter()
                .any(|f| !hexadecimal(&f.sha256, 64) || f.bytes == 0 || f.bytes > 268435456)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
