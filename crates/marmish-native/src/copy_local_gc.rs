//! Same-UID local receiver lifecycle. Remote messages cannot call these operations.
use crate::{
    Error, Result,
    command_cli::Context,
    copy_files, ipc,
    stream_service::{StreamLocalRequest, StreamLocalResult},
};
use marmish_stream::{
    copy_protocol::{CopyError, Launch},
    wire::Binding,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Identity {
    pub device: u64,
    pub inode: u64,
}
impl Identity {
    pub(crate) fn read(file: &std::fs::File) -> Result<Self> {
        let m = file.metadata()?;
        Ok(Self {
            device: m.dev(),
            inode: m.ino(),
        })
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReceiverBase {
    pub path: Vec<u8>,
    pub identity: Identity,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Location {
    pub base: ReceiverBase,
    pub parent: Identity,
    pub stream: [u8; 16],
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Disposition {
    NeverSubmitted,
    Attempted,
    Established { location: Location },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LeaseId {
    pub namespace: String,
    pub stream: [u8; 16],
    pub revision: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub id: [u8; 16],
    pub operation: Operation,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Operation {
    Reserve {
        launch: Launch,
        binding: Binding,
        base: ReceiverBase,
    },
    Activate {
        lease: LeaseId,
    },
    Release {
        lease: LeaseId,
        disposition: Disposition,
    },
    Claim {
        exclude: String,
    },
    Complete {
        claim: [u8; 16],
        revision: u64,
        outcome: GcOutcome,
    },
    Forget {
        namespace: String,
        revision: u64,
        acknowledge_retained_state: bool,
    },
    Inspect,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Response {
    Rejected { code: CopyError },
    Reserved { lease: LeaseId, retention: u64 },
    Granted { claim: Option<Box<Claim>> },
    Entries { entries: Vec<Entry> },
    Ok,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub namespace: String,
    pub state: String,
    pub revision: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Claim {
    pub id: [u8; 16],
    pub namespace: String,
    pub revision: u64,
    pub launch: Launch,
    pub location: Location,
    pub timestamp: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum GcOutcome {
    Cleaned,
    Busy,
    NotExpired { expires: u64 },
    Uncertain,
    Transient,
}
pub(crate) fn namespace(l: &Launch) -> String {
    format!(
        "{}:{}:{}",
        hex::encode(l.client),
        hex::encode(l.server),
        hex::encode(l.transfer)
    )
}
fn fail(code: CopyError) -> Error {
    Error::Copy(code)
}
fn fresh() -> Result<[u8; 16]> {
    let mut id = [0; 16];
    id[..8].copy_from_slice(
        &crate::copy_worker::now()
            .map_err(|e| fail(e.code))?
            .to_be_bytes(),
    );
    getrandom::fill(&mut id[8..]).map_err(|_| Error::Identity("random"))?;
    Ok(id)
}
async fn request(context: &Context, operation: Operation) -> Result<Response> {
    let req = Request {
        id: fresh()?,
        operation,
    };
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut last = None;
        for _ in 0..2 {
            match ipc::stream_request(
                &context.socket,
                StreamLocalRequest::CopyReceiver {
                    request: Box::new(req.clone()),
                },
            )
            .await
            {
                Ok(StreamLocalResult::CopyReceiver { response }) => {
                    return match *response {
                        Response::Rejected { code } => Err(fail(code)),
                        response => Ok(response),
                    };
                }
                Ok(StreamLocalResult::Failure { .. }) => return Err(fail(CopyError::Conflict)),
                Ok(_) => return Err(fail(CopyError::Protocol)),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or(Error::Timeout))
    })
    .await
    .map_err(|_| Error::Timeout)?
}
pub(crate) struct Lease {
    context: Arc<Context>,
    id: LeaseId,
    base: ReceiverBase,
    retention: u64,
}
impl Lease {
    pub(crate) async fn reserve(
        context: &Arc<Context>,
        launch: &Launch,
        binding: &Binding,
    ) -> Result<Self> {
        let base = owned_filesystem(
            || {
                let path = std::fs::canonicalize(".")?;
                let file = copy_files::trusted_base(&path).map_err(|e| fail(e.code))?;
                Ok(ReceiverBase {
                    path: path.as_os_str().as_bytes().to_vec(),
                    identity: Identity::read(&file)?,
                })
            },
            Duration::from_secs(15),
        )
        .await?;
        // The sweep precedes Reserve so a full row index can still reclaim capacity.
        sweep(context, &namespace(launch)).await?;
        match request(
            context,
            Operation::Reserve {
                launch: launch.clone(),
                binding: binding.clone(),
                base: base.clone(),
            },
        )
        .await?
        {
            Response::Reserved { lease, retention } => Ok(Self {
                context: context.clone(),
                id: lease,
                base,
                retention,
            }),
            _ => Err(fail(CopyError::Protocol)),
        }
    }
    pub(crate) fn base(&self) -> ReceiverBase {
        self.base.clone()
    }
    pub(crate) fn retention(&self) -> u64 {
        self.retention
    }
    pub(crate) async fn activate(&self) -> Result<()> {
        match request(
            &self.context,
            Operation::Activate {
                lease: self.id.clone(),
            },
        )
        .await?
        {
            Response::Ok => Ok(()),
            _ => Err(fail(CopyError::Protocol)),
        }
    }
    pub(crate) async fn release(self, disposition: Disposition) -> Result<()> {
        match request(
            &self.context,
            Operation::Release {
                lease: self.id,
                disposition,
            },
        )
        .await?
        {
            Response::Ok => Ok(()),
            _ => Err(fail(CopyError::Protocol)),
        }
    }
}
/// Await physical completion before consuming an outcome. Timeout or future
/// cancellation detaches the owned thread without joining; callers retain the
/// durable claim and must not publish Complete for an unobserved result.
async fn owned_filesystem<T, F>(job: F, timeout: Duration) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let thread = std::thread::Builder::new()
        .name("marmish-copy-filesystem".into())
        .spawn(job)
        .map_err(|_| fail(CopyError::LocalIo))?;
    let deadline = tokio::time::Instant::now() + timeout;
    while !thread.is_finished() {
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    thread.join().map_err(|_| fail(CopyError::LocalIo))?
}
async fn sweep(context: &Context, exclude: &str) -> Result<()> {
    for _ in 0..8 {
        let claim = match request(
            context,
            Operation::Claim {
                exclude: exclude.to_owned(),
            },
        )
        .await?
        {
            Response::Granted { claim } => claim,
            _ => return Err(fail(CopyError::Protocol)),
        };
        let Some(claim) = claim else { break };
        let task = claim.clone();
        // A timed-out or dropped physical job retains its durable claim.
        // The owned thread is outside Tokio's blocking pool, so runtime teardown
        // does not wait indefinitely for stalled filesystem IO.
        let outcome = owned_filesystem(move || Ok(reclaim(&task)), Duration::from_secs(15)).await?;
        request(
            context,
            Operation::Complete {
                claim: claim.id,
                revision: claim.revision,
                outcome,
            },
        )
        .await?;
    }
    Ok(())
}
// The shared descriptor-anchored Get reclaim implementation is coordinated with
// copy_files ownership; this adapter never scans or authorizes unknown state.
fn reclaim(claim: &Claim) -> GcOutcome {
    let path = PathBuf::from(OsString::from_vec(claim.location.base.path.clone()));
    let base = match copy_files::trusted_base(Path::new(&path)) {
        Ok(base) => base,
        Err(_) => return GcOutcome::Uncertain,
    };
    let expected = claim.location.base.identity;
    let parent = claim.location.parent;
    let timestamp = match i64::try_from(claim.timestamp) {
        Ok(value) => value,
        Err(_) => return GcOutcome::Uncertain,
    };
    use marmish_stream::copy_protocol::{CopyError, FileIdentity};
    match copy_files::reclaim_local_get_authorized(
        &base,
        FileIdentity {
            device: expected.device,
            inode: expected.inode,
        },
        FileIdentity {
            device: parent.device,
            inode: parent.inode,
        },
        &claim.launch,
        claim.location.stream,
        timestamp,
    ) {
        Ok(copy_files::ReclaimOutcome::Cleaned) => GcOutcome::Cleaned,
        Ok(copy_files::ReclaimOutcome::NotExpired(expires)) => match u64::try_from(expires) {
            Ok(expires) => GcOutcome::NotExpired { expires },
            Err(_) => GcOutcome::Uncertain,
        },
        Ok(copy_files::ReclaimOutcome::UnanchoredAbsent) => GcOutcome::Uncertain,
        Err(error) => match error.code {
            CopyError::Busy => GcOutcome::Busy,
            CopyError::LocalIo
            | CopyError::DiskFull
            | CopyError::Quota
            | CopyError::PermissionDenied => GcOutcome::Transient,
            _ => GcOutcome::Uncertain,
        },
    }
}
pub(crate) async fn inspect(context: &Context) -> Result<Vec<Entry>> {
    match request(context, Operation::Inspect).await? {
        Response::Entries { entries } => Ok(entries),
        _ => Err(fail(CopyError::Protocol)),
    }
}
pub(crate) async fn forget(context: &Context, namespace: String, revision: u64) -> Result<()> {
    match request(
        context,
        Operation::Forget {
            namespace,
            revision,
            acknowledge_retained_state: true,
        },
    )
    .await?
    {
        Response::Ok => Ok(()),
        _ => Err(fail(CopyError::Protocol)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marmish_stream::copy_protocol::{
        self as cp, AttemptMode, Direction, FileIdentity, Manifest,
    };
    use sha2::{Digest, Sha256};
    #[tokio::test]
    async fn owned_filesystem_waits_for_thread_destructors_before_success() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Completed(Arc<AtomicBool>);
        impl Drop for Completed {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let finished = Arc::new(AtomicBool::new(false));
        let seen = finished.clone();
        let value = owned_filesystem(
            move || {
                let _completed = Completed(seen);
                Ok(42u8)
            },
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(value, 42);
        assert!(finished.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn owned_filesystem_timeout_and_dropped_future_do_not_join_running_job() {
        use std::sync::atomic::{AtomicBool, Ordering};
        for cancel in [false, true] {
            let (release, gate) = std::sync::mpsc::sync_channel(1);
            let entered = Arc::new(AtomicBool::new(false));
            let finished = Arc::new(AtomicBool::new(false));
            let active = entered.clone();
            let done = finished.clone();
            let task = tokio::spawn(owned_filesystem(
                move || {
                    active.store(true, Ordering::Release);
                    // Every test-owned thread also has its own bounded fallback.
                    let _ = gate.recv_timeout(Duration::from_secs(3));
                    done.store(true, Ordering::Release);
                    Ok(())
                },
                if cancel {
                    Duration::from_secs(2)
                } else {
                    Duration::from_millis(20)
                },
            ));
            let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
            while !entered.load(Ordering::Acquire) && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            assert!(entered.load(Ordering::Acquire));
            let result = if cancel {
                task.abort();
                task.await
            } else {
                task.await
            };
            // Release before assertions, including when the await failed.
            let was_running = !finished.load(Ordering::Acquire);
            let _ = release.send(());
            if cancel {
                assert!(result.unwrap_err().is_cancelled());
            } else {
                assert!(matches!(result.unwrap(), Err(Error::Timeout)));
            }
            assert!(was_running);
            let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
            while !finished.load(Ordering::Acquire) && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            assert!(finished.load(Ordering::Acquire));
        }
    }
    fn launch() -> Launch {
        Launch {
            version: cp::VERSION,
            mode: AttemptMode::Create,
            transfer: [3; 32],
            client: [1; 32],
            server: [2; 32],
            direction: Direction::Get,
            source: b"source".to_vec(),
            destination: b"parent/destination".to_vec(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
        }
    }
    fn fixture() -> (tempfile::TempDir, Claim, copy_files::CopyReceiver) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("parent")).unwrap();
        let path = dir.path().canonicalize().unwrap();
        let base = copy_files::trusted_base(&path).unwrap();
        let launch = launch();
        let parent = copy_files::PinnedPath::open(&base, &launch.destination)
            .unwrap()
            .parent_identity()
            .unwrap();
        let manifest = Manifest {
            version: launch.version,
            transfer: launch.transfer,
            client: launch.client,
            server: launch.server,
            direction: launch.direction,
            source: launch.source.clone(),
            destination: launch.destination.clone(),
            overwrite: false,
            preserve_mode: false,
            preserve_mtime: false,
            size: 0,
            sha256: Sha256::digest([]).into(),
            mode: None,
            mtime: None,
        };
        let receiver = copy_files::CopyReceiver::open_or_recover(
            &base,
            &launch.destination,
            manifest,
            AttemptMode::Create,
            [4; 16],
            1000,
            cp::MIN_RETENTION_SECS,
        )
        .ok()
        .unwrap();
        let claim = Claim {
            id: [5; 16],
            namespace: namespace(&launch),
            revision: 1,
            launch,
            location: Location {
                base: ReceiverBase {
                    path: path.as_os_str().as_bytes().to_vec(),
                    identity: Identity::read(&base).unwrap(),
                },
                parent: Identity {
                    device: parent.device,
                    inode: parent.inode,
                },
                stream: [4; 16],
            },
            timestamp: 1_000_000,
        };
        (dir, claim, receiver)
    }
    #[test]
    fn anchored_get_gc_is_busy_while_owned_then_collects_and_repeats_absence() {
        let (_dir, claim, receiver) = fixture();
        assert!(matches!(reclaim(&claim), GcOutcome::Busy));
        drop(receiver);
        assert!(matches!(reclaim(&claim), GcOutcome::Cleaned));
        assert!(matches!(reclaim(&claim), GcOutcome::Cleaned));
    }
    #[test]
    fn anchored_get_gc_refuses_replaced_parent_even_if_namespace_absent() {
        let (dir, claim, receiver) = fixture();
        drop(receiver);
        std::fs::rename(dir.path().join("parent"), dir.path().join("old")).unwrap();
        std::fs::create_dir(dir.path().join("parent")).unwrap();
        assert!(matches!(reclaim(&claim), GcOutcome::Uncertain));
        assert!(dir.path().join("old").read_dir().unwrap().count() > 0);
    }
    #[test]
    fn anchored_get_gc_checks_expiry_last_stream_and_base_identity() {
        let (_dir, mut claim, receiver) = fixture();
        drop(receiver);
        claim.timestamp = 1001;
        assert!(matches!(reclaim(&claim), GcOutcome::NotExpired { .. }));
        claim.timestamp = 1_000_000;
        claim.location.stream = [9; 16];
        assert!(matches!(reclaim(&claim), GcOutcome::Uncertain));
        claim.location.stream = [4; 16];
        claim.location.base.identity.inode += 1;
        assert!(matches!(reclaim(&claim), GcOutcome::Uncertain));
    }
    #[test]
    fn receiver_side_cannot_be_changed_to_gain_get_cleanup() {
        let (_dir, claim, receiver) = fixture();
        drop(receiver);
        let path = PathBuf::from(OsString::from_vec(claim.location.base.path));
        let base = copy_files::trusted_base(&path).unwrap();
        let mut put = claim.launch;
        put.direction = Direction::Put;
        let id = FileIdentity {
            device: 0,
            inode: 0,
        };
        assert!(
            copy_files::reclaim_local_get_authorized(&base, id, id, &put, [4; 16], 1_000_000)
                .is_err()
        );
    }
}
