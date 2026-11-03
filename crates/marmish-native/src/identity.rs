//! Fresh v2 profiles and bounded, immutable native-key custody.
use crate::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use fs2::FileExt;
use nostr::key::{Keys, PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const MANIFEST_LIMIT: u64 = 8192;
const HELPER_LIMIT: usize = 8192;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BackendChoice {
    Auto,
    Keychain,
    File,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Backend {
    Keychain,
    Files,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Initializing,
    Ready,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u8,
    profile_id: String,
    name: String,
    transaction: String,
    phase: Phase,
    backend: Backend,
    nostr_ref: String,
    sqlcipher_ref: String,
    device_slot: String,
    public_key: Option<String>,
}
/// No secret fields or implicit serialization on an opened identity.
pub struct IdentityMaterial {
    signer: Arc<Keys>,
    root: Zeroizing<[u8; 32]>,
    slot: [u8; 16],
}
impl IdentityMaterial {
    pub fn signer(&self) -> Arc<Keys> {
        self.signer.clone()
    }
    pub fn public_key(&self) -> PublicKey {
        self.signer.public_key()
    }
    pub fn sqlcipher_root(&self) -> &[u8; 32] {
        &self.root
    }
    pub fn device_slot(&self) -> &[u8; 16] {
        &self.slot
    }
}
/// One lifetime lock, held before any key provider/database/network operation.
pub struct ProfileLease {
    _lock: File,
    root: PathBuf,
    manifest: Option<Manifest>,
    profile_id: String,
    name: String,
    mdk: PathBuf,
    journal: PathBuf,
    arti_state: PathBuf,
    arti_cache: PathBuf,
    runtime: PathBuf,
}
impl Drop for ProfileLease {
    fn drop(&mut self) {
        // Closing only this descriptor leaves flock held if a concurrent
        // process spawn inherited the same open-file description before exec.
        // The lease lifetime, rather than the last inherited close, owns it.
        let _ = FileExt::unlock(&self._lock);
    }
}
impl ProfileLease {
    pub fn acquire(base: &Path, name: &str) -> Result<Self> {
        validate_name(name)?;
        private_dir(base)?;
        let base = base.canonicalize()?;
        let root = base.join(name);
        private_dir(&root)?;
        let lock = open_private(&root.join("owner.lock"), true)?;
        lock.try_lock_exclusive()
            .map_err(|_| Error::Identity("profile_in_use"))?;
        let manifest = read_manifest(&root.join("profile.json"))?;
        if let Some(m) = &manifest {
            validate_manifest(m, name)?;
        }
        let profile_id = manifest
            .as_ref()
            .map(|m| m.profile_id.clone())
            .unwrap_or(random_hex(16)?);
        let runtime =
            std::env::temp_dir().join(format!("mr2-{}-{}", unsafe { libc::geteuid() }, profile_id));
        if runtime.as_os_str().as_encoded_bytes().len() + 10 >= 104 {
            return Err(Error::Identity("runtime_path_too_long"));
        }
        Ok(Self {
            _lock: lock,
            mdk: root.join("mdk.db"),
            journal: root.join("journal.db"),
            arti_state: root.join("arti-state"),
            arti_cache: root.join("arti-cache"),
            root,
            manifest,
            profile_id,
            name: name.to_owned(),
            runtime,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn mdk_database(&self) -> &Path {
        &self.mdk
    }
    pub fn journal_database(&self) -> &Path {
        &self.journal
    }
    pub fn arti_state(&self) -> &Path {
        &self.arti_state
    }
    pub fn arti_cache(&self) -> &Path {
        &self.arti_cache
    }
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime
    }
    pub fn mdk_path(&self) -> &Path {
        &self.mdk
    }
    pub fn journal_path(&self) -> &Path {
        &self.journal
    }
    pub fn socket_path(&self) -> PathBuf {
        self.runtime.join("s")
    }
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }
    pub fn public_key(&self) -> Option<PublicKey> {
        self.manifest
            .as_ref()?
            .public_key
            .as_ref()
            .and_then(|s| PublicKey::from_hex(s).ok())
    }
    /// Must run outside Tokio executor workers. Only an unavailable provider may select files.
    pub fn prepare(
        &mut self,
        choice: BackendChoice,
        helper: &Path,
        deadline: Instant,
    ) -> Result<IdentityMaterial> {
        self.prepare_with(choice, &mut NativeProvider { helper, deadline })
    }
    fn prepare_with(
        &mut self,
        choice: BackendChoice,
        provider: &mut impl Provider,
    ) -> Result<IdentityMaterial> {
        if self.manifest.is_none() {
            let backend = match choice {
                BackendChoice::File => Backend::Files,
                BackendChoice::Auto | BackendChoice::Keychain => match provider.probe() {
                    Ok(()) => Backend::Keychain,
                    Err(Error::Identity("unavailable")) if choice == BackendChoice::Auto => {
                        Backend::Files
                    }
                    Err(e) => return Err(e),
                },
            };
            let m = Manifest {
                format: 2,
                profile_id: self.profile_id.clone(),
                name: self.name.clone(),
                transaction: random_hex(16)?,
                phase: Phase::Initializing,
                backend,
                nostr_ref: random_hex(32)?,
                sqlcipher_ref: random_hex(32)?,
                device_slot: random_hex(16)?,
                public_key: None,
            };
            atomic_json(&self.root.join("profile.json"), &m)?;
            self.manifest = Some(m);
        }
        let m = self
            .manifest
            .as_ref()
            .ok_or(Error::Identity("missing_manifest"))?;
        if (choice == BackendChoice::File && m.backend != Backend::Files)
            || (choice == BackendChoice::Keychain && m.backend != Backend::Keychain)
        {
            return Err(Error::Identity("backend_mismatch"));
        }
        let initializing = m.phase == Phase::Initializing;
        let mut files = FileProvider {
            root: self.root.join("keys"),
        };
        let selected: &mut dyn Provider = if m.backend == Backend::Files {
            private_dir(&files.root)?;
            &mut files
        } else {
            provider
        };
        let secret = load_or_create(
            selected,
            "nostr",
            &m.nostr_ref,
            initializing && m.public_key.is_none(),
        )?;
        let key =
            SecretKey::from_slice(&secret[..]).map_err(|_| Error::Identity("invalid_nostr_key"))?;
        let signer = Arc::new(Keys::new(key));
        let public = signer.public_key().to_hex();
        if let Some(expected) = &m.public_key {
            if *expected != public {
                return Err(Error::Identity("identity_mismatch"));
            }
        } else {
            let m = self
                .manifest
                .as_mut()
                .ok_or(Error::Identity("missing_manifest"))?;
            m.public_key = Some(public);
            atomic_json(&self.root.join("profile.json"), m)?;
        }
        let m = self
            .manifest
            .as_ref()
            .ok_or(Error::Identity("missing_manifest"))?;
        let root = load_or_create(selected, "sqlcipher", &m.sqlcipher_ref, initializing)?;
        private_dir(&self.arti_state)?;
        private_dir(&self.arti_cache)?;
        self.bind_runtime()?;
        Ok(IdentityMaterial {
            signer,
            root,
            slot: decode_hex(&m.device_slot)?,
        })
    }
    fn bind_runtime(&self) -> Result<()> {
        private_dir(&self.runtime)?;
        #[derive(Serialize, Deserialize, PartialEq)]
        #[serde(deny_unknown_fields)]
        struct Binding {
            format: u8,
            profile_id: String,
            root: PathBuf,
        }
        let expected = Binding {
            format: 2,
            profile_id: self.profile_id.clone(),
            root: self.root.clone(),
        };
        let path = self.runtime.join("binding.json");
        match open_private(&path, false) {
            Ok(f) => {
                let mut bytes = Vec::new();
                f.take(MANIFEST_LIMIT + 1).read_to_end(&mut bytes)?;
                let actual: Binding = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::Identity("runtime_collision"))?;
                if actual != expected {
                    return Err(Error::Identity("runtime_collision"));
                }
            }
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                write_new(
                    &path,
                    &serde_json::to_vec(&expected)
                        .map_err(|_| Error::Identity("invalid_manifest"))?,
                )?;
                File::open(&self.runtime)?.sync_all()?;
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }
    /// Explicit fixture cleanup after the daemon and its transport have stopped.
    /// Never recursively removes runtime contents; the profile lease remains held.
    pub fn cleanup_runtime(&self) -> Result<()> {
        match fs::symlink_metadata(&self.runtime) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
            Ok(_) => (),
        }
        if socket_path_for(
            self.root
                .parent()
                .ok_or(Error::Identity("unsafe_profile_path"))?,
            &self.name,
        )? != self.socket_path()
        {
            return Err(Error::Identity("runtime_collision"));
        }
        for entry in fs::read_dir(&self.runtime)? {
            let entry = entry?;
            if entry.file_name() != "binding.json" && entry.file_name() != "s" {
                return Err(Error::Identity("unexpected_runtime_file"));
            }
        }
        let socket = self.socket_path();
        match fs::symlink_metadata(&socket) {
            Ok(m) => {
                if !m.file_type().is_socket()
                    || m.uid() != unsafe { libc::geteuid() }
                    || m.nlink() != 1
                    || m.mode() & 0o777 != 0o600
                {
                    return Err(Error::Identity("unsafe_runtime_socket"));
                }
                fs::remove_file(&socket)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        fs::remove_file(self.runtime.join("binding.json"))?;
        fs::remove_dir(&self.runtime)?;
        Ok(())
    }
    pub fn mark_ready(&mut self, opened: &crate::mdk_adapter::OpenedAccountBinding) -> Result<()> {
        if opened.profile_id() != self.profile_id || Some(opened.public_key()) != self.public_key()
        {
            return Err(Error::Identity("storage_binding_mismatch"));
        }
        let m = self
            .manifest
            .as_mut()
            .ok_or(Error::Identity("missing_manifest"))?;
        m.phase = Phase::Ready;
        atomic_json(&self.root.join("profile.json"), m)
    }
}
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Identity("invalid_profile_name"));
    }
    Ok(())
}
/// Resolve an existing daemon socket without opening its owner lock or databases.
pub fn socket_path_for(base: &Path, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    check_dir(base)?;
    let root = base.canonicalize()?.join(name);
    check_dir(&root)?;
    let m =
        read_manifest(&root.join("profile.json"))?.ok_or(Error::Identity("missing_manifest"))?;
    validate_manifest(&m, name)?;
    let runtime = std::env::temp_dir().join(format!(
        "mr2-{}-{}",
        unsafe { libc::geteuid() },
        m.profile_id
    ));
    check_dir(&runtime)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Binding {
        format: u8,
        profile_id: String,
        root: PathBuf,
    }
    let mut bytes = Vec::new();
    open_private(&runtime.join("binding.json"), false)?
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    let binding: Binding =
        serde_json::from_slice(&bytes).map_err(|_| Error::Identity("runtime_collision"))?;
    if binding.format != 2 || binding.profile_id != m.profile_id || binding.root != root {
        return Err(Error::Identity("runtime_collision"));
    }
    let socket = runtime.join("s");
    if socket.as_os_str().as_encoded_bytes().len() >= 104 {
        return Err(Error::Identity("runtime_path_too_long"));
    }
    Ok(socket)
}
fn validate_manifest(m: &Manifest, name: &str) -> Result<()> {
    if m.format != 2
        || m.name != name
        || !valid_hex(&m.profile_id, 16)
        || !valid_hex(&m.transaction, 16)
        || !valid_hex(&m.nostr_ref, 32)
        || !valid_hex(&m.sqlcipher_ref, 32)
        || m.nostr_ref == m.sqlcipher_ref
        || !valid_hex(&m.device_slot, 16)
        || m.public_key
            .as_ref()
            .is_some_and(|s| !valid_hex(s, 32) || PublicKey::from_hex(s).is_err())
        || (m.phase == Phase::Ready && m.public_key.is_none())
    {
        return Err(Error::Identity("invalid_manifest"));
    }
    Ok(())
}
fn read_manifest(path: &Path) -> Result<Option<Manifest>> {
    let mut file = match open_private(path, false) {
        Ok(f) => f,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MANIFEST_LIMIT as usize {
        return Err(Error::Identity("invalid_manifest"));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Error::Identity("invalid_manifest"))
}
fn valid_hex(s: &str, n: usize) -> bool {
    s.len() == 2 * n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn decode_hex<const N: usize>(s: &str) -> Result<[u8; N]> {
    if !valid_hex(s, N) {
        return Err(Error::Identity("invalid_hex"));
    }
    let mut out = [0; N];
    for (i, p) in s.as_bytes().chunks_exact(2).enumerate() {
        let d = |v: u8| if v <= b'9' { v - b'0' } else { v - b'a' + 10 };
        out[i] = d(p[0]) * 16 + d(p[1]);
    }
    Ok(out)
}
fn random_hex(n: usize) -> Result<String> {
    let mut bytes = Zeroizing::new(vec![0; n]);
    getrandom::fill(&mut bytes).map_err(|_| Error::Identity("random_failed"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn private_dir(path: &Path) -> Result<()> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    check_dir(path)
}
pub(crate) fn check_dir(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
        return Err(Error::Identity("unsafe_profile_path"));
    }
    Ok(())
}
pub(crate) fn open_private(path: &Path, create: bool) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let m = f.metadata()?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
    {
        return Err(Error::Identity("unsafe_profile_file"));
    }
    Ok(f)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Identity("invalid_manifest"))?;
    let parent = path
        .parent()
        .ok_or(Error::Identity("unsafe_profile_path"))?;
    let temp = parent.join(format!(".init-{}", random_hex(16)?));
    write_new(&temp, &bytes)?;
    let result = fs::rename(&temp, path).and_then(|()| File::open(parent)?.sync_all());
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(Error::from)
}
trait Provider {
    fn probe(&mut self) -> Result<()>;
    fn load(&mut self, purpose: &str, id: &str) -> Result<Zeroizing<[u8; 32]>>;
    fn create(&mut self, purpose: &str, id: &str, secret: &[u8; 32]) -> Result<()>;
}
fn load_or_create(
    provider: &mut dyn Provider,
    purpose: &str,
    id: &str,
    initializing: bool,
) -> Result<Zeroizing<[u8; 32]>> {
    match provider.load(purpose, id) {
        Ok(value) => Ok(value),
        Err(Error::Identity("missing")) if initializing => {
            let mut bytes = Zeroizing::new([0; 32]);
            loop {
                getrandom::fill(&mut *bytes).map_err(|_| Error::Identity("random_failed"))?;
                if purpose != "nostr" || SecretKey::from_slice(&bytes[..]).is_ok() {
                    break;
                }
            }
            match provider.create(purpose, id, &bytes) {
                Ok(()) => (),
                Err(Error::Identity("exists")) => (),
                Err(e) => return Err(e),
            }
            let loaded = provider.load(purpose, id)?;
            if *loaded != *bytes {
                return Err(Error::Identity("key_readback_mismatch"));
            }
            Ok(loaded)
        }
        Err(e) => Err(e),
    }
}
struct FileProvider {
    root: PathBuf,
}
impl Provider for FileProvider {
    fn probe(&mut self) -> Result<()> {
        Ok(())
    }
    fn load(&mut self, purpose: &str, id: &str) -> Result<Zeroizing<[u8; 32]>> {
        let mut f = match open_private(&self.root.join(format!("{purpose}-{id}")), false) {
            Ok(f) => f,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Identity("missing"));
            }
            Err(e) => return Err(e),
        };
        let mut bytes = Zeroizing::new(Vec::with_capacity(33));
        Read::by_ref(&mut f).take(33).read_to_end(&mut bytes)?;
        if bytes.len() != 32 {
            return Err(Error::Identity("invalid_secret"));
        }
        let mut out = Zeroizing::new([0; 32]);
        out.copy_from_slice(&bytes);
        Ok(out)
    }
    fn create(&mut self, purpose: &str, id: &str, secret: &[u8; 32]) -> Result<()> {
        match write_new(&self.root.join(format!("{purpose}-{id}")), secret) {
            Ok(()) => {
                File::open(&self.root)?.sync_all()?;
                Ok(())
            }
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(Error::Identity("exists"))
            }
            Err(e) => Err(e),
        }
    }
}
struct NativeProvider<'a> {
    helper: &'a Path,
    deadline: Instant,
}
impl NativeProvider<'_> {
    fn call(
        &self,
        action: &str,
        purpose: Option<&str>,
        id: Option<&str>,
        secret: Option<&[u8; 32]>,
    ) -> Result<Option<Zeroizing<[u8; 32]>>> {
        let mut request = serde_json::json!({"action":action,"version":1,"namespace":"marmish-v2"});
        if let (Some(purpose), Some(id)) = (purpose, id) {
            request["reference"] = serde_json::json!({"version":2,"purpose":purpose,"id":id});
        }
        if let Some(secret) = secret {
            request["label"] = serde_json::json!("Marmish v2");
            request["secret"] = serde_json::Value::String(STANDARD.encode(secret));
        }
        let mut bytes = Zeroizing::new(vec![0u8; HELPER_LIMIT]);
        let encoded = {
            let mut writer = std::io::Cursor::new(&mut bytes[..]);
            serde_json::to_writer(&mut writer, &request)
                .map(|()| writer.position() as usize)
                .map_err(|_| Error::Identity("invalid_helper_request"))
        };
        // The only secret-bearing Value is removed and explicitly wiped before drop.
        if let Some(serde_json::Value::String(mut value)) =
            request.as_object_mut().and_then(|o| o.remove("secret"))
        {
            use zeroize::Zeroize;
            value.zeroize();
        }
        bytes.truncate(encoded?);
        let response = helper_exchange(self.helper, &bytes, self.deadline)?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Response {
            version: u8,
            ok: bool,
            #[serde(default)]
            result: Option<Loaded>,
            #[serde(default)]
            error: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Loaded {
            #[serde(default)]
            secret: Option<SecretText>,
        }
        struct SecretText(Zeroizing<String>);
        impl<'de> Deserialize<'de> for SecretText {
            fn deserialize<D: serde::Deserializer<'de>>(
                d: D,
            ) -> std::result::Result<Self, D::Error> {
                String::deserialize(d).map(|s| Self(Zeroizing::new(s)))
            }
        }
        let mut result: Response = serde_json::from_slice(&response)
            .map_err(|_| Error::Identity("invalid_helper_response"))?;
        if result.version != 1
            || result.ok == result.error.is_some()
            || (!result.ok && result.result.is_some())
        {
            return Err(Error::Identity("invalid_helper_response"));
        }
        if !result.ok {
            return Err(Error::Identity(match result.error.as_deref() {
                Some("unavailable") => "unavailable",
                Some("uninitialized") => "uninitialized",
                Some("locked_or_denied") => "locked_or_denied",
                Some("missing") => "missing",
                Some("exists") => "exists",
                Some("ambiguous") => "ambiguous",
                Some("malformed") => "invalid_secret",
                _ => "keychain_failed",
            }));
        }
        let loaded = result
            .result
            .as_mut()
            .ok_or(Error::Identity("invalid_helper_response"))?;
        match loaded.secret.take() {
            None if action != "load" => Ok(None),
            Some(s) if action == "load" => {
                let s = s.0;
                let raw = Zeroizing::new(
                    STANDARD
                        .decode(s.as_bytes())
                        .map_err(|_| Error::Identity("invalid_secret"))?,
                );
                if raw.len() != 32 || Zeroizing::new(STANDARD.encode(&raw)).as_str() != s.as_str() {
                    return Err(Error::Identity("invalid_secret"));
                }
                let mut out = Zeroizing::new([0; 32]);
                out.copy_from_slice(&raw);
                Ok(Some(out))
            }
            _ => Err(Error::Identity("invalid_helper_response")),
        }
    }
}
impl Provider for NativeProvider<'_> {
    fn probe(&mut self) -> Result<()> {
        self.call("probe", None, None, None).map(|_| ())
    }
    fn load(&mut self, p: &str, id: &str) -> Result<Zeroizing<[u8; 32]>> {
        self.call("load", Some(p), Some(id), None)?
            .ok_or(Error::Identity("invalid_helper_response"))
    }
    fn create(&mut self, p: &str, id: &str, s: &[u8; 32]) -> Result<()> {
        self.call("create", Some(p), Some(id), Some(s)).map(|_| ())
    }
}
fn helper_exchange(path: &Path, request: &[u8], deadline: Instant) -> Result<Zeroizing<Vec<u8>>> {
    if !path.is_absolute() || request.len() > HELPER_LIMIT {
        return Err(Error::Identity("invalid_helper"));
    }
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    let mut command = Command::new(path);
    command
        .arg("_vault-helper")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for name in ["HOME", "DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command.spawn()?;
    let result = (|| {
        let mut input = Some(child.stdin.take().ok_or(Error::Identity("helper_pipe"))?);
        let mut output = child.stdout.take().ok_or(Error::Identity("helper_pipe"))?;
        for fd in [input.as_ref().unwrap().as_raw_fd(), output.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(Error::Identity("helper_pipe"));
            }
        }
        let mut offset = 0;
        let mut bytes = Zeroizing::new(Vec::with_capacity(HELPER_LIMIT + 1));
        let mut eof = false;
        let mut status = None;
        loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            if let Some(writer) = input.as_mut() {
                match writer.write(&request[offset..]) {
                    Ok(n) => {
                        offset += n;
                        if offset == request.len() {
                            input.take();
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e.into()),
                }
            }
            let mut chunk = Zeroizing::new([0u8; 1024]);
            // Read at most the remaining capacity, including one oversize byte.
            // The buffer must never reallocate after receiving secret data.
            let available = chunk.len().min(HELPER_LIMIT + 1 - bytes.len());
            match output.read(&mut chunk[..available]) {
                Ok(0) => eof = true,
                Ok(n) => {
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > HELPER_LIMIT {
                        return Err(Error::Identity("helper_oversize"));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e.into()),
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            if let (true, Some(status)) = (eof, status) {
                if !status.success() && bytes.is_empty() {
                    return Err(Error::Identity("keychain_failed"));
                }
                return Ok(bytes);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[derive(Default)]
    struct Fake {
        values: HashMap<(String, String), [u8; 32]>,
        creates: usize,
        fail_at: Option<usize>,
        probe_error: Option<&'static str>,
    }
    impl Provider for Fake {
        fn probe(&mut self) -> Result<()> {
            match self.probe_error {
                Some(e) => Err(Error::Identity(e)),
                None => Ok(()),
            }
        }
        fn load(&mut self, p: &str, id: &str) -> Result<Zeroizing<[u8; 32]>> {
            self.values
                .get(&(p.into(), id.into()))
                .copied()
                .map(Zeroizing::new)
                .ok_or(Error::Identity("missing"))
        }
        fn create(&mut self, p: &str, id: &str, secret: &[u8; 32]) -> Result<()> {
            self.creates += 1;
            if self.fail_at == Some(self.creates) {
                return Err(Error::Identity("locked_or_denied"));
            }
            if self.values.insert((p.into(), id.into()), *secret).is_some() {
                return Err(Error::Identity("exists"));
            }
            Ok(())
        }
    }
    struct Fixture {
        base: tempfile::TempDir,
        runtime: Option<PathBuf>,
    }
    impl Fixture {
        fn new() -> Self {
            let base = tempfile::tempdir().unwrap();
            fs::set_permissions(base.path(), fs::Permissions::from_mode(0o700)).unwrap();
            Self {
                base,
                runtime: None,
            }
        }
        fn lease(&self, name: &str) -> ProfileLease {
            ProfileLease::acquire(self.base.path(), name).unwrap()
        }
        fn remember(&mut self, p: &ProfileLease) {
            self.runtime = Some(p.runtime.clone());
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(p) = &self.runtime {
                let _ = fs::remove_dir_all(p);
            }
        }
    }
    #[test]
    fn dropping_owner_releases_lock_even_with_an_inherited_descriptor() {
        let f = Fixture::new();
        let lease = f.lease("alice");
        // A duplicated descriptor shares the open-file description, as does
        // a descriptor briefly inherited by a concurrent fork before exec.
        let inherited = lease._lock.try_clone().unwrap();
        drop(lease);
        let replacement = f.lease("alice");
        assert!(matches!(
            ProfileLease::acquire(f.base.path(), "alice"),
            Err(Error::Identity("profile_in_use"))
        ));
        drop(inherited);
        assert!(matches!(
            ProfileLease::acquire(f.base.path(), "alice"),
            Err(Error::Identity("profile_in_use"))
        ));
        drop(replacement);
        let _next = f.lease("alice");
    }
    #[test]
    fn profile_lock_precedes_provider_and_socket_reader_needs_no_lock() {
        let mut f = Fixture::new();
        let mut lease = f.lease("alice");
        let mut fake = Fake::default();
        let material = lease
            .prepare_with(BackendChoice::Keychain, &mut fake)
            .unwrap();
        f.remember(&lease);
        assert!(matches!(
            ProfileLease::acquire(f.base.path(), "alice"),
            Err(Error::Identity("profile_in_use"))
        ));
        assert_eq!(
            socket_path_for(f.base.path(), "alice").unwrap(),
            lease.socket_path()
        );
        assert!(lease.socket_path().as_os_str().as_encoded_bytes().len() < 104);
        assert_eq!(Some(material.public_key()), lease.public_key());
        assert_ne!(
            material.signer().secret_key().to_secret_bytes(),
            *material.sqlcipher_root()
        );
        let before = *material.sqlcipher_root();
        let public = material.public_key();
        drop(material);
        drop(lease);
        let mut lease = f.lease("alice");
        let material = lease.prepare_with(BackendChoice::Auto, &mut fake).unwrap();
        assert_eq!(material.public_key(), public);
        assert_eq!(*material.sqlcipher_root(), before);
        assert_eq!(fake.creates, 2);
    }
    #[test]
    fn interrupted_initialization_resumes_exact_references() {
        let mut f = Fixture::new();
        let mut lease = f.lease("alice");
        let mut fake = Fake {
            fail_at: Some(2),
            ..Fake::default()
        };
        assert!(matches!(
            lease.prepare_with(BackendChoice::Keychain, &mut fake),
            Err(Error::Identity("locked_or_denied"))
        ));
        let original = lease.manifest.as_ref().unwrap().nostr_ref.clone();
        let key = lease.public_key().unwrap();
        drop(lease);
        let mut lease = f.lease("alice");
        fake.fail_at = None;
        let loaded = lease
            .prepare_with(BackendChoice::Keychain, &mut fake)
            .unwrap();
        f.remember(&lease);
        assert_eq!(loaded.public_key(), key);
        assert_eq!(lease.manifest.as_ref().unwrap().nostr_ref, original);
        assert_eq!(fake.creates, 3);
    }
    #[test]
    fn ready_missing_key_never_regenerates_or_switches_backend() {
        let mut f = Fixture::new();
        let mut lease = f.lease("alice");
        let mut fake = Fake::default();
        let material = lease
            .prepare_with(BackendChoice::Keychain, &mut fake)
            .unwrap();
        f.remember(&lease);
        drop(material);
        lease.manifest.as_mut().unwrap().phase = Phase::Ready;
        atomic_json(
            &lease.root.join("profile.json"),
            lease.manifest.as_ref().unwrap(),
        )
        .unwrap();
        fake.values.clear();
        assert!(matches!(
            lease.prepare_with(BackendChoice::Auto, &mut fake),
            Err(Error::Identity("missing"))
        ));
        assert_eq!(fake.creates, 2);
        assert!(!lease.root.join("keys").exists());
    }
    #[test]
    fn only_unavailable_can_select_files() {
        for category in [
            "locked_or_denied",
            "ambiguous",
            "uninitialized",
            "keychain_failed",
        ] {
            let f = Fixture::new();
            let mut lease = f.lease("alice");
            let mut fake = Fake {
                probe_error: Some(category),
                ..Fake::default()
            };
            assert!(lease.prepare_with(BackendChoice::Auto, &mut fake).is_err());
            assert!(!lease.root.join("profile.json").exists());
        }
        let mut f = Fixture::new();
        let mut lease = f.lease("alice");
        let mut fake = Fake {
            probe_error: Some("unavailable"),
            ..Fake::default()
        };
        let material = lease.prepare_with(BackendChoice::Auto, &mut fake).unwrap();
        f.remember(&lease);
        assert!(lease.root.join("keys").is_dir());
        assert_eq!(fake.creates, 0);
        let public = material.public_key();
        drop(lease);
        let mut lease = f.lease("alice");
        assert_eq!(
            lease
                .prepare_with(BackendChoice::Auto, &mut Fake::default())
                .unwrap()
                .public_key(),
            public
        );
    }
    #[test]
    fn names_paths_permissions_and_manifest_are_strict() {
        let f = Fixture::new();
        for name in ["", "../bob", "a/b", "雪", "."] {
            assert!(ProfileLease::acquire(f.base.path(), name).is_err());
        }
        let target = f.base.path().join("target");
        private_dir(&target).unwrap();
        symlink(&target, f.base.path().join("alice")).unwrap();
        assert!(ProfileLease::acquire(f.base.path(), "alice").is_err());
        fs::remove_file(f.base.path().join("alice")).unwrap();
        let lease = f.lease("alice");
        drop(lease);
        let path = f.base.path().join("alice/profile.json");
        write_new(&path, b"{\"format\":2,\"format\":2}").unwrap();
        assert!(ProfileLease::acquire(f.base.path(), "alice").is_err());
        fs::remove_file(&path).unwrap();
        let lock = f.base.path().join("alice/owner.lock");
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(ProfileLease::acquire(f.base.path(), "alice").is_err());
    }
    #[test]
    fn file_secrets_reject_links_corruption_and_wrong_identity() {
        let mut f = Fixture::new();
        let mut lease = f.lease("alice");
        let mut fake = Fake::default();
        drop(lease.prepare_with(BackendChoice::File, &mut fake).unwrap());
        f.remember(&lease);
        let m = lease.manifest.as_ref().unwrap();
        let path = lease
            .root
            .join("keys")
            .join(format!("nostr-{}", m.nostr_ref));
        fs::write(&path, [0u8; 32]).unwrap();
        assert!(matches!(
            lease.prepare_with(BackendChoice::File, &mut fake),
            Err(Error::Identity("invalid_nostr_key"))
        ));
        fs::remove_file(&path).unwrap();
        symlink("/dev/null", &path).unwrap();
        assert!(lease.prepare_with(BackendChoice::File, &mut fake).is_err());
    }
    #[test]
    fn helper_absolute_path_and_deadline_are_enforced() {
        assert!(matches!(
            helper_exchange(
                Path::new("relative"),
                b"{}",
                Instant::now() + Duration::from_secs(1)
            ),
            Err(Error::Identity("invalid_helper"))
        ));
        assert!(matches!(
            helper_exchange(Path::new("/unused"), b"{}", Instant::now()),
            Err(Error::Timeout)
        ));
    }
    #[test]
    fn native_helper_timeout_and_oversize_are_bounded() {
        let f = Fixture::new();
        let helper = f.base.path().join("fake");
        write_new(&helper, b"#!/bin/sh\nexec sleep 10\n").unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let start = Instant::now();
        assert!(matches!(
            helper_exchange(&helper, b"{}", start + Duration::from_millis(60)),
            Err(Error::Timeout)
        ));
        assert!(start.elapsed() < Duration::from_secs(2));
        fs::write(&helper, b"#!/bin/sh\nexec head -c 9000 /dev/zero\n").unwrap();
        assert!(matches!(
            helper_exchange(&helper, b"{}", Instant::now() + Duration::from_secs(2)),
            Err(Error::Identity("helper_oversize"))
        ));
    }
    #[test]
    fn named_profiles_have_independent_keys_roots_and_slots() {
        let mut first = Fixture::new();
        let mut second = Fixture::new();
        let mut a = first.lease("alice");
        let mut b = second.lease("bob");
        let mut provider = Fake::default();
        let one = a
            .prepare_with(BackendChoice::Keychain, &mut provider)
            .unwrap();
        let two = b
            .prepare_with(BackendChoice::Keychain, &mut provider)
            .unwrap();
        first.remember(&a);
        second.remember(&b);
        assert_ne!(one.public_key(), two.public_key());
        assert_ne!(one.sqlcipher_root(), two.sqlcipher_root());
        assert_ne!(one.device_slot(), two.device_slot());
        assert_ne!(a.profile_id(), b.profile_id());
        assert_eq!(provider.creates, 4);
    }
    #[test]
    fn hardlinked_secrets_and_manifests_are_rejected() {
        let mut f = Fixture::new();
        let mut lease = f.lease("alice");
        drop(
            lease
                .prepare_with(BackendChoice::File, &mut Fake::default())
                .unwrap(),
        );
        f.remember(&lease);
        let secret = lease.root.join("keys").join(format!(
            "sqlcipher-{}",
            lease.manifest.as_ref().unwrap().sqlcipher_ref
        ));
        let alias = f.base.path().join("secret-alias");
        fs::hard_link(&secret, &alias).unwrap();
        assert!(matches!(
            lease.prepare_with(BackendChoice::File, &mut Fake::default()),
            Err(Error::Identity("unsafe_profile_file"))
        ));
        fs::remove_file(alias).unwrap();
        let manifest = lease.root.join("profile.json");
        fs::hard_link(&manifest, f.base.path().join("manifest-alias")).unwrap();
        drop(lease);
        assert!(matches!(
            ProfileLease::acquire(f.base.path(), "alice"),
            Err(Error::Identity("unsafe_profile_file"))
        ));
    }
    #[test]
    fn provider_readback_must_match_created_secret() {
        struct Conflicting {
            created: bool,
        }
        impl Provider for Conflicting {
            fn probe(&mut self) -> Result<()> {
                Ok(())
            }
            fn load(&mut self, _: &str, _: &str) -> Result<Zeroizing<[u8; 32]>> {
                if self.created {
                    Ok(Zeroizing::new([3; 32]))
                } else {
                    Err(Error::Identity("missing"))
                }
            }
            fn create(&mut self, _: &str, _: &str, _: &[u8; 32]) -> Result<()> {
                self.created = true;
                Err(Error::Identity("exists"))
            }
        }
        assert!(matches!(
            load_or_create(
                &mut Conflicting { created: false },
                "nostr",
                &"a".repeat(64),
                true
            ),
            Err(Error::Identity("key_readback_mismatch"))
        ));
    }
    #[test]
    fn explicit_runtime_cleanup_rejects_unknown_files_then_allows_reopen() {
        let mut f = Fixture::new();
        let mut lease = f.lease("alice");
        drop(
            lease
                .prepare_with(BackendChoice::File, &mut Fake::default())
                .unwrap(),
        );
        f.remember(&lease);
        let extra = lease.runtime.join("extra");
        write_new(&extra, b"do not delete").unwrap();
        assert!(matches!(
            lease.cleanup_runtime(),
            Err(Error::Identity("unexpected_runtime_file"))
        ));
        assert!(lease.runtime.join("binding.json").exists());
        fs::remove_file(extra).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(lease.socket_path()).unwrap();
        fs::set_permissions(lease.socket_path(), fs::Permissions::from_mode(0o600)).unwrap();
        drop(listener);
        lease.cleanup_runtime().unwrap();
        assert!(!lease.runtime.exists());
        drop(
            lease
                .prepare_with(BackendChoice::File, &mut Fake::default())
                .unwrap(),
        );
        assert!(lease.runtime.join("binding.json").exists());
    }
    #[test]
    fn persisted_public_identity_prevents_regeneration_during_recovery() {
        let f = Fixture::new();
        let mut lease = f.lease("alice");
        let mut fake = Fake {
            fail_at: Some(2),
            ..Fake::default()
        };
        assert!(
            lease
                .prepare_with(BackendChoice::Keychain, &mut fake)
                .is_err()
        );
        assert!(lease.public_key().is_some());
        fake.values.clear();
        fake.fail_at = None;
        assert!(matches!(
            lease.prepare_with(BackendChoice::Keychain, &mut fake),
            Err(Error::Identity("missing"))
        ));
        assert_eq!(fake.creates, 2);
    }
    #[test]
    fn native_helper_accepts_exact_limit_without_reallocation_and_rejects_next_byte() {
        let f = Fixture::new();
        let helper = f.base.path().join("fake");
        write_new(&helper, b"#!/bin/sh\nexec head -c 8192 /dev/zero\n").unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let response =
            helper_exchange(&helper, b"{}", Instant::now() + Duration::from_secs(2)).unwrap();
        assert_eq!(response.len(), HELPER_LIMIT);
        assert_eq!(response.capacity(), HELPER_LIMIT + 1);
        assert!(response.iter().all(|b| *b == 0));
        fs::write(&helper, b"#!/bin/sh\nexec head -c 8193 /dev/zero\n").unwrap();
        assert!(matches!(
            helper_exchange(&helper, b"{}", Instant::now() + Duration::from_secs(2)),
            Err(Error::Identity("helper_oversize"))
        ));
    }
    #[test]
    fn oversized_helper_request_is_rejected_before_process_launch() {
        let provider = NativeProvider {
            helper: Path::new("/unused"),
            deadline: Instant::now() + Duration::from_secs(1),
        };
        assert!(matches!(
            provider.call(
                "create",
                Some("nostr"),
                Some(&"a".repeat(HELPER_LIMIT)),
                Some(&[1; 32])
            ),
            Err(Error::Identity("invalid_helper_request"))
        ));
    }
}
