//! Actual executable smoke proof. API readback and offline spawn are engine tests.
use std::process::Command;

#[test]
fn actual_native_entrypoints_accept_inherited_limits() {
    for binary in [
        env!("CARGO_BIN_EXE_marmish"),
        env!("CARGO_BIN_EXE_marmishd"),
    ] {
        let mut command = Command::new(binary);
        command.arg("--help");
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::process::CommandExt;
            // No environment/global limit mutation in the parallel test runner.
            // This closure uses only syscalls in the isolated child before exec.
            unsafe {
                command.pre_exec(|| {
                    let mut limit: libc::rlimit = std::mem::zeroed();
                    if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if limit.rlim_max <= 65_536 {
                        return Err(std::io::Error::from_raw_os_error(libc::ENOTSUP));
                    }
                    limit.rlim_cur = 65_537;
                    if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    let mut actual: libc::rlimit = std::mem::zeroed();
                    if libc::getrlimit(libc::RLIMIT_NOFILE, &mut actual) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if actual.rlim_cur != 65_537 || actual.rlim_max != limit.rlim_max {
                        return Err(std::io::Error::from_raw_os_error(libc::EINVAL));
                    }
                    Ok(())
                });
            }
        }
        let result = command.output().unwrap();
        assert!(result.status.success(), "native help smoke failed");
        assert!(!result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }
}
