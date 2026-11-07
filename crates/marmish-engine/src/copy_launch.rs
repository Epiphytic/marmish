//! Server-owned native helper launch. No remote argv or descriptor selector can
//! construct this preparation through ordinary live execution.
use crate::{AuthorizedContext, CommandRequest, Engine, EngineError, PreparedLiveExec, Result};
use marmish_stream::copy_protocol::{HelperContext, HELPER_ENTRYPOINT};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
};

/// Registration follows native_layout's installed-image validation. This is a
/// local consistency value, never a serialized capability or network input.
#[derive(Clone)]
pub struct RegisteredCopyHelper {
    path: PathBuf,
    version: String,
    target: String,
    commit: [u8; 20],
    image: HelperImage,
}

#[derive(Clone, PartialEq, Eq)]
struct HelperImage {
    dev: u64,
    ino: u64,
    size: u64,
    mode: u32,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
    sha256: [u8; 32],
}
impl HelperImage {
    fn stamp(metadata: &std::fs::Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            size: metadata.len(),
            mode: metadata.mode(),
            mtime: metadata.mtime(),
            mtime_ns: metadata.mtime_nsec(),
            ctime: metadata.ctime(),
            ctime_ns: metadata.ctime_nsec(),
            sha256: [0; 32],
        }
    }
    fn capture(path: &Path) -> Result<Self> {
        if crate::validate_executable_path(path)? != path {
            return Err(EngineError::InvalidRequest);
        }
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| EngineError::InvalidRequest)?;
        let before = file.metadata().map_err(|_| EngineError::InvalidRequest)?;
        if !before.is_file() {
            return Err(EngineError::InvalidRequest);
        }
        let mut image = Self::stamp(&before);
        let mut hash = Sha256::new();
        let mut bytes = [0; 8192];
        loop {
            match file.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => hash.update(&bytes[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(EngineError::InvalidRequest),
            }
        }
        let after = file.metadata().map_err(|_| EngineError::InvalidRequest)?;
        let named = std::fs::symlink_metadata(path).map_err(|_| EngineError::InvalidRequest)?;
        if Self::stamp(&after) != image || Self::stamp(&named) != image {
            return Err(EngineError::InvalidRequest);
        }
        image.sha256 = hash.finalize().into();
        Ok(image)
    }
}
impl RegisteredCopyHelper {
    fn verify(&self) -> Result<()> {
        if HelperImage::capture(&self.path)? != self.image {
            return Err(EngineError::InvalidRequest);
        }
        Ok(())
    }
    pub fn register(path: &Path, version: &str, target: &str, commit: [u8; 20]) -> Result<Self> {
        let canonical = crate::validate_executable_path(path)?;
        if canonical != path
            || version.is_empty()
            || target.is_empty()
            || version.len() > 64
            || target.len() > 64
            || !version
                .bytes()
                .chain(target.bytes())
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(EngineError::InvalidRequest);
        }
        Ok(Self {
            image: HelperImage::capture(&canonical)?,
            path: canonical,
            version: version.to_owned(),
            target: target.to_owned(),
            commit,
        })
    }
}

/// The parent owns only the read end, with CLOEXEC left set. Its child copy is
/// the sole descriptor selectively exposed after the ordinary blanket closure.
pub(crate) struct OwnedCopyContext(OwnedFd, Option<RegisteredCopyHelper>);
impl OwnedCopyContext {
    fn new(context: &HelperContext) -> Result<Self> {
        let bytes = context.encode().map_err(|_| EngineError::InvalidRequest)?;
        let (reader, mut writer) = std::io::pipe().map_err(|_| EngineError::WorkerUnavailable)?;
        writer
            .write_all(&bytes)
            .map_err(|_| EngineError::WorkerUnavailable)?;
        drop(writer);
        // Reserve above stdio AND FD3 without replacing any process-global FD.
        // Keeping the allocation owned prevents Rust's exec-error pipe from
        // receiving this locator during Command::spawn.
        let fd = unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 4) };
        if fd < 0 {
            return Err(EngineError::WorkerUnavailable);
        }
        Ok(Self(unsafe { OwnedFd::from_raw_fd(fd) }, None))
    }
    pub(crate) fn validate_image(&self) -> Result<()> {
        if let Some(helper) = &self.1 {
            helper.verify()?;
        }
        Ok(())
    }
    pub(crate) fn locator(&self) -> i32 {
        self.0.as_raw_fd()
    }
}
impl Engine {
    pub fn prepare_native_copy(
        &self,
        ctx: &AuthorizedContext,
        helper: &RegisteredCopyHelper,
        context: HelperContext,
    ) -> Result<PreparedLiveExec> {
        // Authority precedes any executable or transfer-path inspection.
        self.owner(ctx)?;
        context
            .validate()
            .map_err(|_| EngineError::InvalidRequest)?;
        if context.client != ctx.principal.0
            || context.server != ctx.server.0
            || <[u8; 32]>::from(Sha256::digest(&context.group)) != ctx.endpoint.0
        {
            return Err(EngineError::Unauthorized);
        }
        context
            .validate_build(&helper.version, &helper.target, helper.commit)
            .map_err(|_| EngineError::InvalidRequest)?;
        helper.verify()?;
        let mut owned = OwnedCopyContext::new(&context)?;
        owned.1 = Some(helper.clone());
        let executable = helper.path.as_os_str().as_bytes().to_vec();
        let request = CommandRequest::ResolvedExec {
            executable: executable.clone(),
            invoked_path: executable.clone(),
            argv: vec![
                executable,
                HELPER_ENTRYPOINT.as_bytes().to_vec(),
                b"--context-fd".to_vec(),
                owned.locator().to_string().into_bytes(),
            ],
        };
        // Policy checks the actual registered image and final internal argv.
        // Spawn reuses this exact request and revalidates its canonical image.
        self.check_execute_request(ctx, &request)?;
        Ok(self.prepared_live(request, Some(owned)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CommandPolicy, EndpointId, EngineLimits, EnginePolicy, ExitOutcome, KeyRestrictions,
        NativeBinding, PrincipalId, ProvidedMaster, Restrictions, ServerId,
    };
    use std::{
        fs::{self, File},
        io::Read,
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };

    fn context() -> HelperContext {
        HelperContext {
            context_version: marmish_stream::copy_protocol::CONTEXT_VERSION,
            helper_protocol: marmish_stream::copy_protocol::VERSION,
            client: [1; 32],
            server: [2; 32],
            group: b"engine-copy-test".to_vec(),
            session: [3; 16],
            stream: [4; 16],
            transfer: [5; 32],
            direction: marmish_stream::copy_protocol::Direction::Put,
            launch_digest: [6; 32],
            retention_seconds: marmish_stream::copy_protocol::MIN_RETENTION_SECS as u64,
            expected_version: "test-1".to_owned(),
            expected_target: "test-target".to_owned(),
            expected_commit: [7; 20],
        }
    }
    fn fixture() -> (
        tempfile::TempDir,
        Engine,
        AuthorizedContext,
        RegisteredCopyHelper,
        HelperContext,
    ) {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let root = tempfile::Builder::new()
            .prefix("marmish-copy-launch-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let image = root.path().join("marmish");
        fs::write(&image, b"#!/bin/sh\n[ \"$1\" = __copy-helper-v1 ] || exit 91\n[ \"$2\" = --context-fd ] || exit 92\nexec /bin/cat /dev/fd/\"$3\"\n").unwrap();
        fs::set_permissions(&image, fs::Permissions::from_mode(0o700)).unwrap();
        let policy = EnginePolicy {
            state_root: root.path().join("engine"),
            worker_executable: image.clone(),
            server: ServerId([2; 32]),
            allowed_principals: vec![PrincipalId([1; 32])],
            allow_shell_command: false,
            command_policy: None,
            cwd: Some(root.path().to_owned()),
            limits: EngineLimits::default(),
        };
        let engine = Engine::open_with_master(
            policy,
            NativeBinding::new([8; 32], [9; 32]),
            ProvidedMaster::new([10; 32]),
        )
        .unwrap();
        let context = context();
        let ctx = engine
            .authorize(
                PrincipalId(context.client),
                EndpointId(Sha256::digest(&context.group).into()),
                ServerId(context.server),
                unsafe { libc::geteuid() },
            )
            .unwrap();
        let helper = RegisteredCopyHelper::register(
            &image,
            &context.expected_version,
            &context.expected_target,
            context.expected_commit,
        )
        .unwrap();
        (root, engine, ctx, helper, context)
    }
    fn identity(fd: i32) -> Option<(libc::dev_t, libc::ino_t)> {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
            return None;
        }
        let stat = unsafe { stat.assume_init() };
        Some((stat.st_dev, stat.st_ino))
    }
    fn finish(mut child: crate::LiveChild) -> (ExitOutcome, Vec<u8>) {
        let pipes = child.take_pipes().unwrap();
        drop(pipes.stdin);
        let mut output = pipes.stdout;
        let mut bytes = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut buffer = [0; 4096];
            match output.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(error) => panic!("unexpected pipe error: {error}"),
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        while !child.try_exit().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        child.signal_kill().unwrap();
        (child.reap_last().unwrap(), bytes)
    }

    #[test]
    fn owned_context_is_read_only_cloexec_above_three_and_has_no_writer() {
        let expected = context();
        let owned = OwnedCopyContext::new(&expected).unwrap();
        let fd = owned.locator();
        assert!(fd > 3);
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let before = identity(fd).unwrap();
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 4) };
        assert!(duplicate >= 4);
        let mut file = unsafe { File::from_raw_fd(duplicate) };
        let mut bytes = Vec::new();
        // WouldBlock would prove an accidentally retained writer and fails.
        file.read_to_end(&mut bytes).unwrap();
        assert!(HelperContext::decode(&bytes).unwrap() == expected);
        drop(file);
        drop(owned);
        assert_ne!(identity(fd), Some(before));
    }

    #[test]
    fn normal_exec_has_no_copy_context_and_native_copy_inherits_exact_context() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (_root, engine, ctx, helper, context) = fixture();
        let prepared = engine
            .prepare_native_copy(&ctx, &helper, context.clone())
            .unwrap();
        let (outcome, bytes) = finish(prepared.spawn().unwrap());
        assert_eq!(outcome, ExitOutcome::Code(0));
        assert!(HelperContext::decode(&bytes).unwrap() == context);
        let plain = engine
            .prepare_live_exec(
                &ctx,
                vec![
                    helper.path.as_os_str().as_bytes().to_vec(),
                    HELPER_ENTRYPOINT.as_bytes().to_vec(),
                    b"--context-fd".to_vec(),
                    b"7777".to_vec(),
                ],
                None,
            )
            .unwrap();
        let (outcome, bytes) = finish(plain.spawn().unwrap());
        assert_ne!(outcome, ExitOutcome::Code(0));
        assert!(bytes.is_empty());
    }

    #[test]
    fn preparation_enforces_authenticated_accounts_group_and_registered_build() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (_root, engine, ctx, helper, original) = fixture();
        for selector in 0..4 {
            let mut context = original.clone();
            match selector {
                0 => context.client[0] ^= 1,
                1 => context.server[0] ^= 1,
                2 => context.group.push(0),
                _ => context.expected_commit[0] ^= 1,
            }
            let result = engine.prepare_native_copy(&ctx, &helper, context);
            assert!(matches!(
                result,
                Err(EngineError::Unauthorized | EngineError::InvalidRequest)
            ));
        }
        let mut bad_ctx = ctx.clone();
        bad_ctx.principal = PrincipalId([99; 32]);
        let mut absent_image = helper.clone();
        absent_image.path = PathBuf::from("/does/not/exist/marmish");
        assert!(matches!(
            engine.prepare_native_copy(&bad_ctx, &absent_image, original),
            Err(EngineError::Unauthorized)
        ));
    }

    #[test]
    fn canonical_helper_policy_defaults_and_matching_key_layers_are_additive() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (_root, mut engine, ctx, helper, context) = fixture();
        let path = helper.path.to_str().unwrap().to_owned();
        engine.policy.command_policy = Some(CommandPolicy {
            version: 1,
            defaults: Restrictions {
                allow_exec: Some(vec![path.clone()]),
                deny_exec: vec![],
            },
            keys: vec![KeyRestrictions {
                principal: ctx.principal,
                restrictions: Restrictions {
                    allow_exec: None,
                    deny_exec: vec![path.clone()],
                },
            }],
        });
        assert!(matches!(
            engine.prepare_native_copy(&ctx, &helper, context.clone()),
            Err(EngineError::CommandDenied)
        ));
        engine.policy.command_policy.as_mut().unwrap().keys[0]
            .restrictions
            .deny_exec
            .clear();
        assert!(engine
            .prepare_native_copy(&ctx, &helper, context.clone())
            .is_ok());
        engine
            .policy
            .command_policy
            .as_mut()
            .unwrap()
            .defaults
            .allow_exec = Some(vec!["/different/**".to_owned()]);
        engine.policy.command_policy.as_mut().unwrap().keys[0]
            .restrictions
            .allow_exec = Some(vec![path]);
        assert!(matches!(
            engine.prepare_native_copy(&ctx, &helper, context),
            Err(EngineError::CommandDenied)
        ));
    }

    #[test]
    fn preparation_revalidates_registered_image_before_spawning() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (_root, engine, ctx, helper, context) = fixture();
        fs::set_permissions(&helper.path, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(engine.prepare_native_copy(&ctx, &helper, context).is_err());
    }

    #[test]
    fn context_fd_allocation_and_spawn_survive_closed_stdio_and_original_fd_three() {
        const MARKER: &str = "MARMISH_COPY_CLOSED_FD_CHILD";
        if std::env::var_os(MARKER).is_some() {
            for fd in 0..=3 {
                unsafe {
                    libc::close(fd);
                }
            }
            let owned = OwnedCopyContext::new(&context()).unwrap();
            assert!(owned.locator() > 3);
            assert!(identity(0).is_none());
            assert!(identity(1).is_none());
            assert!(identity(2).is_none());
            drop(owned);
            let stdio: Vec<_> = (0..3)
                .map(|expected| {
                    let file = File::open("/dev/null").unwrap();
                    assert_eq!(file.as_raw_fd(), expected);
                    file
                })
                .collect();
            let owned = OwnedCopyContext::new(&context()).unwrap();
            assert!(owned.locator() > 3);
            assert!(
                identity(3).is_none(),
                "original pipe read end at FD3 was not closed"
            );
            drop(owned);
            if unsafe { libc::geteuid() } != 0 {
                let (root, engine, ctx, helper, context) = fixture();
                drop(stdio);
                let prepared = engine
                    .prepare_native_copy(&ctx, &helper, context.clone())
                    .unwrap();
                let (outcome, bytes) = finish(prepared.spawn().unwrap());
                assert_eq!(outcome, ExitOutcome::Code(0));
                assert!(HelperContext::decode(&bytes).unwrap() == context);
                drop(engine);
                drop(root);
            }
            // No test harness output/destructors may touch its deliberately
            // closed stdio. This isolated child owns no surviving test state.
            unsafe {
                libc::_exit(0);
            }
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "copy_launch::tests::context_fd_allocation_and_spawn_survive_closed_stdio_and_original_fd_three", "--nocapture"])
            .env(MARKER, "1").spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("isolated closed-FD test timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn concurrent_spawns_each_receive_their_own_context_without_cross_contamination() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (_root, engine, ctx, helper, original) = fixture();
        let mut handles = Vec::new();
        for attempt in 0..8 {
            let mut context = original.clone();
            context.stream = [attempt; 16];
            let prepared = engine
                .prepare_native_copy(&ctx, &helper, context.clone())
                .unwrap();
            handles.push(std::thread::spawn(move || {
                let (outcome, bytes) = finish(prepared.spawn().unwrap());
                assert_eq!(outcome, ExitOutcome::Code(0));
                assert!(HelperContext::decode(&bytes).unwrap() == context);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn registered_helper_detects_in_place_and_atomic_replacement_before_prepare() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (root, engine, ctx, helper, context) = fixture();
        let original = fs::read(&helper.path).unwrap();
        let mut changed = original.clone();
        changed[0] ^= 1;
        fs::write(&helper.path, &changed).unwrap();
        assert!(engine
            .prepare_native_copy(&ctx, &helper, context.clone())
            .is_err());
        fs::write(&helper.path, &original).unwrap();
        let registered = RegisteredCopyHelper::register(
            &helper.path,
            &helper.version,
            &helper.target,
            helper.commit,
        )
        .unwrap();
        let replacement = root.path().join("replacement");
        fs::write(&replacement, &original).unwrap();
        fs::set_permissions(&replacement, fs::Permissions::from_mode(0o700)).unwrap();
        fs::rename(replacement, &helper.path).unwrap();
        assert!(engine
            .prepare_native_copy(&ctx, &registered, context)
            .is_err());
    }
    #[test]
    fn prepared_helper_rechecks_image_immediately_before_spawn() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (_root, engine, ctx, helper, context) = fixture();
        let prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
        let mut bytes = fs::read(&helper.path).unwrap();
        bytes[0] ^= 1;
        fs::write(&helper.path, bytes).unwrap();
        assert!(matches!(
            prepared.spawn(),
            Err(crate::LiveSpawnError::NotStarted(
                crate::LaunchFailure::OtherStartupFailure
            ))
        ));
    }
    #[test]
    fn context_inheritance_does_not_hide_bad_exec_or_pre_exec_startup_failures() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (_root, engine, ctx, helper, context) = fixture();
        fs::write(&helper.path, b"plain text without executable format").unwrap();
        let helper = RegisteredCopyHelper::register(
            &helper.path,
            &helper.version,
            &helper.target,
            helper.commit,
        )
        .unwrap();
        let prepared = engine.prepare_native_copy(&ctx, &helper, context).unwrap();
        assert!(matches!(
            prepared.spawn(),
            Err(crate::LiveSpawnError::NotStarted(
                crate::LaunchFailure::InvalidExecutableFormat
            ))
        ));
        let request = engine
            .resolve_exec(
                &CommandRequest::Exec {
                    argv: vec![b"/usr/bin/true".to_vec()],
                },
                None,
            )
            .unwrap();
        let spec = crate::launcher::ChildSpec {
            account: engine.account.clone(),
            request,
            environment: crate::launcher::child_environment(&engine.account),
            cwd: engine.account.cwd.clone(),
            umask: 0o077,
            canonical_exec_required: true,
        };
        // An invalid private locator fails through std's untouched exec-error
        // channel and is not mislabeled a kernel executable-permission failure.
        assert!(matches!(
            crate::launcher::spawn_child_with_context(&spec, Some(i32::MAX)),
            Err(crate::launcher::ChildSpawnError::NotStarted(
                crate::LaunchFailure::OtherStartupFailure
            ))
        ));
    }
}
