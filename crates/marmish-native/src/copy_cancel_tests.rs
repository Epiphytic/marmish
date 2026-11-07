//! Actual helper + local worker cancel qualification; not daemon/relay Cancel.
//! Commit barriers test overwrite=false only; overwrite=true's final cancel
//! check is not qualified here. The point byte + 0xa5 release form the fixed
//! owned-socket handshake. Holding an unpolled attachment at explicit cancel
//! is deliberate; it is dropped before joining, never treated as implicit cancel.
use super::*;
use crate::{
    copy_commit_hook::{Hook, Point},
    copy_worker::{Cancellation, Worker},
};
use std::sync::atomic::Ordering;
#[derive(Clone, Copy)]
enum Trigger {
    Buffered,
    Commit(Point),
    Receipt,
}
async fn wait_buffered(dir: &Path, launch: &Launch) -> u64 {
    loop {
        if let Ok(raw) = std::fs::read(dir.join(launch.namespace()).join("checkpoint"))
            && let Ok(cp) = cp::Checkpoint::decode(&raw)
            && cp.phase == cp::Phase::Receiving
            && cp.durable_offset > 0
            && cp.durable_offset < cp.manifest.size
            && let Ok(data) = std::fs::metadata(dir.join(launch.namespace()).join("data"))
            && data.len() > cp.durable_offset
            && data.len() < cp.manifest.size
            && !Path::new(std::ffi::OsStr::from_bytes(&launch.destination)).exists()
        {
            return cp.durable_offset;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
async fn wait_flag(bridge: &Bridge, direction: Direction, full: bool) {
    loop {
        let metrics = bridge.metrics();
        if if full {
            match direction {
                Direction::Put => metrics.input_full,
                Direction::Get => metrics.output_full,
            }
        } else {
            metrics.gated
        } {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
async fn case(direction: Direction, trigger: Trigger) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let source = dir.join("source");
    let destination = dir.join("destination");
    let size = if matches!(trigger, Trigger::Buffered) {
        3 * 1024 * 1024 + 65537
    } else {
        65537
    };
    let bytes = pattern(size);
    std::fs::write(&source, &bytes).unwrap();
    set_metadata(&source);
    let mut launch = launch(&source, &destination, direction);
    let local = Arc::new(AtomicBool::new(false));
    let mut remote_hook = None;
    let mut local_hook = None;
    let mut readiness = None;
    if let Trigger::Commit(point) = trigger {
        let (parent, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        parent.set_nonblocking(true).unwrap();
        readiness = Some(tokio::net::UnixStream::from_std(parent).unwrap());
        if direction == Direction::Put {
            remote_hook = Some((peer, point));
        } else {
            local_hook = Some(Hook::new(peer, point).unwrap());
        }
    }
    let worker = if let Some(hook) = local_hook {
        Worker::start_with_commit_hook(Cancellation::Owned(local.clone()), hook)
    } else {
        Worker::start(Cancellation::Owned(local.clone()))
    };
    let socket = dir.join("first");
    let mut bridge = Bridge::start_options(
        &socket,
        &launch,
        Codec::Zlib,
        [3; 16],
        Fault::None,
        remote_hook,
        matches!(trigger, Trigger::Receipt),
    )
    .await;
    let mut io = io(&socket, &launch, Codec::Zlib, [3; 16]);
    let mut before = 0;
    let outcome = {
        let operation = copy_client::run_with_worker(
            &mut io,
            launch.clone(),
            [3; 16],
            cp::MIN_RETENTION_SECS as u64,
            local.clone(),
            &worker,
        );
        tokio::pin!(operation);
        tokio::time::timeout(Duration::from_secs(120),async {
            match trigger {
                Trigger::Buffered=>{
                    before=tokio::select!{result=&mut operation=>return Err(format!("completed before buffered gate: {result:?}")),offset=wait_buffered(&dir,&launch)=>offset};
                    tokio::select!{result=&mut operation=>return Err(format!("completed while installing gate: {result:?}")),result=bridge.gate(direction)=>{result?;}}
                    tokio::select!{result=&mut operation=>return Err(format!("completed before full window: {result:?}")),_=wait_flag(&bridge,direction,true)=>{}}
                }
                Trigger::Receipt=>tokio::select!{result=&mut operation=>return Err(format!("completed before held receipt: {result:?}")),_=wait_flag(&bridge,direction,false)=>{}},
                Trigger::Commit(point)=>{
                    let mut byte=[0];
                    tokio::select!{result=&mut operation=>return Err(format!("completed before commit barrier: {result:?}")),result=readiness.as_mut().unwrap().read_exact(&mut byte)=>{result.map_err(|_|"hook readiness".to_string())?;}}
                    if byte!=[point as u8] {return Err("hook point mismatch".into());}
                }
            }
            if direction==Direction::Get {
                local.store(true,Ordering::Release);
                if matches!(trigger,Trigger::Commit(_)) {
                    readiness.as_mut().unwrap().write_all(&[0xa5]).await.map_err(|_|"hook release".to_string())?;
                    // Keep polling production receive/commit after local cancellation.
                    let _result=(&mut operation).await;
                }
            }
            // Explicit request, not an attachment-loss side effect.
            let signaled=bridge.cancel().await?;
            if !signaled && !matches!(trigger,Trigger::Commit(Point::AfterRename)) {
                return Err("helper already exited before explicit cancel".into());
            }
            Ok::<(),String>(())
        }).await
    };
    // Operation has been dropped, but local filesystem ownership stays here.
    local.store(true, Ordering::Release);
    let disposition = worker.shutdown(Duration::from_secs(15)).await;
    let cleanup = bridge.finish_checked(false).await;
    let metrics = bridge.metrics();
    // Only assert once owned child and local worker have been joined.
    assert!(cleanup.unwrap());
    assert!(metrics.cleanup_confirmed);
    assert!(disposition.is_ok(), "worker shutdown failed");
    outcome.expect("bounded cancellation case").unwrap();
    assert_eq!(metrics.input.errors, 0);
    assert_eq!(metrics.output.errors, 0);
    assert_eq!(metrics.stderr_produced, 0);
    if !matches!(trigger, Trigger::Commit(Point::AfterRename)) || direction == Direction::Put {
        assert_eq!(metrics.helper_exit, Some(1));
        assert!(metrics.signaled);
    }
    if direction == Direction::Put {
        assert!(matches!(
            disposition.unwrap(),
            crate::copy_local_gc::Disposition::NeverSubmitted
        ));
    } else {
        assert!(matches!(
            disposition.unwrap(),
            crate::copy_local_gc::Disposition::Attempted
        ));
    }
    if matches!(trigger, Trigger::Buffered) {
        assert!(
            !(if direction == Direction::Put {
                metrics.input.done
            } else {
                metrics.output.done
            }),
            "buffered cancellation preceded Done"
        );
        assert!(
            metrics.input_peak <= marmish_stream::WINDOW_BYTES
                && metrics.output_peak <= marmish_stream::WINDOW_BYTES
        );
        assert!(match direction {
            Direction::Put => metrics.input_full,
            Direction::Get => metrics.output_full,
        });
    }
    let checkpoint = cp::Checkpoint::decode(
        &std::fs::read(dir.join(launch.namespace()).join("checkpoint")).unwrap(),
    )
    .unwrap();
    let durable = checkpoint.durable_offset;
    let expected: [u8; 32] = Sha256::digest(&bytes[..durable as usize]).into();
    assert_eq!(checkpoint.prefix_sha256, expected);
    match trigger {
        Trigger::Buffered => {
            assert_eq!(checkpoint.phase, cp::Phase::Receiving);
            assert!(durable >= before && durable > 0 && durable < bytes.len() as u64);
            assert!(!destination.exists());
        }
        Trigger::Commit(Point::BeforeRename) => {
            assert_eq!(checkpoint.phase, cp::Phase::CommitPrepared);
            assert!(!destination.exists());
        }
        Trigger::Commit(Point::AfterRename) => {
            assert!(destination.exists());
            assert_eq!(
                checkpoint.phase,
                if direction == Direction::Put {
                    cp::Phase::CommitPrepared
                } else {
                    cp::Phase::Committed
                }
            );
        }
        Trigger::Receipt => {
            assert_eq!(checkpoint.phase, cp::Phase::Committed);
            assert!(destination.exists());
        }
    }
    if destination.exists() {
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        assert_metadata(&source, &destination);
    }
    // A CommitPrepared reopen itself recovers rename/receipt state. Leave it
    // untouched so the fresh actual helper below must perform that recovery.
    if checkpoint.phase != cp::Phase::CommitPrepared {
        let base = crate::copy_files::trusted_base(&dir).unwrap();
        let receiver = crate::copy_files::CopyReceiver::open_or_recover(
            &base,
            &launch.destination,
            checkpoint.manifest,
            AttemptMode::Resume,
            [4; 16],
            crate::copy_worker::now().unwrap(),
            cp::MIN_RETENTION_SECS,
        )
        .unwrap();
        assert_eq!(receiver.durable_prefix().sha256, expected);
        drop(receiver);
    }
    launch.mode = AttemptMode::Resume;
    let (result, metrics) =
        attempt_observed(&dir.join("resume"), launch.clone(), Codec::Zlib, [5; 16]).await;
    result.unwrap();
    let flow = if direction == Direction::Put {
        &metrics.input
    } else {
        &metrics.output
    };
    assert_eq!(flow.bytes(), bytes.len() as u64 - durable);
    if durable < bytes.len() as u64 {
        assert_eq!(flow.first_offset, Some(durable));
    }
    let final_checkpoint = cp::Checkpoint::decode(
        &std::fs::read(dir.join(launch.namespace()).join("checkpoint")).unwrap(),
    )
    .unwrap();
    assert_eq!(final_checkpoint.phase, cp::Phase::Committed);
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    assert_metadata(&source, &destination);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_helper_cancel_full_windows_retains_checkpoint_and_resumes() {
    tokio::time::timeout(Duration::from_secs(600), async {
        for direction in [Direction::Put, Direction::Get] {
            case(direction, Trigger::Buffered).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_helper_cancel_at_exact_commit_barriers_preserves_committed_outcomes() {
    tokio::time::timeout(Duration::from_secs(600), async {
        for direction in [Direction::Put, Direction::Get] {
            for point in [Point::BeforeRename, Point::AfterRename] {
                case(direction, Trigger::Commit(point)).await;
            }
        }
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_helper_postreceipt_cancel_never_rolls_back_and_resumes_without_data() {
    tokio::time::timeout(Duration::from_secs(600), async {
        for direction in [Direction::Put, Direction::Get] {
            case(direction, Trigger::Receipt).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn closed_bridge_control_is_reported_without_panicking_before_cleanup() {
    let (control, receive) = mpsc::channel(1);
    drop(receive);
    let (stop, _) = oneshot::channel();
    let mut bridge = Bridge {
        metrics: Arc::new(Mutex::new(Metrics::default())),
        task: tokio::spawn(async { true }),
        stop: Some(stop),
        control,
    };
    let gate = bridge.gate(Direction::Put).await;
    let cancel = bridge.cancel().await;
    let joined = bridge.finish_checked(false).await;
    assert!(joined.unwrap());
    assert_eq!(gate.unwrap_err(), "gate_control_closed");
    assert_eq!(cancel.unwrap_err(), "cancel_control_closed");
}
