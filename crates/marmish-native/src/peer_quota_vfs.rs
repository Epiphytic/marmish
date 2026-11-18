//! Checked SQLite file operations for provisional Peer SQLCipher storage.

use std::{
    cell::Cell,
    ffi::{CStr, c_char, c_int, c_void},
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use rusqlite::ffi as sql;

const FILE_TYPES: c_int = sql::SQLITE_OPEN_MAIN_DB
    | sql::SQLITE_OPEN_MAIN_JOURNAL
    | sql::SQLITE_OPEN_TEMP_DB
    | sql::SQLITE_OPEN_TEMP_JOURNAL
    | sql::SQLITE_OPEN_TRANSIENT_DB
    | sql::SQLITE_OPEN_SUBJOURNAL
    | sql::SQLITE_OPEN_SUPER_JOURNAL
    | sql::SQLITE_OPEN_WAL;

const ATOMIC_CAPABILITIES: c_int = sql::SQLITE_IOCAP_ATOMIC
    | sql::SQLITE_IOCAP_ATOMIC512
    | sql::SQLITE_IOCAP_ATOMIC1K
    | sql::SQLITE_IOCAP_ATOMIC2K
    | sql::SQLITE_IOCAP_ATOMIC4K
    | sql::SQLITE_IOCAP_ATOMIC8K
    | sql::SQLITE_IOCAP_ATOMIC16K
    | sql::SQLITE_IOCAP_ATOMIC32K
    | sql::SQLITE_IOCAP_ATOMIC64K
    | sql::SQLITE_IOCAP_BATCH_ATOMIC;

#[cfg(test)]
#[derive(Clone, Copy, Default)]
pub(super) struct Evidence {
    pub(super) peak: u64,
    pub(super) rejected: [u64; 2],
    pub(super) journal_writes: u64,
    pub(super) database_opens: u64,
    pub(super) refused_opens: u64,
    pub(super) growth_truncates: u64,
    pub(super) ordinary_errors: u64,
}

struct Accounted {
    lengths: [u64; 2],
    #[cfg(test)]
    evidence: Evidence,
    opened_main: bool,
    uncertain: bool,
    #[cfg(any(
        test,
        all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )
    ))]
    journal_synced: bool,
    journal_witness: Option<(u64, u64)>,
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    recovered_journal: bool,
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    recovery_write: bool,
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    recovery_synced: bool,
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
struct RecoveryImage {
    files: crate::peer_invitation::RollbackFiles,
    header: recovery_header::JournalHeader,
}

pub(super) struct Quota {
    base: *mut sql::sqlite3_vfs,
    files: [PathBuf; 2],
    capacity: u64,
    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    founder_main_limit: Option<u64>,
    witness: Option<(u64, u64)>,
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    recovery: Option<RecoveryImage>,
    panicked: AtomicBool,
    account: Mutex<Accounted>,
    #[cfg(test)]
    crash_after_database_write: bool,
}

// Safety: the captured VFS is SQLite's process-lifetime stock VFS. All
// mutable stage state is behind the mutex and the paths are immutable.
unsafe impl Sync for Quota {}
unsafe impl Send for Quota {}

#[repr(C)]
struct Handle {
    outer: sql::sqlite3_file,
    stock: Stock,
    state: Arc<Quota>,
    index: usize,
}

impl Quota {
    #[cfg(test)]
    pub(super) fn install(
        main: &Path,
        capacity: u64,
        recover: bool,
        crash_after_database_write: bool,
    ) -> Result<&'static Self, &'static str> {
        marmish_engine::initialize_storage().map_err(|_| "sqlite_initialize")?;
        // SQLite owns the stock descriptor for the process lifetime.
        let base = unsafe { sql::sqlite3_vfs_find(ptr::null()) };
        if base.is_null()
            || unsafe { (*base).szOsFile } <= 0
            || unsafe { CStr::from_ptr((*base).zName) } != c"unix"
        {
            return Err("stock_unix_vfs");
        }
        let mut quota = Self::new(base, main, capacity, recover)?;
        Arc::get_mut(&mut quota)
            .expect("new_quota")
            .crash_after_database_write = crash_after_database_write;
        let quota = Box::leak(Box::new(quota));
        let wrapper = Box::leak(Box::new(sql::sqlite3_vfs {
            iVersion: 1,
            szOsFile: size_of::<Handle>().try_into().map_err(|_| "file_header")?,
            mxPathname: unsafe { (*base).mxPathname },
            pNext: ptr::null_mut(),
            zName: c"marmish-test-provisional-quota".as_ptr(),
            pAppData: ptr::from_ref::<Arc<Quota>>(quota).cast_mut().cast(),
            xOpen: Some(open),
            xDelete: Some(delete),
            xAccess: Some(access),
            xFullPathname: Some(full_pathname),
            xDlOpen: Some(no_dl_open),
            xDlError: Some(no_dl_error),
            xDlSym: Some(no_dl_symbol),
            xDlClose: Some(no_dl_close),
            xRandomness: Some(randomness),
            xSleep: Some(sleep),
            xCurrentTime: Some(current_time),
            xGetLastError: Some(last_error),
            xCurrentTimeInt64: None,
            xSetSystemCall: None,
            xGetSystemCall: None,
            xNextSystemCall: None,
        }));
        // Registration belongs to this one-case process and stays live until exit.
        let code = unsafe { sql::sqlite3_vfs_register(wrapper, 1) };
        if code != sql::SQLITE_OK || unsafe { sql::sqlite3_vfs_find(ptr::null()) } != wrapper {
            return Err("quota_vfs_registration");
        }
        Ok(quota)
    }

    fn new(
        base: *mut sql::sqlite3_vfs,
        main: &Path,
        capacity: u64,
        recover: bool,
    ) -> Result<Arc<Self>, &'static str> {
        let parent = main.parent().ok_or("stage_parent")?;
        let metadata = parent.symlink_metadata().map_err(|_| "stage_directory")?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o700
            || !parent.canonicalize().is_ok_and(|path| path == parent)
        {
            return Err("stage_directory_identity");
        }
        if bytes(&sidecar(main, "-wal"))?.is_some() || bytes(&sidecar(main, "-shm"))?.is_some() {
            return Err("stage_wal_sidecar");
        }
        let journal = sidecar(main, "-journal");
        let journal_length = bytes(&journal)?;
        if journal_length.is_some() && !recover {
            return Err("unexpected_journal");
        }
        let lengths = [bytes(main)?.unwrap_or(0), journal_length.unwrap_or(0)];
        let sum = lengths[0].checked_add(lengths[1]).ok_or("initial_size")?;
        if sum > capacity {
            return Err("initial_quota");
        }
        Ok(Arc::new(Self {
            base,
            files: [main.to_owned(), journal],
            capacity,
            #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
            founder_main_limit: None,
            witness: None,
            #[cfg(all(
                any(test, marmish_peer_receiver_candidate),
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            recovery: None,
            panicked: AtomicBool::new(false),
            account: Mutex::new(Accounted {
                lengths,
                #[cfg(test)]
                evidence: Evidence {
                    peak: sum,
                    ..Evidence::default()
                },
                opened_main: false,
                uncertain: false,
                #[cfg(any(
                    test,
                    all(
                        marmish_peer_receiver_candidate,
                        target_os = "linux",
                        target_arch = "x86_64"
                    )
                ))]
                journal_synced: false,
                journal_witness: None,
                #[cfg(all(
                    any(test, marmish_peer_receiver_candidate),
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                recovered_journal: false,
                #[cfg(all(
                    any(test, marmish_peer_receiver_candidate),
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                recovery_write: false,
                #[cfg(all(
                    any(test, marmish_peer_receiver_candidate),
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                recovery_synced: false,
            }),
            #[cfg(test)]
            crash_after_database_write: false,
        }))
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    fn recovering(&self) -> bool {
        self.recovery.is_some()
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    fn check_recovery_access(&self, name: *const c_char, flag: c_int) -> bool {
        if name.is_null() {
            return false;
        }
        let mut state = self.account.lock().unwrap();
        let matches_family = self.index(name).is_some();
        let wal = sidecar(&self.files[0], "-wal");
        let absent_wal_check = flag == sql::SQLITE_ACCESS_EXISTS
            && unsafe { CStr::from_ptr(name) }.to_bytes() == wal.as_os_str().as_bytes();
        if state.uncertain
            || !(matches_family || absent_wal_check)
            || self.observe(&mut state) != sql::SQLITE_OK
        {
            state.uncertain = true;
            return false;
        }
        true
    }

    fn index(&self, name: *const c_char) -> Option<usize> {
        if name.is_null() {
            return None;
        }
        // SQLite supplies a null-terminated name during this callback.
        let name = unsafe { CStr::from_ptr(name) }.to_bytes();
        self.files
            .iter()
            .position(|path| path.as_os_str().as_bytes() == name)
    }

    #[cfg(test)]
    pub(super) fn evidence(&self) -> Evidence {
        let (uncertain, evidence) = {
            let state = self.account.lock().unwrap();
            (state.uncertain, state.evidence)
        };
        assert!(!uncertain, "uncertain_stage_extent");
        evidence
    }

    #[cfg(test)]
    pub(super) fn admitted_length(&self) -> u64 {
        let state = self.account.lock().unwrap();
        state.lengths[0] + state.lengths[1]
    }

    fn verify_dispatch(&self) -> Result<(), &'static str> {
        let mut state = self.account.lock().map_err(|_| "dispatch_indeterminate")?;
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if self.recovering() && !state.recovered_journal {
            return Err("dispatch_no_clean_receipt");
        }
        if self.panicked.load(Ordering::Relaxed)
            || state.uncertain
            || !state.opened_main
            || state.journal_witness.is_some()
            || bytes(&self.files[1])?.is_some()
            || bytes(&sidecar(&self.files[0], "-wal"))?.is_some()
            || bytes(&sidecar(&self.files[0], "-shm"))?.is_some()
            || bytes(&self.files[0])?.ok_or("dispatch_missing_main")? > dispatch::CLOSED_STAGE_LIMIT
            || self.observe(&mut state) != sql::SQLITE_OK
        {
            return Err("dispatch_no_clean_receipt");
        }
        Ok(())
    }

    fn identities(&self, state: &Accounted) -> bool {
        let Some(expected) = self.witness else {
            return true;
        };
        let identity = |path: &Path| {
            path.symlink_metadata()
                .ok()
                .map(|meta| (meta.dev(), meta.ino()))
        };
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(recovery) = &self.recovery
            && !recovery
                .files
                .matches(state.lengths[0], state.journal_witness.is_some())
        {
            return false;
        }
        identity(&self.files[0]) == Some(expected)
            && identity(&self.files[1]) == state.journal_witness
            && state
                .journal_witness
                .is_none_or(|journal| journal.0 == expected.0)
    }

    fn observe(&self, state: &mut Accounted) -> c_int {
        let measured = bytes(&self.files[0]).and_then(|main| {
            let journal = bytes(&self.files[1])?;
            Ok([main.unwrap_or(0), journal.unwrap_or(0)])
        });
        let total = state.lengths[0].checked_add(state.lengths[1]);
        if let (Ok(measured), Some(total)) = (measured, total)
            && self.identities(state)
            && measured == state.lengths
            && total <= self.capacity
        {
            #[cfg(test)]
            {
                state.evidence.peak = state.evidence.peak.max(total);
            }
            return sql::SQLITE_OK;
        }
        state.uncertain = true;
        sql::SQLITE_IOERR_FSTAT
    }
}

fn sidecar(main: &Path, suffix: &str) -> PathBuf {
    let mut name = main.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn bytes(path: &Path) -> Result<Option<u64>, &'static str> {
    match path.symlink_metadata() {
        Ok(meta)
            if meta.is_file()
                && meta.uid() == unsafe { libc::geteuid() }
                && meta.mode() & 0o7777 == 0o600
                && meta.nlink() == 1 =>
        {
            Ok(Some(meta.len()))
        }
        Ok(_) => Err("stage_file_identity"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("stage_file_stat"),
    }
}

#[cfg(test)]
fn vfs_state(vfs: *mut sql::sqlite3_vfs) -> &'static Arc<Quota> {
    // Called only by SQLite using the registered wrapper and its stable pAppData.
    unsafe { &*(*vfs).pAppData.cast::<Arc<Quota>>() }
}

#[cfg(test)]
unsafe extern "C" fn open(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    outer: *mut sql::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    guarded(sql::SQLITE_IOERR, || unsafe {
        open_managed(vfs_state(vfs), name, outer, flags, out_flags)
    })
}

unsafe fn open_managed(
    quota: &Arc<Quota>,
    name: *const c_char,
    outer: *mut sql::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    // pMethods must be NULL on failure because this function cleans up the
    // stock handle itself.
    unsafe { (*outer).pMethods = ptr::null() };
    let mut state = quota.account.lock().unwrap();
    let kind = flags & FILE_TYPES;
    let index = match (quota.index(name), kind) {
        (Some(0), sql::SQLITE_OPEN_MAIN_DB) if flags & sql::SQLITE_OPEN_DELETEONCLOSE == 0 => 0,
        (Some(1), sql::SQLITE_OPEN_MAIN_JOURNAL) if flags & sql::SQLITE_OPEN_DELETEONCLOSE == 0 => {
            1
        }
        _ => {
            #[cfg(test)]
            {
                state.evidence.refused_opens += 1;
            }
            return sql::SQLITE_CANTOPEN;
        }
    };
    if quota.panicked.load(Ordering::Relaxed) || state.uncertain || !quota.identities(&state) {
        state.uncertain = true;
        return sql::SQLITE_IOERR;
    }
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    if quota.recovering()
        && (flags & sql::SQLITE_OPEN_CREATE != 0
            || (index == 0 && flags & sql::SQLITE_OPEN_READWRITE == 0))
    {
        state.uncertain = true;
        return sql::SQLITE_CANTOPEN;
    }
    // SQLite allocates both the outer and stock file buffers with the alignment
    // of its native file implementations. The stock buffer is separate.
    unsafe {
        let base = &*quota.base;
        let stock = sql::sqlite3_malloc64(base.szOsFile as u64).cast::<sql::sqlite3_file>();
        if stock.is_null() {
            return sql::SQLITE_NOMEM;
        }
        stock.cast::<u8>().write_bytes(0, base.szOsFile as usize);
        let stock = Stock(stock);
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        let mut actual = 0;
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        let out_flags = if quota.recovering() && out_flags.is_null() {
            &mut actual
        } else {
            out_flags
        };
        let rc = base.xOpen.unwrap()(quota.base, name, stock.0, flags, out_flags);
        if rc != sql::SQLITE_OK {
            #[cfg(test)]
            {
                state.evidence.ordinary_errors += 1;
            }
            preserve_failure_errno(rc, || drop(stock));
            return rc;
        }
        let handle = Handle {
            outer: sql::sqlite3_file { pMethods: &METHODS },
            stock,
            state: quota.clone(),
            index,
        };
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(recovery) = &quota.recovery
            && (quota.observe(&mut state) != sql::SQLITE_OK
                || (flags & sql::SQLITE_OPEN_READWRITE != 0
                    && *out_flags & sql::SQLITE_OPEN_READWRITE == 0)
                || (index == 1 && !handle.check_recovery_header(&recovery.header)))
        {
            state.uncertain = true;
            return sql::SQLITE_IOERR;
        }
        let replace_journal_witness = quota.witness.is_some() && index == 1;
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        let replace_journal_witness = replace_journal_witness && !quota.recovering();
        if replace_journal_witness {
            state.journal_witness = quota.files[1]
                .symlink_metadata()
                .ok()
                .map(|meta| (meta.dev(), meta.ino()));
        }
        if quota.witness.is_some() && index == 0 {
            let mut moved: c_int = 0;
            let rc = handle.methods().xFileControl.unwrap()(
                handle.stock.0,
                sql::SQLITE_FCNTL_HAS_MOVED,
                ptr::from_mut(&mut moved).cast(),
            );
            if rc != sql::SQLITE_OK || moved != 0 {
                state.uncertain = true;
                return sql::SQLITE_IOERR;
            }
        }
        let rc = handle.refresh(&mut state);
        if rc != sql::SQLITE_OK {
            return rc;
        }
        if index == 0 {
            state.opened_main = true;
            #[cfg(test)]
            {
                state.evidence.database_opens += 1;
            }
        }
        outer.cast::<Handle>().write(handle);
        sql::SQLITE_OK
    }
}

struct Stock(*mut sql::sqlite3_file);

impl Drop for Stock {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { close_stock(self.0) };
        }
    }
}

thread_local! {
    static ACTIVE_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

fn guarded(failure: c_int, callback: impl FnOnce() -> c_int) -> c_int {
    struct Exit;
    impl Drop for Exit {
        fn drop(&mut self) {
            ACTIVE_CALLBACK.set(false);
        }
    }
    if ACTIVE_CALLBACK.replace(true) {
        dispatch::poison_scope();
        return failure;
    }
    let _exit = Exit;
    catch_unwind(AssertUnwindSafe(callback)).unwrap_or_else(|_| {
        dispatch::poison_scope();
        failure
    })
}

fn errno_slot() -> *mut c_int {
    // libc provides a writable errno slot for the current thread.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::__errno_location()
    }
    #[cfg(target_os = "macos")]
    unsafe {
        libc::__error()
    }
}

fn preserve_failure_errno<T>(rc: c_int, reconcile: impl FnOnce() -> T) -> T {
    if rc == sql::SQLITE_OK {
        return reconcile();
    }
    // SQLite's Unix xGetLastError reads the calling thread's errno after an
    // I/O failure. Wrapper metadata probes must not replace the stock value.
    let slot = errno_slot();
    let errno = unsafe { slot.read() };
    let result = reconcile();
    unsafe { slot.write(errno) };
    result
}

unsafe fn close_stock(stock: *mut sql::sqlite3_file) -> c_int {
    // SQLite permits partial xOpen failures to publish xClose.
    unsafe {
        let methods = (*stock).pMethods;
        let rc = if methods.is_null() {
            sql::SQLITE_OK
        } else {
            (*methods).xClose.unwrap()(stock)
        };
        sql::sqlite3_free(stock.cast());
        rc
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
enum Resize {
    Write { offset: i64 },
    Truncate,
}

impl Handle {
    unsafe fn get<'a>(file: *mut sql::sqlite3_file) -> &'a Self {
        // After xOpen, SQLite supplies the initialized, still-open outer handle.
        // References remain limited to the current callback.
        unsafe { &*file.cast::<Self>() }
    }

    fn methods(&self) -> &sql::sqlite3_io_methods {
        // The stock handle remains open and owns the method pointer.
        unsafe { &*(*self.stock.0).pMethods }
    }

    fn refresh(&self, state: &mut Accounted) -> c_int {
        let mut len = 0;
        let rc = unsafe { self.methods().xFileSize.unwrap()(self.stock.0, &mut len) };
        if let Ok(len) = u64::try_from(len)
            && rc == sql::SQLITE_OK
        {
            state.lengths[self.index] = len;
            return self.state.observe(state);
        }
        state.uncertain = true;
        sql::SQLITE_IOERR_FSTAT
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    fn check_recovery_header(&self, header: &recovery_header::JournalHeader) -> bool {
        let mut length = 0;
        let mut head = [0_u8; 28];
        let mut tail = [0_u8; 16];
        let tail_offset = header.length - tail.len() as u64;
        // Read the file SQLite already owns. Opening and closing another
        // descriptor here could release Unix VFS process-wide locks.
        unsafe {
            let methods = self.methods();
            let file = self.stock.0;
            let read = methods.xRead.unwrap();
            methods.xFileSize.unwrap()(file, &mut length) == sql::SQLITE_OK
                && u64::try_from(length) == Ok(header.length)
                && read(file, head.as_mut_ptr().cast(), head.len() as c_int, 0) == sql::SQLITE_OK
                && read(
                    file,
                    tail.as_mut_ptr().cast(),
                    tail.len() as c_int,
                    tail_offset as i64,
                ) == sql::SQLITE_OK
                && head == header.head
                && tail == header.tail
        }
    }

    fn resize(
        &self,
        end: u64,
        #[cfg(any(
            test,
            all(
                marmish_peer_receiver_candidate,
                target_os = "linux",
                target_arch = "x86_64"
            )
        ))]
        kind: Resize,
        operation: impl FnOnce() -> c_int,
    ) -> c_int {
        let mut state = self.state.account.lock().unwrap();
        let old = state.lengths[self.index];
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if self.state.recovering() {
            if self.index == 1 {
                state.uncertain = true;
                return sql::SQLITE_READONLY;
            }
            if end > dispatch::CLOSED_STAGE_LIMIT {
                state.uncertain = true;
                return sql::SQLITE_FULL;
            }
            if state.uncertain || self.state.observe(&mut state) != sql::SQLITE_OK {
                return sql::SQLITE_IOERR_FSTAT;
            }
        }
        if !self.state.identities(&state) {
            state.uncertain = true;
            return sql::SQLITE_IOERR_FSTAT;
        }
        if (state.uncertain || self.state.panicked.load(Ordering::Relaxed)) && end > old {
            return sql::SQLITE_IOERR_FSTAT;
        }
        let exceeds_total = end
            .checked_add(state.lengths[1 - self.index])
            .is_none_or(|sum| sum > self.state.capacity);
        #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
        let exceeds_total = exceeds_total
            || (self.index == 0
                && self
                    .state
                    .founder_main_limit
                    .is_some_and(|limit| end > limit));
        if end > old && exceeds_total {
            #[cfg(test)]
            {
                state.evidence.rejected[self.index] += 1;
            }
            return sql::SQLITE_FULL;
        }
        let rc = operation();
        #[cfg(test)]
        if rc != sql::SQLITE_OK {
            state.evidence.ordinary_errors += 1;
        }
        // Reconcile even on a partial write error before another file may grow.
        let measured = preserve_failure_errno(rc, || self.refresh(&mut state));
        if rc != sql::SQLITE_OK || measured != sql::SQLITE_OK {
            return if rc != sql::SQLITE_OK { rc } else { measured };
        }
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if self.state.recovering() {
            state.recovery_synced = false;
            state.recovery_write |= matches!(kind, Resize::Write { .. });
        }
        #[cfg(test)]
        if matches!(kind, Resize::Truncate) && end > old {
            state.evidence.growth_truncates += 1;
        }
        #[cfg(test)]
        if matches!(kind, Resize::Write { .. }) && self.index == 1 && end > old {
            state.evidence.journal_writes += 1;
        }
        #[cfg(test)]
        if matches!(kind, Resize::Write { offset } if offset > 0)
            && self.index == 0
            && self.state.crash_after_database_write
            && state.journal_synced
        {
            // Abruptly stop after a database page was written and while the
            // synced hot journal remains, without SQLite destructors.
            unsafe { libc::_exit(80) };
        }
        sql::SQLITE_OK
    }
}

unsafe extern "C" fn close(file: *mut sql::sqlite3_file) -> c_int {
    guarded(sql::SQLITE_IOERR_CLOSE, || unsafe {
        let mut handle = file.cast::<Handle>().read();
        (*file).pMethods = ptr::null();
        let stock = std::mem::replace(&mut handle.stock.0, ptr::null_mut());
        close_stock(stock)
    })
}

unsafe extern "C" fn write(
    file: *mut sql::sqlite3_file,
    data: *const c_void,
    count: c_int,
    offset: i64,
) -> c_int {
    guarded(sql::SQLITE_IOERR_WRITE, || {
        let (Ok(offset_bound), Ok(count_bound)) = (u64::try_from(offset), u64::try_from(count))
        else {
            return sql::SQLITE_IOERR_WRITE;
        };
        let Some(end) = offset_bound
            .checked_add(count_bound)
            .filter(|n| *n <= i64::MAX as u64)
        else {
            return sql::SQLITE_IOERR_WRITE;
        };
        let handle = unsafe { Handle::get(file) };
        handle.resize(
            end,
            #[cfg(any(
                test,
                all(
                    marmish_peer_receiver_candidate,
                    target_os = "linux",
                    target_arch = "x86_64"
                )
            ))]
            Resize::Write { offset },
            || unsafe { handle.methods().xWrite.unwrap()(handle.stock.0, data, count, offset) },
        )
    })
}

unsafe extern "C" fn truncate(file: *mut sql::sqlite3_file, size: i64) -> c_int {
    guarded(sql::SQLITE_IOERR_TRUNCATE, || {
        let Ok(end) = u64::try_from(size) else {
            return sql::SQLITE_IOERR_TRUNCATE;
        };
        let handle = unsafe { Handle::get(file) };
        handle.resize(
            end,
            #[cfg(any(
                test,
                all(
                    marmish_peer_receiver_candidate,
                    target_os = "linux",
                    target_arch = "x86_64"
                )
            ))]
            Resize::Truncate,
            || unsafe { handle.methods().xTruncate.unwrap()(handle.stock.0, size) },
        )
    })
}

unsafe extern "C" fn sync(file: *mut sql::sqlite3_file, flags: c_int) -> c_int {
    guarded(sql::SQLITE_IOERR_FSYNC, || {
        let handle = unsafe { Handle::get(file) };
        let rc = unsafe { handle.methods().xSync.unwrap()(handle.stock.0, flags) };
        #[cfg(any(
            test,
            all(
                marmish_peer_receiver_candidate,
                target_os = "linux",
                target_arch = "x86_64"
            )
        ))]
        if rc == sql::SQLITE_OK && handle.index == 1 {
            handle.state.account.lock().unwrap().journal_synced = true;
        }
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if rc == sql::SQLITE_OK && handle.index == 0 && handle.state.recovering() {
            let mut state = handle.state.account.lock().unwrap();
            state.recovery_synced = state.recovery_write;
        }
        rc
    })
}

unsafe extern "C" fn file_control(
    file: *mut sql::sqlite3_file,
    op: c_int,
    arg: *mut c_void,
) -> c_int {
    guarded(sql::SQLITE_IOERR, || {
        match op {
            sql::SQLITE_FCNTL_LOCKSTATE
            | sql::SQLITE_FCNTL_HAS_MOVED
            | sql::SQLITE_FCNTL_VFSNAME
            | sql::SQLITE_FCNTL_SYNC
            | sql::SQLITE_FCNTL_COMMIT_PHASETWO
            | sql::SQLITE_FCNTL_BUSYHANDLER => {
                let handle = unsafe { Handle::get(file) };
                unsafe { handle.methods().xFileControl.unwrap()(handle.stock.0, op, arg) }
            }
            // Unix SIZE_HINT can extend the file and CHUNK_SIZE can make writes
            // and truncation allocate beyond the requested end. Both stay disabled.
            _ => sql::SQLITE_NOTFOUND,
        }
    })
}

unsafe extern "C" fn capabilities(file: *mut sql::sqlite3_file) -> c_int {
    guarded(0, || {
        let handle = unsafe { Handle::get(file) };
        let bits = unsafe { handle.methods().xDeviceCharacteristics.unwrap()(handle.stock.0) };
        bits & !ATOMIC_CAPABILITIES
    })
}

#[cfg(test)]
unsafe extern "C" fn delete(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    sync_dir: c_int,
) -> c_int {
    guarded(sql::SQLITE_IOERR_DELETE, || {
        delete_managed(vfs_state(vfs), name, sync_dir)
    })
}

fn delete_managed(quota: &Quota, name: *const c_char, sync_dir: c_int) -> c_int {
    if quota.index(name) != Some(1) {
        return sql::SQLITE_IOERR_DELETE;
    }
    let mut state = quota.account.lock().unwrap();
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    if quota.recovering()
        && (state.uncertain
            || !state.journal_synced
            || !state.recovery_synced
            || state.journal_witness.is_none()
            || quota.observe(&mut state) != sql::SQLITE_OK)
    {
        state.uncertain = true;
        return sql::SQLITE_IOERR_DELETE;
    }
    if !quota.identities(&state) {
        state.uncertain = true;
        return sql::SQLITE_IOERR_DELETE;
    }
    let rc = unsafe { (*quota.base).xDelete.unwrap()(quota.base, name, sync_dir) };
    let measured = preserve_failure_errno(rc, || {
        match bytes(&quota.files[1]) {
            Ok(len) => state.lengths[1] = len.unwrap_or(0),
            Err(_) => state.uncertain = true,
        }
        if matches!(bytes(&quota.files[1]), Ok(None)) {
            state.journal_witness = None;
            #[cfg(any(
                test,
                all(
                    marmish_peer_receiver_candidate,
                    target_os = "linux",
                    target_arch = "x86_64"
                )
            ))]
            {
                state.journal_synced = false;
            }
        }
        #[cfg(test)]
        if rc != sql::SQLITE_OK && rc != sql::SQLITE_IOERR_DELETE_NOENT {
            state.evidence.ordinary_errors += 1;
        }
        quota.observe(&mut state)
    });
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    if quota.recovering() {
        state.recovered_journal =
            rc == sql::SQLITE_OK && measured == sql::SQLITE_OK && state.journal_witness.is_none();
        if !state.recovered_journal {
            state.uncertain = true;
        }
    }
    if rc == sql::SQLITE_OK { measured } else { rc }
}

// The remaining methods preserve the stock Unix VFS argument and typed ABI.
macro_rules! file_delegate {
    ($name:ident, $member:ident $(, $arg:ident : $ty:ty)*) => {
        unsafe extern "C" fn $name(file: *mut sql::sqlite3_file $(, $arg: $ty)*) -> c_int {
            guarded(sql::SQLITE_IOERR, || {
                let handle = unsafe { Handle::get(file) };
                unsafe { handle.methods().$member.unwrap()(handle.stock.0 $(, $arg)*) }
            })
        }
    };
}

unsafe extern "C" fn read(
    file: *mut sql::sqlite3_file,
    data: *mut c_void,
    count: c_int,
    offset: i64,
) -> c_int {
    guarded(sql::SQLITE_IOERR, || {
        let handle = unsafe { Handle::get(file) };
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if handle.state.recovering() {
            let mut state = handle.state.account.lock().unwrap();
            if state.uncertain || handle.state.observe(&mut state) != sql::SQLITE_OK {
                return sql::SQLITE_IOERR_FSTAT;
            }
        }
        unsafe { handle.methods().xRead.unwrap()(handle.stock.0, data, count, offset) }
    })
}
file_delegate!(file_size, xFileSize, length: *mut i64);
file_delegate!(lock, xLock, level: c_int);
file_delegate!(unlock, xUnlock, level: c_int);
file_delegate!(reserved, xCheckReservedLock, result: *mut c_int);
file_delegate!(sector_size, xSectorSize);

#[cfg(test)]
macro_rules! vfs_delegate {
    ($name:ident, $member:ident $(, $arg:ident : $ty:ty)*) => {
        unsafe extern "C" fn $name(vfs: *mut sql::sqlite3_vfs $(, $arg: $ty)*) -> c_int {
            let base = vfs_state(vfs).base;
            unsafe { (*base).$member.unwrap()(base $(, $arg)*) }
        }
    };
}

#[cfg(test)]
vfs_delegate!(access, xAccess, name: *const c_char, flags: c_int, result: *mut c_int);
#[cfg(test)]
vfs_delegate!(full_pathname, xFullPathname, name: *const c_char, len: c_int, out: *mut c_char);
#[cfg(test)]
vfs_delegate!(randomness, xRandomness, len: c_int, out: *mut c_char);
#[cfg(test)]
vfs_delegate!(sleep, xSleep, micros: c_int);
#[cfg(test)]
vfs_delegate!(current_time, xCurrentTime, out: *mut f64);
#[cfg(test)]
vfs_delegate!(last_error, xGetLastError, len: c_int, out: *mut c_char);

#[cfg(test)]
unsafe extern "C" fn no_dl_open(_: *mut sql::sqlite3_vfs, _: *const c_char) -> *mut c_void {
    ptr::null_mut()
}

#[cfg(test)]
unsafe extern "C" fn no_dl_error(_: *mut sql::sqlite3_vfs, len: c_int, out: *mut c_char) {
    if len > 0 && !out.is_null() {
        unsafe { out.write(0) };
    }
}

#[cfg(test)]
unsafe extern "C" fn no_dl_symbol(
    _: *mut sql::sqlite3_vfs,
    _: *mut c_void,
    _: *const c_char,
) -> Option<unsafe extern "C" fn(*mut sql::sqlite3_vfs, *mut c_void, *const c_char)> {
    None
}

#[cfg(test)]
unsafe extern "C" fn no_dl_close(_: *mut sql::sqlite3_vfs, _: *mut c_void) {}

static METHODS: sql::sqlite3_io_methods = sql::sqlite3_io_methods {
    iVersion: 1,
    xClose: Some(close),
    xRead: Some(read),
    xWrite: Some(write),
    xTruncate: Some(truncate),
    xSync: Some(sync),
    xFileSize: Some(file_size),
    xLock: Some(lock),
    xUnlock: Some(unlock),
    xCheckReservedLock: Some(reserved),
    xFileControl: Some(file_control),
    xSectorSize: Some(sector_size),
    xDeviceCharacteristics: Some(capabilities),
    xShmMap: None,
    xShmLock: None,
    xShmBarrier: None,
    xShmUnmap: None,
    xFetch: None,
    xUnfetch: None,
};

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "peer_quota_recovery_header.rs"]
mod recovery_header;

#[path = "peer_quota_dispatch.rs"]
pub(crate) mod dispatch;

#[cfg(test)]
#[path = "peer_quota_vfs_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
pub(crate) fn fixture_recovery_crash(path: &Path, key: &storage_sqlite::SqlCipherKey) {
    use rusqlite::{Connection, OpenFlags};
    use storage_sqlite::{SqlCipherHardening, open_hardened_sqlcipher};

    Quota::install(path, dispatch::CLOSED_STAGE_LIMIT, false, true).unwrap();
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .unwrap();
    open_hardened_sqlcipher(&db, key, SqlCipherHardening::live_cache()).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
         BEGIN IMMEDIATE; UPDATE fixture SET value='interrupted'; COMMIT;",
    )
    .unwrap();
    panic!("recovery crash cut was not reached");
}
