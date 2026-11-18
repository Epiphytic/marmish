//! Own every candidate SQLite handle before any potentially refused close.

use crate::model::{EngineError, Result};
use rusqlite::{ffi, Connection, OpenFlags, Transaction, TransactionBehavior};
use std::{
    ffi::CString,
    mem::ManuallyDrop,
    ops::Deref,
    os::unix::ffi::OsStrExt,
    path::Path,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// Synchronous admission and terminal custody notification for a daemon engine.
/// Both callbacks must be bounded and non-panicking, including during unwinding.
#[derive(Clone)]
pub struct ConnectionFence(Arc<State>);

struct State {
    verify: Box<dyn Fn() -> bool + Send + Sync>,
    owner_failed: Box<dyn Fn() + Send + Sync>,
    refused: AtomicBool,
}

impl ConnectionFence {
    pub fn new(
        verify: impl Fn() -> bool + Send + Sync + 'static,
        owner_failed: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(State {
            verify: Box::new(verify),
            owner_failed: Box::new(owner_failed),
            refused: AtomicBool::new(false),
        }))
    }

    pub fn admit(&self) -> Result<()> {
        Self::check(Some(self))
    }

    pub(crate) fn check(fence: Option<&Self>) -> Result<()> {
        let Some(fence) = fence else {
            return Ok(());
        };
        if fence.0.refused.load(Ordering::Acquire) || !(fence.0.verify)() {
            fence.0.refused.store(true, Ordering::Release);
            return Err(EngineError::StorageAdmissionRefused);
        }
        Ok(())
    }

    fn failed_close(&self) -> EngineError {
        if !self.0.refused.swap(true, Ordering::AcqRel) {
            (self.0.owner_failed)();
        }
        EngineError::StorageAdmissionRefused
    }
}

/// Access to an already existing database. Neither variant creates a file.
pub enum ExistingAccess {
    ReadOnly,
    ReadWrite,
}

/// Keeps custody of an existing SQLite connection through checked close.
pub struct ExistingConnection(Owned);

impl ExistingConnection {
    pub fn open(path: &Path, fence: &ConnectionFence, access: ExistingAccess) -> Result<Self> {
        Owned::open_existing(path, fence, access).map(Self)
    }

    pub fn stock(connection: Connection) -> Self {
        Self(Owned::stock(connection))
    }

    pub fn transaction(&mut self) -> rusqlite::Result<Transaction<'_>> {
        self.0
            .transaction_with_behavior(TransactionBehavior::Deferred)
    }

    pub fn close(self) -> Result<()> {
        self.0.close()
    }
}

impl Deref for ExistingConnection {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        &self.0
    }
}

pub(crate) struct Owned {
    connection: Option<Connection>,
    fence: Option<ConnectionFence>,
}

impl Owned {
    pub(crate) fn stock(connection: Connection) -> Self {
        Self {
            connection: Some(connection),
            fence: None,
        }
    }

    pub(crate) fn guarded(path: &Path, fence: &ConnectionFence) -> Result<Self> {
        Self::open_existing(path, fence, ExistingAccess::ReadWrite)
    }

    fn open_existing(path: &Path, fence: &ConnectionFence, access: ExistingAccess) -> Result<Self> {
        let mut raw = Raw {
            handle: ptr::null_mut(),
            fence,
        };
        let path =
            CString::new(path.as_os_str().as_bytes()).map_err(|_| EngineError::UnsafePath)?;
        ConnectionFence::check(Some(fence))?;
        crate::initialize_storage()?;
        check_threading()?;
        let access = match access {
            ExistingAccess::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
            ExistingAccess::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
        };
        let flags = access
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_EXRESCODE;
        #[cfg(test)]
        tests::mark(tests::Boundary::BeforeOpen);
        ConnectionFence::check(Some(fence))?;
        // SAFETY: path is NUL-terminated and raw exclusively owns the out pointer.
        let opened = unsafe {
            ffi::sqlite3_open_v2(path.as_ptr(), &mut raw.handle, flags.bits(), ptr::null())
        };
        #[cfg(test)]
        tests::mark(if opened == ffi::SQLITE_OK {
            tests::Boundary::Opened
        } else if raw.handle.is_null() {
            tests::Boundary::OpenFailed
        } else {
            tests::Boundary::Partial
        });
        if opened != ffi::SQLITE_OK {
            let error = match opened & 0xff {
                ffi::SQLITE_BUSY | ffi::SQLITE_LOCKED => EngineError::Busy,
                _ => EngineError::StorageFailure,
            };
            raw.close()?;
            return Err(error);
        }
        // SAFETY: a successful open must supply a live handle. Keep the guard
        // if the linked runtime did not provide an effective connection mutex.
        if raw.handle.is_null() || unsafe { ffi::sqlite3_db_mutex(raw.handle) }.is_null() {
            raw.close()?;
            return Err(EngineError::StorageFailure);
        }
        // SAFETY: locked rusqlite transfers this live handle without any SQLite
        // call. Raw keeps custody until that transfer has succeeded.
        let connection = unsafe { Connection::from_handle_owned(raw.handle) }
            .map_err(crate::cipher::db_error)?;
        raw.handle = ptr::null_mut();
        Ok(Self {
            connection: Some(connection),
            fence: Some(fence.clone()),
        })
    }

    pub(crate) fn fence(&self) -> Option<&ConnectionFence> {
        self.fence.as_ref()
    }

    pub(crate) fn transaction(&mut self) -> rusqlite::Result<Transaction<'_>> {
        self.transaction_with_behavior(TransactionBehavior::Immediate)
    }

    fn transaction_with_behavior(
        &mut self,
        behavior: TransactionBehavior,
    ) -> rusqlite::Result<Transaction<'_>> {
        self.connection
            .as_mut()
            .expect("held SQLite connection")
            .transaction_with_behavior(behavior)
    }

    pub(crate) fn close(mut self) -> Result<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> Result<()> {
        let Some(connection) = self.connection.take() else {
            return Ok(());
        };
        let Some(fence) = self.fence.as_ref() else {
            drop(connection);
            return Ok(());
        };
        let connection = ManuallyDrop::new(connection);
        ConnectionFence::check(Some(fence))?;
        #[cfg(test)]
        tests::mark(tests::Boundary::OwnedClose);
        match ManuallyDrop::into_inner(connection).close() {
            Ok(()) => Ok(()),
            Err((owned, _)) => {
                let _parked = ManuallyDrop::new(owned);
                Err(fence.failed_close())
            }
        }
    }
}

impl Deref for Owned {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.connection.as_ref().expect("held SQLite connection")
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

struct Raw<'a> {
    handle: *mut ffi::sqlite3,
    fence: &'a ConnectionFence,
}

impl Raw<'_> {
    fn close(&mut self) -> Result<()> {
        let handle = std::mem::replace(&mut self.handle, ptr::null_mut());
        if handle.is_null() {
            return Ok(());
        }
        ConnectionFence::check(Some(self.fence))?;
        // SAFETY: the raw holder owns this possibly half-open handle, and has
        // removed it from its drop slot to prevent retry even if close fails.
        #[cfg(test)]
        tests::mark(tests::Boundary::RawClose);
        if unsafe { ffi::sqlite3_close(handle) } == ffi::SQLITE_OK {
            Ok(())
        } else {
            Err(self.fence.failed_close())
        }
    }
}

impl Drop for Raw<'_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn check_threading() -> Result<()> {
    // SAFETY: query the initialized linked SQLite, and free the one mutex
    // returned by its allocator. Match locked rusqlite's single-thread probe.
    unsafe {
        if ffi::sqlite3_threadsafe() == 0 {
            return Err(EngineError::StorageFailure);
        }
        let mutex = ffi::sqlite3_mutex_alloc(0);
        if mutex.is_null() {
            return Err(EngineError::StorageFailure);
        }
        ffi::sqlite3_mutex_free(mutex);
        if mutex as usize == 8 {
            return Err(EngineError::StorageFailure);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "cipher_fence/tests.rs"]
pub(crate) mod tests;
