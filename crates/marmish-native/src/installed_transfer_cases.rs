//! Two positive transfers per directed cell, using installed product commands only.
use super::streaming_cases::fixture_args;
use super::{
    matrix, process,
    records::{self, Context, IdentityReady},
    streaming_cases::Endpoint,
    streaming_cleanup, streaming_prerequisites,
    streaming_records::{self, ControlProof, Pair, ServerReady, TransferProof},
    transfer_fixture,
};
use crate::{Error, Result};
use std::{collections::BTreeMap, ffi::OsString};
use tokio::time::Instant;

fn copy_target(account: &str) -> Result<Vec<OsString>> {
    Ok(vec![
        "--".into(),
        records::public_destination(account)?.into(),
    ])
}

fn remote_operand(account: &str, path: &str) -> Result<String> {
    Ok(format!("{}:{path}/", records::public_destination(account)?))
}

fn bad() -> Error {
    Error::Invalid("installed_transfer_case")
}
struct Cell<'a, 'b> {
    ctx: &'a Context,
    ids: &'a BTreeMap<String, IdentityReady>,
    client: &'a str,
    server: &'a str,
    ready: &'a ServerReady,
    ep: &'a Endpoint<'b>,
    until: Instant,
}
struct Observation {
    entry: streaming_cleanup::Entry,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit: u8,
    wall_ns: u64,
    transfer_id: Option<String>,
}
pub(super) fn copy_ids_codec(
    stderr: &[u8],
    codec: marmish_stream::codec::Codec,
) -> Result<(String, String)> {
    let text = std::str::from_utf8(stderr).map_err(|_| bad())?;
    let mut lines = text.lines();
    let line = lines
        .next()
        .and_then(|l| l.strip_prefix("marmish copy: transfer "))
        .ok_or_else(bad)?;
    let (transfer, stream) = line.split_once(" stream ").ok_or_else(bad)?;
    let codec_name = match codec {
        marmish_stream::codec::Codec::None => "none",
        marmish_stream::codec::Codec::Zlib => "zlib",
    };
    if !records::hexadecimal(transfer, 64)
        || !records::hexadecimal(stream, 32)
        || lines.next() != Some(format!("marmish copy: transport codec {codec_name}").as_str())
        || lines.next()
            != Some(format!("marmish copy: committed and verified transfer {transfer}").as_str())
        || lines.next().is_some()
    {
        return Err(bad());
    }
    Ok((stream.into(), transfer.into()))
}

async fn observed(
    ep: &Endpoint<'_>,
    program: &std::path::Path,
    args: &[OsString],
    until: Instant,
    copy: bool,
) -> Result<Observation> {
    let start = Instant::now();
    let out = process::run(program, args, ep.workspace, ep.environment, until, 65536).await?;
    let (id, stderr, transfer_id) = if copy {
        let codec = if args.iter().any(|arg| arg == "--compress") {
            marmish_stream::codec::Codec::Zlib
        } else {
            marmish_stream::codec::Codec::None
        };
        let (id, transfer) = copy_ids_codec(&out.stderr, codec)?;
        (id, &[][..], Some(transfer))
    } else {
        let (id, stderr) = super::streaming_cases::stderr(&out.stderr)?;
        (id, stderr, None)
    };
    let entry = streaming_cleanup::entries(*ep.daemon.lock().await, ep.socket, until)
        .await?
        .into_iter()
        .find(|e| e.stream == id)
        .ok_or_else(bad)?;
    let exit = out
        .status
        .code()
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(bad)?;
    if exit != 0
        || entry.direction != "client"
        || entry.terminal != Some(marmish_stream::wire::Terminal::Exited { code: 0 })
    {
        return Err(bad());
    }
    Ok(Observation {
        entry,
        stdout: out.stdout,
        stderr: stderr.to_vec(),
        exit,
        wall_ns: u64::try_from(start.elapsed().as_nanos()).map_err(|_| bad())?,
        transfer_id,
    })
}
async fn fixture(
    cell: &Cell<'_, '_>,
    action: &str,
    nonce: &str,
    case: &str,
) -> Result<(Option<transfer_fixture::Receipt>, ControlProof)> {
    fixture_checkpoint(cell, action, nonce, case, None).await
}
async fn fixture_checkpoint(
    cell: &Cell<'_, '_>,
    action: &str,
    nonce: &str,
    case: &str,
    checkpoint: Option<(&marmish_stream::copy_protocol::Launch, [u8; 16])>,
) -> Result<(Option<transfer_fixture::Receipt>, ControlProof)> {
    let ep = cell.ep;
    let mut args = matrix::base(ep.workspace, "client", ep.socket, &cell.ctx.relays);
    args.extend(fixture_args(&cell.ready.server_account)?);
    args.extend(match checkpoint {
        Some((launch, stream)) => {
            transfer_fixture::checkpoint_arguments(nonce, cell.client, launch, stream)?
        }
        None => transfer_fixture::arguments(action, nonce, cell.client, case)?,
    });
    let out = observed(
        ep,
        &ep.workspace.join("bin/marmish"),
        &args,
        cell.until,
        false,
    )
    .await?;
    if !out.stderr.is_empty() {
        return Err(bad());
    }
    let control = ControlProof {
        action: action.into(),
        stream_sha256: records::digest(out.entry.stream.as_bytes()),
        session_sha256: records::digest(out.entry.session.as_bytes()),
        open_digest: out.entry.open_digest.ok_or_else(bad)?,
        group_sha256: out.entry.group_sha256,
        stdout_sha256: records::digest(&out.stdout),
    };
    if action == "absent" {
        if !out.stdout.is_empty() {
            return Err(bad());
        }
        return Ok((None, control));
    }
    let receipt: transfer_fixture::Receipt =
        serde_json::from_slice(&out.stdout).map_err(|_| bad())?;
    if receipt.schema != 1
        || receipt.nonce != nonce
        || receipt.client != cell.client
        || receipt.case != case
        || receipt.items != transfer_fixture::expected(nonce, case)?
    {
        return Err(bad());
    }
    Ok((Some(receipt), control))
}
pub(super) fn rsh(ep: &Endpoint<'_>, ctx: &Context, compress: bool) -> Result<OsString> {
    let mut args = vec![ep.workspace.join("bin/marmish").into_os_string()];
    args.extend(matrix::base(ep.workspace, "client", ep.socket, &ctx.relays));
    args.extend(["--no-autostart".into(), "rsh".into()]);
    if compress {
        args.push("--compress".into());
    }
    args.into_iter()
        .map(|a| {
            let value = a.to_str().ok_or_else(bad)?;
            if value.contains(['\n', '\r', '\0']) {
                return Err(bad());
            }
            Ok(format!("'{}'", value.replace('\'', "'\\''")))
        })
        .collect::<Result<Vec<_>>>()
        .map(|a| a.join(" ").into())
}
fn make_pair(
    cell: &Cell<'_, '_>,
    case: &str,
    nonce: String,
    out: Observation,
    receipt: transfer_fixture::Receipt,
    evidence: (Vec<transfer_fixture::Item>, Vec<ControlProof>),
) -> Result<Pair> {
    let Cell {
        ctx,
        ids,
        client,
        server,
        ready,
        ..
    } = cell;
    if out.entry.client != ids[*client].client_account
        || out.entry.server != ids[*server].server_account
        || receipt.executable_sha256 != ready.fixture_sha256
    {
        return Err(bad());
    }
    let payload_bytes = receipt.items.iter().map(|i| i.bytes).sum();
    let p = Pair {
        client: (*client).into(),
        server: (*server).into(),
        case: case.into(),
        nonce,
        stream: records::digest(out.entry.stream.as_bytes()),
        session: records::digest(out.entry.session.as_bytes()),
        open_digest: out.entry.open_digest.ok_or_else(bad)?,
        group_sha256: out.entry.group_sha256,
        stdin_bytes: 0,
        stdin_sha256: records::digest(&[]),
        stdout_bytes: out.stdout.len(),
        stdout_sha256: records::digest(&out.stdout),
        stderr_bytes: out.stderr.len(),
        stderr_sha256: records::digest(&out.stderr),
        exit: out.exit,
        wall_ns: out.wall_ns,
        complete: true,
        failure: None,
        transfer: Some(TransferProof {
            client_items: evidence.0,
            controls: evidence.1,
            items: receipt.items,
            payload_bytes,
            rsync: ready.rsync.clone(),
            transfer_id: out.transfer_id.map(|id| records::digest(id.as_bytes())),
            checkpoint: receipt.checkpoint,
        }),
    };
    if ctx.cohort != "copy-matrix" {
        p.validate(ctx, ids)?;
    }
    Ok(p)
}
pub(super) async fn rsync(
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    client: &str,
    server: &str,
    ready: &ServerReady,
    ep: &Endpoint<'_>,
    until: Instant,
) -> Result<Vec<Pair>> {
    let cell = Cell {
        ctx,
        ids,
        client,
        server,
        ready,
        ep,
        until,
    };
    let local = streaming_prerequisites::rsync(ctx, ep.workspace)?.ok_or_else(bad)?;
    let remote = ready.rsync.as_ref().ok_or_else(bad)?;
    let root = ep.workspace.join("local-transfers");
    match std::fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let mut pairs = Vec::new();
    for case in ["push", "pull"] {
        let nonce = streaming_records::nonce(ctx, ids, client, server, case)?;
        let (_, precondition) = fixture(
            &cell,
            if case == "push" { "absent" } else { "init" },
            &nonce,
            case,
        )
        .await?;
        let localroot = root.join(format!("{case}-{nonce}"));
        if case == "push" {
            transfer_fixture::create(&localroot, &nonce, case)?;
        } else {
            // A pre-existing identical tree cannot prove this pull transferred bytes.
            match std::fs::symlink_metadata(&localroot) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(_) => return Err(bad()),
            }
        }
        let remote_path = remote_operand(
            &ready.server_account,
            transfer_fixture::target(&nonce, case)?
                .to_str()
                .ok_or_else(bad)?,
        )?;
        let local_path: OsString = format!("{}/", localroot.to_str().ok_or_else(bad)?).into();
        let mut args = vec![
            "-as".into(),
            "--rsync-path".into(),
            remote.path.clone().into(),
            "-e".into(),
            rsh(ep, ctx, case == "push")?,
            "--".into(),
        ];
        if case == "push" {
            args.extend([local_path, remote_path.into()]);
        } else {
            args.extend([remote_path.into(), local_path]);
        }
        let out = observed(ep, std::path::Path::new(&local.path), &args, until, false).await?;
        let local_items = transfer_fixture::inventory(&localroot)?;
        if local_items != transfer_fixture::expected(&nonce, case)? {
            return Err(bad());
        }
        let (receipt, observation) = fixture(&cell, "observe", &nonce, case).await?;
        let receipt = receipt.ok_or_else(bad)?;
        pairs.push(make_pair(
            &cell,
            case,
            nonce,
            out,
            receipt,
            (local_items, vec![precondition, observation]),
        )?);
    }
    Ok(pairs)
}
use std::os::unix::fs::DirBuilderExt;

pub(super) async fn copy(
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    client: &str,
    server: &str,
    ready: &ServerReady,
    ep: &Endpoint<'_>,
    until: Instant,
) -> Result<Vec<Pair>> {
    let cell = Cell {
        ctx,
        ids,
        client,
        server,
        ready,
        ep,
        until,
    };
    let root = ep.workspace.join("local-transfers");
    match std::fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let mut pairs = Vec::new();
    for case in ["put", "get"] {
        let nonce = streaming_records::nonce(ctx, ids, client, server, case)?;
        let (_, precondition) = fixture(
            &cell,
            if case == "put" { "absent" } else { "init" },
            &nonce,
            case,
        )
        .await?;
        let localroot = root.join(format!("{case}-{nonce}"));
        if case == "put" {
            transfer_fixture::create(&localroot, &nonce, case)?;
        } else {
            std::fs::DirBuilder::new().mode(0o700).create(&localroot)?;
        }
        let localpath = localroot.join("data").into_os_string();
        let remotepath = transfer_fixture::target(&nonce, case)?
            .join("data")
            .into_os_string();
        let mut args = matrix::base(ep.workspace, "client", ep.socket, &ctx.relays);
        args.extend([
            "--no-autostart".into(),
            case.into(),
            "--preserve-mode".into(),
            "--preserve-mtime".into(),
        ]);
        if case == "put" {
            args.push("--compress".into());
        }
        args.extend(copy_target(&ready.server_account)?);
        if case == "put" {
            args.extend([localpath.clone(), remotepath.clone()]);
        } else {
            args.extend([remotepath.clone(), localpath.clone()]);
        }
        let out = observed(ep, &ep.workspace.join("bin/marmish"), &args, until, true).await?;
        use marmish_stream::copy_protocol::{AttemptMode, Direction, Launch, VERSION};
        use std::os::unix::ffi::OsStrExt;
        fn array<const N: usize>(hexadecimal: &str) -> Result<[u8; N]> {
            hex::decode(hexadecimal)
                .map_err(|_| bad())?
                .try_into()
                .map_err(|_| bad())
        }
        let launch = Launch {
            version: VERSION,
            mode: AttemptMode::Create,
            transfer: array(out.transfer_id.as_deref().ok_or_else(bad)?)?,
            client: array(&ids[client].client_account)?,
            server: array(&ids[server].server_account)?,
            direction: if case == "put" {
                Direction::Put
            } else {
                Direction::Get
            },
            source: if case == "put" {
                localpath.as_bytes()
            } else {
                remotepath.as_bytes()
            }
            .into(),
            destination: if case == "put" {
                remotepath.as_bytes()
            } else {
                localpath.as_bytes()
            }
            .into(),
            overwrite: false,
            preserve_mode: true,
            preserve_mtime: true,
        };
        let stream = array(&out.entry.stream)?;
        let (local_items, local_checkpoint) = if case == "get" {
            let expected = transfer_fixture::expected(&nonce, case)?;
            let (proof, items) =
                super::copy_checkpoint::inventory(super::copy_checkpoint::Expected {
                    parent: &localroot.canonicalize()?,
                    launch: &launch,
                    stream,
                    size: expected[0].bytes,
                    sha256: array(expected[0].sha256.as_deref().ok_or_else(bad)?)?,
                    committed: true,
                })?;
            (items, Some(proof))
        } else {
            (transfer_fixture::inventory(&localroot)?, None)
        };
        if !out.stdout.is_empty() || local_items != transfer_fixture::expected(&nonce, case)? {
            return Err(bad());
        }
        let (receipt, observation) = fixture_checkpoint(
            &cell,
            "observe",
            &nonce,
            case,
            if case == "put" {
                Some((&launch, stream))
            } else {
                None
            },
        )
        .await?;
        let receipt = receipt.ok_or_else(bad)?;
        let mut pair = make_pair(
            &cell,
            case,
            nonce,
            out,
            receipt,
            (local_items, vec![precondition, observation]),
        )?;
        if case == "get" {
            pair.transfer.as_mut().ok_or_else(bad)?.checkpoint = local_checkpoint;
        }
        pair.validate(ctx, ids)?;
        pairs.push(pair);
    }
    Ok(pairs)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_success_requires_bound_transfer_commit_and_single_stream() {
        let transfer = "ab".repeat(32);
        let stream = "cd".repeat(16);
        let text = format!(
            "marmish copy: transfer {transfer} stream {stream}\nmarmish copy: transport codec none\nmarmish copy: committed and verified transfer {transfer}\n"
        );
        assert!(copy_ids_codec(text.as_bytes(), marmish_stream::codec::Codec::Zlib).is_err());
        assert_eq!(
            copy_ids_codec(text.as_bytes(), marmish_stream::codec::Codec::None).unwrap(),
            (stream, transfer.clone())
        );
        assert!(
            copy_ids_codec(
                text.replace(
                    &format!("verified transfer {transfer}"),
                    &format!("verified transfer {}", "12".repeat(32))
                )
                .as_bytes(),
                marmish_stream::codec::Codec::None
            )
            .is_err()
        );
        assert!(
            copy_ids_codec(
                text.lines().next().unwrap().as_bytes(),
                marmish_stream::codec::Codec::None
            )
            .is_err()
        );
        assert!(
            copy_ids_codec(
                format!("{text}marmish copy: uncertain\n").as_bytes(),
                marmish_stream::codec::Codec::None
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod public_destination_tests {
    use super::*;
    #[test]
    fn public_destination_builder_obeys_product_contract() {
        let account = "ab".repeat(32);
        let fixture = fixture_args(&account).unwrap();
        records::assert_public_target(&fixture[2], &account);
        assert_eq!(fixture[3..], ["--", "./fixture/main"]);
        let copy = copy_target(&account).unwrap();
        records::assert_public_target(&copy[1], &account);
        assert_eq!(copy[0], "--");
        let remote = remote_operand(&account, "literal space/'quote\n雪").unwrap();
        let (target, path) = remote.split_once(':').unwrap();
        records::assert_public_target(target.as_ref(), &account);
        assert_eq!(path, "literal space/'quote\n雪/");
        assert!(remote_operand("bad", "path").is_err());
    }
}
