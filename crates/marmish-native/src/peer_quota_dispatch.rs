//! Default-VFS routing and scoped quota for fixed provisional Peer slots.

use super::*;

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "peer_founder_quota.rs"]
pub(crate) mod founder;
#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "peer_quota_recovery_guard_tests.rs"]
pub(crate) mod recovery_guard_tests;

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "peer_quota_candidate_registration.rs"]
pub(crate) mod candidate_registration;

use crate::{command_config::CommandConfig, identity::ProfileLease};
use std::{
    cell::RefCell, ffi::OsStr, marker::PhantomData, num::NonZeroU64, rc::Rc, sync::OnceLock,
};

const RAW_AUTOPROXY: c_int = 0x20;
pub(crate) const CLOSED_STAGE_LIMIT: u64 = 16 * 1024 * 1024;
// The 128 MiB admission in the pending activation design. No production quota
// owner exists at this revision; the proof fixture supplies the other charges.
pub(crate) const TOTAL_ADMISSION: u64 = 128 * 1024 * 1024;
const NAMES: [&str; 2] = ["provisional-known.db", "provisional-unknown.db"];
#[cfg(any(test, marmish_peer_receiver_candidate))]
const SNAPSHOT: usize = NAMES.len();
#[cfg(test)]
const SNAPSHOT_NAME: &str = "terminal-leave-snapshot.db";
#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
const RECEIVER_SNAPSHOT_NAME: &str = "receiver-join-snapshot.db";
const SLOT_COUNT: usize = NAMES.len() + cfg!(any(test, marmish_peer_receiver_candidate)) as usize;
const SIDECARS: [&str; 3] = ["-journal", "-wal", "-shm"];

type Result<T> = std::result::Result<T, &'static str>;
#[cfg(test)]
type NativeResult = std::result::Result<(ProvisionalPackage, bool), StageError>;

struct Dispatch {
    stock: *mut sql::sqlite3_vfs,
    managed: *mut sql::sqlite3_vfs,
    wrapper: *mut sql::sqlite3_vfs,
    paths: OnceLock<ReservedPaths>,
    slots: Mutex<[Slot; SLOT_COUNT]>,
    #[cfg(any(test, marmish_peer_receiver_candidate))]
    file_turn: Mutex<()>,
    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    founder_paths: OnceLock<founder::FounderPaths>,
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
    root: PathBuf,
    directories: [(u64, u64); 2],
    mains: [PathBuf; SLOT_COUNT],
    all: [PathBuf; SLOT_COUNT * 4],
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    receiver: Option<std::sync::Weak<ProfileLease>>,
}

impl ReservedPaths {
    fn directory_identities(root: &Path) -> Result<[(u64, u64); 2]> {
        let directories = [root.to_path_buf(), root.join("peer")];
        let mut identities = [(0, 0); 2];
        for (identity, path) in identities.iter_mut().zip(&directories) {
            let meta = path.symlink_metadata().map_err(|_| "stage_directory")?;
            // SAFETY: reads this process's effective UID.
            if !meta.is_dir()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.mode() & 0o7777 != 0o700
                || path.canonicalize().as_ref().ok() != Some(path)
            {
                return Err("dispatch_profile");
            }
            *identity = witness(&meta);
        }
        if identities[0].0 != identities[1].0 {
            return Err("dispatch_profile");
        }
        Ok(identities)
    }

    #[cfg(test)]
    fn new(lease: &ProfileLease, snapshot: &str) -> Result<Self> {
        let root = lease.root().to_path_buf();
        let directory = root.join("peer");
        let directories = Self::directory_identities(&root)?;
        if !local_volume(&directory)? {
            return Err("dispatch_profile");
        }
        Ok(Self::with_names(lease, directories, &directory, snapshot))
    }

    #[cfg(any(test, marmish_peer_receiver_candidate))]
    fn with_names(
        lease: &ProfileLease,
        directories: [(u64, u64); 2],
        directory: &Path,
        snapshot: &str,
    ) -> Self {
        let mains = [NAMES[0], NAMES[1], snapshot].map(|name| directory.join(name));
        let all = std::array::from_fn(|index| {
            let main = &mains[index / 4];
            match index % 4 {
                0 => main.clone(),
                offset => sidecar(main, SIDECARS[offset - 1]),
            }
        });
        Self {
            profile_id: lease.profile_id().to_owned(),
            root: lease.root().to_path_buf(),
            directories,
            mains,
            all,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            receiver: None,
        }
    }

    fn check(&self, lease: &ProfileLease) -> Result<()> {
        if self.root != lease.root()
            || self.profile_id != lease.profile_id()
            || Self::directory_identities(lease.root())? != self.directories
        {
            return Err("dispatch_profile_conflict");
        }
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(owner) = &self.receiver
            && owner
                .upgrade()
                .is_none_or(|owner| !ptr::eq(owner.as_ref(), lease))
        {
            return Err("dispatch_lease_conflict");
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) struct ProfileSlots<'a> {
    dispatch: &'static Dispatch,
    charged_other: u64,
    crash_after_database_write: bool,
    owner: ReceiverQuota<'a>,
}

/// Validated receive intent for this lease incarnation. No relay or quota
/// admission can be constructed from this candidate alone.
pub(crate) struct ReceiverCandidate {
    lease: Arc<ProfileLease>,
    config: CommandConfig,
}

impl ReceiverCandidate {
    pub(crate) fn evaluate(lease: &Arc<ProfileLease>) -> crate::Result<Option<Self>> {
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Ok(None);
        }
        let config = match CommandConfig::load(lease.root()) {
            Ok(config) => config,
            Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if config.peer_receive().is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            lease: lease.clone(),
            config,
        }))
    }

    pub(crate) fn recheck(&self, lease: &Arc<ProfileLease>) -> crate::Result<()> {
        if !Arc::ptr_eq(&self.lease, lease) {
            return Err(crate::Error::Invalid("peer_profile_changed"));
        }
        let loaded = CommandConfig::load(lease.root())?;
        self.config.bound_peer_relay(&loaded)?;
        Ok(())
    }
}

/// A mounted owner has no production constructor in this phase.
pub(crate) struct ReceiverQuota<'a> {
    dispatch: &'static Dispatch,
    lease: &'a ProfileLease,
}

#[derive(Clone, Copy)]
enum SlotClass {
    Known,
    Unknown,
    #[cfg(any(test, marmish_peer_receiver_candidate))]
    Snapshot,
}

impl SlotClass {
    fn index(self) -> usize {
        match self {
            Self::Known => 0,
            Self::Unknown => 1,
            #[cfg(any(test, marmish_peer_receiver_candidate))]
            Self::Snapshot => SNAPSHOT,
        }
    }
}

pub(crate) struct Reservation<'a> {
    owner: &'a ReceiverQuota<'a>,
    class: SlotClass,
    generation: NonZeroU64,
    witness: (u64, u64),
    charged_other: u64,
    #[cfg(test)]
    crash_after_database_write: bool,
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(crate) struct RecoveryRequest<'a> {
    pub(crate) generation: NonZeroU64,
    pub(crate) files: crate::peer_invitation::RollbackFiles,
    pub(crate) charged_other: u64,
    pub(crate) key: &'a SqlCipherKey,
    #[cfg(test)]
    pub(crate) interrupt_after_write: bool,
}

impl ReceiverQuota<'_> {
    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(crate) fn bind_receiver(lease: &Arc<ProfileLease>) -> Result<ReceiverQuota<'_>> {
        let mut paths = ReservedPaths::new(lease, RECEIVER_SNAPSHOT_NAME)?;
        paths.receiver = Some(Arc::downgrade(lease));
        let dispatch = Dispatch::bind(lease, paths)?;
        Ok(ReceiverQuota { dispatch, lease })
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(crate) fn receiver_files(&self, lease: &ProfileLease) -> Result<ReceiverFiles<'_>> {
        if !ptr::eq(self.lease, lease) {
            return Err("dispatch_reservation_owner");
        }
        let lock = self
            .dispatch
            .file_turn
            .try_lock()
            .map_err(|_| "dispatch_owner_busy")?;
        let paths = self.dispatch.checked_paths(self.lease)?;
        if paths.receiver.is_none() {
            return Err("dispatch_receiver_owner");
        }
        Ok(ReceiverFiles {
            quota: self,
            snapshot: &paths.mains[SNAPSHOT],
            _lock: lock,
        })
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(crate) fn recover_unknown(
        &self,
        exact: RecoveryRequest<'_>,
        fence: &crate::peer_invitation::NativeFence<'_>,
    ) -> Result<()> {
        fence.check().map_err(|_| "recovery_fence")?;
        let key = exact.key;
        let mut turn = self.admit_recovery(exact)?;
        turn.enter()?;
        let result = recover_database(&turn.quota.files[0], key, fence)
            .and_then(|()| turn.finish())
            .and_then(|()| fence.check().map_err(|_| "recovery_fence"));
        if result.is_err() {
            turn.quota.panicked.store(true, Ordering::Relaxed);
        }
        result
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    fn admit_recovery(&self, exact: RecoveryRequest<'_>) -> Result<QuotaTurn<'_>> {
        self.admit_checked(
            Reservation {
                owner: self,
                class: SlotClass::Unknown,
                generation: exact.generation,
                witness: exact.files.main_identity(),
                charged_other: exact.charged_other,
                #[cfg(test)]
                crash_after_database_write: exact.interrupt_after_write,
            },
            Some(exact.files),
        )
    }

    pub(crate) fn admit(&self, reservation: Reservation<'_>) -> Result<QuotaTurn<'_>> {
        self.admit_checked(
            reservation,
            #[cfg(all(
                any(test, marmish_peer_receiver_candidate),
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            None,
        )
    }

    fn admit_checked(
        &self,
        reservation: Reservation<'_>,
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        recovery: Option<crate::peer_invitation::RollbackFiles>,
    ) -> Result<QuotaTurn<'_>> {
        if !ptr::eq(self, reservation.owner) {
            return Err("dispatch_reservation_owner");
        }
        let Reservation {
            class,
            generation,
            witness: file,
            charged_other,
            #[cfg(test)]
            crash_after_database_write,
            ..
        } = reservation;
        let index = class.index();
        if ACTIVE.with(|active| active.borrow().is_some()) {
            return Err("dispatch_recursive_turn");
        }
        let paths = self.dispatch.checked_paths(self.lease)?;
        let main = paths.mains.get(index).ok_or("dispatch_stage_path")?;
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
            || (index < NAMES.len()
                && (slot.generation.is_some_and(|current| generation < current)
                    || (slot.generation == Some(generation) && slot.witness != Some(file))))
        {
            return Err("dispatch_stage_generation");
        }
        let checked = main
            .symlink_metadata()
            .map_err(|_| "dispatch_stage_missing");
        let quota = (|| {
            let meta = checked?;
            if witness(&meta) != file
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
            #[cfg(all(
                any(test, marmish_peer_receiver_candidate),
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            let recovery = recovery
                .map(|files| {
                    if !matches!(class, SlotClass::Unknown)
                        || paths.receiver.is_none()
                        || !files.belongs_to(paths.directories[1])
                        || files.main_identity() != file
                    {
                        return Err("dispatch_recovery_reservation");
                    }
                    let (head, tail) = files
                        .read_journal_bounds()
                        .ok_or("dispatch_recovery_journal")?;
                    let header = recovery_header::JournalHeader::check(
                        files.journal_length(),
                        files.main_length(),
                        head,
                        tail,
                    )?;
                    Ok(RecoveryImage { files, header })
                })
                .transpose()?;
            let recover = false;
            #[cfg(all(
                any(test, marmish_peer_receiver_candidate),
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            let recover = recover || recovery.is_some();
            let mut quota = Quota::new(
                self.dispatch.managed,
                main,
                TOTAL_ADMISSION
                    .checked_sub(charged_other)
                    .ok_or("dispatch_admission")?,
                recover,
            )?;
            let state = Arc::get_mut(&mut quota).expect("new_quota");
            state.witness = Some(file);
            #[cfg(all(
                any(test, marmish_peer_receiver_candidate),
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            if let Some(image) = recovery {
                let recorded = state
                    .account
                    .get_mut()
                    .map_err(|_| "dispatch_indeterminate")?;
                recorded.journal_witness = Some(image.files.journal_identity());
                state.recovery = Some(image);
            }
            #[cfg(test)]
            {
                state
                    .crash_after_database_write
                    .store(crash_after_database_write, Ordering::Relaxed);
            }
            Ok(quota)
        })();
        let quota = match quota {
            Ok(quota) => quota,
            Err(error) => {
                slot.generation = Some(generation);
                slot.witness = Some(file);
                slot.quarantined = true;
                return Err(error);
            }
        };
        slot.generation = Some(generation);
        slot.witness = Some(file);
        slot.active = Some(quota.clone());
        Ok(QuotaTurn {
            dispatch: self.dispatch,
            lease: self.lease,
            slot: index,
            quota,
            entered: false,
            _local: PhantomData,
        })
    }
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
fn recover_database(
    path: &Path,
    key: &SqlCipherKey,
    fence: &crate::peer_invitation::NativeFence<'_>,
) -> Result<()> {
    use rusqlite::{Connection, OpenFlags};
    use storage_sqlite::{SqlCipherHardening, open_hardened_sqlcipher};

    fence.check().map_err(|_| "recovery_fence")?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|_| "recovery_open")?;
    let query = (|| {
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(|_| "recovery_open")?;
        open_hardened_sqlcipher(&connection, key, SqlCipherHardening::cipher_only())
            .map_err(|_| "recovery_cipher")?;
        connection
            .execute_batch("PRAGMA temp_store=MEMORY; PRAGMA threads=0;")
            .map_err(|_| "recovery_cipher")?;
        fence.check().map_err(|_| "recovery_fence")?;
        connection
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|_| "recovery_read")?;
        Ok(())
    })();
    connection.close().map_err(|_| "recovery_close")?;
    query
}

thread_local! {
    static ACTIVE: RefCell<Option<Arc<Quota>>> = const { RefCell::new(None) };
}

pub(crate) struct QuotaTurn<'a> {
    dispatch: &'a Dispatch,
    lease: &'a ProfileLease,
    slot: usize,
    quota: Arc<Quota>,
    entered: bool,
    _local: PhantomData<Rc<()>>,
}

impl QuotaTurn<'_> {
    // Private to the checked dispatcher. Merely holding the permission does
    // not put arbitrary work on this thread inside the quota scope.
    fn enter(&mut self) -> Result<()> {
        self.dispatch.checked_paths(self.lease)?;
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            if self.entered || active.is_some() {
                return Err("dispatch_recursive_turn");
            }
            *active = Some(self.quota.clone());
            self.entered = true;
            Ok(())
        })
    }

    fn finish(&self) -> Result<()> {
        self.dispatch.checked_paths(self.lease)?;
        if Arc::strong_count(&self.quota) != 3 {
            return Err("dispatch_live_handles");
        }
        self.quota.verify_dispatch()
    }
}

impl Drop for QuotaTurn<'_> {
    fn drop(&mut self) {
        if self.entered {
            ACTIVE.with(|active| {
                active.borrow_mut().take();
            });
        }
        let (mut slots, poisoned) = match self.dispatch.slots.lock() {
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
                || (self.entered
                    && (self.dispatch.checked_paths(self.lease).is_err()
                        || self.quota.verify_dispatch().is_err()))
            {
                slot.quarantined = true;
            }
            slot.active = None;
        }
    }
}

impl Dispatch {
    #[cfg(test)]
    fn mount() -> Result<&'static Self> {
        static DISPATCH: OnceLock<Result<&'static Dispatch>> = OnceLock::new();
        *DISPATCH.get_or_init(Self::register)
    }

    #[cfg(test)]
    fn register() -> Result<&'static Self> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let _guard = candidate_registration::fixture_registration_guard()?;
        let (stock, managed) = Self::checked_stock()?;
        let descriptor = forwarding_descriptor(unsafe { &*stock })?;
        let dispatch = Self::with_descriptor(stock, managed, descriptor, OnceLock::new());
        unsafe {
            if sql::sqlite3_vfs_register(dispatch.wrapper, 1) != sql::SQLITE_OK {
                return Err("dispatch_registration");
            }
        }
        dispatch.assert_default()?;
        Ok(dispatch)
    }

    #[cfg(any(test, marmish_peer_receiver_candidate))]
    fn checked_stock() -> Result<(*mut sql::sqlite3_vfs, *mut sql::sqlite3_vfs)> {
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
        if unsafe { sql::sqlite3_vfs_find(c"unix".as_ptr()) } != stock {
            return Err("stock_unix_vfs");
        }
        Ok((stock, managed))
    }

    #[cfg(any(test, marmish_peer_receiver_candidate))]
    fn with_descriptor(
        stock: *mut sql::sqlite3_vfs,
        managed: *mut sql::sqlite3_vfs,
        descriptor: sql::sqlite3_vfs,
        paths: OnceLock<ReservedPaths>,
    ) -> &'static Self {
        let dispatch = Box::leak(Box::new(Self {
            stock,
            managed,
            wrapper: Box::into_raw(Box::new(descriptor)),
            paths,
            slots: Mutex::default(),
            #[cfg(any(test, marmish_peer_receiver_candidate))]
            file_turn: Mutex::default(),
            #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
            founder_paths: OnceLock::new(),
        }));
        unsafe {
            (*dispatch.wrapper).pAppData = ptr::from_ref(dispatch).cast_mut().cast();
        }
        dispatch
    }

    fn assert_default(&self) -> Result<()> {
        if unsafe { sql::sqlite3_vfs_find(ptr::null()) } != self.wrapper {
            return Err("dispatch_displaced");
        }
        #[cfg(target_os = "linux")]
        let managed_name = c"unix";
        #[cfg(target_os = "macos")]
        let managed_name = c"unix-posix";
        if unsafe { sql::sqlite3_vfs_find(c"unix".as_ptr()) } != self.stock
            || unsafe { sql::sqlite3_vfs_find(managed_name.as_ptr()) } != self.managed
        {
            return Err("dispatch_stock_displaced");
        }
        Ok(())
    }

    fn checked_paths(&self, lease: &ProfileLease) -> Result<&ReservedPaths> {
        self.assert_default()?;
        let paths = self.paths.get().ok_or("dispatch_unreserved")?;
        paths.check(lease)?;
        Ok(paths)
    }

    #[cfg(test)]
    fn bind(lease: &ProfileLease, expected: ReservedPaths) -> Result<&'static Self> {
        let dispatch = Self::mount()?;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        let _lock = dispatch
            .file_turn
            .try_lock()
            .map_err(|_| "dispatch_owner_busy")?;
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        if dispatch.founder_paths.get().is_some() {
            return Err("dispatch_founder_owner");
        }
        expected.check(lease)?;
        let competing = dispatch.paths.set(expected).err();
        let actual = dispatch.checked_paths(lease)?;
        if let Some(expected) = competing
            && (actual.mains != expected.mains || actual.all != expected.all)
        {
            return Err("dispatch_profile_conflict");
        }
        Ok(dispatch)
    }

    fn protected(&self, name: *const c_char) -> bool {
        if name.is_null() {
            return false;
        }
        // Name is borrowed only for this callback. Pass the original pointer,
        // rather than reconstructed bytes, to the stock implementation.
        let path = Path::new(OsStr::from_bytes(
            unsafe { CStr::from_ptr(name) }.to_bytes(),
        ));
        #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
        if self
            .founder_paths
            .get()
            .is_some_and(|scope| scope.protects(path))
        {
            return true;
        }
        let Some(paths) = self.paths.get() else {
            return false;
        };
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

#[cfg(test)]
impl<'a> ProfileSlots<'a> {
    pub(crate) fn reserve(lease: &'a ProfileLease, charged_other: u64) -> Result<Self> {
        if charged_other > TOTAL_ADMISSION {
            return Err("dispatch_admission");
        }
        let dispatch = Dispatch::bind(lease, ReservedPaths::new(lease, SNAPSHOT_NAME)?)?;
        Ok(Self {
            dispatch,
            charged_other,
            crash_after_database_write: false,
            owner: ReceiverQuota { dispatch, lease },
        })
    }

    fn begin(&self, exact: RawBinding) -> Result<QuotaTurn<'_>> {
        let index = usize::from(!exact.known);
        let paths = self.dispatch.paths.get().ok_or("dispatch_unreserved")?;
        if paths.mains[index] != exact.path {
            return Err("dispatch_stage_path");
        }
        let mut turn = self
            .owner
            .admit(self.reservation(exact, self.charged_other))?;
        turn.enter()?;
        Ok(turn)
    }

    fn reservation(&self, exact: RawBinding<'_>, charged_other: u64) -> Reservation<'_> {
        Reservation {
            owner: &self.owner,
            class: if exact.known {
                SlotClass::Known
            } else {
                SlotClass::Unknown
            },
            generation: exact.generation,
            witness: exact.witness,
            charged_other,
            crash_after_database_write: self.crash_after_database_write,
        }
    }

    fn finish(&self, turn: &QuotaTurn<'_>) -> Result<()> {
        turn.finish()
    }

    fn other_charge(&self, active: usize) -> Result<u64> {
        let paths = self.dispatch.paths.get().expect("reserved_paths");
        paths
            .all
            .iter()
            .enumerate()
            .try_fold(self.charged_other, |charged, (index, name)| {
                if index / 4 == active {
                    return Ok(charged);
                }
                let extent = bytes(name)?.unwrap_or(0);
                charged
                    .checked_add(extent)
                    .filter(|total| *total <= TOTAL_ADMISSION)
                    .ok_or("dispatch_admission")
            })
    }

    pub(crate) fn file_turn(&self) -> Result<FixtureFileTurn<'_>> {
        self.dispatch.checked_paths(self.owner.lease)?;
        let exclusive = self
            .dispatch
            .file_turn
            .try_lock()
            .map_err(|_| "dispatch_file_turn_unavailable")?;
        let slots = self
            .dispatch
            .slots
            .try_lock()
            .map_err(|_| "dispatch_reservations")?;
        if slots[SNAPSHOT].quarantined || slots.iter().any(|slot| slot.active.is_some()) {
            return Err("dispatch_file_turn_unavailable");
        }
        Ok(FixtureFileTurn {
            slots: self,
            _exclusive: exclusive,
        })
    }

    pub(crate) fn leave_turn(
        &self,
        binding: RawBinding<'_>,
        operation: LeaveTurn<'_>,
        expected: (&[u8], u64, [u8; 32], u32),
    ) -> Result<std::result::Result<cgka_traits::MessageId, StageError>> {
        let files = self.file_turn()?;
        let turn = files.original(binding)?;
        let result = operation.run(binding.path, expected);
        self.finish(&turn)?;
        result
    }

    pub(crate) fn native_turn(
        &self,
        binding: RawBinding<'_>,
        operation: NativeTurn<'_>,
    ) -> Result<NativeResult> {
        let files = self.file_turn()?;
        files.require_empty_snapshot()?;
        let turn = self.begin(binding)?;
        let result = operation.run(binding.path);
        self.finish(&turn)?;
        Ok(result)
    }
}

/// The existing file exclusion, restricted to the checked receiver role.
#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(crate) struct ReceiverFiles<'a> {
    quota: &'a ReceiverQuota<'a>,
    snapshot: &'a Path,
    _lock: std::sync::MutexGuard<'a, ()>,
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
impl ReceiverFiles<'_> {
    pub(crate) fn snapshot(&self) -> &Path {
        self.snapshot
    }

    pub(crate) fn check_package_paths(
        &self,
        directories: [(u64, u64); 2],
        unknown: &Path,
        snapshot: &Path,
    ) -> Result<()> {
        let paths = self.quota.dispatch.checked_paths(self.quota.lease)?;
        if paths.directories != directories
            || paths.mains[SlotClass::Unknown.index()] != unknown
            || self.snapshot != snapshot
        {
            return Err("dispatch_package_topology");
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn matches_quota(&self, quota: &ReceiverQuota<'_>) -> bool {
        ptr::eq(self.quota, quota)
    }

    fn measured_turn(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        class: SlotClass,
    ) -> Result<(&Path, QuotaTurn<'_>)> {
        let measured = permit
            .verify(self, matches!(class, SlotClass::Snapshot))
            .ok_or("dispatch_reservation_owner")?;
        let paths = self.quota.dispatch.checked_paths(self.quota.lease)?;
        let path = &paths.mains[class.index()];
        let stat = path
            .symlink_metadata()
            .map_err(|_| "dispatch_stage_extent")?;
        let allocated = stat
            .blocks()
            .checked_mul(512)
            .ok_or("dispatch_stage_extent")?;
        if stat.len() != measured.logical || stat.len().max(allocated) != measured.charged {
            return Err("dispatch_stage_extent");
        }
        let mut turn = self.quota.admit(Reservation {
            owner: self.quota,
            class,
            generation: measured.generation,
            witness: measured.identity,
            charged_other: measured.charged_other,
            #[cfg(test)]
            crash_after_database_write: false,
        })?;
        turn.enter()?;
        Ok((path, turn))
    }

    fn inspect_snapshot<T>(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        inspect: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<T> {
        let (path, turn) = self.measured_turn(permit, SlotClass::Snapshot)?;
        let result = inspect(path);
        turn.finish()?;
        result
    }

    pub(crate) fn prestage_package(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        operation: crate::peer_invitation::SchemaOperation<'_>,
        file: crate::peer_invitation::FileWitness,
    ) -> Result<std::result::Result<(), crate::mdk_adapter::peer_native_signer::StageError>> {
        let measured = permit
            .verify(self, false)
            .ok_or("dispatch_reservation_owner")?;
        if measured.logical != 0 || measured.identity != package_file_identity(file) {
            return Err("dispatch_reservation_owner");
        }
        let (path, turn) = self.measured_turn(permit, SlotClass::Unknown)?;
        let result = operation.run(path);
        turn.finish()?;
        Ok(result)
    }

    pub(crate) fn generate_package(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        operation: crate::peer_invitation::PackageOperation<'_>,
        start: crate::peer_invitation::NativeStart,
        stage: &crate::peer_invitation::BoundStage,
        file: crate::peer_invitation::FileWitness,
    ) -> Result<
        std::result::Result<
            crate::mdk_adapter::peer_native_signer::ProvisionalPackage,
            crate::mdk_adapter::peer_native_signer::StageError,
        >,
    > {
        let measured = permit
            .verify(self, false)
            .ok_or("dispatch_reservation_owner")?;
        if measured.logical == 0
            || measured.identity != package_file_identity(file)
            || !start.matches(stage, file)
        {
            return Err("dispatch_reservation_owner");
        }
        let (path, turn) = self.measured_turn(permit, SlotClass::Unknown)?;
        let result = resolve_once(operation.issue(path));
        turn.finish()?;
        result
    }

    pub(crate) fn attest_package(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        operation: crate::peer_invitation::PackageOperation<'_>,
    ) -> Result<
        std::result::Result<
            crate::mdk_adapter::peer_native_signer::ProvisionalPackage,
            crate::mdk_adapter::peer_native_signer::StageError,
        >,
    > {
        self.inspect_snapshot(permit, |path| Ok(operation.attest(path)))
    }

    #[cfg(test)]
    pub(crate) fn prepare_join(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        operation: JoinTurn<'_>,
    ) -> Result<std::result::Result<(), StageError>> {
        self.inspect_snapshot(permit, |path| Ok(operation.prepare(path)))
    }

    #[cfg(test)]
    pub(crate) fn read_join(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        operation: JoinTurn<'_>,
    ) -> Result<std::result::Result<JoinedAuthority, StageError>> {
        self.inspect_snapshot(permit, |path| Ok(operation.read(path)))
    }

    #[cfg(test)]
    pub(crate) fn join_original(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        operation: JoinTurn<'_>,
    ) -> Result<(std::result::Result<(), StageError>, JoinMetrics)> {
        let (path, turn) = self.measured_turn(permit, SlotClass::Unknown)?;
        operation.join(path, turn)
    }

    #[cfg(test)]
    pub(crate) fn check_snapshot_cipher(
        &self,
        permit: crate::peer_invitation::CleanPermit<'_>,
        key: &SqlCipherKey,
        fence: &crate::peer_invitation::TurnFence<'_>,
    ) -> Result<()> {
        self.inspect_snapshot(permit, |path| recover_database(path, key, &fence.into()))
    }
}

#[cfg(any(test, marmish_peer_receiver_candidate))]
fn package_file_identity(file: crate::peer_invitation::FileWitness) -> (u64, u64) {
    (
        u64::from_be_bytes(file.device),
        u64::from_be_bytes(file.inode),
    )
}

#[cfg(test)]
pub(crate) struct FixtureFileTurn<'a> {
    slots: &'a ProfileSlots<'a>,
    _exclusive: std::sync::MutexGuard<'a, ()>,
}

#[cfg(test)]
impl FixtureFileTurn<'_> {
    fn paths(&self) -> &ReservedPaths {
        self.slots.dispatch.paths.get().expect("reserved_paths")
    }

    fn require_empty_snapshot(&self) -> Result<()> {
        if self.paths().all[SNAPSHOT * 4..]
            .iter()
            .any(|name| !matches!(bytes(name), Ok(None)))
        {
            return Err("dispatch_snapshot_present");
        }
        Ok(())
    }

    pub(crate) fn snapshot_path(&self, extent: u64) -> Result<&Path> {
        self.require_empty_snapshot()?;
        if !(1..=CLOSED_STAGE_LIMIT).contains(&extent) {
            return Err("dispatch_snapshot_present");
        }
        self.slots
            .other_charge(SNAPSHOT)?
            .checked_add(extent)
            .filter(|total| *total <= TOTAL_ADMISSION)
            .ok_or("dispatch_admission")?;
        Ok(&self.paths().mains[SNAPSHOT])
    }

    pub(crate) fn quarantine_snapshot(&self) {
        // If this mutex was poisoned, every later acquisition already refuses.
        if let Ok(mut slots) = self.slots.dispatch.slots.lock() {
            slots[SNAPSHOT].quarantined = true;
        }
    }

    fn inspect_snapshot<T>(
        &self,
        path: &Path,
        file: (u64, u64),
        inspect: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if self.paths().mains[SNAPSHOT] != path {
            return Err("dispatch_stage_path");
        }
        let mut turn = self.slots.owner.admit(Reservation {
            owner: &self.slots.owner,
            class: SlotClass::Snapshot,
            generation: NonZeroU64::MIN,
            witness: file,
            charged_other: self.slots.other_charge(SNAPSHOT)?,
            crash_after_database_write: false,
        })?;
        turn.enter()?;
        let result = inspect();
        turn.finish()?;
        result
    }

    pub(crate) fn inspect_leave_snapshot(
        &self,
        path: &Path,
        file: (u64, u64),
        operation: LeaveTurn<'_>,
        expected: Option<[u8; 32]>,
    ) -> Result<std::result::Result<LeaveInspection, StageError>> {
        self.inspect_snapshot(path, file, || operation.inspect(path, expected))
    }

    pub(crate) fn prepare_join_snapshot(
        &self,
        path: &Path,
        file: (u64, u64),
        operation: JoinTurn<'_>,
    ) -> Result<std::result::Result<(), StageError>> {
        self.inspect_snapshot(path, file, || Ok(operation.prepare(path)))
    }

    pub(crate) fn read_join_snapshot(
        &self,
        path: &Path,
        file: (u64, u64),
        operation: JoinTurn<'_>,
    ) -> Result<std::result::Result<JoinedAuthority, StageError>> {
        self.inspect_snapshot(path, file, || Ok(operation.read(path)))
    }

    fn original(&self, binding: RawBinding<'_>) -> Result<QuotaTurn<'_>> {
        self.require_empty_snapshot()?;
        let index = usize::from(!binding.known);
        if self.paths().mains[index] != binding.path {
            return Err("dispatch_stage_path");
        }
        let mut turn = self.slots.owner.admit(
            self.slots
                .reservation(binding, self.slots.other_charge(index)?),
        )?;
        turn.enter()?;
        Ok(turn)
    }

    pub(crate) fn join_original(
        &self,
        binding: RawBinding<'_>,
        operation: JoinTurn<'_>,
    ) -> Result<(std::result::Result<(), StageError>, JoinMetrics)> {
        let turn = self.original(binding)?;
        operation.join(binding.path, turn)
    }
}

#[cfg(test)]
pub(crate) struct JoinMetrics {
    pub(crate) polls: u8,
    pub(crate) peak: u64,
    pub(crate) rejected: [u64; 2],
    pub(crate) main_opens: u64,
}

#[cfg(test)]
thread_local! {
    static WELCOME_POLLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CLOSED_JOINS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn welcome_counts() -> (usize, usize) {
    (WELCOME_POLLS.get(), CLOSED_JOINS.get())
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct RawBinding<'a> {
    pub(crate) path: &'a Path,
    pub(crate) generation: NonZeroU64,
    pub(crate) known: bool,
    pub(crate) witness: (u64, u64),
}

#[cfg(test)]
use crate::mdk_adapter::peer_native_signer::{
    JoinWitness, JoinedAuthority, LeaveInspection, OpenStage, ProvisionalPackage, StageError,
};
#[cfg(test)]
use nostr::key::Keys;
#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
use std::time::Duration;
#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
use storage_sqlite::SqlCipherKey;

#[cfg(test)]
pub(crate) struct JoinTurn<'a> {
    pub(crate) key: &'a SqlCipherKey,
    pub(crate) signer: Arc<Keys>,
    pub(crate) origin: &'a str,
    pub(crate) label: String,
    pub(crate) join: &'a JoinWitness<'a>,
    pub(crate) fence: &'a crate::peer_invitation::TurnFence<'a>,
}

#[cfg(test)]
impl JoinTurn<'_> {
    fn open(&self, path: &Path) -> std::result::Result<OpenStage, StageError> {
        self.fence.check()?;
        OpenStage::open_provisional(
            path,
            self.key,
            self.signer.clone(),
            self.origin,
            self.label.clone(),
            false,
        )
    }

    fn prepare(self, path: &Path) -> std::result::Result<(), StageError> {
        let stage = self.open(path)?;
        stage.require_prepared_join(self.join, Duration::from_secs(self.fence.sample()?))
    }

    fn join(
        self,
        path: &Path,
        turn: QuotaTurn<'_>,
    ) -> Result<(std::result::Result<(), StageError>, JoinMetrics)> {
        use std::future::{Future as _, poll_fn};
        let mut polls = 0;
        let result = match self.open(path) {
            Ok(stage) => {
                let mut welcome = std::pin::pin!(stage.join_once_fenced(self.join, self.fence));
                resolve_once(poll_fn(|cx| {
                    polls += 1;
                    WELCOME_POLLS.set(WELCOME_POLLS.get() + 1);
                    welcome.as_mut().poll(cx)
                }))
            }
            Err(error) => Ok(Err(error)),
        };
        turn.finish()?;
        let evidence = turn.quota.evidence();
        let result = result?;
        if result.is_ok() {
            CLOSED_JOINS.set(CLOSED_JOINS.get() + 1);
        }
        Ok((
            result,
            JoinMetrics {
                polls,
                peak: evidence.peak,
                rejected: evidence.rejected,
                main_opens: evidence.database_opens,
            },
        ))
    }

    fn read(self, path: &Path) -> std::result::Result<JoinedAuthority, StageError> {
        let stage = self.open(path)?;
        self.fence.check()?;
        stage.read_joined(self.join)
    }
}

#[cfg(test)]
pub(crate) struct LeaveTurn<'a> {
    pub(crate) key: &'a SqlCipherKey,
    pub(crate) signer: Arc<Keys>,
    pub(crate) origin: &'a str,
    pub(crate) label: String,
    pub(crate) join: &'a JoinWitness<'a>,
    pub(crate) fence: &'a crate::peer_invitation::TurnFence<'a>,
}

#[cfg(test)]
impl LeaveTurn<'_> {
    fn run(
        self,
        path: &Path,
        expected: (&[u8], u64, [u8; 32], u32),
    ) -> Result<std::result::Result<cgka_traits::MessageId, StageError>> {
        if let Err(error) = self.fence.check() {
            return Ok(Err(error));
        }
        let native = match OpenStage::open_provisional(
            path,
            self.key,
            self.signer,
            self.origin,
            self.label,
            false,
        ) {
            Ok(native) => native,
            Err(error) => return Ok(Err(error)),
        };
        resolve_once(native.leave_for_custody(self.join, expected, self.fence))
    }

    fn inspect(
        self,
        path: &Path,
        expected: Option<[u8; 32]>,
    ) -> Result<std::result::Result<LeaveInspection, StageError>> {
        if let Err(error) = self.fence.check() {
            return Ok(Err(error));
        }
        let native = match OpenStage::open_provisional(
            path,
            self.key,
            self.signer,
            self.origin,
            self.label,
            false,
        ) {
            Ok(native) => native,
            Err(error) => return Ok(Err(error)),
        };
        resolve_once(native.inspect_leave(self.join, expected))
    }
}

#[cfg(test)]
pub(crate) struct NativeTurn<'a> {
    pub(crate) key: &'a SqlCipherKey,
    pub(crate) signer: Arc<Keys>,
    pub(crate) origin: &'a str,
    pub(crate) label: String,
    pub(crate) sample: Duration,
    pub(crate) operation: NativeOperation,
}

#[cfg(test)]
pub(crate) enum NativeOperation {
    Generate { withhold_signing: bool },
    Reopen { prepared: bool },
}

#[cfg(test)]
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

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
pub(crate) fn resolve_once<T>(future: impl std::future::Future<Output = T>) -> Result<T> {
    use std::task::{Context, Poll, Waker};
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

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
pub(crate) fn local_volume(path: &Path) -> Result<bool> {
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| "stage_parent")?;
    let mut status = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::statfs(name.as_ptr(), status.as_mut_ptr()) } != 0 {
        return Err("stage_filesystem");
    }
    let status = unsafe { status.assume_init() };
    #[cfg(target_os = "macos")]
    let local = status.f_flags & libc::MNT_LOCAL as u32 != 0;
    #[cfg(target_os = "linux")]
    let local = supported_linux_volume(&status);
    Ok(local)
}

#[cfg(all(target_os = "linux", any(test, marmish_peer_receiver_candidate)))]
fn supported_linux_volume(status: &libc::statfs) -> bool {
    matches!(
        status.f_type as u32,
        0xef53 | 0x58465342 | 0x9123683e | 0x01021994 | 0x794c7630 | 0x2fc12fc1
    )
}

fn dispatcher(vfs: *mut sql::sqlite3_vfs) -> &'static Dispatch {
    unsafe { &*(*vfs).pAppData.cast::<Dispatch>() }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
pub(crate) mod account_open_probe {
    use std::cell::Cell;

    thread_local! {
        static OPEN: Cell<Option<fn()>> = const { Cell::new(None) };
    }

    pub(crate) fn before_main_open(probe: fn()) {
        OPEN.with(|armed| {
            assert!(armed.replace(Some(probe)).is_none());
        });
    }

    pub(super) fn stock_open(flags: std::ffi::c_int) {
        if flags & rusqlite::ffi::SQLITE_OPEN_MAIN_DB != 0
            && let Some(probe) = OPEN.with(Cell::take)
        {
            probe();
        }
    }
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
        #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
        account_open_probe::stock_open(flags);
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
unsafe extern "C" fn forward_access(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    flag: c_int,
    result: *mut c_int,
) -> c_int {
    let base = dispatcher(vfs).stock;
    // The stock callback was present at registration.
    let stock_access = || unsafe { (*base).xAccess.unwrap_unchecked()(base, name, flag, result) };
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    return guarded(sql::SQLITE_IOERR_ACCESS, || {
        let quota = ACTIVE
            .try_with(|scope| scope.borrow().clone())
            .ok()
            .flatten();
        if quota.is_some_and(|quota| quota.recovering() && !quota.check_recovery_access(name, flag))
        {
            return sql::SQLITE_IOERR_ACCESS;
        }
        stock_access()
    });
    #[cfg(not(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    )))]
    stock_access()
}
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

#[cfg(test)]
#[path = "peer_quota_dispatch_tests.rs"]
mod tests;

#[cfg(test)]
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

#[cfg(test)]
impl ProfileSlots<'_> {
    pub(crate) fn fixture_stop_in_synced_transaction(mut self) -> Self {
        self.crash_after_database_write = true;
        self
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
pub(crate) fn arm_package_generation_crash() {
    ACTIVE.with(|scope| {
        let active = scope.borrow();
        let quota = active
            .as_ref()
            .expect("candidate package generation needs an active quota");
        assert!(!quota.recovering());
        quota
            .crash_after_database_write
            .store(true, Ordering::Relaxed);
    });
}
