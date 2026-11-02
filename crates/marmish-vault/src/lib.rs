//! Host-native immutable named secrets. Calls have no built-in deadline;
//! subprocess callers must bound execution, including native provider prompts.
use std::fmt;
use zeroize::Zeroizing;

pub mod helper;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    Legacy,
    MarmishV2,
}
impl Namespace {
    pub fn version(self) -> u8 {
        match self {
            Self::Legacy => 1,
            Self::MarmishV2 => 2,
        }
    }
    pub(crate) fn version_str(self) -> &'static str {
        match self {
            Self::Legacy => "1",
            Self::MarmishV2 => "2",
        }
    }
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
use linux::Provider;
#[cfg(target_os = "macos")]
use macos::Provider;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purpose {
    Nostr,
    Sqlcipher,
}
impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nostr => "nostr",
            Self::Sqlcipher => "sqlcipher",
        }
    }
    pub fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "nostr" => Ok(Self::Nostr),
            "sqlcipher" => Ok(Self::Sqlcipher),
            _ => Err(Error::Malformed),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyRef {
    version: u8,
    purpose: Purpose,
    id: String,
}
impl KeyRef {
    pub fn new(version: u8, purpose: Purpose, id: &str) -> Result<Self, Error> {
        if !matches!(version, 1 | 2)
            || id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Malformed);
        }
        Ok(Self {
            version,
            purpose,
            id: id.into(),
        })
    }
    pub fn version(&self) -> u8 {
        self.version
    }
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }
    pub fn id(&self) -> &str {
        &self.id
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label(String);
impl Label {
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.is_empty()
            || value.len() > 128
            || value.chars().any(|c| {
                c.is_control() || matches!(c, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        {
            return Err(Error::Malformed);
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
pub struct Secret(Zeroizing<[u8; 32]>);
impl Secret {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }
    pub fn from_slice(bytes: &[u8]) -> Result<Self, Error> {
        Ok(Self::from_bytes(
            bytes.try_into().map_err(|_| Error::Malformed)?,
        ))
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub key_ref: KeyRef,
    pub label: Label,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Uninitialized,
    LockedOrDenied,
    Missing,
    Exists,
    Ambiguous,
    Malformed,
    BackendFailure,
}
impl Error {
    pub fn category(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Uninitialized => "uninitialized",
            Self::LockedOrDenied => "locked_or_denied",
            Self::Missing => "missing",
            Self::Exists => "exists",
            Self::Ambiguous => "ambiguous",
            Self::Malformed => "malformed",
            Self::BackendFailure => "backend_failure",
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.category())
    }
}
impl std::error::Error for Error {}

pub struct Vault {
    namespace: Namespace,
    provider: Provider,
}
impl Vault {
    pub fn open_default() -> Result<Self, Error> {
        Self::open_namespaced(Namespace::Legacy)
    }
    pub fn open_namespaced(namespace: Namespace) -> Result<Self, Error> {
        Ok(Self {
            namespace,
            provider: Provider::open_default(namespace)?,
        })
    }
    #[cfg(target_os = "macos")]
    pub fn open_keychain(
        keychain: &security_framework::os::macos::keychain::SecKeychain,
    ) -> Result<Self, Error> {
        Self::open_keychain_namespaced(keychain, Namespace::Legacy)
    }
    #[cfg(target_os = "macos")]
    pub fn open_keychain_namespaced(
        keychain: &security_framework::os::macos::keychain::SecKeychain,
        namespace: Namespace,
    ) -> Result<Self, Error> {
        Ok(Self {
            namespace,
            provider: Provider::open_keychain(keychain, namespace)?,
        })
    }
    fn check_reference(&self, reference: &KeyRef) -> Result<(), Error> {
        if reference.version() == self.namespace.version() {
            Ok(())
        } else {
            Err(Error::Malformed)
        }
    }
    pub fn probe(&self) -> Result<(), Error> {
        self.provider.probe()
    }
    pub fn create(&self, key_ref: &KeyRef, label: &Label, secret: &Secret) -> Result<(), Error> {
        self.check_reference(key_ref)?;
        self.provider.create(key_ref, label, secret)
    }
    pub fn load(&self, key_ref: &KeyRef) -> Result<Secret, Error> {
        self.check_reference(key_ref)?;
        self.provider.load(key_ref)
    }
    pub fn list(&self) -> Result<Vec<Metadata>, Error> {
        self.provider.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_public_values() {
        assert!(KeyRef::new(1, Purpose::Nostr, &"ab".repeat(32)).is_ok());
        for (version, id) in [
            (0, "a".repeat(64)),
            (1, "A".repeat(64)),
            (1, "f".repeat(63)),
        ] {
            assert_eq!(
                KeyRef::new(version, Purpose::Nostr, &id),
                Err(Error::Malformed)
            );
        }
        for label in ["", "\n", "a\u{202e}b", "\u{2066}", "\u{2028}"] {
            assert_eq!(Label::new(label), Err(Error::Malformed));
        }
        assert!(Label::new("marmotte 🦫").is_ok());
        assert!(Label::new(&"é".repeat(64)).is_ok());
        assert!(Label::new(&"é".repeat(65)).is_err());
        assert!(Secret::from_slice(&[0; 31]).is_err());
        assert_eq!(Secret::from_slice(&[7; 32]).unwrap().as_bytes(), &[7; 32]);
    }
}
