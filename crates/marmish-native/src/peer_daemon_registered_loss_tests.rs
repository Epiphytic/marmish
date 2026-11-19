//! Private, process-owned qualification of registered dispatcher loss.
#[path = "peer_daemon_registered_loss_snapshot.rs"]
mod snapshot;
use self::snapshot::Image;
use crate::{
    command_engine::Admission,
    daemon_launch::{StartupReporter, candidate_child::Failure},
    dispatcher_fatal::Fatal,
    ipc::Response,
    mdk_adapter::healthy_stop::OwnerReport,
};
use rusqlite::ffi as sql;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    os::unix::{ffi::OsStringExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::time::Instant;

const BOOT: Duration = Duration::from_secs(5);
const FINISH: Duration = Duration::from_secs(2);
const FRAME_CAP: usize = 4200;
const FAILURE_EXIT: i32 = 86;
const PROFILE: &str = "test";
const SELECTOR: &str = concat!(module_path!(), "::dispatcher_loss_child");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Ready = 1,
    Status = 2,
}

struct ChildControl {
    scenario: Scenario,
    root: PathBuf,
    writer: Mutex<UnixStream>,
    release: Mutex<mpsc::Receiver<()>>,
    cut: AtomicU8,
    active_status: AtomicU8,
    admission: OnceLock<Admission>,
    ready_failure: AtomicU8,
    finish: AtomicU8,
}

static CONTROL: OnceLock<ChildControl> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatusState {
    Activating,
    Active,
    Degraded,
    Unexpected,
}

fn status_state(response: &Response) -> StatusState {
    if !response.ok
        || response.error.is_some()
        || response.command.is_some()
        || response.stream.is_some()
        || response.terminal_stream.is_some()
    {
        return StatusState::Unexpected;
    }
    match response
        .result
        .as_ref()
        .and_then(|result| result.get("state"))
        .and_then(serde_json::Value::as_str)
    {
        Some("activating") => StatusState::Activating,
        Some("active") => StatusState::Active,
        Some("transport_degraded") => StatusState::Degraded,
        _ => StatusState::Unexpected,
    }
}

macro_rules! refusal_categories {
    ($($name:ident = $category:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        enum Refusal { $($name),+ }

        impl Refusal {
            fn code(self) -> &'static str {
                match self { $(Self::$name => $category),+ }
            }

            fn from_bytes(bytes: &[u8]) -> Option<Self> {
                match std::str::from_utf8(bytes).ok()? {
                    $($category => Some(Self::$name),)+
                    _ => None,
                }
            }
        }
    };
}

refusal_categories! {
    Boot = "control_boot",
    ReleaseFrame = "release_frame",
    ReleaseOrder = "release_order",
    ReleaseReceiver = "release_receiver",
    ControlLost = "control_lost_after_release",
    CandidateReturned = "candidate_returned",
    RepeatedCut = "repeated_cut",
    DeadlineBeforeImage = "deadline_before_image",
    ImageDeadline = "image_deadline",
    ImageUnavailable = "image_unavailable",
    DeadlineAfterImage = "deadline_after_image",
    DispatcherReplacement = "dispatcher_replacement",
    SampleWrite = "sample_write",
    OwnAdmission = "own_admission",
    BeforeCommitAdmission = "before_commit_admission",
    ReadinessFailure = "readiness_failure",
    StatusDegradedBeforeCut = "status_degraded_before_cut",
    RepeatedStatus = "repeated_status",
    BeforeStatusAdmission = "before_status_admission",
    StatusFatalUnobserved = "status_fatal_unobserved",
    FinishBasis = "finish_basis",
    FinishCut = "finish_cut",
    FinishWrite = "finish_write",
    FinishRelease = "finish_release",
}

fn refuse_at(stage: Refusal) -> ! {
    crate::diagnostics::event("registered_fault_fixture", stage.code());
    if let Some(control) = CONTROL.get()
        && let Ok(mut writer) = control.writer.try_lock()
    {
        let _ = send(&mut writer, b'X', stage.code().as_bytes());
    }
    std::process::exit(FAILURE_EXIT);
}

fn send(stream: &mut UnixStream, tag: u8, body: &[u8]) -> io::Result<()> {
    if body.len() > FRAME_CAP {
        return Err(invalid());
    }
    let mut header = [0; 5];
    header[0] = tag;
    header[1..].copy_from_slice(&(body.len() as u32).to_be_bytes());
    stream.write_all(&header)?;
    stream.write_all(body)
}

fn recv(stream: &mut UnixStream, cap: usize) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0; 5];
    stream.read_exact(&mut header)?;
    let size = u32::from_be_bytes(header[1..].try_into().map_err(|_| invalid())?) as usize;
    if size > cap {
        return Err(invalid());
    }
    let mut body = vec![0; size];
    stream.read_exact(&mut body)?;
    Ok((header[0], body))
}

fn invalid() -> io::Error {
    io::Error::other("dispatcher-loss controller frame")
}

fn decode_boot(mut body: &[u8]) -> io::Result<(Scenario, PathBuf, Vec<OsString>)> {
    let scenario = match body.first() {
        Some(1) => Scenario::Ready,
        Some(2) => Scenario::Status,
        _ => return Err(invalid()),
    };
    body = &body[1..];
    let mut next = || -> io::Result<OsString> {
        let mut len = [0; 2];
        body.read_exact(&mut len)?;
        let len = usize::from(u16::from_be_bytes(len));
        if len == 0 || len > 1024 {
            return Err(invalid());
        }
        let (bytes, rest) = body.split_at_checked(len).ok_or_else(invalid)?;
        if bytes.contains(&0) {
            return Err(invalid());
        }
        body = rest;
        Ok(OsString::from_vec(bytes.to_owned()))
    };
    let daemon = next()?;
    let state = next()?;
    let relay = next()?;
    let ca = next()?;
    if !body.is_empty() {
        return Err(invalid());
    }
    let root = Path::new(&state).join(PROFILE);
    let args = vec![daemon, state, PROFILE.into(), relay, "off".into(), ca];
    Ok((scenario, root, args))
}

fn start_control(mut reader: UnixStream) -> io::Result<Vec<OsString>> {
    reader.set_read_timeout(Some(BOOT))?;
    let (tag, body) = recv(&mut reader, FRAME_CAP)?;
    if tag != b'B' {
        return Err(invalid());
    }
    let (scenario, root, args) = decode_boot(&body)?;
    reader.set_read_timeout(None)?;
    let writer = reader.try_clone()?;
    writer.set_write_timeout(Some(FINISH))?;
    let (tx, release) = mpsc::sync_channel(1);
    CONTROL
        .set(ChildControl {
            scenario,
            root,
            writer: Mutex::new(writer),
            release: Mutex::new(release),
            cut: AtomicU8::new(0),
            active_status: AtomicU8::new(0),
            admission: OnceLock::new(),
            ready_failure: AtomicU8::new(0),
            finish: AtomicU8::new(0),
        })
        .map_err(|_| invalid())?;
    std::thread::Builder::new()
        .name("marmish-test-fault-control".into())
        .spawn(move || {
            let Ok((b'G', body)) = recv(&mut reader, 0) else {
                refuse_at(Refusal::ReleaseFrame)
            };
            let control = CONTROL.get().unwrap_or_else(|| refuse_at(Refusal::Boot));
            // Serialize against exact completion of the physical frame write.
            let writer = control
                .writer
                .lock()
                .unwrap_or_else(|_| refuse_at(Refusal::ReleaseOrder));
            if !body.is_empty() || control.finish.load(Ordering::Acquire) != 1 {
                drop(writer);
                refuse_at(Refusal::ReleaseOrder);
            }
            drop(writer);
            if tx.send(()).is_err() {
                refuse_at(Refusal::ReleaseReceiver);
            }
            // The test child exits without further control traffic. Extra
            // bytes or controller loss before that race are fixture failure.
            let _ = recv(&mut reader, 0);
            refuse_at(Refusal::ControlLost);
        })?;
    Ok(args)
}

#[test]
#[ignore = "private child selected by the bounded dispatcher-loss controller"]
fn dispatcher_loss_child() {
    let Ok((startup, control)) = StartupReporter::claim_fault_fixture() else {
        std::process::exit(FAILURE_EXIT)
    };
    crate::diagnostics::enable();
    let args = start_control(control).unwrap_or_else(|_| refuse_at(Refusal::Boot));
    let _ = crate::registered_qualification::run_fault_fixture(startup, args.into_iter());
    // A qualified child must terminate through the unmodified fatal path.
    refuse_at(Refusal::CandidateReturned);
}

fn displace(control: &ChildControl, deadline: Instant) {
    let one_second = Duration::from_secs(1);
    if control.cut.swap(1, Ordering::AcqRel) != 0 {
        refuse_at(Refusal::RepeatedCut);
    }
    if deadline.saturating_duration_since(Instant::now()) < one_second {
        refuse_at(Refusal::DeadlineBeforeImage);
    }
    let image = Image::capture(&control.root).unwrap_or_else(|error| {
        if error.kind() == io::ErrorKind::TimedOut {
            refuse_at(Refusal::ImageDeadline);
        }
        refuse_at(Refusal::ImageUnavailable);
    });
    if deadline.saturating_duration_since(Instant::now()) < one_second {
        refuse_at(Refusal::DeadlineAfterImage);
    }
    // SAFETY: this exact test child owns live registration, the static
    // initialized unix descriptor lasts for the process, and no restore runs.
    unsafe {
        let stock = sql::sqlite3_vfs_find(c"unix".as_ptr());
        let current = sql::sqlite3_vfs_find(std::ptr::null());
        if stock.is_null()
            || current.is_null()
            || current == stock
            || sql::sqlite3_vfs_register(stock, 1) != sql::SQLITE_OK
            || sql::sqlite3_vfs_find(std::ptr::null()) != stock
        {
            refuse_at(Refusal::DispatcherReplacement);
        }
    }
    let mut frame = Vec::with_capacity(1 + image.bytes().len());
    frame.push(control.scenario as u8);
    frame.extend_from_slice(image.bytes());
    let mut writer = control
        .writer
        .lock()
        .unwrap_or_else(|_| refuse_at(Refusal::SampleWrite));
    if send(&mut writer, b'S', &frame).is_err() {
        drop(writer);
        refuse_at(Refusal::SampleWrite);
    }
}

pub(crate) fn own_admission(admission: &Admission) {
    if let Some(control) = CONTROL.get()
        && (!admission.admit() || control.admission.set(admission.clone()).is_err())
    {
        refuse_at(Refusal::OwnAdmission);
    }
}

pub(crate) fn before_commit(admission: &Admission, deadline: Instant) {
    if let Some(control) = CONTROL.get()
        && control.scenario == Scenario::Ready
    {
        if !admission.admit() {
            refuse_at(Refusal::BeforeCommitAdmission);
        }
        displace(control, deadline);
    }
}

pub(crate) fn readiness_failure(failure: Failure) {
    if let Some(control) = CONTROL.get()
        && control.scenario == Scenario::Ready
    {
        if failure != Failure::Fatal || control.cut.load(Ordering::Acquire) != 1 {
            refuse_at(Refusal::ReadinessFailure);
        }
        control.ready_failure.store(1, Ordering::Release);
    }
}

pub(crate) async fn before_status_write(
    response: &Response,
    fatal: Option<&Fatal>,
    deadline: Instant,
) {
    let Some(control) = CONTROL.get() else { return };
    let Some(fatal) = fatal else { return };
    if control.scenario != Scenario::Status {
        return;
    }
    match status_state(response) {
        StatusState::Active => {}
        StatusState::Degraded if control.active_status.load(Ordering::Acquire) == 1 => {
            refuse_at(Refusal::StatusDegradedBeforeCut);
        }
        _ => return,
    }
    match control.active_status.fetch_add(1, Ordering::AcqRel) {
        0 => return,
        1 => {}
        _ => refuse_at(Refusal::RepeatedStatus),
    }
    if fatal.is_tripped() || !control.admission.get().is_some_and(Admission::admit) {
        refuse_at(Refusal::BeforeStatusAdmission);
    }
    displace(control, deadline);
    let observed_by = deadline - Duration::from_millis(250);
    // The live owner and the real listener may independently observe the
    // same latch, and the listener may abort this task only after it trips.
    if tokio::time::timeout_at(observed_by, fatal.tripped())
        .await
        .is_err()
    {
        refuse_at(Refusal::StatusFatalUnobserved);
    }
}

pub(crate) fn before_fatal_exit(report: &OwnerReport, listener_joined: bool, fatal: bool) {
    let Some(control) = CONTROL.get() else { return };
    let basis = match control.scenario {
        Scenario::Ready if control.ready_failure.load(Ordering::Acquire) == 1 => 1,
        Scenario::Status if control.active_status.load(Ordering::Acquire) == 2 => 2,
        _ => refuse_at(Refusal::FinishBasis),
    };
    if control.cut.load(Ordering::Acquire) != 1 {
        refuse_at(Refusal::FinishCut);
    }
    let mut body = vec![
        basis,
        u8::from(listener_joined),
        u8::from(fatal),
        u8::from(report.graceful_complete),
    ];
    body.extend_from_slice(&report.fixture_physical());
    let mut writer = control
        .writer
        .lock()
        .unwrap_or_else(|_| refuse_at(Refusal::FinishWrite));
    if control.finish.load(Ordering::Acquire) != 0 || send(&mut writer, b'P', &body).is_err() {
        drop(writer);
        refuse_at(Refusal::FinishWrite);
    }
    control.finish.store(1, Ordering::Release);
    drop(writer);
    let release = control
        .release
        .lock()
        .unwrap_or_else(|_| refuse_at(Refusal::FinishRelease));
    if release.recv_timeout(FINISH).is_err() {
        refuse_at(Refusal::FinishRelease);
    }
}

#[path = "peer_daemon_registered_loss_parent.rs"]
mod parent;
