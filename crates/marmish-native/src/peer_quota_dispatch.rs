//! Test-only default-VFS dispatch for the two fixed provisional Peer slots.

use super::*;

use crate::identity::ProfileLease;
use std::{
    cell::RefCell,
    ffi::OsStr,
    future::Future,
    marker::PhantomData,
    num::NonZeroU64,
    rc::Rc,
    sync::OnceLock,
    task::{Context, Poll, Waker},
};

const RAW_AUTOPROXY: c_int = 0x20;
pub(super) const CLOSED_STAGE_LIMIT: u64 = 16 * 1024 * 1024;
// The 128 MiB admission in the pending activation design. No production quota
// owner exists at this revision; the proof fixture supplies the other charges.
const TOTAL_ADMISSION: u64 = 128 * 1024 * 1024;
const NAMES: [&str; 2] = ["provisional-known.db", "provisional-unknown.db"];
const SIDECARS: [&str; 3] = ["-journal", "-wal", "-shm"];

type Result<T> = std::result::Result<T, &'static str>;
type NativeResult = std::result::Result<(ProvisionalPackage, bool), StageError>;

struct Dispatch {
    stock: *mut sql::sqlite3_vfs,
    managed: *mut sql::sqlite3_vfs,
    wrapper: *mut sql::sqlite3_vfs,
    paths: OnceLock<ReservedPaths>,
    slots: Mutex<[Slot; 2]>,
}

// SQLite owns the captured stock descriptors for process lifetime. Our
// descriptor is registered once and never mutated by this test harness again.
// Cross-thread reservation state uses OnceLock/Mutex, scope is thread-local.
unsafe impl Sync for Dispatch {}

#[derive(Default)]
struct Slot {
    generation: Option<NonZeroU64>,
    witness: Option<(u64, u64)>,
    active: Option<Arc<Quota>>,
    quarantined: bool,
}

struct ReservedPaths {
    profile_id: String,
    mains: [PathBuf; 2],
    all: [PathBuf; 8],
}

pub(crate) struct ProfileSlots<'a> {
    dispatch: &'static Dispatch,
    lease: &'a ProfileLease,
    charged_other: u64,
    crash_after_database_write: bool,
}

thread_local! {
    static ACTIVE: RefCell<Option<Arc<Quota>>> = const { RefCell::new(None) };
}

struct Turn<'a> {
    profile: &'a ProfileSlots<'a>,
    slot: usize,
    quota: Arc<Quota>,
    _local: PhantomData<Rc<()>>,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.borrow_mut().take();
        });
        let (mut slots, poisoned) = match self.profile.dispatch.slots.lock() {
            Ok(slots) => (slots, false),
            Err(error) => (error.into_inner(), true),
        };
        let slot = &mut slots[self.slot];
        if slot
            .active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.quota))
        {
            // The profile reservation and this guard are the only remaining
            // owning references after every initialized SQLite file closes.
            // A poisoned lock cannot authorize another turn. Recover its
            // guard only to quarantine and clear this exact ownership claim.
            if poisoned
                || Arc::strong_count(&self.quota) != 2
                || self.quota.verify_dispatch().is_err()
            {
                slot.quarantined = true;
            }
            slot.active = None;
        }
    }
}

impl Dispatch {
    fn mount() -> Result<&'static Self> {
        static DISPATCH: OnceLock<Result<&'static Dispatch>> = OnceLock::new();
        *DISPATCH.get_or_init(Self::register)
    }

    fn register() -> Result<&'static Self> {
        marmish_engine::initialize_storage().map_err(|_| "sqlite_initialize")?;
        let stock = unsafe { sql::sqlite3_vfs_find(ptr::null()) };
        if stock.is_null()
            || unsafe { (*stock).szOsFile } <= 0
            || unsafe { CStr::from_ptr((*stock).zName) } != c"unix"
        {
            return Err("stock_unix_vfs");
        }
        #[cfg(target_os = "linux")]
        let managed = stock;
        #[cfg(target_os = "macos")]
        let managed = unsafe { sql::sqlite3_vfs_find(c"unix-posix".as_ptr()) };
        if managed.is_null() || unsafe { (*managed).szOsFile } <= 0 {
            return Err("stock_managed_vfs");
        }
        if unsafe { sql::sqlite3_compileoption_used(c"TEMP_STORE=2".as_ptr()) } != 1
            || unsafe { sql::sqlite3_compileoption_used(c"PREFER_PROXY_LOCKING".as_ptr()) } != 0
        {
            return Err("sqlite_compile_options");
        }
        let descriptor = Box::new(forwarding_descriptor(unsafe { &*stock })?);
        let dispatch = Box::leak(Box::new(Self {
            stock,
            managed,
            wrapper: Box::into_raw(descriptor),
            paths: OnceLock::new(),
            slots: Mutex::default(),
        }));
        unsafe {
            (*dispatch.wrapper).pAppData = ptr::from_ref(dispatch).cast_mut().cast();
            if sql::sqlite3_vfs_register(dispatch.wrapper, 1) != sql::SQLITE_OK {
                return Err("dispatch_registration");
            }
        }
        dispatch.assert_default()?;
        Ok(dispatch)
    }

    fn assert_default(&self) -> Result<()> {
        if unsafe { sql::sqlite3_vfs_find(ptr::null()) } != self.wrapper {
            return Err("dispatch_displaced");
        }
        Ok(())
    }

    fn protected(&self, name: *const c_char) -> bool {
        let Some(paths) = self.paths.get() else {
            return false;
        };
        if name.is_null() {
            return false;
        }
        // Name is borrowed only for this callback. Pass the original pointer,
        // rather than reconstructed bytes, to the stock implementation.
        let path = Path::new(OsStr::from_bytes(
            unsafe { CStr::from_ptr(name) }.to_bytes(),
        ));
        if paths.all.iter().any(|reserved| reserved == path) {
            return true;
        }
        if let Some((parent, leaf)) = path.parent().zip(path.file_name())
            && let Ok(parent) = parent.canonicalize()
            && paths
                .all
                .iter()
                .any(|reserved| reserved == &parent.join(leaf))
        {
            return true;
        }
        // Match inode aliases even under unrelated basenames. Metadata errors
        // for ordinary opens do not alter the stock invocation or its result.
        let Ok(meta) = path.metadata() else {
            return false;
        };
        paths.all.iter().any(|reserved| {
            reserved
                .symlink_metadata()
                .is_ok_and(|other| witness(&meta) == witness(&other))
        })
    }
}

impl<'a> ProfileSlots<'a> {
    pub(crate) fn reserve(lease: &'a ProfileLease, charged_other: u64) -> Result<Self> {
        let dispatch = Dispatch::mount()?;
        dispatch.assert_default()?;
        let directory = lease.root().join("peer");
        let parent = directory
            .symlink_metadata()
            .map_err(|_| "stage_directory")?;
        if !parent.is_dir()
            || parent.uid() != unsafe { libc::geteuid() }
            || parent.mode() & 0o777 != 0o700
            || directory.canonicalize().as_ref().ok() != Some(&directory)
            || lease
                .root()
                .symlink_metadata()
                .map_err(|_| "profile_root")?
                .dev()
                != parent.dev()
            || !local_volume(&directory)?
        {
            return Err("dispatch_profile");
        }
        let paths = dispatch.paths.get_or_init(|| {
            let mains = NAMES.map(|name| directory.join(name));
            let all = std::array::from_fn(|index| {
                let main = &mains[index / 4];
                match index % 4 {
                    0 => main.clone(),
                    offset => sidecar(main, SIDECARS[offset - 1]),
                }
            });
            ReservedPaths {
                profile_id: lease.profile_id().to_owned(),
                mains,
                all,
            }
        });
        if paths.mains[0].parent() != Some(&directory) || paths.profile_id != lease.profile_id() {
            return Err("dispatch_profile_conflict");
        }
        if charged_other > TOTAL_ADMISSION {
            return Err("dispatch_admission");
        }
        Ok(Self {
            dispatch,
            lease,
            charged_other,
            crash_after_database_write: false,
        })
    }

    fn begin(&self, exact: RawBinding) -> Result<Turn<'_>> {
        self.dispatch.assert_default()?;
        if ACTIVE.with(|active| active.borrow().is_some()) {
            return Err("dispatch_recursive_turn");
        }
        let (index, generation) = (usize::from(!exact.known), exact.generation);
        let main = &self.dispatch.paths.get().expect("reserved_paths").mains[index];
        if main != exact.path || main.parent().and_then(Path::parent) != Some(self.lease.root()) {
            return Err("dispatch_stage_path");
        }
        let mut slots = self
            .dispatch
            .slots
            .lock()
            .map_err(|_| "dispatch_reservations")?;
        if slots.iter().any(|slot| slot.active.is_some()) {
            return Err("dispatch_active_turn");
        }
        let slot = &mut slots[index];
        if slot.quarantined
            || slot.generation.is_some_and(|current| generation < current)
            || (slot.generation == Some(generation) && slot.witness != Some(exact.witness))
        {
            return Err("dispatch_stage_generation");
        }
        let checked = main
            .symlink_metadata()
            .map_err(|_| "dispatch_stage_missing");
        let quota = (|| {
            let meta = checked?;
            if witness(&meta) != exact.witness
                || main
                    .parent()
                    .expect("stage_parent")
                    .symlink_metadata()
                    .map_err(|_| "stage_parent")?
                    .dev()
                    != meta.dev()
                || bytes(main)? != Some(meta.len())
                || meta.len() > CLOSED_STAGE_LIMIT
            {
                return Err("dispatch_stage_witness");
            }
            let mut quota = Quota::new(
                self.dispatch.managed,
                main,
                TOTAL_ADMISSION - self.charged_other,
                false,
                self.crash_after_database_write,
            )?;
            Arc::get_mut(&mut quota).expect("new_quota").witness = Some(exact.witness);
            Ok(quota)
        })();
        let quota = match quota {
            Ok(quota) => quota,
            Err(error) => {
                slot.generation = Some(generation);
                slot.witness = Some(exact.witness);
                slot.quarantined = true;
                return Err(error);
            }
        };
        slot.generation = Some(generation);
        slot.witness = Some(exact.witness);
        slot.active = Some(quota.clone());
        ACTIVE.with(|active| *active.borrow_mut() = Some(quota.clone()));
        Ok(Turn {
            profile: self,
            slot: index,
            quota,
            _local: PhantomData,
        })
    }

    fn finish(&self, turn: &Turn<'_>) -> Result<()> {
        self.dispatch.assert_default()?;
        if Arc::strong_count(&turn.quota) != 3 {
            return Err("dispatch_live_handles");
        }
        turn.quota.verify_dispatch()
    }

    pub(crate) fn native_turn(
        &self,
        binding: RawBinding<'_>,
        operation: NativeTurn<'_>,
    ) -> Result<NativeResult> {
        let turn = self.begin(binding)?;
        let result = operation.run(binding.path);
        self.finish(&turn)?;
        Ok(result)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RawBinding<'a> {
    pub(crate) path: &'a Path,
    pub(crate) generation: NonZeroU64,
    pub(crate) known: bool,
    pub(crate) witness: (u64, u64),
}

use crate::mdk_adapter::peer_native_signer::{OpenStage, ProvisionalPackage, StageError};
use nostr::key::Keys;
use std::time::Duration;
use storage_sqlite::SqlCipherKey;

pub(crate) struct NativeTurn<'a> {
    pub(crate) key: &'a SqlCipherKey,
    pub(crate) signer: Arc<Keys>,
    pub(crate) origin: &'a str,
    pub(crate) label: String,
    pub(crate) sample: Duration,
    pub(crate) operation: NativeOperation,
}

pub(crate) enum NativeOperation {
    Generate { withhold_signing: bool },
    Reopen { prepared: bool },
}

impl NativeTurn<'_> {
    fn run(self, path: &Path) -> NativeResult {
        resolve_once(self.run_unscoped(path)).map_err(|_| StageError::Unavailable)?
    }

    pub(crate) async fn run_unscoped(self, path: &Path) -> NativeResult {
        let native = OpenStage::open_provisional(
            path,
            self.key,
            self.signer,
            self.origin,
            self.label,
            matches!(self.operation, NativeOperation::Generate { .. }),
        )?;
        match self.operation {
            NativeOperation::Generate { withhold_signing } => native
                .generate_provisional(self.sample, withhold_signing)
                .await
                .map(|package| (package, false)),
            NativeOperation::Reopen { prepared } => {
                if prepared || native.provisional_is_signed(self.sample)? {
                    native
                        .read_provisional(self.sample)
                        .map(|package| (package, true))
                } else {
                    native
                        .finalize_provisional(self.sample)
                        .await
                        .map(|package| (package, false))
                }
            }
        }
    }
}

pub(crate) fn resolve_once<T>(future: impl Future<Output = T>) -> Result<T> {
    // The private publisher does not await I/O. Drop both the owning handle
    // and any uncompleted future before leaving the scoped caller thread.
    match std::pin::pin!(future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => Ok(result),
        Poll::Pending => Err("dispatch_pending"),
    }
}

fn witness(meta: &fs::Metadata) -> (u64, u64) {
    (meta.dev(), meta.ino())
}

fn local_volume(path: &Path) -> Result<bool> {
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| "stage_parent")?;
    let mut status = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::statfs(name.as_ptr(), status.as_mut_ptr()) } != 0 {
        return Err("stage_filesystem");
    }
    let status = unsafe { status.assume_init() };
    #[cfg(target_os = "macos")]
    let local = status.f_flags & libc::MNT_LOCAL as u32 != 0;
    #[cfg(target_os = "linux")]
    let local = matches!(
        status.f_type as u32,
        0xef53 | 0x58465342 | 0x9123683e | 0x01021994 | 0x794c7630 | 0x2fc12fc1
    );
    Ok(local)
}

fn dispatcher(vfs: *mut sql::sqlite3_vfs) -> &'static Dispatch {
    unsafe { &*(*vfs).pAppData.cast::<Dispatch>() }
}

unsafe extern "C" fn dispatch_open(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    outer: *mut sql::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    unsafe { (*outer).pMethods = ptr::null() };
    guarded(sql::SQLITE_CANTOPEN, || {
        if let Some(quota) = ACTIVE.with(|scope| scope.borrow().clone()) {
            if flags & RAW_AUTOPROXY != 0 {
                return sql::SQLITE_CANTOPEN;
            }
            return unsafe { open_managed(&quota, name, outer, flags, out_flags) };
        }
        let dispatch = dispatcher(vfs);
        if dispatch.protected(name) {
            return sql::SQLITE_CANTOPEN;
        }
        let base = dispatch.stock;
        unsafe { (*base).xOpen.expect("stock_open")(base, name, outer, flags, out_flags) }
    })
}

unsafe extern "C" fn dispatch_delete(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    sync_dir: c_int,
) -> c_int {
    guarded(sql::SQLITE_IOERR_DELETE, || {
        if let Some(quota) = ACTIVE.with(|scope| scope.borrow().clone()) {
            return delete_managed(&quota, name, sync_dir);
        }
        let dispatch = dispatcher(vfs);
        if dispatch.protected(name) {
            return sql::SQLITE_IOERR_DELETE;
        }
        let base = dispatch.stock;
        unsafe { (*base).xDelete.expect("stock_delete")(base, name, sync_dir) }
    })
}

macro_rules! delegate {
    ($name:ident, $member:ident, $ret:ty $(, $arg:ident : $ty:ty)*) => {
        unsafe extern "C" fn $name(vfs: *mut sql::sqlite3_vfs $(, $arg: $ty)*) -> $ret {
            let base = dispatcher(vfs).stock;
            // The corresponding stock callback was present at registration.
            unsafe { (*base).$member.unwrap_unchecked()(base $(, $arg)*) }
        }
    };
}

type DlSymbol = Option<unsafe extern "C" fn(*mut sql::sqlite3_vfs, *mut c_void, *const c_char)>;
delegate!(forward_access, xAccess, c_int, name: *const c_char, flag: c_int, result: *mut c_int);
delegate!(forward_path, xFullPathname, c_int, name: *const c_char, len: c_int, out: *mut c_char);
delegate!(forward_dl_open, xDlOpen, *mut c_void, name: *const c_char);
delegate!(forward_dl_error, xDlError, (), len: c_int, out: *mut c_char);
delegate!(forward_dl_symbol, xDlSym, DlSymbol, module: *mut c_void, symbol: *const c_char);
delegate!(forward_dl_close, xDlClose, (), module: *mut c_void);
delegate!(forward_random, xRandomness, c_int, len: c_int, out: *mut c_char);
delegate!(forward_sleep, xSleep, c_int, micros: c_int);
delegate!(forward_time, xCurrentTime, c_int, out: *mut f64);
delegate!(forward_error, xGetLastError, c_int, len: c_int, out: *mut c_char);
delegate!(forward_time_64, xCurrentTimeInt64, c_int, out: *mut i64);
delegate!(forward_set_system, xSetSystemCall, c_int, name: *const c_char, call: sql::sqlite3_syscall_ptr);
delegate!(forward_get_system, xGetSystemCall, sql::sqlite3_syscall_ptr, name: *const c_char);
delegate!(forward_next_system, xNextSystemCall, *const c_char, name: *const c_char);

fn forwarding_descriptor(stock: &sql::sqlite3_vfs) -> Result<sql::sqlite3_vfs> {
    if !(1..=3).contains(&stock.iVersion) || stock.xOpen.is_none() || stock.xDelete.is_none() {
        return Err("stock_vfs_topology");
    }
    let header: c_int = size_of::<Handle>().try_into().map_err(|_| "file_header")?;
    Ok(sql::sqlite3_vfs {
        iVersion: stock.iVersion,
        szOsFile: stock.szOsFile.max(header),
        mxPathname: stock.mxPathname,
        pNext: ptr::null_mut(),
        zName: c"marmish-test-peer-quota-dispatch".as_ptr(),
        pAppData: ptr::null_mut(),
        xOpen: Some(dispatch_open),
        xDelete: Some(dispatch_delete),
        xAccess: stock.xAccess.map(|_| forward_access as _),
        xFullPathname: stock.xFullPathname.map(|_| forward_path as _),
        xDlOpen: stock.xDlOpen.map(|_| forward_dl_open as _),
        xDlError: stock.xDlError.map(|_| forward_dl_error as _),
        xDlSym: stock.xDlSym.map(|_| forward_dl_symbol as _),
        xDlClose: stock.xDlClose.map(|_| forward_dl_close as _),
        xRandomness: stock.xRandomness.map(|_| forward_random as _),
        xSleep: stock.xSleep.map(|_| forward_sleep as _),
        xCurrentTime: stock.xCurrentTime.map(|_| forward_time as _),
        xGetLastError: stock.xGetLastError.map(|_| forward_error as _),
        xCurrentTimeInt64: stock.xCurrentTimeInt64.map(|_| forward_time_64 as _),
        xSetSystemCall: stock.xSetSystemCall.map(|_| forward_set_system as _),
        xGetSystemCall: stock.xGetSystemCall.map(|_| forward_get_system as _),
        xNextSystemCall: stock.xNextSystemCall.map(|_| forward_next_system as _),
    })
}

#[path = "peer_quota_dispatch_tests.rs"]
mod tests;

pub(crate) fn mount_for_proof() -> Result<()> {
    Dispatch::mount().map(|_| ())
}

pub(super) fn poison_scope() {
    let _ = ACTIVE.try_with(|scope| {
        if let Ok(active) = scope.try_borrow()
            && let Some(quota) = active.as_ref()
        {
            quota
                .panicked
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
}

impl ProfileSlots<'_> {
    pub(crate) fn fixture_stop_in_synced_transaction(mut self) -> Self {
        self.crash_after_database_write = true;
        self
    }
}
