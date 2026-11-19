//! A one-exchange Unix fixture for a committed approval whose reply is lost.
use crate::ipc::peer_approve::Selection;
use nostr::nips::nip19::ToBech32 as _;
use serde_json::{Value, json};
use std::{
    fs::Permissions, os::unix::fs::PermissionsExt as _, path::Path, process::Output, time::Duration,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{UnixListener, UnixStream},
    process::Command,
    time::timeout,
};

const FRAME_CAP: usize = 4_096;
// Leave headroom inside the actual client's 15-second exchange budget.
const LOSS_BUDGET: Duration = Duration::from_secs(14);

pub(super) fn approval_cli(
    daemon: &Path,
    root: &Path,
    profile: &str,
    socket: &Path,
    selection: Selection,
) -> Command {
    let mut cli = Command::new(daemon.with_file_name("marmish"));
    cli.kill_on_drop(true)
        .arg("--state-dir")
        .arg(root)
        .arg("--identity")
        .arg(profile)
        .arg("--socket")
        .arg(socket)
        .args(["servers", "approve", "--server"])
        .arg(selection.server.to_bech32().unwrap())
        .arg("--offer-id")
        .arg(hex::encode(selection.offer_id))
        .arg("--revision")
        .arg(hex::encode(selection.revision))
        .arg("--generation")
        .arg(selection.generation.to_string());
    cli
}

async fn frame(stream: &mut UnixStream) -> Vec<u8> {
    let len = stream.read_u32().await.unwrap() as usize;
    assert!((1..=FRAME_CAP).contains(&len), "bounded approval frame");
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).await.unwrap();
    bytes
}

async fn withhold_committed_reply(listener: UnixListener, backend: &Path, selection: Selection) {
    let (mut frontend, _) = listener.accept().await.unwrap();
    // SAFETY: geteuid has no arguments and reads the process credential.
    let uid = unsafe { libc::geteuid() };
    assert_eq!(frontend.peer_cred().unwrap().uid(), uid);
    let request = frame(&mut frontend).await;
    let expected = json!({
        "operation": "peer_approve",
        "request": { "version": 1, "selection": selection },
    });
    assert!(
        serde_json::from_slice::<Value>(&request).unwrap() == expected,
        "exact approval request"
    );

    let mut daemon = UnixStream::connect(backend).await.unwrap();
    assert_eq!(daemon.peer_cred().unwrap().uid(), uid);
    daemon
        .write_u32(u32::try_from(request.len()).unwrap())
        .await
        .unwrap();
    daemon.write_all(&request).await.unwrap();
    // Keep both streams live through the committed reply. The listening
    // worker can refuse an exchange whose sender disconnects before commit.
    let reply = frame(&mut daemon).await;
    let expected = json!({
        "ok": true,
        "result": { "version": 1, "selection": selection, "state": "activating" },
        "error": null,
    });
    assert!(
        serde_json::from_slice::<Value>(&reply).unwrap() == expected,
        "daemon recorded the exact approval before the reply was withheld"
    );
    drop(frontend);
}

pub(super) async fn test_lost_reply(
    daemon: &Path,
    root: &Path,
    profile: &str,
    backend: &Path,
    selection: Selection,
) -> Output {
    let directory = tempfile::Builder::new()
        .prefix("mra-")
        .permissions(Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    assert_eq!(
        directory.path().metadata().unwrap().permissions().mode() & 0o777,
        0o700
    );
    let socket = directory.path().join("approval.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, Permissions::from_mode(0o600)).unwrap();
    let mut cli = approval_cli(daemon, root, profile, &socket, selection);
    timeout(LOSS_BUDGET, async {
        let (output, ()) = tokio::join!(
            cli.output(),
            withhold_committed_reply(listener, backend, selection)
        );
        output.expect("installed approval CLI starts")
    })
    .await
    .expect("committed approval reply lost within the client budget")
}

pub(super) async fn retry_approval_cli(
    daemon: &Path,
    root: &Path,
    profile: &str,
    socket: &Path,
    selection: Selection,
    budget: Duration,
) -> Output {
    timeout(budget, async {
        loop {
            let output = approval_cli(daemon, root, profile, socket, selection)
                .output()
                .await
                .expect("installed retry starts");
            if output.status.code() == Some(1)
                && output.stdout.is_empty()
                && matches!(
                    output.stderr.as_slice(),
                    b"marmish: busy\n" | b"marmish: peer_unavailable\n"
                )
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            } else {
                return output;
            }
        }
    })
    .await
    .expect("reopened candidate replies to the original CLI selection")
}
