//! Rsync's secluded-argument remote shell. Never joins argv or invokes a shell.
use clap::{Command, Parser, error::ErrorKind};
use std::{ffi::OsString, os::unix::ffi::OsStrExt, time::Duration};

pub(crate) struct Invocation {
    pub root: Vec<OsString>,
    pub destination: String,
    pub login: Option<String>,
    pub argv: Vec<OsString>,
    pub compress: bool,
    pub idle: Duration,
    pub keepalive: Duration,
}

#[derive(Parser)]
#[command(
    name = "marmish rsh",
    about = "Rsync remote shell; use rsync -s -e 'marmish rsh'. Global Marmish options go before rsh."
)]
struct RshArgs {
    #[arg(long)]
    compress: bool,
    #[arg(long, default_value_t = 660, value_parser = clap::value_parser!(u64).range(1..=86400))]
    idle_seconds: u64,
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=86400))]
    keepalive_seconds: u64,
    #[arg(short = 'l', value_name = "REMOTE_IDENTITY")]
    login: Option<String>,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 1.., required = true)]
    target_and_argv: Vec<OsString>,
}

fn bad(reason: &'static str) -> clap::Error {
    clap::Error::raw(ErrorKind::InvalidValue, format!("marmish rsh: {reason}\n"))
}

fn allowed_global(id: &str) -> bool {
    matches!(
        id,
        "debug"
            | "state_dir"
            | "identity"
            | "socket"
            | "relay"
            | "srv"
            | "tor"
            | "key_storage"
            | "no_autostart"
            | "retention_seconds"
    )
}

/// Split BEFORE root Clap sees the host/remote argv: global options in the
/// remote bootstrap can never configure this local process. Other subcommands
/// retain the existing parser. Metadata supplies exact prefix option arities.
pub(crate) fn split(
    raw: &[OsString],
    mut command: Command,
) -> Result<Option<Invocation>, clap::Error> {
    command.build();
    let mut index = 1;
    let mut supplied = Vec::new();
    while let Some(value) = raw.get(index) {
        let bytes = value.as_bytes();
        if bytes == b"rsh" {
            break;
        }
        if bytes == b"--" || !bytes.starts_with(b"-") || bytes == b"-" {
            return Ok(None);
        }
        if matches!(bytes, b"--help" | b"-h" | b"--version" | b"-V") {
            return Ok(None);
        }
        let (argument, attached) = if let Some(long) = bytes.strip_prefix(b"--") {
            let name = long.split(|byte| *byte == b'=').next().unwrap_or_default();
            (
                command.get_arguments().find(|arg| {
                    arg.is_global_set()
                        && arg.get_long().is_some_and(|long| long.as_bytes() == name)
                }),
                long.contains(&b'='),
            )
        } else if bytes.len() >= 2 {
            (
                command.get_arguments().find(|arg| {
                    arg.is_global_set()
                        && arg
                            .get_short()
                            .is_some_and(|short| short as u32 == u32::from(bytes[1]))
                }),
                bytes.len() > 2,
            )
        } else {
            return Ok(None);
        };
        let Some(argument) = argument else {
            return Ok(None);
        };
        let takes_value = argument.get_action().takes_values();
        let range = argument.get_num_args();
        if range.is_some_and(|range| {
            range.min_values() != usize::from(takes_value)
                || range.max_values() != usize::from(takes_value)
        }) {
            return Err(bad("unsupported global option arity"));
        }
        supplied.push(argument.get_id().to_string());
        if attached && !takes_value {
            return Err(bad("unexpected attached flag value"));
        }
        index += 1;
        if takes_value && !attached {
            if index >= raw.len() {
                return Err(bad("missing global option value"));
            }
            index += 1;
        }
    }
    if raw.get(index).is_none() {
        return Ok(None);
    }
    if supplied.iter().any(|id| !allowed_global(id)) {
        return Err(bad(
            "unsupported global option; use rsync -l for the remote identity",
        ));
    }
    let root = raw[..=index].to_vec();
    let suffix = &raw[index + 1..];
    reject_separator_before_host(suffix)?;
    let parsed = RshArgs::try_parse_from(
        std::iter::once(OsString::from("marmish rsh")).chain(suffix.iter().cloned()),
    )?;
    let mut tail = parsed.target_and_argv.into_iter();
    let host = tail.next().ok_or_else(|| bad("missing host"))?;
    let destination = host
        .to_str()
        .filter(|host| {
            !host.is_empty()
                && !host.starts_with('-')
                && !host.contains('\0')
                && host.matches('@').count() <= 1
        })
        .ok_or_else(|| bad("invalid host identity or relay"))?
        .to_owned();
    if destination
        .split_once('@')
        .is_some_and(|(identity, relay)| identity.is_empty() || relay.is_empty())
    {
        return Err(bad("empty inline identity or relay"));
    }
    if parsed
        .login
        .as_ref()
        .is_some_and(|login| login.is_empty() || login.contains('\0'))
        || (parsed.login.is_some() && destination.contains('@'))
    {
        return Err(bad("ambiguous or invalid remote identity"));
    }
    let argv = tail.collect::<Vec<_>>();
    validate_bootstrap(&argv)?;
    Ok(Some(Invocation {
        root,
        destination,
        login: parsed.login,
        argv,
        compress: parsed.compress,
        idle: Duration::from_secs(parsed.idle_seconds),
        keepalive: Duration::from_secs(parsed.keepalive_seconds),
    }))
}

fn reject_separator_before_host(suffix: &[OsString]) -> Result<(), clap::Error> {
    let mut index = 0;
    while let Some(value) = suffix.get(index) {
        let value = value.as_bytes();
        if value == b"--" {
            return Err(bad("-- before host is unsupported"));
        }
        if matches!(value, b"-l" | b"--idle-seconds" | b"--keepalive-seconds") {
            index += 2;
        } else if value == b"--compress"
            || value == b"--help"
            || value == b"-h"
            || value.starts_with(b"--idle-seconds=")
            || value.starts_with(b"--keepalive-seconds=")
            || value.starts_with(b"-l")
        {
            index += 1;
        } else {
            break;
        }
    }
    Ok(())
}

fn validate_bootstrap(argv: &[OsString]) -> Result<(), clap::Error> {
    let bytes = argv
        .iter()
        .map(|arg| arg.as_bytes().to_vec())
        .collect::<Vec<_>>();
    marmish_stream::wire::open_digest(&bytes, marmish_stream::codec::Codec::None)
        .map_err(|_| bad("invalid or oversized rsync bootstrap"))?;
    if bytes[0].starts_with(b"-") || bytes.get(1).map(Vec::as_slice) != Some(b"--server") {
        return Err(bad(
            "expected literal executable and rsync --server bootstrap",
        ));
    }
    let cluster_index = if bytes.get(2).map(Vec::as_slice) == Some(b"--sender") {
        3
    } else {
        2
    };
    let cluster = bytes
        .get(cluster_index)
        .ok_or_else(|| bad("missing rsync server flags"))?;
    if !cluster.starts_with(b"-")
        || cluster.starts_with(b"--")
        || cluster[1..]
            .iter()
            .any(|byte| !byte.is_ascii_alphanumeric() && *byte != b'.')
        || !cluster[1..]
            .split(|byte| *byte == b'e')
            .next()
            .unwrap_or_default()
            .contains(&b's')
    {
        return Err(bad(
            "rsync requires secluded arguments: add -s to local rsync",
        ));
    }
    match &bytes[cluster_index + 1..] {
        [] => (),
        [iconv]
            if iconv
                .strip_prefix(b"--iconv=")
                .is_some_and(|value| !value.is_empty() && value.len() <= 1024) => {}
        _ => {
            return Err(bad(
                "unsupported rsync bootstrap operands or options; use -s",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn captured_rsync_bootstrap_fixtures_are_literal() {
        for tail in [
            vec!["rsync", "--server", "-slogDtpre.iLsfxCIvu"],
            vec!["rsync", "--server", "--sender", "-slogDtpre.iLsfxCIvu"],
            vec!["rsync", "--server", "-se.LsfxCIvu"],
            vec!["rsync", "--server", "-sze.LsfxCIvu"],
            vec!["rsync", "--server", "-s"],
            vec!["/opt/bin/my rsync", "--server", "-slogDtpre.iLsfxCIvu"],
            vec!["mkdir -p /tmp/x; rsync", "--server", "-slogDtpre.iLsfxCIvu"],
            vec!["rsync", "--server", "-slogDtpre.iLsfxCIvu", "--iconv=utf8"],
            vec![
                "rsync",
                "--server",
                "-slogDtpre.iLsfxCIvu",
                "--iconv=ISO-8859-1",
            ],
            vec!["rsync", "--server", "-slogDtpre.iLsfxCIvu", "--iconv=."],
        ] {
            let argv = tail.iter().map(OsString::from).collect::<Vec<_>>();
            validate_bootstrap(&argv).unwrap();
        }
    }
    #[test]
    fn unsafe_or_unobserved_bootstraps_fail_closed() {
        for tail in [
            vec!["rsync", "--server", "-logDtpre.iLsfxCIvu"],
            vec!["rsync", "--server", "-s", ".", "path"],
            vec!["rsync", "--daemon", "-s"],
            vec!["rsync", "--server"],
            vec!["-rsync", "--server", "-s"],
            vec!["rsync", "--server", "-s", "--help"],
            vec!["rsync", "--server", "-s", "--iconv="],
            vec!["rsync", "--server", "-s", "--iconv", "UTF8"],
            vec!["rsync", "--server", "-s", "--iconv=utf8", "--iconv=utf8"],
        ] {
            assert!(
                validate_bootstrap(&tail.iter().map(OsString::from).collect::<Vec<_>>()).is_err(),
                "{tail:?}"
            );
        }
        for invalid in [
            vec![
                OsString::from_vec(b"r\0sync".to_vec()),
                "--server".into(),
                "-s".into(),
            ],
            vec![
                "rsync".into(),
                "--server".into(),
                "-s".into(),
                OsString::from(format!("--iconv={}", "x".repeat(1025))),
            ],
        ] {
            assert!(validate_bootstrap(&invalid).is_err());
        }
        validate_bootstrap(&[
            OsString::from_vec(b"r\xffsync".to_vec()),
            "--server".into(),
            "-s".into(),
            OsString::from_vec(b"--iconv=\xff".to_vec()),
        ])
        .unwrap();
    }
}
