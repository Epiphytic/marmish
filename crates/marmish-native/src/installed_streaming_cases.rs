//! Workloads invoke the installed CLI; IPC is used only for owned local evidence.
use super::{
    matrix,
    process::{self, Environment},
    records::{self, Context, IdentityReady},
    streaming_cleanup, streaming_fixture,
    streaming_records::{self, Pair, ServerReady},
};
use crate::{Error, Result};
use std::{collections::BTreeMap, ffi::OsString, path::Path, time::Duration};
use tokio::{process::Child, time::Instant};
fn bad() -> Error {
    Error::Invalid("installed_stream_case")
}
pub(super) struct Endpoint<'a> {
    pub daemon: std::sync::Arc<tokio::sync::Mutex<&'a mut Child>>,
    pub socket: &'a Path,
    pub environment: &'a Environment,
    pub workspace: &'a Path,
}
pub(super) fn stderr(stderr: &[u8]) -> Result<(String, &[u8])> {
    let rest = stderr.strip_prefix(b"marmish stream: ").ok_or_else(bad)?;
    if rest.len() < 33 || rest[32] != b'\n' {
        return Err(bad());
    }
    let stream = std::str::from_utf8(&rest[..32]).map_err(|_| bad())?;
    if !records::hexadecimal(stream, 32) {
        return Err(bad());
    }
    Ok((stream.into(), &rest[33..]))
}
pub(super) async fn duplex(
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    role: &str,
    server: &str,
    ready: &ServerReady,
    ep: &Endpoint<'_>,
    deadline: Instant,
) -> Result<Pair> {
    let nonce = streaming_records::nonce(ctx, ids, role, server, "duplex")?;
    let mut args = matrix::base(ep.workspace, "client", ep.socket, &ctx.relays);
    args.extend([
        "--no-autostart".into(),
        "stream".into(),
        ready.server_account.clone().into(),
    ]);
    if role <= server {
        args.push("--compress".into());
    }
    args.extend([OsString::from("--"), "./fixture/main".into()]);
    args.extend(streaming_fixture::arguments(&nonce, role)?);
    let input = streaming_fixture::bytes(&nonce, 7, streaming_fixture::STDIN_BYTES)?;
    let start = Instant::now();
    let output = process::run_input(
        &ep.workspace.join("bin/marmish"),
        &args,
        ep.workspace,
        ep.environment,
        deadline,
        1024 * 1024,
        Some(&input),
    )
    .await?;
    let (stream, stderr) = stderr(&output.stderr)?;
    let entry = streaming_cleanup::entries(*ep.daemon.lock().await, ep.socket, deadline)
        .await?
        .into_iter()
        .find(|e| e.stream == stream)
        .ok_or_else(bad)?;
    if entry.direction != "client"
        || entry.client != ids[role].client_account
        || entry.server != ids[server].server_account
        || entry.terminal != Some(marmish_stream::wire::Terminal::Exited { code: 7 })
    {
        return Err(bad());
    }
    let pair = Pair {
        client: role.into(),
        server: server.into(),
        case: "duplex".into(),
        nonce,
        stream: records::digest(stream.as_bytes()),
        session: records::digest(entry.session.as_bytes()),
        open_digest: entry.open_digest.ok_or_else(bad)?,
        group_sha256: entry.group_sha256,
        stdin_bytes: input.len(),
        stdin_sha256: records::digest(&input),
        stdout_bytes: output.stdout.len(),
        stdout_sha256: records::digest(&output.stdout),
        stderr_bytes: stderr.len(),
        stderr_sha256: records::digest(stderr),
        exit: output
            .status
            .code()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(bad)?,
        wall_ns: u64::try_from(start.elapsed().as_nanos()).map_err(|_| bad())?,
        complete: true,
        failure: None,
        transfer: None,
    };
    pair.validate_duplex(ctx, ids)?;
    Ok(pair)
}
pub(super) fn cell_deadline(overall: Instant) -> Instant {
    overall.min(Instant::now() + Duration::from_secs(330))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_id_parser_preserves_binary_stderr_exactly() {
        let bytes = [
            b"marmish stream: ".as_slice(),
            "a".repeat(32).as_bytes(),
            b"\n\x00\xff\n",
        ]
        .concat();
        let (id, payload) = stderr(&bytes).unwrap();
        assert_eq!(id, "a".repeat(32));
        assert_eq!(payload, b"\x00\xff\n");
        assert!(stderr(b"marmish stream: invalid\n").is_err());
        assert!(stderr(&[b"prefix".as_slice(), &bytes].concat()).is_err());
    }
}
