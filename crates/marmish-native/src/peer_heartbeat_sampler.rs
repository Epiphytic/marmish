//! Bounded local snapshots for a future private Peer heartbeat publisher.
use crate::command_limits::continuous_time;
use crate::peer_status_wire::{DiskStatus, HostStatus, MAX_DISKS, safe_text};
use std::ffi::{CStr, CString};
use std::io;
use std::net::IpAddr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
};
use std::thread;
use std::time::Duration;

#[path = "peer_heartbeat_sampler_native.rs"]
mod native;

const MAX_INTERFACES: usize = 8;
const MAX_PATH_BYTES: usize = 4095;
const MAX_BUDGET: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct BaseFields {
    pub(crate) username: bool,
    pub(crate) uptime: bool,
    pub(crate) load: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Families {
    V4,
    V6,
    Both,
}

impl Families {
    fn includes(self, family: i32) -> bool {
        match family {
            libc::AF_INET => matches!(self, Self::V4 | Self::Both),
            libc::AF_INET6 => matches!(self, Self::V6 | Self::Both),
            _ => false,
        }
    }
}

pub(crate) struct AddressSelection(Vec<(String, Families)>);

impl AddressSelection {
    // The caller still needs to authorize this selection for a given audience.
    pub(crate) fn new(mut interfaces: Vec<(String, Families)>) -> Option<Self> {
        if !(1..=MAX_INTERFACES).contains(&interfaces.len())
            || interfaces.iter().any(|(name, _)| {
                name.is_empty() || name.len() >= libc::IFNAMSIZ || name.as_bytes().contains(&0)
            })
        {
            return None;
        }
        interfaces.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        if interfaces.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return None;
        }
        Some(Self(interfaces))
    }

    fn families(&self, name: &[u8]) -> Option<Families> {
        self.0
            .iter()
            .find_map(|(configured, families)| (configured.as_bytes() == name).then_some(*families))
    }
}

pub(crate) struct Request {
    fields: BaseFields,
    disks: Vec<(String, CString)>,
    addresses: Option<AddressSelection>,
}

impl Request {
    pub(crate) fn new(
        fields: BaseFields,
        mut disks: Vec<(String, PathBuf)>,
        addresses: Option<AddressSelection>,
    ) -> Option<Self> {
        if disks.len() > MAX_DISKS {
            return None;
        }
        disks.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        if disks.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return None;
        }
        let disks = disks
            .into_iter()
            .map(|(label, path)| {
                let bytes = path.as_os_str().as_bytes();
                if !safe_text(&label, 32)
                    || label.contains('/')
                    || !bytes.starts_with(b"/")
                    || bytes.starts_with(b"//")
                    || bytes.len() > MAX_PATH_BYTES
                {
                    return None;
                }
                Some((label, CString::new(bytes).ok()?))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            fields,
            disks,
            addresses,
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum StartError {
    Busy,
    InvalidBudget,
    Unavailable,
}

pub(crate) enum Progress {
    Idle,
    Pending,
    Ready(HostStatus),
    Expired,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stopped {
    Expired,
    Unavailable,
}

impl From<Stopped> for Progress {
    fn from(stopped: Stopped) -> Self {
        match stopped {
            Stopped::Expired => Self::Expired,
            Stopped::Unavailable => Self::Unavailable,
        }
    }
}

type Clock = Arc<dyn Fn() -> Option<Duration> + Send + Sync>;

struct Deadline {
    clock: Clock,
    latest: Duration,
    end: Duration,
}

impl Deadline {
    fn check(&mut self) -> Result<Duration, Stopped> {
        let now = (self.clock)().ok_or(Stopped::Unavailable)?;
        if now < self.latest {
            return Err(Stopped::Unavailable);
        }
        self.latest = now;
        if now >= self.end {
            return Err(Stopped::Expired);
        }
        Ok(now)
    }

    fn inspect<T>(
        &mut self,
        source: impl FnOnce(&mut Self) -> Result<Option<T>, Stopped>,
    ) -> Result<Option<T>, Stopped> {
        self.check()?;
        let value = source(self)?;
        self.check()?;
        Ok(value)
    }
}

trait Sources {
    fn username(&mut self, deadline: &mut Deadline) -> Result<Option<String>, Stopped>;
    fn uptime(&mut self) -> Option<u64>;
    fn load(&mut self) -> Option<[u32; 3]>;
    fn disk(&mut self, label: &str, path: &CStr) -> Option<DiskStatus>;
    fn addresses(
        &mut self,
        selection: &AddressSelection,
        deadline: &mut Deadline,
    ) -> Result<Option<Vec<IpAddr>>, Stopped>;
}

fn sample(
    source: &mut impl Sources,
    request: &Request,
    deadline: &mut Deadline,
) -> Result<HostStatus, Stopped> {
    let mut host = HostStatus::default();
    if request.fields.username {
        host.username = deadline.inspect(|deadline| source.username(deadline))?;
    }
    if request.fields.uptime {
        host.uptime = deadline.inspect(|_| Ok(source.uptime()))?;
    }
    if request.fields.load {
        host.load = deadline.inspect(|_| Ok(source.load()))?;
    }
    let mut disks = Vec::new();
    for (label, path) in &request.disks {
        if let Some(disk) = deadline.inspect(|_| Ok(source.disk(label, path)))? {
            disks.push(disk);
        }
    }
    if !disks.is_empty() {
        host.disks = Some(disks);
    }
    if let Some(selection) = &request.addresses {
        host.ips = deadline.inspect(|deadline| source.addresses(selection, deadline))?;
    }
    if host.valid() {
        Ok(host)
    } else {
        Err(Stopped::Unavailable)
    }
}

type Outcome = Result<(Duration, HostStatus), Stopped>;

struct Job {
    request: Request,
    deadline: Deadline,
    response: SyncSender<Outcome>,
}

struct Pending {
    response: Receiver<Outcome>,
    deadline: Deadline,
    draining: bool,
}

struct Permit(Arc<AtomicBool>);

impl Permit {
    fn reserve(domain: &Arc<AtomicBool>) -> Result<Self, StartError> {
        domain
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| StartError::Busy)?;
        Ok(Self(Arc::clone(domain)))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(crate) struct Sampler {
    jobs: SyncSender<Job>,
    pending: Option<Pending>,
    clock: Clock,
}

type Worker = Box<dyn FnOnce() + Send>;

fn spawn_thread(worker: Worker) -> io::Result<()> {
    thread::Builder::new()
        .name("marmish-peer-sample".into())
        .spawn(worker)
        .map(|_| ())
}

impl Sampler {
    pub(crate) fn spawn() -> Result<Self, StartError> {
        static PERMIT: OnceLock<Arc<AtomicBool>> = OnceLock::new();
        let domain = PERMIT.get_or_init(|| Arc::new(AtomicBool::new(false)));
        Self::spawn_with(
            domain,
            Arc::new(|| continuous_time().ok()),
            native::Native,
            spawn_thread,
        )
    }

    fn spawn_with(
        domain: &Arc<AtomicBool>,
        clock: Clock,
        mut source: impl Sources + Send + 'static,
        launch: impl FnOnce(Worker) -> io::Result<()>,
    ) -> Result<Self, StartError> {
        let permit = Permit::reserve(domain)?;
        let (jobs, queue) = sync_channel::<Job>(1);
        launch(Box::new(move || {
            let _permit = permit;
            while let Ok(mut job) = queue.recv() {
                let result = (|| {
                    job.deadline.check()?;
                    let host = sample(&mut source, &job.request, &mut job.deadline)?;
                    let completed_at = job.deadline.check()?;
                    Ok((completed_at, host))
                })();
                let _ = job.response.try_send(result);
            }
        }))
        .map_err(|_| StartError::Unavailable)?;
        Ok(Self {
            jobs,
            pending: None,
            clock,
        })
    }

    pub(crate) fn start(&mut self, request: Request, budget: Duration) -> Result<(), StartError> {
        if self.pending.is_some() {
            return Err(StartError::Busy);
        }
        if budget.is_zero() || budget > MAX_BUDGET {
            return Err(StartError::InvalidBudget);
        }
        let now = (self.clock)().ok_or(StartError::Unavailable)?;
        let end = now.checked_add(budget).ok_or(StartError::InvalidBudget)?;
        let deadline = || Deadline {
            clock: Arc::clone(&self.clock),
            latest: now,
            end,
        };
        let (response, result) = sync_channel(1);
        self.jobs
            .try_send(Job {
                request,
                deadline: deadline(),
                response,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => StartError::Busy,
                TrySendError::Disconnected(_) => StartError::Unavailable,
            })?;
        self.pending = Some(Pending {
            response: result,
            deadline: deadline(),
            draining: false,
        });
        Ok(())
    }

    pub(crate) fn poll(&mut self) -> Progress {
        let Some(pending) = &mut self.pending else {
            return Progress::Idle;
        };
        if !pending.draining
            && let Err(stopped) = pending.deadline.check()
        {
            pending.draining = true;
            return stopped.into();
        }
        let outcome = pending.response.try_recv();
        if matches!(outcome, Err(TryRecvError::Empty)) {
            return Progress::Pending;
        }
        let Some(mut pending) = self.pending.take() else {
            return Progress::Unavailable;
        };
        if pending.draining {
            return Progress::Idle;
        }
        match outcome {
            Ok(Ok((completed_at, host))) => {
                pending.deadline.latest = pending.deadline.latest.max(completed_at);
                match pending.deadline.check() {
                    Ok(_) => Progress::Ready(host),
                    Err(stopped) => stopped.into(),
                }
            }
            Ok(Err(stopped)) => stopped.into(),
            Err(_) => Progress::Unavailable,
        }
    }
}

#[cfg(test)]
#[path = "peer_heartbeat_sampler_tests.rs"]
mod tests;
