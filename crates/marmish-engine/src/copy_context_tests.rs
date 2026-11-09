//! Descriptor lifetime qualification; context is same-UID consistency, not an
//! authentication capability. Hidden native CLI argument rejection is separate.
use super::*;
use crate::copy_launch::{
    tests::{fixture, identity},
    RegisteredCopyHelper,
};
use std::{
    fs::{self, File},
    os::fd::FromRawFd,
    time::{Duration, Instant},
};

struct PinnedLocator {
    fd: i32,
    identity: (libc::dev_t, libc::ino_t),
    _pin: File,
}
impl PinnedLocator {
    fn new(prepared: &PreparedLiveExec) -> Self {
        let fd = prepared.copy_context.as_ref().unwrap().locator();
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 4) };
        assert!(duplicate >= 4);
        Self {
            fd,
            identity: identity(fd).unwrap(),
            _pin: unsafe { File::from_raw_fd(duplicate) },
        }
    }
    fn released(&self) {
        assert_ne!(identity(self.fd), Some(self.identity));
    }
    fn retained(&self) {
        assert_eq!(identity(self.fd), Some(self.identity));
        assert_ne!(
            unsafe { libc::fcntl(self.fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
    }
}
fn kill_and_reap(mut child: LiveChild) {
    child.signal_kill().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !child.try_exit().unwrap() {
        assert!(Instant::now() < deadline, "owned helper did not exit");
        std::thread::sleep(Duration::from_millis(2));
    }
    child.reap_last().unwrap();
}

fn assert_launcher_failure(
    result: std::result::Result<launcher::SpawnedChild, launcher::ChildSpawnError>,
    expected: LaunchFailure,
) {
    match result {
        Err(launcher::ChildSpawnError::NotStarted(reason)) => assert_eq!(reason, expected),
        Err(launcher::ChildSpawnError::Uncertain) => panic!("unexpected uncertain launch"),
        Ok(spawned) => {
            let child = LiveChild {
                owner: ChildOwner {
                    child: spawned.child,
                    identity: spawned.identity,
                    worker: launcher::process_identity(std::process::id() as i32).unwrap(),
                    pgid: spawned.pgid,
                    reaped: false,
                },
                pipes: None,
                kill_cleanup: KillCleanup::default(),
                term_cleanup: crate::process::TermCleanup::default(),
            };
            kill_and_reap(child);
            panic!("negative launcher test unexpectedly spawned a child");
        }
    }
}

#[test]
fn dropping_unspawned_preparation_closes_only_its_owned_locator() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (_root, engine, ctx, helper, context) = fixture();
    let prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
    let locator = PinnedLocator::new(&prepared);
    locator.retained();
    drop(prepared);
    locator.released();
}

#[test]
fn successful_spawn_consumes_parent_context_before_owned_child_cleanup() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (_root, engine, ctx, helper, context) = fixture();
    let prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
    let locator = PinnedLocator::new(&prepared);
    let child = prepared.spawn().unwrap();
    locator.released();
    kill_and_reap(child);
}

#[test]
fn image_revalidation_failure_releases_owned_context() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (root, engine, ctx, helper, context) = fixture();
    let prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
    let locator = PinnedLocator::new(&prepared);
    fs::write(root.path().join("marmish"), b"replacement").unwrap();
    assert!(matches!(
        prepared.spawn(),
        Err(LiveSpawnError::NotStarted(
            LaunchFailure::OtherStartupFailure
        ))
    ));
    locator.released();
}

#[test]
fn prefork_working_directory_failure_releases_owned_context() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (root, engine, ctx, helper, context) = fixture();
    let mut prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
    let locator = PinnedLocator::new(&prepared);
    prepared.spec.cwd = root.path().join("absent");
    assert!(matches!(
        prepared.spawn(),
        Err(LiveSpawnError::NotStarted(
            LaunchFailure::InvalidWorkingDirectory
        ))
    ));
    locator.released();
}

#[test]
fn kernel_exec_format_failure_releases_owned_context() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (root, engine, ctx, _helper, context) = fixture();
    let image = root.path().join("marmish");
    fs::write(&image, b"plain text without executable format").unwrap();
    let helper = RegisteredCopyHelper::register(
        &image,
        &context.expected_version,
        &context.expected_target,
        context.expected_commit,
    )
    .unwrap();
    let prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
    let locator = PinnedLocator::new(&prepared);
    assert!(matches!(
        prepared.spawn(),
        Err(LiveSpawnError::NotStarted(
            LaunchFailure::InvalidExecutableFormat
        ))
    ));
    locator.released();
}

#[test]
fn borrowed_launcher_context_survives_prefork_and_postfork_failure_until_owner_drop() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    for postfork in [false, true] {
        let (root, engine, ctx, helper, context) = fixture();
        let image = root.path().join("marmish");
        let helper = if postfork {
            fs::write(&image, b"plain text without executable format").unwrap();
            RegisteredCopyHelper::register(
                &image,
                &context.expected_version,
                &context.expected_target,
                context.expected_commit,
            )
            .unwrap()
        } else {
            helper
        };
        let mut prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
        let locator = PinnedLocator::new(&prepared);
        let owned = prepared.copy_context.take().unwrap();
        if !postfork {
            prepared.spec.cwd = root.path().join("absent");
        }
        let expected = if postfork {
            LaunchFailure::InvalidExecutableFormat
        } else {
            LaunchFailure::InvalidWorkingDirectory
        };
        assert_launcher_failure(
            launcher::spawn_child_with_context(&prepared.spec, Some(owned.locator())),
            expected,
        );
        locator.retained();
        drop(prepared);
        locator.retained();
        drop(owned);
        locator.released();
    }
}

#[test]
fn reserved_stdio_locator_claims_fail_without_parent_descriptor_mutation() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let (_root, engine, ctx, helper, context) = fixture();
    let prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
    let locator = PinnedLocator::new(&prepared);
    for claimed in [0, 3] {
        assert_launcher_failure(
            launcher::spawn_child_with_context(&prepared.spec, Some(claimed)),
            LaunchFailure::OtherStartupFailure,
        );
        locator.retained();
    }
    drop(prepared);
    locator.released();
}
