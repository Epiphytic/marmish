//! An isolated test VFS for measuring and limiting provisional SQLCipher files.

use std::{
    ffi::{CStr, c_char, c_int, c_void},
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    ptr,
    sync::Mutex,
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
    evidence: Evidence,
    uncertain: bool,
    journal_synced: bool,
}

pub(super) struct Quota {
    base: *mut sql::sqlite3_vfs,
    files: [PathBuf; 2],
    capacity: u64,
    account: Mutex<Accounted>,
    crash_after_database_write: bool,
}

// Safety: the captured VFS is SQLite's process-lifetime stock VFS. All
// mutable stage state is behind the mutex and the paths are immutable.
unsafe impl Sync for Quota {}

#[repr(C)]
struct Handle {
    outer: sql::sqlite3_file,
    stock: *mut sql::sqlite3_file,
    state: &'static Quota,
    index: usize,
}

impl Quota {
    pub(super) fn install(
        main: &Path,
        capacity: u64,
        recover: bool,
        crash_after_database_write: bool,
    ) -> Result<&'static Self, &'static str> {
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
        marmish_engine::initialize_storage().map_err(|_| "sqlite_initialize")?;
        // SQLite owns the stock descriptor for the process lifetime.
        let base = unsafe { sql::sqlite3_vfs_find(ptr::null()) };
        if base.is_null()
            || unsafe { (*base).szOsFile } <= 0
            || unsafe { CStr::from_ptr((*base).zName) } != c"unix"
        {
            return Err("stock_unix_vfs");
        }
        let quota = Box::leak(Box::new(Self {
            base,
            files: [main.to_owned(), journal],
            capacity,
            account: Mutex::new(Accounted {
                lengths,
                evidence: Evidence {
                    peak: sum,
                    ..Evidence::default()
                },
                uncertain: false,
                journal_synced: false,
            }),
            crash_after_database_write,
        }));
        let wrapper = Box::leak(Box::new(sql::sqlite3_vfs {
            iVersion: 1,
            szOsFile: size_of::<Handle>().try_into().map_err(|_| "file_header")?,
            mxPathname: unsafe { (*base).mxPathname },
            pNext: ptr::null_mut(),
            zName: c"marmish-test-provisional-quota".as_ptr(),
            pAppData: ptr::from_ref(quota).cast_mut().cast(),
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

    pub(super) fn evidence(&self) -> Evidence {
        let (uncertain, evidence) = {
            let state = self.account.lock().unwrap();
            (state.uncertain, state.evidence)
        };
        assert!(!uncertain, "uncertain_stage_extent");
        evidence
    }

    pub(super) fn admitted_length(&self) -> u64 {
        let state = self.account.lock().unwrap();
        state.lengths[0] + state.lengths[1]
    }

    fn observe(&self, state: &mut Accounted) -> c_int {
        let measured = bytes(&self.files[0]).and_then(|main| {
            let journal = bytes(&self.files[1])?;
            Ok([main.unwrap_or(0), journal.unwrap_or(0)])
        });
        let total = state.lengths[0].checked_add(state.lengths[1]);
        if let (Ok(measured), Some(total)) = (measured, total)
            && measured == state.lengths
            && total <= self.capacity
        {
            state.evidence.peak = state.evidence.peak.max(total);
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
                && meta.mode() & 0o077 == 0
                && meta.nlink() == 1 =>
        {
            Ok(Some(meta.len()))
        }
        Ok(_) => Err("stage_file_identity"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("stage_file_stat"),
    }
}

fn vfs_state(vfs: *mut sql::sqlite3_vfs) -> &'static Quota {
    // Called only by SQLite using the registered wrapper and its stable pAppData.
    unsafe { &*(*vfs).pAppData.cast::<Quota>() }
}

unsafe extern "C" fn open(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    outer: *mut sql::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    // pMethods must be NULL on failure because this function cleans up the
    // stock handle itself.
    unsafe { (*outer).pMethods = ptr::null() };
    let quota = vfs_state(vfs);
    let mut state = quota.account.lock().unwrap();
    let kind = flags & FILE_TYPES;
    let index = match (quota.index(name), kind) {
        (Some(0), sql::SQLITE_OPEN_MAIN_DB) if flags & sql::SQLITE_OPEN_DELETEONCLOSE == 0 => 0,
        (Some(1), sql::SQLITE_OPEN_MAIN_JOURNAL) if flags & sql::SQLITE_OPEN_DELETEONCLOSE == 0 => {
            1
        }
        _ => {
            state.evidence.refused_opens += 1;
            return sql::SQLITE_CANTOPEN;
        }
    };
    if state.uncertain {
        return sql::SQLITE_IOERR;
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
        let rc = base.xOpen.unwrap()(quota.base, name, stock, flags, out_flags);
        if rc != sql::SQLITE_OK {
            state.evidence.ordinary_errors += 1;
            close_stock(stock);
            return rc;
        }
        let handle = Handle {
            outer: sql::sqlite3_file { pMethods: &METHODS },
            stock,
            state: quota,
            index,
        };
        let rc = handle.refresh(&mut state);
        if rc != sql::SQLITE_OK {
            close_stock(stock);
            return rc;
        }
        if index == 0 {
            state.evidence.database_opens += 1;
        }
        outer.cast::<Handle>().write(handle);
        sql::SQLITE_OK
    }
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
        unsafe { &*(*self.stock).pMethods }
    }

    fn refresh(&self, state: &mut Accounted) -> c_int {
        let mut len = 0;
        let rc = unsafe { self.methods().xFileSize.unwrap()(self.stock, &mut len) };
        if let Ok(len) = u64::try_from(len)
            && rc == sql::SQLITE_OK
        {
            state.lengths[self.index] = len;
            return self.state.observe(state);
        }
        state.uncertain = true;
        sql::SQLITE_IOERR_FSTAT
    }

    fn resize(&self, end: u64, kind: Resize, operation: impl FnOnce() -> c_int) -> c_int {
        let mut state = self.state.account.lock().unwrap();
        let old = state.lengths[self.index];
        if state.uncertain && end > old {
            return sql::SQLITE_IOERR_FSTAT;
        }
        if end > old
            && end
                .checked_add(state.lengths[1 - self.index])
                .is_none_or(|sum| sum > self.state.capacity)
        {
            state.evidence.rejected[self.index] += 1;
            return sql::SQLITE_FULL;
        }
        let rc = operation();
        if rc != sql::SQLITE_OK {
            state.evidence.ordinary_errors += 1;
        }
        // Reconcile even on a partial write error before another file may grow.
        let measured = self.refresh(&mut state);
        if rc != sql::SQLITE_OK || measured != sql::SQLITE_OK {
            return if rc != sql::SQLITE_OK { rc } else { measured };
        }
        if matches!(kind, Resize::Truncate) && end > old {
            state.evidence.growth_truncates += 1;
        }
        if matches!(kind, Resize::Write { .. }) && self.index == 1 && end > old {
            state.evidence.journal_writes += 1;
        }
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
    let stock = unsafe { Handle::get(file) }.stock;
    unsafe {
        (*file).pMethods = ptr::null();
        close_stock(stock)
    }
}

unsafe extern "C" fn write(
    file: *mut sql::sqlite3_file,
    data: *const c_void,
    count: c_int,
    offset: i64,
) -> c_int {
    let (Ok(offset_bound), Ok(count_bound)) = (u64::try_from(offset), u64::try_from(count)) else {
        return sql::SQLITE_IOERR_WRITE;
    };
    let Some(end) = offset_bound
        .checked_add(count_bound)
        .filter(|n| *n <= i64::MAX as u64)
    else {
        return sql::SQLITE_IOERR_WRITE;
    };
    let handle = unsafe { Handle::get(file) };
    handle.resize(end, Resize::Write { offset }, || unsafe {
        handle.methods().xWrite.unwrap()(handle.stock, data, count, offset)
    })
}

unsafe extern "C" fn truncate(file: *mut sql::sqlite3_file, size: i64) -> c_int {
    let Ok(end) = u64::try_from(size) else {
        return sql::SQLITE_IOERR_TRUNCATE;
    };
    let handle = unsafe { Handle::get(file) };
    handle.resize(end, Resize::Truncate, || unsafe {
        handle.methods().xTruncate.unwrap()(handle.stock, size)
    })
}

unsafe extern "C" fn sync(file: *mut sql::sqlite3_file, flags: c_int) -> c_int {
    let handle = unsafe { Handle::get(file) };
    let rc = unsafe { handle.methods().xSync.unwrap()(handle.stock, flags) };
    if rc == sql::SQLITE_OK && handle.index == 1 {
        handle.state.account.lock().unwrap().journal_synced = true;
    }
    rc
}

unsafe extern "C" fn file_control(
    file: *mut sql::sqlite3_file,
    op: c_int,
    arg: *mut c_void,
) -> c_int {
    match op {
        sql::SQLITE_FCNTL_LOCKSTATE
        | sql::SQLITE_FCNTL_HAS_MOVED
        | sql::SQLITE_FCNTL_VFSNAME
        | sql::SQLITE_FCNTL_SYNC
        | sql::SQLITE_FCNTL_COMMIT_PHASETWO
        | sql::SQLITE_FCNTL_BUSYHANDLER => {
            let handle = unsafe { Handle::get(file) };
            unsafe { handle.methods().xFileControl.unwrap()(handle.stock, op, arg) }
        }
        // Unix SIZE_HINT can extend the file and CHUNK_SIZE can make writes
        // and truncation allocate beyond the requested end. Both stay disabled.
        _ => sql::SQLITE_NOTFOUND,
    }
}

unsafe extern "C" fn capabilities(file: *mut sql::sqlite3_file) -> c_int {
    let handle = unsafe { Handle::get(file) };
    let bits = unsafe { handle.methods().xDeviceCharacteristics.unwrap()(handle.stock) };
    bits & !ATOMIC_CAPABILITIES
}

unsafe extern "C" fn delete(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    sync_dir: c_int,
) -> c_int {
    let quota = vfs_state(vfs);
    if quota.index(name) != Some(1) {
        return sql::SQLITE_IOERR_DELETE;
    }
    let mut state = quota.account.lock().unwrap();
    let rc = unsafe { (*quota.base).xDelete.unwrap()(quota.base, name, sync_dir) };
    match bytes(&quota.files[1]) {
        Ok(len) => state.lengths[1] = len.unwrap_or(0),
        Err(_) => state.uncertain = true,
    }
    if state.lengths[1] == 0 {
        state.journal_synced = false;
    }
    if rc != sql::SQLITE_OK && rc != sql::SQLITE_IOERR_DELETE_NOENT {
        state.evidence.ordinary_errors += 1;
    }
    let measured = quota.observe(&mut state);
    if rc == sql::SQLITE_OK { measured } else { rc }
}

// The remaining methods preserve the stock Unix VFS argument and typed ABI.
macro_rules! file_delegate {
    ($name:ident, $member:ident $(, $arg:ident : $ty:ty)*) => {
        unsafe extern "C" fn $name(file: *mut sql::sqlite3_file $(, $arg: $ty)*) -> c_int {
            let handle = unsafe { Handle::get(file) };
            unsafe { handle.methods().$member.unwrap()(handle.stock $(, $arg)*) }
        }
    };
}

file_delegate!(read, xRead, data: *mut c_void, count: c_int, offset: i64);
file_delegate!(file_size, xFileSize, length: *mut i64);
file_delegate!(lock, xLock, level: c_int);
file_delegate!(unlock, xUnlock, level: c_int);
file_delegate!(reserved, xCheckReservedLock, result: *mut c_int);
file_delegate!(sector_size, xSectorSize);

macro_rules! vfs_delegate {
    ($name:ident, $member:ident $(, $arg:ident : $ty:ty)*) => {
        unsafe extern "C" fn $name(vfs: *mut sql::sqlite3_vfs $(, $arg: $ty)*) -> c_int {
            let base = vfs_state(vfs).base;
            unsafe { (*base).$member.unwrap()(base $(, $arg)*) }
        }
    };
}

vfs_delegate!(access, xAccess, name: *const c_char, flags: c_int, result: *mut c_int);
vfs_delegate!(full_pathname, xFullPathname, name: *const c_char, len: c_int, out: *mut c_char);
vfs_delegate!(randomness, xRandomness, len: c_int, out: *mut c_char);
vfs_delegate!(sleep, xSleep, micros: c_int);
vfs_delegate!(current_time, xCurrentTime, out: *mut f64);
vfs_delegate!(last_error, xGetLastError, len: c_int, out: *mut c_char);

unsafe extern "C" fn no_dl_open(_: *mut sql::sqlite3_vfs, _: *const c_char) -> *mut c_void {
    ptr::null_mut()
}

unsafe extern "C" fn no_dl_error(_: *mut sql::sqlite3_vfs, len: c_int, out: *mut c_char) {
    if len > 0 && !out.is_null() {
        unsafe { out.write(0) };
    }
}

unsafe extern "C" fn no_dl_symbol(
    _: *mut sql::sqlite3_vfs,
    _: *mut c_void,
    _: *const c_char,
) -> Option<unsafe extern "C" fn(*mut sql::sqlite3_vfs, *mut c_void, *const c_char)> {
    None
}

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

#[path = "peer_quota_vfs_tests.rs"]
mod tests;
