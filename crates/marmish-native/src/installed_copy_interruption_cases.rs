//! Actual installed CLI detach/cancel/resume orchestration for one owned host.
use super::{
    cleanup, copy_checkpoint, copy_detach,
    copy_interrupt::{Binding, Evidence},
    matrix, process,
    records::{self, Context, IdentityReady},
    streaming_cases::Endpoint,
    streaming_cleanup::{self, Entry},
    streaming_records::ServerReady,
    transfer_cases,
};
use crate::{
    Error, Result,
    stream_service::{StreamLocalRequest, StreamLocalResult},
};
use marmish_stream::{
    copy_protocol::{AttemptMode, Direction, Launch, VERSION},
    wire::Terminal,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::Instant;
const SIZE: usize = super::copy_interrupt::PAYLOAD_BYTES as usize;

fn copy_operands(account: &str, source: &Path, target: &Path) -> Result<Vec<OsString>> {
    Ok(vec![
        "--".into(),
        records::public_destination(account)?.into(),
        source.as_os_str().into(),
        target.as_os_str().into(),
    ])
}

fn bad() -> Error {
    Error::Invalid("installed_copy_interruption_case")
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
fn bytes() -> Vec<u8> {
    (0..SIZE)
        .map(|n| (n as u8).wrapping_mul(73).wrapping_add(19))
        .collect()
}
fn decode<const N: usize>(value: &str) -> Result<[u8; N]> {
    hex::decode(value)
        .map_err(|_| bad())?
        .try_into()
        .map_err(|_| bad())
}
fn binding(entry: &Entry) -> Result<Binding> {
    Ok(Binding {
        stream_sha256: records::digest(entry.stream.as_bytes()),
        session_sha256: records::digest(entry.session.as_bytes()),
        open_digest: entry.open_digest.clone().ok_or_else(bad)?,
        group_sha256: entry.group_sha256.clone(),
    })
}
async fn entry(ep: &Endpoint<'_>, stream: &str, deadline: Instant) -> Result<Option<Entry>> {
    Ok(
        streaming_cleanup::entries(*ep.daemon.lock().await, ep.socket, deadline)
            .await?
            .into_iter()
            .find(|entry| entry.stream == stream),
    )
}
fn account_matches(c: &Cohort<'_, '_>, e: &Entry) -> bool {
    e.direction == "server"
        && e.client == c.identities[c.role].client_account
        && e.server == c.identities[c.role].server_account
}
async fn charged(c: &Cohort<'_, '_>, stream: &str) -> Result<Entry> {
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        if let Some(e) = entry(c.server, stream, c.deadline).await? {
            if !account_matches(c, &e) || e.terminal.is_some() || e.cancel_requested || !e.active {
                return Err(bad());
            }
            if e.state == "running" {
                let status = streaming_cleanup::status(
                    *c.server.daemon.lock().await,
                    c.server.socket,
                    c.deadline,
                )
                .await?;
                if status.active != 1 || status.uncertain != 0 || status.unfinished_runners != 1 {
                    return Err(bad());
                }
                return Ok(e);
            }
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(100))).await;
    }
}
async fn terminal(c: &Cohort<'_, '_>, stream: &str, cancelled: bool) -> Result<Entry> {
    loop {
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        if let Some(e) = entry(c.server, stream, c.deadline).await? {
            if !account_matches(c, &e) {
                return Err(bad());
            }
            if e.state == "terminal" && !e.active {
                match (&e.terminal, cancelled) {
                    (Some(Terminal::Cancelled), true) if e.cancel_requested => return Ok(e),
                    (Some(Terminal::Exited { .. }), false) if !e.cancel_requested => return Ok(e),
                    _ => return Err(bad()),
                }
            }
            if e.state == "lost" || e.state == "unknown_launch" {
                return Err(bad());
            }
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(100))).await;
    }
}
fn roots(c: &Cohort<'_, '_>, put: bool) -> Result<(PathBuf, PathBuf)> {
    let name = if put {
        "copy-interrupt-put"
    } else {
        "copy-interrupt-get"
    };
    let local = c.client.workspace.join(format!("client-{name}"));
    let remote = c.server.workspace.join(format!("server-{name}"));
    fs::DirBuilder::new().mode(0o700).create(&local)?;
    fs::DirBuilder::new().mode(0o700).create(&remote)?;
    let (source, target) = if put {
        (local.join("source"), remote.join("data"))
    } else {
        (remote.join("source"), local.join("data"))
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&source)?;
    file.write_all(&bytes())?;
    let times = [libc::timespec {
        tv_sec: 1_700_000_000,
        tv_nsec: 0,
    }; 2];
    use std::os::fd::AsRawFd;
    if unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    file.sync_all()?;
    Ok((source, target))
}
fn arguments(
    c: &Cohort<'_, '_>,
    source: &Path,
    target: &Path,
    put: bool,
    resume: Option<&str>,
) -> Result<Vec<OsString>> {
    let mut args = matrix::base(
        c.client.workspace,
        "client",
        c.client.socket,
        &c.context.relays,
    );
    args.extend([
        "--no-autostart".into(),
        if put { "put" } else { "get" }.into(),
        "--preserve-mode".into(),
        "--preserve-mtime".into(),
    ]);
    if put {
        args.push("--compress".into());
    }
    if let Some(id) = resume {
        args.extend(["--resume".into(), id.into()]);
    }
    args.extend(copy_operands(&c.ready.server_account, source, target)?);
    Ok(args)
}
fn busy_ids(stderr: &[u8], transfer: &str, old: &str) -> Result<String> {
    let end = stderr.iter().position(|b| *b == b'\n').ok_or_else(bad)? + 1;
    let (stream, id) = copy_detach::start_line(&stderr[..end])?;
    if id != transfer
        || stream == old
        || &stderr[end..] != b"marmish copy: transport codec zlib\nmarmish: copy: transfer_busy\n"
    {
        return Err(bad());
    }
    Ok(stream)
}
async fn busy(
    c: &Cohort<'_, '_>,
    source: &Path,
    target: &Path,
    transfer: &str,
    old: &str,
) -> Result<Binding> {
    let out = process::run(
        &c.client.workspace.join("bin/marmish"),
        &arguments(c, source, target, true, Some(transfer))?,
        c.client.workspace,
        c.client.environment,
        c.deadline,
        65536,
    )
    .await?;
    if out.status.code() != Some(1) || !out.stdout.is_empty() {
        return Err(bad());
    }
    let stream = busy_ids(&out.stderr, transfer, old)?;
    let e = terminal(c, &stream, false).await?;
    if !matches!(e.terminal, Some(Terminal::Exited { code }) if code != 0) {
        return Err(bad());
    }
    binding(&e)
}
/// All paths and handles are created by this cohort. A receipt never selects an
/// executable, filesystem root, process ID or signaling target.
pub(super) async fn run(c: Cohort<'_, '_>, put: bool) -> Result<Evidence> {
    let start = Instant::now();
    let (source, target) = roots(&c, put)?;
    let receiver = target.parent().ok_or_else(bad)?.canonicalize()?;
    let digest: [u8; 32] = Sha256::digest(bytes()).into();
    let make_launch = |transfer: &str, mode| -> Result<Launch> {
        Ok(Launch {
            version: VERSION,
            mode,
            transfer: decode(transfer)?,
            client: decode(&c.identities[c.role].client_account)?,
            server: decode(&c.identities[c.role].server_account)?,
            direction: if put { Direction::Put } else { Direction::Get },
            source: source.as_os_str().as_bytes().into(),
            destination: target.as_os_str().as_bytes().into(),
            overwrite: false,
            preserve_mode: true,
            preserve_mtime: true,
        })
    };
    let detached = copy_detach::run(
        &c.client.workspace.join("bin/marmish"),
        &arguments(&c, &source, &target, put, None)?,
        c.client.workspace,
        c.client.environment,
        c.deadline,
        if put {
            marmish_stream::codec::Codec::Zlib
        } else {
            marmish_stream::codec::Codec::None
        },
        |stream, transfer| {
            let launch = make_launch(transfer, AttemptMode::Create)?;
            copy_checkpoint::observe(copy_checkpoint::Expected {
                parent: &receiver,
                launch: &launch,
                stream: decode(stream)?,
                size: SIZE as u64,
                sha256: digest,
                committed: false,
            })
        },
    )
    .await?;
    let old = charged(&c, &detached.stream).await?;
    let put_busy = if put {
        Some(busy(&c, &source, &target, &detached.transfer, &detached.stream).await?)
    } else {
        None
    };
    let cancelled = cleanup::stream_request(
        *c.server.daemon.lock().await,
        c.server.socket,
        StreamLocalRequest::AdminCancel {
            stream: detached.stream.clone(),
        },
        c.deadline,
    )
    .await?;
    if matches!(cancelled, StreamLocalResult::Failure { .. }) {
        return Err(bad());
    }
    terminal(&c, &detached.stream, true).await?;
    let after_cancel = loop {
        let status =
            streaming_cleanup::status(*c.server.daemon.lock().await, c.server.socket, c.deadline)
                .await?;
        if status.validate_idle().is_ok() {
            break status;
        }
        if Instant::now() >= c.deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep_until(c.deadline.min(Instant::now() + Duration::from_millis(100))).await;
    };
    // The first live observation proves interruption opportunity, but may lag
    // later writes. Re-sample only after the old helper is physically stopped;
    // this stable prefix is the exact logical payload omitted by the resume.
    let launch = make_launch(&detached.transfer, AttemptMode::Create)?;
    let resume_prefix = copy_checkpoint::observe(copy_checkpoint::Expected {
        parent: &receiver,
        launch: &launch,
        stream: decode(&detached.stream)?,
        size: SIZE as u64,
        sha256: digest,
        committed: false,
    })?;
    let resume_start = Instant::now();
    let result = process::run(
        &c.client.workspace.join("bin/marmish"),
        &arguments(&c, &source, &target, put, Some(&detached.transfer))?,
        c.client.workspace,
        c.client.environment,
        c.deadline,
        65536,
    )
    .await?;
    if !result.status.success() || !result.stdout.is_empty() {
        return Err(bad());
    }
    let resume_wall_ns = u64::try_from(resume_start.elapsed().as_nanos()).map_err(|_| bad())?;
    let (stream, transfer) = transfer_cases::copy_ids_codec(
        &result.stderr,
        if put {
            marmish_stream::codec::Codec::Zlib
        } else {
            marmish_stream::codec::Codec::None
        },
    )?;
    if transfer != detached.transfer || stream == detached.stream {
        return Err(bad());
    }
    let resumed = terminal(&c, &stream, false).await?;
    let launch = make_launch(&transfer, AttemptMode::Resume)?;
    let committed = copy_checkpoint::observe(copy_checkpoint::Expected {
        parent: &receiver,
        launch: &launch,
        stream: decode(&stream)?,
        size: SIZE as u64,
        sha256: digest,
        committed: true,
    })?;
    let metadata = fs::symlink_metadata(&target)?;
    let evidence = Evidence {
        direction: if put { "put" } else { "get" }.into(),
        wall_ns: u64::try_from(start.elapsed().as_nanos()).map_err(|_| bad())?,
        resume_wall_ns,
        partial: detached.partial,
        resume_prefix,
        old: binding(&old)?,
        old_charged_while_client_detached: true,
        put_busy,
        old_cancelled: true,
        after_cancel,
        resumed: binding(&resumed)?,
        committed,
        final_sha256: records::digest(&records::read_bytes(&target, SIZE as u64)?),
        final_size: metadata.len(),
        final_mode: metadata.mode() & 0o7777,
        final_mtime: metadata.mtime(),
    };
    evidence.validate()?;
    Ok(evidence)
}

pub(super) async fn confirm_server(
    evidence: &Evidence,
    endpoint: &Endpoint<'_>,
    client: &str,
    server: &str,
    deadline: Instant,
) -> Result<()> {
    evidence.validate()?;
    let entries =
        streaming_cleanup::entries(*endpoint.daemon.lock().await, endpoint.socket, deadline)
            .await?;
    for (expected, cancelled, success) in [
        Some((&evidence.old, true, false)),
        Some((&evidence.resumed, false, true)),
        evidence.put_busy.as_ref().map(|b| (b, false, false)),
    ]
    .into_iter()
    .flatten()
    {
        let entry = entries
            .iter()
            .find(|e| records::digest(e.stream.as_bytes()) == expected.stream_sha256)
            .ok_or_else(bad)?;
        if entry.direction != "server"
            || entry.client != client
            || entry.server != server
            || entry.active
            || entry.state != "terminal"
            || entry.cancel_requested != cancelled
            || records::canonical(&binding(entry)?)? != records::canonical(expected)?
        {
            return Err(bad());
        }
        match (&entry.terminal, cancelled, success) {
            (Some(Terminal::Cancelled), true, _) => {}
            (Some(Terminal::Exited { code: 0 }), false, true) => {}
            (Some(Terminal::Exited { code }), false, false) if *code != 0 => {}
            _ => return Err(bad()),
        }
    }
    streaming_cleanup::status(*endpoint.daemon.lock().await, endpoint.socket, deadline)
        .await?
        .validate_idle()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_requires_exact_failure_and_fresh_stream_on_same_transfer() {
        let transfer = "a".repeat(64);
        let old = "b".repeat(32);
        let fresh = "c".repeat(32);
        let stderr = format!(
            "marmish copy: transfer {transfer} stream {fresh}\nmarmish copy: transport codec zlib\nmarmish: copy: transfer_busy\n"
        );
        assert_eq!(busy_ids(stderr.as_bytes(), &transfer, &old).unwrap(), fresh);
        assert!(busy_ids(stderr.as_bytes(), &"d".repeat(64), &old).is_err());
        assert!(busy_ids(stderr.as_bytes(), &transfer, &fresh).is_err());
        assert!(
            busy_ids(
                stderr
                    .replace("transfer_busy", "transfer_conflict")
                    .as_bytes(),
                &transfer,
                &old
            )
            .is_err()
        );
        assert!(busy_ids(format!("{stderr}extra\n").as_bytes(), &transfer, &old).is_err());
    }
}

#[cfg(test)]
mod public_destination_tests {
    use super::*;
    #[test]
    fn public_destination_builder_obeys_product_contract() {
        let account = "ab".repeat(32);
        use std::os::unix::ffi::OsStringExt;
        let source = std::path::PathBuf::from(OsString::from_vec(b"source \xff\n--".to_vec()));
        let target = Path::new("target '雪\n--");
        let args = copy_operands(&account, &source, target).unwrap();
        records::assert_public_target(&args[1], &account);
        assert_eq!(args[0], "--");
        assert_eq!(args[2], source.as_os_str());
        assert_eq!(args[3], target.as_os_str());
        assert!(copy_operands("bad", &source, target).is_err());
    }
}
