//! Workloads invoke the installed CLI; IPC is used only for owned local evidence.
use super::{
    matrix,
    process::{self, Environment},
    records::{self, Context, IdentityReady},
    streaming_cleanup, streaming_fixture,
    streaming_observation::{
        self as observation, Evidence, Outcome, PrefixFailure, RemoteDiagnostic, Stage, Trace,
    },
    streaming_records::{self, Pair, ServerReady},
};
use crate::{Error, Result};
use std::{collections::BTreeMap, ffi::OsString, path::Path, time::Duration};
use tokio::{process::Child, time::Instant};

fn stream_args(account: &str) -> Result<Vec<OsString>> {
    Ok(vec![
        "--no-autostart".into(),
        "stream".into(),
        records::public_destination(account)?.into(),
    ])
}

pub(super) fn hold_args(account: &str, nonce: &str, role: &str) -> Result<Vec<OsString>> {
    Ok(vec![
        "--no-autostart".into(),
        "stream".into(),
        records::public_destination(account)?.into(),
        "--".into(),
        "./fixture/main".into(),
        "edge-v1".into(),
        "hold".into(),
        nonce.into(),
        role.into(),
    ])
}

pub(super) fn cancel_args(account: &str, stream: &str) -> Result<Vec<OsString>> {
    Ok(vec![
        "--no-autostart".into(),
        "cancel".into(),
        "--stream".into(),
        stream.into(),
        records::public_destination(account)?.into(),
    ])
}

pub(super) fn fixture_args(account: &str) -> Result<Vec<OsString>> {
    Ok(vec![
        "--no-autostart".into(),
        "stream".into(),
        records::public_destination(account)?.into(),
        "--".into(),
        "./fixture/main".into(),
    ])
}

fn bad() -> Error {
    Error::Invalid("installed_stream_case")
}
pub(super) struct Endpoint<'a> {
    pub daemon: std::sync::Arc<tokio::sync::Mutex<&'a mut Child>>,
    pub socket: &'a Path,
    pub environment: &'a Environment,
    pub workspace: &'a Path,
}
fn parse_prefix(stderr: &[u8]) -> std::result::Result<(&str, &[u8]), PrefixFailure> {
    let rest = stderr
        .strip_prefix(b"marmish stream: ")
        .ok_or(PrefixFailure::MissingPrefix)?;
    if rest.len() < 33 {
        return Err(PrefixFailure::Short);
    }
    if rest[32] != b'\n' {
        return Err(PrefixFailure::Newline);
    }
    let stream = std::str::from_utf8(&rest[..32]).map_err(|_| PrefixFailure::Utf8)?;
    if !records::hexadecimal(stream, 32) {
        return Err(PrefixFailure::Hex);
    }
    Ok((stream, &rest[33..]))
}
pub(super) fn stderr(stderr: &[u8]) -> Result<(String, &[u8])> {
    parse_prefix(stderr)
        .map(|(stream, payload)| (stream.into(), payload))
        .map_err(|_| bad())
}
fn remote_diagnostic(stderr: &[u8]) -> RemoteDiagnostic {
    use marmish_stream::wire::WireErrorCode as Code;
    let mut found = None;
    // split_inclusive ensures only complete lines are inspected, even in binary payloads.
    for line in stderr.split_inclusive(|byte| *byte == b'\n') {
        let Some(line) = line.strip_suffix(b"\n") else {
            continue;
        };
        let Some(code) = line
            .strip_prefix(b"marmish stream: remote error ")
            .or_else(|| line.strip_prefix(b"marmish stream: remote failure "))
        else {
            continue;
        };
        let code = match code {
            b"Unsupported" => Code::Unsupported,
            b"Invalid" => Code::Invalid,
            b"Denied" => Code::Denied,
            b"Conflict" => Code::Conflict,
            b"Gap" => Code::Gap,
            b"Expired" => Code::Expired,
            b"Capacity" => Code::Capacity,
            b"Uncertain" => Code::Uncertain,
            b"NotFound" => Code::NotFound,
            b"Closed" => Code::Closed,
            _ => continue,
        };
        if found.is_some_and(|previous| previous != code) {
            return RemoteDiagnostic::Ambiguous;
        }
        found = Some(code);
    }
    found.map_or(RemoteDiagnostic::None, RemoteDiagnostic::Code)
}
fn observed_stderr<'a>(bytes: &'a [u8], trace: Option<&Trace<'_>>) -> Result<(String, &'a [u8])> {
    observation::note(
        trace,
        Stage::RemoteDiagnostic,
        Outcome::Ok,
        Some(Evidence::Remote {
            diagnostic: remote_diagnostic(bytes),
        }),
    );
    observation::note(trace, Stage::Prefix, Outcome::Begin, None);
    match parse_prefix(bytes) {
        Ok((stream, payload)) => {
            observation::note(
                trace,
                Stage::Prefix,
                Outcome::Ok,
                Some(Evidence::Payload {
                    stderr_bytes: payload.len(),
                    stderr_sha256: records::digest(payload),
                }),
            );
            Ok((stream.into(), payload))
        }
        Err(failure) => {
            observation::note(
                trace,
                Stage::Prefix,
                Outcome::Error,
                Some(Evidence::Prefix { failure }),
            );
            Err(bad())
        }
    }
}
fn list_result<T>(result: Result<T>, trace: Option<&Trace<'_>>) -> Result<T> {
    observation::note(
        trace,
        Stage::ListRequest,
        if result.is_ok() {
            Outcome::Ok
        } else {
            Outcome::Error
        },
        None,
    );
    result
}
fn select_entry(
    entries: Vec<streaming_cleanup::Entry>,
    stream: &str,
    expected: (&str, &str),
    trace: Option<&Trace<'_>>,
) -> Result<streaming_cleanup::Entry> {
    observation::note(trace, Stage::Entry, Outcome::Begin, None);
    let entry = entries
        .into_iter()
        .find(|entry| entry.stream == stream)
        .ok_or_else(|| {
            observation::note(trace, Stage::Entry, Outcome::Error, None);
            bad()
        })?;
    observation::note(trace, Stage::Entry, Outcome::Ok, None);
    observation::note(trace, Stage::Binding, Outcome::Begin, None);
    let direction_ok = entry.direction == "client";
    let client_ok = entry.client == expected.0;
    let server_ok = entry.server == expected.1;
    let valid = direction_ok
        && client_ok
        && server_ok
        && entry.terminal == Some(marmish_stream::wire::Terminal::Exited { code: 7 });
    observation::note(
        trace,
        Stage::Binding,
        if valid { Outcome::Ok } else { Outcome::Error },
        Some(Evidence::Binding {
            direction_ok,
            client_ok,
            server_ok,
            active: entry.active,
            terminal: entry.terminal,
        }),
    );
    if !valid {
        return Err(bad());
    }
    Ok(entry)
}
fn construction(
    open: Option<String>,
    exit: Option<i32>,
    elapsed: impl FnOnce() -> u128,
    trace: Option<&Trace<'_>>,
) -> Result<(String, u8, u64)> {
    observation::note(trace, Stage::Construction, Outcome::Begin, None);
    let mut flags = (None, None, None);
    let result = (|| {
        flags.0 = Some(open.is_some());
        let open = open.ok_or_else(bad)?;
        let exit = exit.and_then(|exit| u8::try_from(exit).ok());
        flags.1 = Some(exit.is_some());
        let exit = exit.ok_or_else(bad)?;
        let elapsed = u64::try_from(elapsed());
        flags.2 = Some(elapsed.is_ok());
        let elapsed = elapsed.map_err(|_| bad())?;
        Ok((open, exit, elapsed))
    })();
    observation::note(
        trace,
        Stage::Construction,
        if result.is_ok() {
            Outcome::Ok
        } else {
            Outcome::Error
        },
        Some(Evidence::Construction {
            open_digest_present: flags.0,
            exit_representable: flags.1,
            wall_ns_representable: flags.2,
        }),
    );
    result
}
struct PairParts<'a> {
    roles: (&'a str, &'a str),
    nonce: String,
    stream: String,
    entry: streaming_cleanup::Entry,
    input: &'a [u8],
    output: &'a process::Output,
    stderr: &'a [u8],
}
fn assemble_pair(
    parts: PairParts<'_>,
    elapsed: impl FnOnce() -> u128,
    trace: Option<&Trace<'_>>,
) -> Result<Pair> {
    let stream_digest = records::digest(parts.stream.as_bytes());
    let session_digest = records::digest(parts.entry.session.as_bytes());
    // Match the original construction order: missing Open precedes payload hashing.
    let open = parts.entry.open_digest;
    if open.is_none() {
        construction(None, parts.output.status.code(), || 0, trace)?;
    }
    let stdin_sha256 = records::digest(parts.input);
    let stdout_sha256 = records::digest(&parts.output.stdout);
    let stderr_sha256 = records::digest(parts.stderr);
    let (open_digest, exit, wall_ns) =
        construction(open, parts.output.status.code(), elapsed, trace)?;
    Ok(Pair {
        client: parts.roles.0.into(),
        server: parts.roles.1.into(),
        case: "duplex".into(),
        nonce: parts.nonce,
        stream: stream_digest,
        session: session_digest,
        open_digest,
        group_sha256: parts.entry.group_sha256,
        stdin_bytes: parts.input.len(),
        stdin_sha256,
        stdout_bytes: parts.output.stdout.len(),
        stdout_sha256,
        stderr_bytes: parts.stderr.len(),
        stderr_sha256,
        exit,
        wall_ns,
        complete: true,
        failure: None,
        transfer: None,
    })
}
async fn lock_daemon<'a, 'b>(
    ep: &'a Endpoint<'b>,
    trace: Option<&Trace<'_>>,
) -> tokio::sync::MutexGuard<'a, &'b mut Child> {
    observation::note(trace, Stage::ListLock, Outcome::Begin, None);
    let daemon = ep.daemon.lock().await;
    observation::note(trace, Stage::ListLock, Outcome::Ok, None);
    daemon
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
    let trace = Trace::new(role, server, &observation::stderr_sink);
    observation::note(trace.as_ref(), Stage::Cell, Outcome::Begin, None);
    let result = async {
        let nonce = streaming_records::nonce(ctx, ids, role, server, "duplex")?;
        let mut args = matrix::base(ep.workspace, "client", ep.socket, &ctx.relays);
        args.extend(stream_args(&ready.server_account)?);
        if role <= server {
            args.push("--compress".into());
        }
        args.extend([OsString::from("--"), "./fixture/main".into()]);
        args.extend(streaming_fixture::arguments(&nonce, role)?);
        let input = streaming_fixture::bytes(&nonce, 7, streaming_fixture::STDIN_BYTES)?;
        let start = Instant::now();
        observation::note(trace.as_ref(), Stage::Capture, Outcome::Begin, None);
        let output = process::run_input_observed(
            &ep.workspace.join("bin/marmish"),
            &args,
            ep.workspace,
            ep.environment,
            deadline,
            1024 * 1024,
            (&input, trace.as_ref().ok_or_else(bad)?),
        )
        .await?;
        let (stream, stderr) = observed_stderr(&output.stderr, trace.as_ref())?;
        let mut daemon = lock_daemon(ep, trace.as_ref()).await;
        observation::note(trace.as_ref(), Stage::ListRequest, Outcome::Begin, None);
        let entries = list_result(
            streaming_cleanup::entries(&mut daemon, ep.socket, deadline).await,
            trace.as_ref(),
        )?;
        drop(daemon);
        let entry = select_entry(
            entries,
            &stream,
            (&ids[role].client_account, &ids[server].server_account),
            trace.as_ref(),
        )?;
        let pair = assemble_pair(
            PairParts {
                roles: (role, server),
                nonce,
                stream,
                entry,
                input: &input,
                output: &output,
                stderr,
            },
            || start.elapsed().as_nanos(),
            trace.as_ref(),
        )?;
        observation::note(trace.as_ref(), Stage::PairValidation, Outcome::Begin, None);
        let (checks, valid) = pair.check_duplex(ctx, ids);
        observation::note(
            trace.as_ref(),
            Stage::PairValidation,
            if valid.is_ok() {
                Outcome::Ok
            } else {
                Outcome::Error
            },
            Some(Evidence::Checks { checks }),
        );
        valid?;
        Ok(pair)
    }
    .await;
    if let Some(trace) = trace {
        trace.finish(result.is_ok());
    }
    result
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

#[cfg(test)]
mod public_destination_tests {
    use super::*;
    #[test]
    fn public_destination_builder_obeys_product_contract() {
        let account = "ab".repeat(32);
        let args = stream_args(&account).unwrap();
        records::assert_public_target(&args[2], &account);
        assert_eq!(args[..2], ["--no-autostart", "stream"]);
        assert!(stream_args("bad").is_err());
    }
}

#[cfg(test)]
mod observation_tests {
    use super::*;
    use marmish_stream::wire::{Terminal, WireErrorCode};
    use std::cell::RefCell;
    fn entry() -> streaming_cleanup::Entry {
        streaming_cleanup::Entry {
            stream: "a".repeat(32),
            session: "b".repeat(32),
            client: "c".into(),
            server: "s".into(),
            direction: "client".into(),
            state: "terminal".into(),
            cancel_requested: false,
            terminal: Some(Terminal::Exited { code: 7 }),
            child_disposition: None,
            open_digest: Some("d".repeat(64)),
            group_sha256: "e".repeat(64),
            active: true,
        }
    }
    #[test]
    fn prefix_failure_order_and_informational_wire_scan_preserve_binary_parser() {
        let cases = [
            (b"other".to_vec(), PrefixFailure::MissingPrefix),
            (b"marmish stream: a".to_vec(), PrefixFailure::Short),
            (
                [b"marmish stream: ".as_slice(), &[b'a'; 32], b"x"].concat(),
                PrefixFailure::Newline,
            ),
            (
                [b"marmish stream: ".as_slice(), &[255; 32], b"\n"].concat(),
                PrefixFailure::Utf8,
            ),
            (
                [b"marmish stream: ".as_slice(), &[b'g'; 32], b"\n"].concat(),
                PrefixFailure::Hex,
            ),
        ];
        let failed = |_: &[u8]| Err(std::io::Error::other("SECRET"));
        let trace = Trace::new("linux-amd64", "macos-arm64", &failed).unwrap();
        for (bytes, expected) in cases {
            assert_eq!(parse_prefix(&bytes).unwrap_err(), expected);
            assert!(matches!(
                observed_stderr(&bytes, Some(&trace)),
                Err(Error::Invalid("installed_stream_case"))
            ));
        }
        let payload =
            b"\0\xff\nmarmish stream: remote error Denied\nmarmish stream: remote failure Denied\n";
        let bytes = [b"marmish stream: ".as_slice(), &[b'a'; 32], b"\n", payload].concat();
        assert_eq!(observed_stderr(&bytes, Some(&trace)).unwrap().1, payload);
        assert_eq!(
            remote_diagnostic(&bytes),
            RemoteDiagnostic::Code(WireErrorCode::Denied)
        );
        assert_eq!(
            remote_diagnostic(b"marmish stream: remote error Denied"),
            RemoteDiagnostic::None
        );
        assert_eq!(
            remote_diagnostic(b"\xffprivate\nmarmish stream: remote error Unknown\n"),
            RemoteDiagnostic::None
        );
        assert_eq!(
            remote_diagnostic(
                b"marmish stream: remote error Gap\nmarmish stream: remote failure Closed\n"
            ),
            RemoteDiagnostic::Ambiguous
        );
    }
    #[test]
    fn binding_keeps_existing_active_acceptance_and_projects_all_terminals() {
        let bytes = RefCell::new(Vec::new());
        let sink = |line: &[u8]| {
            bytes.borrow_mut().extend(line);
            Ok(())
        };
        let trace = Trace::new("linux-amd64", "macos-arm64", &sink).unwrap();
        assert!(select_entry(vec![entry()], &"a".repeat(32), ("c", "s"), Some(&trace)).is_ok());
        for terminal in [
            None,
            Some(Terminal::Exited { code: 0 }),
            Some(Terminal::Signaled { signal: 9 }),
            Some(Terminal::Cancelled),
            Some(Terminal::Failed {
                code: WireErrorCode::Uncertain,
            }),
        ] {
            let mut e = entry();
            e.terminal = terminal;
            assert!(select_entry(vec![e], &"a".repeat(32), ("c", "s"), Some(&trace)).is_err());
        }
        assert!(select_entry(vec![], "missing", ("c", "s"), Some(&trace)).is_err());
        assert!(list_result::<()>(Err(Error::Timeout), Some(&trace)).is_err());
        let rendered = String::from_utf8(bytes.into_inner()).unwrap();
        let terminals: Vec<serde_json::Value> = rendered
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(
                    line.strip_prefix("marmish installed stream observation: ")
                        .unwrap(),
                )
                .unwrap()
            })
            .filter(|event| event["stage"] == "binding" && event["evidence"]["kind"] == "binding")
            .map(|event| event["evidence"]["terminal"].clone())
            .collect();
        assert_eq!(
            terminals,
            vec![
                serde_json::json!({"kind":"exited","data":{"code":7}}),
                serde_json::Value::Null,
                serde_json::json!({"kind":"exited","data":{"code":0}}),
                serde_json::json!({"kind":"signaled","data":{"signal":9}}),
                serde_json::json!({"kind":"cancelled"}),
                serde_json::json!({"kind":"failed","data":{"code":"uncertain"}}),
            ]
        );
        assert!(!rendered.contains(&"a".repeat(32)));
    }
    #[test]
    fn pair_assembly_is_literal_and_clock_remains_after_fallible_exit_conversion() {
        use std::os::unix::process::ExitStatusExt;
        let output = process::Output {
            status: std::process::ExitStatus::from_raw(7 << 8),
            stdout: vec![0, 255],
            stderr: vec![],
        };
        let build = |entry, output| PairParts {
            roles: ("linux-amd64", "macos-arm64"),
            nonce: "f".repeat(64),
            stream: "a".repeat(32),
            entry,
            input: b"input",
            output,
            stderr: b"payload",
        };
        let pair = assemble_pair(build(entry(), &output), || 42, None).unwrap();
        assert_eq!(pair.stdout_sha256, records::digest(&[0, 255]));
        assert_eq!(pair.stderr_sha256, records::digest(b"payload"));
        assert_eq!(pair.stdin_sha256, records::digest(b"input"));
        assert_eq!(pair.exit, 7);
        assert_eq!(pair.wall_ns, 42);
        assert!(pair.transfer.is_none());
        let called = std::cell::Cell::new(false);
        let signaled = process::Output {
            status: std::process::ExitStatus::from_raw(9),
            stdout: vec![],
            stderr: vec![],
        };
        assert!(
            assemble_pair(
                build(entry(), &signaled),
                || {
                    called.set(true);
                    42
                },
                None
            )
            .is_err()
        );
        assert!(!called.get());
        let mut missing = entry();
        missing.open_digest = None;
        assert!(
            assemble_pair(
                build(missing, &output),
                || {
                    called.set(true);
                    42
                },
                None
            )
            .is_err()
        );
        assert!(!called.get());
    }
    #[test]
    fn construction_checks_short_circuit_and_failed_sink_does_not_change_errors() {
        let bytes = RefCell::new(Vec::new());
        let sink = |line: &[u8]| {
            bytes.borrow_mut().extend(line);
            Err(std::io::Error::other("SECRET"))
        };
        let trace = Trace::new("linux-amd64", "macos-arm64", &sink).unwrap();
        for (open, exit, elapsed) in [
            (None, None, u128::MAX),
            (Some("x".into()), None, u128::MAX),
            (Some("x".into()), Some(7), u128::MAX),
        ] {
            assert!(matches!(
                construction(open, exit, || elapsed, Some(&trace)),
                Err(Error::Invalid("installed_stream_case"))
            ));
        }
        assert_eq!(
            construction(Some("x".into()), Some(7), || 1, Some(&trace)).unwrap(),
            ("x".into(), 7, 1)
        );
        let lines = String::from_utf8(bytes.into_inner()).unwrap();
        let evidence: Vec<serde_json::Value> = lines
            .lines()
            .filter_map(|line| {
                serde_json::from_str::<serde_json::Value>(
                    line.strip_prefix("marmish installed stream observation: ")?,
                )
                .ok()
            })
            .filter(|v| v["evidence"].is_object())
            .collect();
        assert_eq!(
            evidence[0]["evidence"]["exit_representable"],
            serde_json::Value::Null
        );
        assert_eq!(
            evidence[1]["evidence"]["wall_ns_representable"],
            serde_json::Value::Null
        );
    }
}

#[cfg(test)]
mod lock_observation_tests {
    use super::*;
    use std::cell::RefCell;
    #[tokio::test]
    async fn outer_cancellation_of_original_list_lock_has_begin_without_result() {
        let dir = tempfile::tempdir().unwrap();
        let env = Environment {
            home: dir.path().into(),
            temporary: dir.path().into(),
            utilities: dir.path().into(),
        };
        let mut child = process::command(
            Path::new("/bin/sh"),
            &["-c".into(), "while :; do :; done".into()],
            dir.path(),
            &env,
        )
        .spawn()
        .unwrap();
        let bytes = RefCell::new(Vec::new());
        let sink = |line: &[u8]| {
            bytes.borrow_mut().extend(line);
            Ok(())
        };
        let trace = Trace::new("linux-amd64", "macos-arm64", &sink).unwrap();
        {
            let ep = Endpoint {
                daemon: std::sync::Arc::new(tokio::sync::Mutex::new(&mut child)),
                socket: dir.path(),
                environment: &env,
                workspace: dir.path(),
            };
            let held = ep.daemon.lock().await;
            assert!(
                tokio::time::timeout(Duration::from_millis(20), lock_daemon(&ep, Some(&trace)))
                    .await
                    .is_err()
            );
            drop(held);
        }
        process::terminate(&mut child).await.unwrap();
        let bytes = String::from_utf8(bytes.into_inner()).unwrap();
        assert!(bytes.contains("list_lock"));
        assert!(bytes.contains("begin"));
        assert!(!bytes.contains("\"ok\""));
        assert!(!bytes.contains("\"error\""));
    }
}
