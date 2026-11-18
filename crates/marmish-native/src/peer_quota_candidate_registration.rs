//! Receiver-only default registration, before any native database opens.
#![allow(dead_code, reason = "private actor mount is a separate reviewed phase")]

use super::*;
use crate::{
    identity::IdentityMaterial,
    peer_relay_trust::candidate_registration::{CheckedDirectories, ReceiverPreflight},
};
use std::{ffi::CStr, fs::File, os::fd::AsRawFd, sync::MutexGuard};

const NAME: &CStr = c"marmish-private-peer-receiver-dispatch";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RegistrationFailure {
    Untouched,
    LocalUnavailable,
    Indeterminate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Descriptor {
    version: c_int,
    size: c_int,
    open: Option<usize>,
    delete: Option<usize>,
}

impl Descriptor {
    fn read(raw: &sql::sqlite3_vfs) -> Self {
        Self {
            version: raw.iVersion,
            size: raw.szOsFile,
            open: raw.xOpen.map(|call| call as usize),
            delete: raw.xDelete.map(|call| call as usize),
        }
    }
}

#[derive(Clone, Copy)]
struct Installed {
    dispatch: &'static Dispatch,
    descriptor: Descriptor,
}

impl Installed {
    fn verify(self) -> Result<()> {
        self.dispatch.assert_default()?;
        // SAFETY: descriptors are initialized for process life, and project
        // default writers share the registration lock held by the caller.
        let wrapper = unsafe { &*self.dispatch.wrapper };
        // SAFETY: NAME is a static, NUL-terminated C string.
        if unsafe { sql::sqlite3_vfs_find(NAME.as_ptr()) } != self.dispatch.wrapper
            || Descriptor::read(wrapper) != self.descriptor
            || wrapper.pAppData != ptr::from_ref(self.dispatch).cast_mut().cast()
        {
            return Err("dispatch_descriptor");
        }
        Ok(())
    }
}

enum State {
    Unregistered,
    Registered(Installed),
    Indeterminate,
}

static REGISTRY: Mutex<State> = Mutex::new(State::Unregistered);

// The exclusive default change is shared with the pre-existing fixture
// registrar on Linux AMD64. Candidate subprocess cases never use that registrar.
#[cfg(test)]
pub(super) fn fixture_registration_guard() -> Result<impl Drop> {
    REGISTRY.lock().map_err(|_| "dispatch_registration")
}

fn registry() -> std::result::Result<MutexGuard<'static, State>, RegistrationFailure> {
    REGISTRY
        .lock()
        .map_err(|_| RegistrationFailure::Indeterminate)
}

/// Checks only the process forwarding contract and carries no receiver authority.
#[derive(Clone)]
pub(crate) struct ForwardingVerifier(Installed);

impl ForwardingVerifier {
    pub(crate) fn recheck_default(&self) -> std::result::Result<(), RegistrationFailure> {
        let mut state = registry()?;
        if !matches!(&*state, State::Registered(current)
            if ptr::eq(current.dispatch, self.0.dispatch)
                && current.descriptor == self.0.descriptor
                && self.0.verify().is_ok())
        {
            *state = State::Indeterminate;
            return Err(RegistrationFailure::Indeterminate);
        }
        Ok(())
    }
}

pub(crate) struct RegisteredDispatch(Installed);

impl RegisteredDispatch {
    pub(crate) fn forwarding_verifier(&self) -> ForwardingVerifier {
        ForwardingVerifier(self.0)
    }

    pub(crate) fn recheck_lease(
        &self,
        lease: &Arc<ProfileLease>,
        directories: &CheckedDirectories,
    ) -> Result<()> {
        recheck_receiver_paths(self.0.dispatch, lease, directories)
    }

    pub(crate) fn attach<'lease>(
        self,
        lease: &'lease Arc<ProfileLease>,
        directories: &CheckedDirectories,
    ) -> std::result::Result<ReceiverQuota<'lease>, RegistrationFailure> {
        let forwarding = self.forwarding_verifier();
        forwarding.recheck_default()?;
        let local = self.recheck_lease(lease, directories);
        forwarding.recheck_default()?;
        local.map_err(|_| RegistrationFailure::LocalUnavailable)?;
        Ok(ReceiverQuota {
            dispatch: self.0.dispatch,
            lease,
        })
    }
}

impl ReceiverQuota<'_> {
    pub(crate) fn recheck_registered_lease(
        &self,
        lease: &Arc<ProfileLease>,
        directories: &CheckedDirectories,
    ) -> Result<()> {
        if !ptr::eq(self.lease, Arc::as_ref(lease)) {
            return Err("dispatch_lease_conflict");
        }
        recheck_receiver_paths(self.dispatch, lease, directories)
    }
}

fn recheck_receiver_paths(
    dispatch: &Dispatch,
    lease: &Arc<ProfileLease>,
    directories: &CheckedDirectories,
) -> Result<()> {
    let paths = dispatch.paths.get().ok_or("dispatch_unreserved")?;
    let expected = receiver_paths(lease, directories)?;
    paths.check(lease)?;
    if paths.directories != expected.directories
        || paths.mains != expected.mains
        || paths.all != expected.all
    {
        return Err("dispatch_profile_conflict");
    }
    Ok(())
}

pub(crate) fn register(
    prepared: &mut ReceiverPreflight,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
) -> std::result::Result<(RegisteredDispatch, CheckedDirectories), RegistrationFailure> {
    use RegistrationFailure::{Indeterminate, LocalUnavailable, Untouched};

    let mut state = registry()?;
    match *state {
        State::Indeterminate => return Err(Indeterminate),
        State::Registered(current) => {
            if current.verify().is_ok() {
                return Err(LocalUnavailable);
            }
            *state = State::Indeterminate;
            return Err(Indeterminate);
        }
        State::Unregistered => {}
    }
    let (stock, managed, mut wrapper) = checked_stock().inspect_err(|_| {
        *state = State::Indeterminate;
    })?;
    // SAFETY: checked_stock returns the initialized process-owned descriptor.
    let stock_descriptor = Descriptor::read(unsafe { &*stock });
    let local = prepared
        .admit_directory(lease, material)
        .map_err(|_| "dispatch_profile")
        .and_then(|directories| {
            let paths = receiver_paths(lease, &directories)?;
            Ok((paths, directories))
        });

    // Recheck even on local refusal. A refusal can preserve ordinary startup
    // only while the exact captured stock contract is still current.
    // SAFETY: stock still points to the process-owned descriptor.
    if !same_stock_default(stock, managed)
        || Descriptor::read(unsafe { &*stock }) != stock_descriptor
    {
        *state = State::Indeterminate;
        return Err(Indeterminate);
    }
    let (paths, directories) = local.map_err(|_| Untouched)?;
    wrapper.zName = NAME.as_ptr();
    let descriptor = Descriptor::read(&wrapper);
    let dispatch = Dispatch::with_descriptor(stock, managed, wrapper, OnceLock::from(paths));
    let installed = Installed {
        dispatch,
        descriptor,
    };

    // Once this starts, even a failed C registration is an indeterminate
    // effect. Retain the descriptor and complete topology for process life.
    *state = State::Indeterminate;
    // SAFETY: the fully initialized, leaked wrapper remains live.
    if unsafe { sql::sqlite3_vfs_register(dispatch.wrapper, 1) } != sql::SQLITE_OK
        || installed.verify().is_err()
        // SQLite puts the old VFS list head immediately after the new default.
        // SAFETY: the leaked wrapper is live and this caller owns registration.
        || unsafe { (*dispatch.wrapper).pNext } != stock
    {
        return Err(Indeterminate);
    }
    *state = State::Registered(installed);
    Ok((RegisteredDispatch(installed), directories))
}

fn checked_stock() -> std::result::Result<
    (
        *mut sql::sqlite3_vfs,
        *mut sql::sqlite3_vfs,
        sql::sqlite3_vfs,
    ),
    RegistrationFailure,
> {
    let (stock, managed) =
        Dispatch::checked_stock().map_err(|_| RegistrationFailure::Indeterminate)?;
    if !same_stock_default(stock, managed) {
        return Err(RegistrationFailure::Indeterminate);
    }
    // SAFETY: checked_stock requires an initialized live stock descriptor.
    let wrapper = forwarding_descriptor(unsafe { &*stock })
        .map_err(|_| RegistrationFailure::Indeterminate)?;
    Ok((stock, managed, wrapper))
}

fn same_stock_default(stock: *mut sql::sqlite3_vfs, managed: *mut sql::sqlite3_vfs) -> bool {
    // SAFETY: each name is a static C string or the null selector for default.
    unsafe {
        sql::sqlite3_vfs_find(ptr::null()) == stock
            && sql::sqlite3_vfs_find(c"unix".as_ptr()) == managed
            && sql::sqlite3_vfs_find(NAME.as_ptr()).is_null()
    }
}

fn receiver_paths(
    lease: &Arc<ProfileLease>,
    directories: &CheckedDirectories,
) -> Result<ReservedPaths> {
    let expected = directories
        .identities(lease)
        .map_err(|_| "dispatch_profile")?;
    if ReservedPaths::directory_identities(lease.root())? != expected {
        return Err("dispatch_profile");
    }
    let mut paths = ReservedPaths::with_names(
        lease,
        expected,
        &lease.root().join("peer"),
        RECEIVER_SNAPSHOT_NAME,
    );
    paths.receiver = Some(Arc::downgrade(lease));
    Ok(paths)
}

pub(crate) fn supported_directory_volume(directory: &File) -> Result<bool> {
    let mut status = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: fstatfs writes into status while the borrowed descriptor is live.
    if unsafe { libc::fstatfs(directory.as_raw_fd(), status.as_mut_ptr()) } != 0 {
        return Err("stage_filesystem");
    }
    // SAFETY: successful fstatfs initialized the result.
    Ok(supported_linux_volume(&unsafe { status.assume_init() }))
}
