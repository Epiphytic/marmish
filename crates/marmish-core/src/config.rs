use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use thiserror::Error;
use url::{Host, Url};

/// Errors deliberately omit rejected URLs and configuration values.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("an explicit nonempty relay set is required")]
    EmptyRelaySet,
    #[error("invalid relay URL")]
    InvalidUrl,
    #[error("relay scheme must be ws or wss")]
    DisallowedScheme,
    #[error("relay URL contains unsupported components")]
    DisallowedComponents,
    #[error("relay host is not permitted")]
    DisallowedHost,
    #[error("loopback relays require explicit development opt-in")]
    LoopbackDenied,
    #[error("retention must be between one second and 365 days")]
    RetentionOutOfRange,
}

/// Canonical configured URLs, not an enforced network egress boundary.
///
/// Literal private/nonpublic IPs are rejected. DNS names are not resolved here;
/// this validator provides no DNS-rebinding or discovered-route protection.
#[derive(Clone, PartialEq, Eq)]
pub struct RelaySet(Vec<String>);

impl RelaySet {
    pub fn parse(entries: &[String], allow_loopback: bool) -> Result<Self, ConfigError> {
        if entries.is_empty() {
            return Err(ConfigError::EmptyRelaySet);
        }
        let mut canonical = Vec::new();
        for entry in entries {
            let value = canonical_relay(entry, allow_loopback)?;
            if !canonical.contains(&value) {
                canonical.push(value);
            }
        }
        Ok(Self(canonical))
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

fn canonical_relay(entry: &str, allow_loopback: bool) -> Result<String, ConfigError> {
    if entry.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ConfigError::InvalidUrl);
    }
    if entry.contains([',', '@', '\\']) {
        return Err(ConfigError::DisallowedComponents);
    }
    let mut url = Url::parse(entry).map_err(|_| ConfigError::InvalidUrl)?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(ConfigError::DisallowedScheme);
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(ConfigError::DisallowedComponents);
    }
    let loopback = match url.host().ok_or(ConfigError::DisallowedHost)? {
        Host::Domain(domain) => {
            let host = domain.strip_suffix('.').unwrap_or(domain).to_owned();
            if host.is_empty()
                || host.contains(',')
                || host.split('.').any(|label| {
                    label.is_empty()
                        || label.starts_with('-')
                        || label.ends_with('-')
                        || !label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return Err(ConfigError::DisallowedHost);
            }
            let loopback = host == "localhost" || host.ends_with(".localhost");
            url.set_host(Some(&host))
                .map_err(|_| ConfigError::InvalidUrl)?;
            loopback
        }
        Host::Ipv4(ip) => classify_ip(IpAddr::V4(ip))?,
        Host::Ipv6(ip) => classify_ip(IpAddr::V6(ip))?,
    };
    if loopback && !allow_loopback {
        return Err(ConfigError::LoopbackDenied);
    }
    Ok(url.to_string())
}

fn classify_ip(ip: IpAddr) -> Result<bool, ConfigError> {
    if ip.is_loopback() {
        return Ok(true);
    }
    let allowed = match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => public_v6(ip),
    };
    if allowed {
        Ok(false)
    } else {
        Err(ConfigError::DisallowedHost)
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_link_local()
        || ip.is_documentation()
        || a == 0
        || a >= 224
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && (b == 18 || b == 19))
        || (a == 192 && b == 0 && c == 0))
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let parts = ip.segments();
    // Conservative phase-0 literal policy: global-unicast only, excluding
    // documentation and transition/special-use blocks. IPv4-mapped is denied.
    (parts[0] & 0xe000) == 0x2000
        && parts[0] != 0x2002
        && !(parts[0] == 0x2001 && (parts[1] < 0x0200 || parts[1] == 0x0db8))
        && !(parts[0] == 0x3fff && parts[1] < 0x1000)
}

/// `None` is explicitly disabled; missing config should use `Default`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention(Option<u64>);

impl Retention {
    /// Phase-0 upper bound is 365 days, avoiding unbounded timestamp arithmetic.
    pub const MAX_SECONDS: u64 = 365 * 86_400;

    pub fn from_seconds(seconds: Option<u64>) -> Result<Self, ConfigError> {
        if matches!(seconds, Some(0)) || seconds.is_some_and(|v| v > Self::MAX_SECONDS) {
            return Err(ConfigError::RetentionOutOfRange);
        }
        Ok(Self(seconds))
    }

    pub fn seconds(&self) -> Option<u64> {
        self.0
    }
}

impl Default for Retention {
    fn default() -> Self {
        Self(Some(86_400))
    }
}
