//! Pure destination selection. Explicit SRV names are returned to CLI setup;
//! this module never performs DNS, opens an identity, or changes relay policy.
use crate::command_config::CommandConfig;
use crate::command_wire::Account;
use crate::transport::{RelayPolicy, RouteMode};
use crate::{Error, Result};
use std::collections::BTreeSet;

pub(crate) struct ResolvedTarget {
    pub remote: Account,
    pub relays: Vec<String>,
}
pub(crate) struct TargetSelection {
    pub remote: Account,
    pub source: RelaySource,
}
pub(crate) enum RelaySource {
    Configured,
    Explicit(Vec<String>),
    Srv(Vec<String>),
}
fn invalid(category: &'static str) -> Error {
    Error::Invalid(category)
}
fn account(config: &CommandConfig, value: &str) -> Result<Account> {
    if value.len() > 128 {
        return Err(invalid("invalid_remote_identity"));
    }
    if value.starts_with("npub1") {
        return crate::command_config::account(value)
            .map_err(|_| invalid("invalid_remote_identity"));
    }
    // Reserved Nostr prefixes and raw hex never fall through to alias lookup.
    if ["npub", "nsec", "nprofile"]
        .iter()
        .any(|prefix| value.starts_with(*prefix))
        || (value.len() == 64 && value.bytes().all(|v| v.is_ascii_hexdigit()))
    {
        return Err(invalid("invalid_remote_identity"));
    }
    config
        .aliases()
        .get(value)
        .map(|a| a.remote)
        .ok_or_else(|| invalid("unknown_alias"))
}

pub(crate) fn parse(
    config: &CommandConfig,
    destination: Option<&str>,
    user: Option<&str>,
    relays: &[String],
    srvs: &[String],
) -> Result<TargetSelection> {
    if relays.len() > 16 || srvs.len() > 16 {
        return Err(invalid("relay_count"));
    }
    if !relays.is_empty() && !srvs.is_empty() {
        return Err(invalid("conflicting_destination"));
    }
    let (remote, inline) = match (destination, user) {
        (Some(value), Some(user)) => {
            if value.contains('@') || !relays.is_empty() || !srvs.is_empty() {
                return Err(invalid("conflicting_destination"));
            }
            (account(config, user)?, Some(value))
        }
        (Some(value), None) => match value.split_once('@') {
            Some((remote, host)) => {
                if !relays.is_empty() || !srvs.is_empty() || host.contains('@') {
                    return Err(invalid("conflicting_destination"));
                }
                (account(config, remote)?, Some(host))
            }
            None => {
                // Apply the same identity-before-alias rules as -u, including
                // reserved-prefix rejection, before consulting alias relays.
                let remote = account(config, value)?;
                if value.starts_with("npub1") {
                    (remote, None)
                } else {
                    if !relays.is_empty() || !srvs.is_empty() {
                        return Err(invalid("conflicting_destination"));
                    }
                    let alias = config
                        .aliases()
                        .get(value)
                        .ok_or_else(|| invalid("unknown_alias"))?;
                    return Ok(TargetSelection {
                        remote,
                        source: RelaySource::Explicit(
                            alias
                                .relays
                                .urls()
                                .iter()
                                .map(ToString::to_string)
                                .collect(),
                        ),
                    });
                }
            }
        },
        (None, Some(user)) => (account(config, user)?, None),
        (None, None) => return Err(invalid("invalid_destination")),
    };
    let source = if let Some(inline) = inline {
        if let Some(name) = inline.strip_prefix("srv:") {
            srv_source(config, &[name.to_owned()])?
        } else {
            RelaySource::Explicit(vec![relay_url(inline)?])
        }
    } else if !srvs.is_empty() {
        srv_source(config, srvs)?
    } else if !relays.is_empty() {
        // --relay accepts URLs, while a positional HOST supplies the shorthand.
        if relays.iter().any(|value| !value.contains("://")) {
            return Err(invalid("invalid_destination"));
        }
        RelaySource::Explicit(relays.iter().map(|v| relay_url(v)).collect::<Result<_>>()?)
    } else {
        RelaySource::Configured
    };
    Ok(TargetSelection { remote, source })
}
fn srv_source(config: &CommandConfig, names: &[String]) -> Result<RelaySource> {
    let policy = config.relay_policy();
    if policy.mode() == RouteMode::Always || policy.needs_tor() {
        return Err(invalid("srv_route_unsupported"));
    }
    if names.is_empty() || names.len() > 16 || names.iter().map(String::len).sum::<usize>() > 4096 {
        return Err(invalid("invalid_srv_name"));
    }
    let mut canonical = BTreeSet::new();
    for name in names {
        let name = validate_srv_name(name)?;
        if !canonical.insert(name) {
            return Err(invalid("invalid_srv_name"));
        }
    }
    Ok(RelaySource::Srv(canonical.into_iter().collect()))
}
/// Returns a fully qualified, lowercase owner with a final DNS root dot.
pub(crate) fn validate_srv_name(name: &str) -> Result<String> {
    if !name.is_ascii() || name.is_empty() || name.len() > 255 {
        return Err(invalid("invalid_srv_name"));
    }
    let name = name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase();
    let labels: Vec<_> = name.split('.').collect();
    if labels.len() < 3
        || labels[1] != "_tcp"
        || !labels[0].starts_with('_')
        || labels[0].len() > 63
        || !dns_label(&labels[0][1..])
        || !labels[2..].iter().all(|s| dns_label(s))
        || name.len() + 2 > 255
    {
        return Err(invalid("invalid_srv_name"));
    }
    let domain = labels[2..].join(".");
    if domain == "onion" || domain.ends_with(".onion") || domain.parse::<std::net::IpAddr>().is_ok()
    {
        return Err(invalid("invalid_srv_name"));
    }
    Ok(format!("{name}."))
}
fn dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label.as_bytes()[0].is_ascii_alphanumeric()
        && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
        && label
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || v == b'-')
}
fn relay_url(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 2048
        || value
            .chars()
            .any(|v| v.is_control() || v.is_whitespace() || v == '\\')
    {
        return Err(invalid("invalid_destination"));
    }
    if value.contains("://") {
        return Ok(value.to_owned());
    }
    if !value.is_ascii() || value.contains(['/', '?', '#', '@', '%', ',']) {
        return Err(invalid("invalid_destination"));
    }
    let (host, port) = if value.starts_with('[') {
        let end = value
            .find(']')
            .ok_or_else(|| invalid("invalid_destination"))?;
        value[1..end]
            .parse::<std::net::Ipv6Addr>()
            .map_err(|_| invalid("invalid_destination"))?;
        let tail = &value[end + 1..];
        let port = if tail.is_empty() {
            None
        } else {
            Some(
                tail.strip_prefix(':')
                    .ok_or_else(|| invalid("invalid_destination"))?,
            )
        };
        (&value[..=end], port)
    } else {
        let (host, port) = match value.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (value, None),
        };
        if host.contains(':') || !host.split('.').all(dns_label) {
            return Err(invalid("invalid_destination"));
        }
        // URL's legacy integer/octal/short-IPv4 parser is not HOST grammar.
        let parsed = url::Host::parse(host).map_err(|_| invalid("invalid_destination"))?;
        if (host.bytes().all(|v| v.is_ascii_digit() || v == b'.')
            || matches!(parsed, url::Host::Ipv4(_)))
            && host.parse::<std::net::Ipv4Addr>().is_err()
        {
            return Err(invalid("invalid_destination"));
        }
        (host, port)
    };
    if let Some(port) = port
        && (port.is_empty()
            || !port.bytes().all(|v| v.is_ascii_digit())
            || port.parse::<u16>().ok().filter(|v| *v > 0).is_none())
    {
        return Err(invalid("invalid_destination"));
    }
    let _ = host;
    Ok(format!("wss://{value}/"))
}
/// Both ordinary selections and DNS snapshots must equal the daemon's ceiling.
pub(crate) fn finalize(
    config: &CommandConfig,
    remote: Account,
    relays: &[String],
) -> Result<ResolvedTarget> {
    if relays.is_empty() || relays.len() > 16 {
        return Err(invalid("relay_count"));
    }
    let configured = config.relay_policy();
    let selected = RelayPolicy::parse(relays, configured.mode(), configured.limits())
        .map_err(|_| invalid("invalid_destination"))?;
    if selected.urls() != configured.urls() {
        return Err(invalid("destination_policy_mismatch"));
    }
    Ok(ResolvedTarget {
        remote,
        relays: selected.urls().iter().map(ToString::to_string).collect(),
    })
}
#[cfg(test)]
fn resolve(
    config: &CommandConfig,
    destination: Option<&str>,
    user: Option<&str>,
    relays: &[String],
) -> Result<ResolvedTarget> {
    let selection = parse(config, destination, user, relays, &[])?;
    let urls = match selection.source {
        RelaySource::Configured => config
            .relay_policy()
            .urls()
            .iter()
            .map(ToString::to_string)
            .collect(),
        RelaySource::Explicit(urls) => urls,
        RelaySource::Srv(_) => return Err(invalid("srv_backend_unavailable")),
    };
    finalize(config, selection.remote, &urls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{key::PublicKey, nips::nip19::ToBech32};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    fn npub() -> String {
        PublicKey::from_hex(&"02".repeat(32))
            .unwrap()
            .to_bech32()
            .unwrap()
    }
    fn config(relays: &[&str], mode: &str) -> CommandConfig {
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let value = serde_json::json!({"version":1,"role":"both","relays":relays,"tor":mode,
            "trusted_remotes":[],"execution_clients":[],"command_policy":{"version":1},
            "cwd":null,"limits":{},"aliases":{"remote":{"remote":npub(),"relays":relays}}});
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(d.path().join("config.json"))
            .unwrap();
        f.write_all(&serde_json::to_vec(&value).unwrap()).unwrap();
        CommandConfig::load(d.path()).unwrap()
    }
    #[test]
    fn identity_alias_and_equivalent_explicit_urls_share_exact_policy() {
        let c = config(&["wss://relay.example/"], "auto");
        let inline = format!("{}@relay.example", npub());
        let alias = parse(&c, Some("remote"), None, &[], &[]).unwrap();
        assert!(matches!(alias.source, RelaySource::Explicit(ref urls)
            if urls == &["wss://relay.example/"]));
        for target in [
            resolve(&c, Some(&inline), None, &[]),
            resolve(&c, Some(&npub()), None, &[]),
            resolve(&c, Some("remote"), None, &[]),
            resolve(&c, None, Some(&npub()), &[]),
            resolve(
                &c,
                None,
                Some("remote"),
                &["wss://relay.example:443/".into()],
            ),
        ] {
            let target = target.unwrap();
            assert_eq!(target.remote, [2; 32]);
            assert_eq!(target.relays, ["wss://relay.example/"]);
        }
        assert!(resolve(&c, None, Some(&"02".repeat(32)), &[]).is_err());
        assert!(resolve(&c, None, Some("nsec1invalid"), &[]).is_err());
        assert!(resolve(&c, None, Some("npub1badchecksum"), &[]).is_err());
        assert!(resolve(&c, None, Some("missing"), &[]).is_err());
    }
    #[test]
    fn bare_identity_uses_complete_configured_set_or_explicit_selection() {
        for relays in [
            vec!["wss://relay.example/".to_owned()],
            crate::transport::default_relays(),
        ] {
            let urls: Vec<_> = relays.iter().map(String::as_str).collect();
            let c = config(&urls, "auto");
            let bare = parse(&c, Some(&npub()), None, &[], &[]).unwrap();
            assert_eq!(bare.remote, [2; 32]);
            assert!(matches!(bare.source, RelaySource::Configured));
            let target = resolve(&c, Some(&npub()), None, &[]).unwrap();
            let user = resolve(&c, None, Some(&npub()), &[]).unwrap();
            assert_eq!(target.remote, user.remote);
            assert_eq!(target.relays, user.relays);
            assert_eq!(target.relays, relays);
            let explicit = resolve(&c, Some(&npub()), None, &relays).unwrap();
            assert_eq!(explicit.relays, target.relays);
            assert!(matches!(
                resolve(
                    &c,
                    Some(&npub()),
                    None,
                    &["wss://unconfigured.example/".into()]
                ),
                Err(Error::Invalid("destination_policy_mismatch"))
            ));
            assert!(resolve(&c, Some(&format!("{}@", npub())), None, &[]).is_err());
        }
    }
    #[test]
    fn identity_spellings_never_redirect_through_aliases() {
        let mut value = serde_json::json!({"version":1,"role":"both",
            "relays":["wss://relay.example/"],"tor":"auto",
            "trusted_remotes":[],"execution_clients":[],"command_policy":{"version":1},
            "cwd":null,"limits":{},"aliases":{}});
        let other = PublicKey::from_hex(&"03".repeat(32))
            .unwrap()
            .to_bech32()
            .unwrap();
        let aliases = value["aliases"].as_object_mut().unwrap();
        let reserved = ["npub1badchecksum", "npub-bob", "nsec-bob", "nprofile-bob"];
        let raw_hex = "ab".repeat(32);
        for name in std::iter::once(npub())
            .chain(reserved.iter().map(|s| (*s).to_owned()))
            .chain(std::iter::once(raw_hex.clone()))
        {
            aliases.insert(
                name,
                serde_json::json!({"remote":other,"relays":["wss://relay.example/"]}),
            );
        }
        let c = CommandConfig::from_bytes(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            resolve(&c, Some(&npub()), None, &[]).unwrap().remote,
            [2; 32]
        );
        for name in reserved
            .iter()
            .copied()
            .chain(std::iter::once(raw_hex.as_str()))
        {
            assert!(matches!(
                parse(&c, Some(name), None, &[], &[]),
                Err(Error::Invalid("invalid_remote_identity"))
            ));
        }
    }
    #[test]
    fn conflicting_sources_and_policy_narrowing_fail() {
        let c = config(&["wss://relay.example/", "wss://other.example/"], "auto");
        let inline = format!("{}@relay.example", npub());
        assert!(matches!(
            resolve(&c, Some(&inline), None, &[]),
            Err(Error::Invalid("destination_policy_mismatch"))
        ));
        assert!(parse(&c, Some(&inline), Some(&npub()), &[], &[]).is_err());
        assert!(
            parse(
                &c,
                Some("remote"),
                None,
                &["wss://relay.example/".into()],
                &[]
            )
            .is_err()
        );
        assert!(
            parse(
                &c,
                None,
                Some(&npub()),
                &["wss://relay.example/".into()],
                &["_shell._tcp.example".into()]
            )
            .is_err()
        );
        assert!(
            parse(
                &c,
                Some("relay.example"),
                Some(&npub()),
                &["wss://relay.example/".into()],
                &[]
            )
            .is_err()
        );
        assert!(
            resolve(
                &c,
                None,
                Some(&npub()),
                &[
                    "wss://other.example/".into(),
                    "wss://relay.example:443/".into(),
                    "wss://relay.example/".into()
                ]
            )
            .is_ok()
        );
        assert!(
            resolve(
                &c,
                None,
                Some(&npub()),
                &vec!["wss://relay.example/".into(); 17]
            )
            .is_err()
        );
    }
    #[test]
    fn host_ipv6_ports_paths_and_invalid_authorities() {
        let c = config(&["wss://[::1]:8443/"], "auto");
        assert!(resolve(&c, Some("[::1]:8443"), Some(&npub()), &[]).is_ok());
        let c = config(&["wss://relay.example/socket%20path"], "always");
        assert!(
            resolve(
                &c,
                Some("wss://relay.example/socket%20path"),
                Some(&npub()),
                &[]
            )
            .is_ok()
        );
        for value in [
            "::1",
            "[::1",
            "[::1]extra",
            "[fe80::1%en0]",
            "host:0",
            "host:65536",
            "host:",
            "host:0443",
            "host/path",
            "a,b",
            "a b",
            "a\\b",
            "127.1",
            "0x7f.1",
            "2130706433",
            "010.0.0.1",
            "host?x",
            "host#x",
            "wss://u:p@host/",
            "wss://host/?secret=x",
        ] {
            assert!(
                resolve(&c, Some(value), Some(&npub()), &[]).is_err(),
                "accepted {value}"
            );
        }
    }
    #[test]
    fn srv_is_explicit_absolute_bounded_and_never_mixed_with_tor() {
        assert_eq!(
            validate_srv_name("_Shell._tcp.EXAMPLE.").unwrap(),
            "_shell._tcp.example."
        );
        for value in [
            "srv:443",
            "_shell._udp.example",
            "_._tcp.example",
            "_shell._tcp..example",
            "_shell._tcp.example..",
            "_shell._tcp.127.0.0.1",
            "_shell._tcp.hidden.onion",
            "_shell._tcp.-bad",
            "_shell._tcp.éxample",
        ] {
            assert!(validate_srv_name(value).is_err(), "accepted {value}");
        }
        let c = config(&["wss://relay.example/"], "auto");
        let bare = parse(
            &c,
            Some(&npub()),
            None,
            &[],
            &["_shell._tcp.example".into()],
        )
        .unwrap();
        assert_eq!(bare.remote, [2; 32]);
        assert!(matches!(bare.source, RelaySource::Srv(ref names)
            if names == &["_shell._tcp.example."]));
        let selection = parse(
            &c,
            Some(&format!("{}@srv:_shell._tcp.example", npub())),
            None,
            &[],
            &[],
        )
        .unwrap();
        assert!(
            matches!(selection.source,RelaySource::Srv(ref names) if names==&["_shell._tcp.example."])
        );
        assert!(
            parse(
                &c,
                None,
                Some(&npub()),
                &[],
                &["_shell._tcp.example".into(), "_SHELL._tcp.EXAMPLE.".into()]
            )
            .is_err()
        );
        assert!(parse(&c, Some(&format!("{}@srv:443", npub())), None, &[], &[]).is_err());
        assert!(finalize(&c, selection.remote, &["wss://other.example/".into()]).is_err());
        let always = config(&["wss://relay.example/"], "always");
        assert!(matches!(
            parse(
                &always,
                Some(&npub()),
                None,
                &[],
                &["_shell._tcp.example".into()]
            ),
            Err(Error::Invalid("srv_route_unsupported"))
        ));
        assert!(matches!(
            parse(
                &always,
                None,
                Some(&npub()),
                &[],
                &["_shell._tcp.example".into()]
            ),
            Err(Error::Invalid("srv_route_unsupported"))
        ));
    }
}
