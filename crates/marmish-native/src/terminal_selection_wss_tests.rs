//! Join an actual Command-backed native writer through loopback TLS.
use super::*;
use crate::mdk_adapter::terminal_live_enrollment::ReadyEnrollment;
use crate::terminal_backend::supervisor::live::selection::proof::{
    NativePair, NativeProof, SelectionFault,
};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;

struct TestFlight<'a> {
    relay: &'a Relay,
    path: &'a Path,
    command: &'a CommandRoom,
    client_key: Arc<Keys>,
    server_key: Arc<Keys>,
}

impl TestFlight<'_> {
    async fn ready(
        &self,
        client: &mut MdkOwner,
        server: &mut MdkOwner,
        inbound: &mut InboundState,
    ) -> Result<(ReadyEnrollment, ReadyEnrollment, Arc<AttachmentFence>)> {
        let relay_urls = [self.relay.url.to_string()];
        let client_binding = client
            .terminal_client_binding(
                self.server_key.public_key().to_bytes(),
                &relay_urls,
                self.command.session,
            )?
            .ok_or(Error::Invalid("fixture_client_binding_not_ready"))?;
        let server_binding = server
            .terminal_server_bindings()?
            .into_iter()
            .find(|binding| binding.context.mls_group() == self.command.group)
            .ok_or(Error::Invalid("fixture_no_exact_command_binding"))?;
        let command_route = server_binding.context.transport_group().to_vec();
        let fence = server_binding.fence.clone();
        let (client_signing, client_codec) =
            BoundSigning::bind(&client_binding.context, self.client_key.clone())?;
        let (server_signing, server_codec) =
            BoundSigning::bind(&server_binding.context, self.server_key.clone())?;
        let client_connector = self.relay.connector(self.path, true, None);
        let server_connector = self.relay.connector(self.path, true, None);
        let client_carrier = admit(
            client_binding.context,
            client_binding.fence,
            client_connector.policy(),
            &relay_urls[0],
        )?;
        let server_carrier = admit(
            server_binding.context,
            server_binding.fence,
            server_connector.policy(),
            &relay_urls[0],
        )?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let (mut client_bootstrap, server_bootstrap) = tokio::try_join!(
            client_carrier.open(client_signing, client_codec, client_connector, deadline),
            server_carrier.open(server_signing, server_codec, server_connector, deadline),
        )?;
        client_bootstrap
            .constrain_client_start(EnrollmentCeiling::start(Duration::from_secs(25))?)?;
        let client_rooms = Arc::new(Semaphore::new(1));
        let server_rooms = Arc::new(Semaphore::new(1));
        let client_flight = async {
            client_bootstrap.start_client().await?;
            client_bootstrap.finish_client().await
        };
        let (_, (server_bootstrap, permit)) = tokio::try_join!(
            client_flight,
            server_bootstrap.serve(inbound, &server_rooms, Arc::new(|| true)),
        )?;
        let (mut client, mut server) = tokio::try_join!(
            client_bootstrap.enroll(
                self.relay.connector(self.path, true, None),
                RoomPermit::new(client_rooms.try_acquire_owned().unwrap()),
                Some(Arc::new(|| true)),
            ),
            server_bootstrap.enroll(self.relay.connector(self.path, true, None), permit, None),
        )?;
        let client_room = client.owner_mut();
        let server_room = server.owner_mut();
        let disjoint = |group: &[u8; 32], route: &[u8; 32]| {
            [self.command.group.as_slice(), command_route.as_slice()]
                .iter()
                .all(|other| *other != group && *other != route)
        };
        client_room.offer().await?;
        server_room.found(disjoint).await?;
        client_room.join(disjoint).await?;
        tokio::try_join!(client_room.client_receipts(), server_room.server_receipts())?;
        let (client, server) = tokio::try_join!(client.ready(&|| true), server.ready(&|| true))?;
        Ok((client, server, fence))
    }
}

#[test]
#[ignore = "uses a real private TLS relay, installed native worker, SQLCipher, and tmux"]
fn terminal_selection_native_wss_echo_and_resize() {
    // Debug builds place several joined native MLS futures on this test's
    // stack. Leave normal libtest worker stacks unchanged.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(prove_selection())
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn ready_new_carrier(
    previous: &TestFlight<'_>,
    client: &mut MdkOwner,
    server: &mut MdkOwner,
    cat: &Path,
) -> Result<(ReadyEnrollment, ReadyEnrollment, Arc<AttachmentFence>)> {
    // The old alias retains encrypted Probe records and its listener is rate
    // limited. A separately authorized Command room supplies a new carrier
    // while the same two accounts contend for the same native Default slot.
    let command = client
        .open_prepared_command_room(
            previous.server_key.public_key().to_bytes(),
            Some(fresh_id()),
            Instant::now() + STEP,
        )
        .await?;
    run_command(client, server, &command, cat).await;
    TestFlight {
        command: &command,
        relay: previous.relay,
        path: previous.path,
        client_key: previous.client_key.clone(),
        server_key: previous.server_key.clone(),
    }
    .ready(client, server, &mut InboundState::default())
    .await
}

async fn predictable_shell(terminal: &mut NativePair) {
    // Start from the user's configured login shell, then isolate measurements
    // from prompt hooks, shell startup scripts, and customized key bindings.
    terminal.echo(
        1,
        b"exec /usr/bin/env -i PATH=/usr/bin:/bin TERM=dumb LC_ALL=C PS1='MAR''MISH> ' /bin/sh -i\r".to_vec(),
        b"MARMISH> ",
    ).await.expect("fixture shell prompt");
}

async fn prove_selection() {
    let temp = tempfile::Builder::new()
        .prefix("sel-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let relay = Relay::start().await;
    relay.allow_multi_room_workload();
    let client_peer = CommandPeer::new(temp.path(), "c");
    let mut server_peer = CommandPeer::new(temp.path(), "s");
    let cat = [Path::new("/usr/bin/cat"), Path::new("/bin/cat")]
        .into_iter()
        .find_map(|path| std::fs::canonicalize(path).ok())
        .unwrap();
    for account in [client_peer.public(), server_peer.public()] {
        relay.ordinary(account);
    }
    let config = json!({
        "version": 1, "role": "server", "relays": [relay.url.as_str()], "tor": "off",
        "cwd": "/", "limits": {}, "aliases": {}, "trusted_remotes": [],
        "execution_clients": [client_peer.public().to_hex()],
        "terminal": {"clients": [client_peer.public().to_hex()]},
        "command_policy": {"version": 1, "defaults": {"allow_exec": [&cat]}},
    });
    let profile = server_peer.lease.root().to_owned();
    let mut config_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(profile.join("config.json"))
        .unwrap();
    config_file
        .write_all(&serde_json::to_vec(&config).unwrap())
        .unwrap();
    drop(config_file);
    let mut client = client_peer
        .open(&relay, server_peer.public(), false, &cat)
        .await;
    let mut server = server_peer
        .open_with_terminal(&relay, client_peer.public(), true, &cat, true)
        .await;
    // This fixture checks ordered command results, not the short route-proof
    // transcript collected by CommandPeer for separate tests.
    client.relay_proof_ingress = None;
    server.relay_proof_ingress = None;
    server_peer
        .lease
        .mark_ready(server.opened_binding())
        .unwrap();
    let command = client
        .open_command_room(
            server_peer.public().to_bytes(),
            false,
            Instant::now() + STEP,
        )
        .await
        .unwrap();
    run_command(&mut client, &mut server, &command, &cat).await;
    let setup = Instant::now();
    let flight = TestFlight {
        relay: &relay,
        path: temp.path(),
        command: &command,
        client_key: client_peer.material.signer(),
        server_key: server_peer.material.signer(),
    };
    let (client_room, server_room, _) = flight
        .ready(&mut client, &mut server, &mut InboundState::default())
        .await
        .unwrap();
    let enrollment = setup.elapsed();
    let lease = Arc::new(server_peer.lease);
    let mut owner = NativeProof::start(lease, &server_peer.material).unwrap();
    let mut terminal = owner
        .attach(client_room, server_room, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owner.count().unwrap(), 1);
    let initial_binding = terminal.binding();
    let slot = terminal.slot();
    let selected = terminal.admission;
    predictable_shell(&mut terminal).await;
    let mut samples = Vec::new();
    for sequence in 2..=21 {
        let input = format!("printf '\\033[2J\\033[H[%s]\\n' '{sequence} two words; % 🐾'\r");
        let wanted = format!("[{sequence} two words; % 🐾]");
        samples.push(
            terminal
                .echo(sequence, input.into_bytes(), wanted.as_bytes())
                .await
                .expect("special-character echo"),
        );
    }
    let marker = hex::encode(fresh_id());
    let payload = format!("printf '%16384s%s' x '{marker}'\r");
    let throughput = terminal
        .echo(31, payload.into_bytes(), marker.as_bytes())
        .await
        .unwrap();
    terminal
        .resize(32, frames::Size { rows: 30, cols: 90 })
        .await
        .unwrap();
    terminal
        .echo(33, b"stty size | tr ' ' :\r".to_vec(), b"30:90")
        .await
        .unwrap();
    terminal
        .echo(
            34,
            b"marmish_retained=YES; printf '<%s>\\n' ready\r".to_vec(),
            b"<ready>",
        )
        .await
        .unwrap();

    let (client_room, server_room, _) = ready_new_carrier(&flight, &mut client, &mut server, &cat)
        .await
        .unwrap();
    assert!(
        owner
            .attach(client_room, server_room, false)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(owner.count().unwrap(), 1);

    let (client_room, server_room, _) = ready_new_carrier(&flight, &mut client, &mut server, &cat)
        .await
        .unwrap();
    let mut isolated = owner
        .attach(client_room, server_room, true)
        .await
        .unwrap()
        .unwrap();
    let isolated_slot = isolated.slot();
    assert_ne!(slot, isolated_slot);
    assert_eq!(owner.count().unwrap(), 2);
    predictable_shell(&mut isolated).await;
    isolated
        .echo(
            2,
            b"printf '<%s>\\n' \"${marmish_retained:-EMPTY}\"\r".to_vec(),
            b"<EMPTY>",
        )
        .await
        .unwrap();
    isolated.close().await.unwrap();
    terminal.close().await.unwrap();

    let (client_room, server_room, fence) =
        ready_new_carrier(&flight, &mut client, &mut server, &cat)
            .await
            .unwrap();
    let mut terminal = owner
        .attach(client_room, server_room, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(slot, terminal.slot());
    let (transcript, attachment, generation) = terminal.binding();
    assert_ne!(transcript, initial_binding.0);
    assert_ne!(attachment, initial_binding.1);
    assert!(generation > initial_binding.2);
    let warm = terminal.admission;
    terminal
        .echo(
            1,
            b"printf '<%s>\\n' \"${marmish_retained:-EMPTY}\"\r".to_vec(),
            b"<YES>",
        )
        .await
        .unwrap();
    fence.retire();
    terminal.await_revocation(temp.path()).await.unwrap();
    terminal.close().await.unwrap();
    owner.release(slot).await.unwrap();
    owner.release(isolated_slot).await.unwrap();
    assert!(owner.empty().unwrap());
    for fault in [
        SelectionFault::Early,
        SelectionFault::WrongClaim,
        SelectionFault::Missing,
    ] {
        let (client_room, server_room, _) =
            ready_new_carrier(&flight, &mut client, &mut server, &cat)
                .await
                .unwrap();
        owner
            .rejected_selection(client_room, server_room, fault)
            .await
            .unwrap();
    }
    run_command(&mut client, &mut server, &command, &cat).await;
    client.shutdown(Instant::now() + STEP).await.unwrap();
    server.shutdown(Instant::now() + STEP).await.unwrap();
    relay.assert_healthy();
    relay.assert_no_plaintext(marker.as_bytes());
    let mut databases = 0;
    assert_databases_omit_marker(temp.path(), marker.as_bytes(), &mut databases);
    assert!(databases > 0, "inspected durable profile databases");
    samples.sort();
    eprintln!(
        "terminal_writer_selection platform={}-{} debug_assertions={} transport=loopback_private_wss workload=quoted_utf8_shell_echo enrollment_samples=1 cold_samples=1 warm_samples=1 enrollment_us={} selection_us={} warm_selection_us={} echo_samples={} echo_p50_us={} echo_p95_us={} output_payload_bytes=16384 output_batch_max_bytes=4096 output_samples=1 output_us={} output_bytes_per_second={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(debug_assertions),
        enrollment.as_micros(),
        selected.as_micros(),
        warm.as_micros(),
        samples.len(),
        samples[9].as_micros(),
        samples[18].as_micros(),
        throughput.as_micros(),
        (16_384.0 / throughput.as_secs_f64()) as u64,
    );
}

fn assert_databases_omit_marker(directory: &Path, marker: &[u8], checked: &mut usize) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            assert_databases_omit_marker(&entry.path(), marker, checked);
        } else if entry.file_type().unwrap().is_file() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.contains("sqlite") || name.ends_with(".db") {
                let bytes = std::fs::read(entry.path()).unwrap();
                assert!(!bytes.windows(marker.len()).any(|window| window == marker));
                *checked += 1;
            }
        }
    }
}
