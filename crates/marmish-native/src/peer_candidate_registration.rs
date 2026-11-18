//! Local private receiver authority before opening the stock native stores.
#![allow(dead_code, reason = "private actor mount is a separate reviewed phase")]

use super::{
    CommandConfig, PROFILE_CHANGED, ProfileFiles, Result, inode, physical_route::CheckedPeerRoute,
    private_mode, ready_scope::ReadyReceiverScope,
};
use crate::{
    identity::{IdentityMaterial, ProfileLease},
    peer_quota_vfs::dispatch::{
        ReceiverQuota,
        candidate_registration::{self as quota, RegisteredDispatch},
    },
};
use std::{
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd},
    sync::Arc,
};

pub(crate) use quota::{ForwardingVerifier, RegistrationFailure};

pub(crate) struct ReceiverPreflight {
    scope: ReadyReceiverScope,
    route: CheckedPeerRoute,
}

impl ReceiverPreflight {
    pub(crate) fn prepare(
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
        config: &CommandConfig,
    ) -> Result<Option<Self>> {
        let Some(scope) = ReadyReceiverScope::capture(lease, material, config)? else {
            return Ok(None);
        };
        let route = CheckedPeerRoute::bind_with_profile(Arc::clone(&scope.profile), material)?;
        let mut prepared = Self { scope, route };
        prepared.recheck(lease, material)?;
        Ok(Some(prepared))
    }

    fn recheck(&mut self, lease: &Arc<ProfileLease>, material: &IdentityMaterial) -> Result<()> {
        self.scope.recheck(lease)?;
        self.route.recheck_material(&self.scope.config, material)?;
        self.scope.recheck(lease)
    }

    pub(crate) fn register(
        mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
    ) -> std::result::Result<RegisteredReceiver, RegistrationFailure> {
        let (dispatch, directories) = quota::register(&mut self, lease, material)?;
        Ok(RegisteredReceiver {
            scope: RegisteredScope {
                forwarding: dispatch.forwarding_verifier(),
                prepared: self,
                directories,
            },
            dispatch,
        })
    }

    pub(crate) fn admit_directory(
        &mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
    ) -> Result<CheckedDirectories> {
        self.recheck(lease, material)?;
        let directories = CheckedDirectories::ensure(Arc::clone(&self.scope.profile))?;
        self.recheck(lease, material)?;
        Ok(directories)
    }
}

pub(crate) struct RegisteredReceiver {
    scope: RegisteredScope,
    dispatch: RegisteredDispatch,
}

impl RegisteredReceiver {
    pub(crate) fn forwarding_verifier(&self) -> ForwardingVerifier {
        self.scope.forwarding.clone()
    }

    pub(crate) fn recheck(
        &mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
    ) -> std::result::Result<(), RegistrationFailure> {
        self.scope.recheck(lease, material, |directories| {
            self.dispatch.recheck_lease(lease, directories)
        })
    }

    pub(crate) fn attach<'lease>(
        mut self,
        lease: &'lease Arc<ProfileLease>,
        material: &IdentityMaterial,
    ) -> std::result::Result<LocalReceiverRegistration<'lease>, RegistrationFailure> {
        self.recheck(lease, material)?;
        let quota = self.dispatch.attach(lease, &self.scope.directories)?;
        Ok(LocalReceiverRegistration {
            quota,
            scope: self.scope,
        })
    }
}

pub(crate) struct LocalReceiverRegistration<'lease> {
    quota: ReceiverQuota<'lease>,
    scope: RegisteredScope,
}

impl<'lease> LocalReceiverRegistration<'lease> {
    pub(crate) fn recheck(
        &mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
    ) -> std::result::Result<(), RegistrationFailure> {
        self.scope.recheck(lease, material, |directories| {
            self.quota.recheck_registered_lease(lease, directories)
        })
    }

    pub(crate) fn quota(&self) -> &ReceiverQuota<'lease> {
        &self.quota
    }
}

struct RegisteredScope {
    forwarding: ForwardingVerifier,
    prepared: ReceiverPreflight,
    directories: CheckedDirectories,
}

impl RegisteredScope {
    fn recheck(
        &mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
        check_topology: impl FnOnce(&CheckedDirectories) -> std::result::Result<(), &'static str>,
    ) -> std::result::Result<(), RegistrationFailure> {
        self.forwarding.recheck_default()?;
        let local = self.prepared.recheck(lease, material).is_ok()
            && check_topology(&self.directories).is_ok();
        self.forwarding.recheck_default()?;
        if !local {
            self.prepared.scope.profile.cancel.cancel();
            return Err(RegistrationFailure::LocalUnavailable);
        }
        Ok(())
    }
}

/// The original no-follow descriptors stay live through recheck and attach.
pub(crate) struct CheckedDirectories {
    profile: Arc<ProfileFiles>,
    member: File,
}

impl CheckedDirectories {
    fn ensure(profile: Arc<ProfileFiles>) -> Result<Self> {
        let (member, created) = member_directory(&profile.dir).map_err(|_| PROFILE_CHANGED)?;
        let scope = Self { profile, member };
        scope.identities(&scope.profile.lease)?;
        if created {
            scope.profile.dir.sync_all().map_err(|_| PROFILE_CHANGED)?;
        }
        scope.identities(&scope.profile.lease)?;
        Ok(scope)
    }

    pub(crate) fn identities(&self, lease: &Arc<ProfileLease>) -> Result<[(u64, u64); 2]> {
        if !Arc::ptr_eq(lease, &self.profile.lease) {
            return Err(PROFILE_CHANGED);
        }
        self.profile.recheck()?;
        let root = self.profile.dir.metadata().map_err(|_| PROFILE_CHANGED)?;
        let held = self.member.metadata().map_err(|_| PROFILE_CHANGED)?;
        let current = open_member(&self.profile.dir).map_err(|_| PROFILE_CHANGED)?;
        let current = current.metadata().map_err(|_| PROFILE_CHANGED)?;
        if !held.is_dir()
            || !private_mode(&held, 0o700)
            || !private_mode(&current, 0o700)
            || inode(&held) != inode(&current)
            || inode(&root).0 != inode(&held).0
            || !quota::supported_directory_volume(&self.member).map_err(|_| PROFILE_CHANGED)?
        {
            return Err(PROFILE_CHANGED);
        }
        self.profile.recheck()?;
        Ok([inode(&root), inode(&held)])
    }
}

fn member_directory(root: &File) -> io::Result<(File, bool)> {
    match open_member(root) {
        Ok(file) => return Ok((file, false)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    // SAFETY: the caller holds the checked root and the name is NUL-terminated.
    let status = unsafe { libc::mkdirat(root.as_raw_fd(), c"peer".as_ptr(), 0o700) };
    if status != 0 {
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::AlreadyExists {
            return Err(err);
        }
    }
    Ok((open_member(root)?, status == 0))
}

fn open_member(root: &File) -> io::Result<File> {
    // SAFETY: the root descriptor is live and the static name is NUL-terminated.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            c"peer".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returns a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(test)]
#[path = "peer_candidate_registration_tests.rs"]
mod tests;
