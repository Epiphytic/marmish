//! Explicit, expensive local IPC qualification; these are codec payload metrics,
//! not Marmot or network wire measurements. Run the ignored test by EXACT name.
use super::*;
use futures_util::{StreamExt, stream::FuturesUnordered};
use std::io::Read;
use tokio::sync::watch;

const FILE_BYTES: usize = 66 * 1024 * 1024 + 17;
const CASE_SECONDS: u64 = 1800;

fn generate(path: &Path, length: usize) -> [u8; 32] {
    let mut file = File::create(path).unwrap();
    let mut buffer = [0u8; 65536];
    let mut hash = Sha256::new();
    let mut position = 0;
    let mut random = 0x7355_ba81_e9c4_102du64;
    while position < length {
        let n = buffer.len().min(length - position);
        for (index, byte) in buffer[..n].iter_mut().enumerate() {
            let absolute = position + index;
            *byte = if (absolute / (1024 * 1024)) % 2 == 0 {
                absolute as u8
            } else {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                random as u8
            };
        }
        file.write_all(&buffer[..n]).unwrap();
        hash.update(&buffer[..n]);
        position += n;
    }
    file.sync_all().unwrap();
    hash.finalize().into()
}
fn hash_file(path: &Path) -> (u64, [u8; 32]) {
    let mut file = File::open(path).unwrap();
    let mut bytes = [0; 65536];
    let mut hash = Sha256::new();
    let mut length = 0;
    loop {
        let n = file.read(&mut bytes).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
        length += n as u64;
    }
    (length, hash.finalize().into())
}
fn require_disk(root: &Path) {
    let path = std::ffi::CString::new(root.as_os_str().as_bytes()).unwrap();
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    assert_eq!(
        unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) },
        0
    );
    let stat = unsafe { stat.assume_init() };
    let available = u128::from(stat.f_bavail) * u128::from(stat.f_frsize);
    assert!(
        available >= 1024 * 1024 * 1024,
        "large qualification requires 1GiB available TMPDIR space"
    );
}
async fn large_case(
    codec: Codec,
    direction: Direction,
    mut cancelled: watch::Receiver<bool>,
) -> Result<serde_json::Value, String> {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let source = dir.join("-large ' $ ; \\ \n source.bin");
    let destination = dir.join("-large ' $ ; \\ \n destination.bin");
    let expected = generate(&source, FILE_BYTES);
    set_metadata(&source);
    if *cancelled.borrow() {
        return Err("large_sibling_failed".into());
    }
    let launch = launch(&source, &destination, direction);
    let socket = dir.join("ipc");
    let mut bridge = Bridge::start(&socket, &launch, codec, [3; 16]).await;
    let mut io = io(&socket, &launch, codec, [3; 16]);
    let started = std::time::Instant::now();
    let mut joined = false;
    let outcome = {
        let run = copy_client::run(
            &mut io,
            launch,
            [3; 16],
            cp::MIN_RETENTION_SECS as u64,
            Arc::new(AtomicBool::new(false)),
        );
        tokio::pin!(run);
        tokio::select! {
            result = &mut run => result.map_err(|_| "large_protocol_failed".to_string()),
            _ = tokio::time::sleep(Duration::from_secs(CASE_SECONDS)) => Err("large_case_timeout".into()),
            _ = cancelled.changed() => Err("large_sibling_failed".into()),
            _ = &mut bridge.task => { joined = true; Err("large_bridge_ended".into()) },
        }
    };
    // Cleanup is attempted for every outcome before any verification assertion.
    let cleanup = bridge.finish_checked(joined).await;
    let metrics = bridge.metrics();
    outcome?;
    if !cleanup? || !metrics.cleanup_confirmed {
        return Err("large_cleanup_unconfirmed".into());
    }
    let flow = if direction == Direction::Put {
        &metrics.input
    } else {
        &metrics.output
    };
    let reverse = if direction == Direction::Put {
        &metrics.output
    } else {
        &metrics.input
    };
    assert_eq!(flow.bytes(), FILE_BYTES as u64);
    assert_eq!(flow.first_offset, Some(0));
    assert_eq!(flow.next_offset, Some(FILE_BYTES as u64));
    assert!(!flow.discontiguous);
    assert!(flow.data.len() <= 1024 && flow.max_data <= MAX_BLOCK - 13);
    assert!(flow.done && flow.finished && reverse.committed);
    assert!(reverse.checkpoints > 0);
    assert!(metrics.stdout_drained && metrics.stderr_drained);
    assert!(metrics.output_codec.pending.is_empty());
    assert_eq!(metrics.output_codec.totals.raw, metrics.stdout_produced);
    let totals = if direction == Direction::Put {
        &metrics.input_codec
    } else {
        &metrics.output_codec.totals
    };
    assert!(totals.none > 0);
    match codec {
        Codec::None => assert_eq!(totals.zlib, 0),
        Codec::Zlib => assert!(totals.zlib > 0),
    }
    let (length, actual) = hash_file(&destination);
    assert_eq!((length, actual), (FILE_BYTES as u64, expected));
    assert_metadata(&source, &destination);
    Ok(serde_json::json!({
        "direction": if direction == Direction::Put { "put" } else { "get" },
        "codec": codec, "file_bytes": length, "sha256": hex::encode(actual),
        "wall_seconds": started.elapsed().as_secs_f64(), "data_frames": flow.data_frames,
        "retained_frame_samples": flow.data.len(), "max_data_bytes": flow.max_data,
        "raw_stream_bytes": totals.raw, "encoded_payload_bytes": totals.encoded,
        "none_blocks": totals.none, "zlib_blocks": totals.zlib,
        "local_codec_payload_ratio": totals.encoded as f64 / totals.raw as f64,
        "natural_helper_exit": true, "final_output_acknowledged": true
    }))
}
fn begin_drain(deadline: &mut tokio::time::Instant, draining: &mut bool) -> bool {
    if *draining {
        return false;
    }
    *draining = true;
    *deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    true
}
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "expensive >65MiB actual-helper four-way qualification; exact-name invocation only"]
async fn large_raw_zlib_actual_helper_four_way() {
    let disk = tempfile::tempdir().unwrap();
    require_disk(disk.path());
    let (cancel, receive) = watch::channel(false);
    let mut cases = FuturesUnordered::new();
    for codec in [Codec::None, Codec::Zlib] {
        for direction in [Direction::Put, Direction::Get] {
            let receive = receive.clone();
            cases.push(async move {
                let result = std::panic::AssertUnwindSafe(large_case(codec, direction, receive))
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| Err("large_case_verification_failed".into()));
                (codec, direction, result)
            });
        }
    }
    let mut deadline = tokio::time::Instant::now() + Duration::from_secs(2100);
    let mut draining = false;
    let mut failure: Option<String> = None;
    let mut reports = Vec::new();
    while !cases.is_empty() {
        match tokio::time::timeout_at(deadline, cases.next()).await {
            Ok(Some((_, _, Ok(report)))) => reports.push(report),
            Ok(Some((codec, direction, Err(error)))) => {
                if error != "large_sibling_failed" && failure.is_none() {
                    let direction = if direction == Direction::Put {
                        "put"
                    } else {
                        "get"
                    };
                    failure = Some(format!("{codec:?}/{direction}: {error}"));
                }
                if begin_drain(&mut deadline, &mut draining) {
                    let _ = cancel.send(true);
                }
            }
            Ok(None) => break,
            Err(_) => {
                if begin_drain(&mut deadline, &mut draining) {
                    failure.get_or_insert_with(|| "large_aggregate_timeout".into());
                    let _ = cancel.send(true);
                } else {
                    let original = failure.as_deref().unwrap_or("large_failed");
                    failure = Some(format!(
                        "{original}: large_aggregate_timeout_cleanup_unconfirmed"
                    ));
                    cases.clear();
                    break;
                }
            }
        }
    }
    assert!(
        failure.is_none(),
        "{}",
        failure.as_deref().unwrap_or("large_failed")
    );
    assert_eq!(reports.len(), 4);
    println!(
        "MARMISH_LARGE_HELPER {}",
        serde_json::json!({"schema":1,"scope":"local_ipc_codec_payload_not_network_wire","cases":reports})
    );
}
#[tokio::test]
async fn aggregate_expiry_grants_one_cooperative_cleanup_interval() {
    let now = tokio::time::Instant::now();
    let mut deadline = now;
    let mut draining = false;
    assert!(begin_drain(&mut deadline, &mut draining));
    assert!(deadline >= now + Duration::from_secs(20));
    let granted = deadline;
    assert!(!begin_drain(&mut deadline, &mut draining));
    assert_eq!(deadline, granted);
}
#[test]
fn growing_output_candidate_is_counted_only_at_ack_boundary() {
    let mut ledger = CodecLedger::default();
    let short = codec::encode(0, &[5; 4], Codec::None).unwrap();
    let long = codec::encode(0, &[5; 16], Codec::Zlib).unwrap();
    ledger.emitted(&short);
    ledger.emitted(&long);
    ledger.emitted(&long);
    assert_eq!(ledger.pending.len(), 2);
    assert_eq!(ledger.totals.raw, 0);
    ledger.acknowledge(16);
    assert_eq!(ledger.totals.raw, 16);
    assert_eq!(ledger.totals.encoded, long.data.len() as u64);
    assert_eq!(ledger.totals.zlib, 1);
    assert!(ledger.pending.is_empty());
    ledger.acknowledge(16);
    assert_eq!(ledger.totals.raw, 16);
}
#[tokio::test]
async fn bridge_panic_reaps_owned_helper_before_join_failure() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::write(&source, b"small").unwrap();
    let launch = launch(&source, &dir.path().join("destination"), Direction::Put);
    let mut bridge = Bridge::start_fault(
        &dir.path().join("ipc"),
        &launch,
        Codec::None,
        [3; 16],
        Fault::Panic,
    )
    .await;
    let mut io = io(&dir.path().join("ipc"), &launch, Codec::None, [3; 16]);
    let start = std::time::Instant::now();
    let run = io.send(cp::Frame::Start(launch.clone(), Codec::None));
    tokio::pin!(run);
    tokio::select! {
        _ = &mut bridge.task => {},
        _ = &mut run => {
            assert!(tokio::time::timeout(Duration::from_secs(10), &mut bridge.task).await.unwrap().is_err());
        },
        _ = tokio::time::sleep(Duration::from_secs(10)) => panic!("bridge panic cleanup timed out"),
    }
    assert!(start.elapsed() < Duration::from_secs(10));
    assert!(bridge.metrics().cleanup_confirmed);
    assert!(bridge.finish_checked(true).await.is_err());
}
