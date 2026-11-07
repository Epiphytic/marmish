//! Installed negative admission checks; their evidence never authorizes cleanup.
use super::{
    copy_interrupt::Binding,
    matrix, process,
    records::{self, Context, IdentityReady},
    streaming_cases::{self, Endpoint},
    streaming_cleanup,
    streaming_records::ServerReady,
};
use crate::{Error, Result};
use marmish_stream::wire::{Terminal, WireErrorCode};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ffi::OsString};
use tokio::time::Instant;

fn rsh_args(account: &str) -> Result<Vec<OsString>> {
    Ok(vec![
        "rsh".into(),
        records::public_destination(account)?.into(),
        "rsync".into(),
        "--server".into(),
        "--sender".into(),
        ".".into(),
        "fixture".into(),
    ])
}

fn stream_args(account: &str) -> Result<Vec<OsString>> {
    Ok(vec![
        "stream".into(),
        records::public_destination(account)?.into(),
        "--".into(),
    ])
}

fn bad() -> Error {
    Error::Invalid("installed_edge_admission")
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Observation {
    pub case: String,
    pub wall_ns: u64,
    pub exit: u8,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub binding: Option<Binding>,
    pub terminal: Option<Terminal>,
    pub server_open_count_before: usize,
    pub server_open_count_after: usize,
}
impl Observation {
    pub(super) fn validate(&self) -> Result<()> {
        let empty = records::digest(b"");
        if self.wall_ns == 0
            || self.wall_ns > 120_000_000_000
            || self.exit == 0
            || self.stdout_sha256 != empty
            || self.stderr_sha256 == empty
            || !records::hexadecimal(&self.stderr_sha256, 64)
        {
            return Err(bad());
        }
        match self.case.as_str() {
            "missing-executable" | "policy-denied" => {
                let binding = self.binding.as_ref().ok_or_else(bad)?;
                if [
                    &binding.stream_sha256,
                    &binding.session_sha256,
                    &binding.open_digest,
                    &binding.group_sha256,
                ]
                .into_iter()
                .any(|value| !records::hexadecimal(value, 64))
                    || self.server_open_count_after != self.server_open_count_before + 1
                {
                    return Err(bad());
                }
                let code = if self.case == "missing-executable" {
                    WireErrorCode::NotFound
                } else {
                    WireErrorCode::Denied
                };
                if self.terminal != Some(Terminal::Failed { code }) {
                    return Err(bad());
                }
            }
            "rsync-bootstrap-without-seclusion" => {
                if self.binding.is_some()
                    || self.terminal.is_some()
                    || self.server_open_count_before != self.server_open_count_after
                {
                    return Err(bad());
                }
            }
            _ => return Err(bad()),
        }
        Ok(())
    }
}
pub(super) struct Cohort<'a, 'b> {
    pub context: &'a Context,
    pub identities: &'a BTreeMap<String, IdentityReady>,
    pub role: &'a str,
    pub ready: &'a ServerReady,
    pub client: &'a Endpoint<'b>,
    pub server: &'a Endpoint<'b>,
    pub deadline: Instant,
}
async fn entries(c: &Cohort<'_, '_>) -> Result<Vec<streaming_cleanup::Entry>> {
    streaming_cleanup::entries(*c.server.daemon.lock().await, c.server.socket, c.deadline).await
}
fn normalized(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .copied()
        .filter(u8::is_ascii_alphanumeric)
        .map(|b| b.to_ascii_lowercase())
        .collect()
}
async fn one(c: &Cohort<'_, '_>, case: &str) -> Result<Observation> {
    let before = entries(c).await?;
    let mut args = matrix::base(
        c.client.workspace,
        "client",
        c.client.socket,
        &c.context.relays,
    );
    args.push("--no-autostart".into());
    if case == "rsync-bootstrap-without-seclusion" {
        args.extend(rsh_args(&c.ready.server_account)?);
    } else {
        args.extend(stream_args(&c.ready.server_account)?);
        let command = if case == "missing-executable" {
            c.server.workspace.join("never-created-executable")
        } else {
            "/bin/sh".into()
        };
        if case == "missing-executable" && command.try_exists()? {
            return Err(bad());
        }
        args.push(command.into_os_string());
    }
    let started = Instant::now();
    let output = process::run(
        &c.client.workspace.join("bin/marmish"),
        &args,
        c.client.workspace,
        c.client.environment,
        c.deadline,
        65536,
    )
    .await?;
    let after = entries(c).await?;
    let mut observed = Observation {
        case: case.into(),
        wall_ns: u64::try_from(started.elapsed().as_nanos()).map_err(|_| bad())?,
        exit: output
            .status
            .code()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(bad)?,
        stdout_sha256: records::digest(&output.stdout),
        stderr_sha256: records::digest(&output.stderr),
        binding: None,
        terminal: None,
        server_open_count_before: before.len(),
        server_open_count_after: after.len(),
    };
    if case == "rsync-bootstrap-without-seclusion" {
        // Compare complete private identity sets as well as public counts. A
        // failed pre-Open parse must not consume a fresh server-side stream.
        let keys = |rows: &[streaming_cleanup::Entry]| {
            rows.iter()
                .map(|e| e.stream.clone())
                .collect::<std::collections::BTreeSet<_>>()
        };
        if keys(&before) != keys(&after)
            || output.stderr.windows(16).any(|b| b == b"marmish stream: ")
        {
            return Err(bad());
        }
    } else {
        let (stream, diagnostic) = streaming_cases::stderr(&output.stderr)?;
        let entry = after.iter().find(|e| e.stream == stream).ok_or_else(bad)?;
        if entry.direction != "server"
            || entry.client != c.identities[c.role].client_account
            || entry.server != c.identities[c.role].server_account
            || entry.active
            || entry.cancel_requested
            || entry.state != "terminal"
        {
            return Err(bad());
        }
        let word = if case == "missing-executable" {
            b"notfound".as_slice()
        } else {
            b"denied".as_slice()
        };
        if !normalized(diagnostic)
            .windows(word.len())
            .any(|bytes| bytes == word)
        {
            return Err(bad());
        }
        observed.binding = Some(Binding {
            stream_sha256: records::digest(entry.stream.as_bytes()),
            session_sha256: records::digest(entry.session.as_bytes()),
            open_digest: entry.open_digest.clone().ok_or_else(bad)?,
            group_sha256: entry.group_sha256.clone(),
        });
        observed.terminal = entry.terminal;
    }
    observed.validate()?;
    streaming_cleanup::status(*c.server.daemon.lock().await, c.server.socket, c.deadline)
        .await?
        .validate_idle()?;
    Ok(observed)
}
pub(super) async fn run(c: Cohort<'_, '_>) -> Result<Vec<Observation>> {
    let mut observations = Vec::new();
    for case in [
        "missing-executable",
        "policy-denied",
        "rsync-bootstrap-without-seclusion",
    ] {
        observations.push(one(&c, case).await?);
    }
    Ok(observations)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation(case: &str) -> Observation {
        Observation {
            case: case.into(),
            wall_ns: 1,
            exit: 1,
            stdout_sha256: records::digest(b""),
            stderr_sha256: records::digest(b"failure"),
            binding: Some(Binding {
                stream_sha256: hex::encode([1u8; 32]),
                session_sha256: hex::encode([2u8; 32]),
                open_digest: hex::encode([3u8; 32]),
                group_sha256: hex::encode([4u8; 32]),
            }),
            terminal: Some(Terminal::Failed {
                code: WireErrorCode::NotFound,
            }),
            server_open_count_before: 1,
            server_open_count_after: 2,
        }
    }
    #[test]
    fn admission_evidence_distinguishes_policy_from_missing_and_requires_one_charged_fence() {
        let mut value = observation("missing-executable");
        value.validate().unwrap();
        value.case = "policy-denied".into();
        assert!(value.validate().is_err());
        value.terminal = Some(Terminal::Failed {
            code: WireErrorCode::Denied,
        });
        value.validate().unwrap();
        value.server_open_count_after = 3;
        assert!(value.validate().is_err());
        value.server_open_count_after = 2;
        value.stdout_sha256 = records::digest(b"banner");
        assert!(value.validate().is_err());
    }
    #[test]
    fn unsupported_bootstrap_requires_no_open_identity_or_cancel_receipt() {
        let mut value = observation("rsync-bootstrap-without-seclusion");
        assert!(value.validate().is_err());
        value.binding = None;
        value.terminal = None;
        value.server_open_count_after = 1;
        value.validate().unwrap();
        value.server_open_count_after = 2;
        assert!(value.validate().is_err());
    }
}

#[cfg(test)]
mod public_destination_tests {
    use super::*;
    #[test]
    fn public_destination_builder_obeys_product_contract() {
        let account = "ab".repeat(32);
        let rsh = rsh_args(&account).unwrap();
        records::assert_public_target(&rsh[1], &account);
        assert_eq!(rsh[2..], ["rsync", "--server", "--sender", ".", "fixture"]);
        let stream = stream_args(&account).unwrap();
        records::assert_public_target(&stream[1], &account);
        assert_eq!(stream[2], "--");
    }
}
