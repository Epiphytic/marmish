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
use nostr::key::Keys;
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
        let Self {
            scope: ready,
            route,
        } = self;
        Ok(RegisteredReceiver {
            scope: RegisteredScope {
                forwarding: dispatch.forwarding_verifier(),
                ready,
                directories,
            },
            dispatch,
            route,
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
    route: CheckedPeerRoute,
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
        self.scope.forwarding.recheck_default()?;
        let result =
            self.scope
                .check_local(lease, material, Some(&mut self.route), |directories| {
                    self.dispatch.recheck_lease(lease, directories)
                });
        self.scope.forwarding.recheck_default()?;
        result
    }

    pub(crate) fn attach<'lease>(
        mut self,
        lease: &'lease Arc<ProfileLease>,
        material: &'lease IdentityMaterial,
    ) -> std::result::Result<LocalReceiverRegistration<'lease>, RegistrationFailure> {
        self.recheck(lease, material)?;
        let quota = self.dispatch.attach(lease, &self.scope.directories)?;
        Ok(LocalReceiverRegistration {
            quota,
            scope: self.scope,
            route: Some(self.route),
            material,
        })
    }
}

pub(crate) struct LocalReceiverRegistration<'lease> {
    quota: ReceiverQuota<'lease>,
    scope: RegisteredScope,
    route: Option<CheckedPeerRoute>,
    material: &'lease IdentityMaterial,
}

impl<'lease> LocalReceiverRegistration<'lease> {
    pub(crate) fn recheck(
        &mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
    ) -> std::result::Result<(), RegistrationFailure> {
        self.scope.forwarding.recheck_default()?;
        let result = self.check_local(lease, material);
        self.scope.forwarding.recheck_default()?;
        result
    }

    pub(crate) fn take_route(
        &mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
    ) -> std::result::Result<RegisteredRoute, RegistrationFailure> {
        self.take_route_checked(
            lease,
            material,
            #[cfg(test)]
            || {},
        )
    }

    fn take_route_checked(
        &mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
        #[cfg(test)] after_take: impl FnOnce(),
    ) -> std::result::Result<RegisteredRoute, RegistrationFailure> {
        self.scope.forwarding.recheck_default()?;
        if self.route.is_none() {
            return Err(RegistrationFailure::LocalUnavailable);
        }
        let taken = self.check_local(lease, material).and_then(|()| {
            self.route
                .take()
                .ok_or(RegistrationFailure::LocalUnavailable)
        });
        #[cfg(test)]
        after_take();
        self.scope.forwarding.recheck_default()?;
        Ok(RegisteredRoute {
            route: taken?,
            signer: self.material.signer(),
        })
    }

    fn check_local(
        &mut self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
    ) -> std::result::Result<(), RegistrationFailure> {
        if !std::ptr::eq(self.material, material) {
            self.scope.ready.profile.cancel.cancel();
            return Err(RegistrationFailure::LocalUnavailable);
        }
        self.scope
            .check_local(lease, material, self.route.as_mut(), |directories| {
                self.quota.recheck_registered_lease(lease, directories)
            })
    }

    pub(crate) fn quota(&self) -> &ReceiverQuota<'lease> {
        &self.quota
    }
}

impl Drop for LocalReceiverRegistration<'_> {
    fn drop(&mut self) {
        self.scope.ready.profile.cancel.cancel();
    }
}

pub(crate) struct RegisteredRoute {
    route: CheckedPeerRoute,
    signer: Arc<Keys>,
}

impl RegisteredRoute {
    pub(in crate::peer_relay_trust) fn into_parts(self) -> (CheckedPeerRoute, Arc<Keys>) {
        (self.route, self.signer)
    }
}

struct RegisteredScope {
    forwarding: ForwardingVerifier,
    ready: ReadyReceiverScope,
    directories: CheckedDirectories,
}

impl RegisteredScope {
    fn check_local(
        &self,
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
        route: Option<&mut CheckedPeerRoute>,
        check_topology: impl FnOnce(&CheckedDirectories) -> std::result::Result<(), &'static str>,
    ) -> std::result::Result<(), RegistrationFailure> {
        let checked = (|| {
            self.ready.recheck(lease)?;
            lease.check_prepared_peer_identity(material)?;
            if let Some(route) = route {
                route.recheck_material(&self.ready.config, material)?;
            }
            check_topology(&self.directories).map_err(|_| PROFILE_CHANGED)?;
            self.ready.recheck(lease)
        })();
        if checked.is_err() {
            self.ready.profile.cancel.cancel();
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

#[cfg(test)]
pub(crate) use tests::{package_fixture_registration, package_fixture_root};
