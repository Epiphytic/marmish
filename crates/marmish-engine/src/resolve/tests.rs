use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

fn exec(name: &[u8]) -> CommandRequest {
    CommandRequest::Exec {
        argv: vec![
            name.to_vec(),
            vec![],
            b"a ; $(echo bad) '*".to_vec(),
            vec![255],
        ],
    }
}
fn program(path: &Path) {
    std::fs::write(path, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
#[test]
fn path_order_alias_and_argument_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second space");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::write(first.join("app"), b"not executable").unwrap();
    program(&second.join("real"));
    symlink("real", second.join("app")).unwrap();
    let path = std::ffi::OsString::from(format!("{}:{}", first.display(), second.display()));
    let original = exec(b"app");
    let result = resolve_exec(&original, Some(&path), dir.path()).unwrap();
    let CommandRequest::ResolvedExec {
        executable,
        invoked_path,
        argv,
    } = result
    else {
        panic!()
    };
    assert_eq!(
        executable,
        std::fs::canonicalize(second.join("real"))
            .unwrap()
            .as_os_str()
            .as_bytes()
    );
    assert_eq!(invoked_path, second.join("app").as_os_str().as_bytes());
    let CommandRequest::Exec { argv: expected } = original else {
        panic!()
    };
    assert_eq!(argv, expected);
}
#[test]
fn relative_empty_and_unset_path_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    program(&dir.path().join("app"));
    for path in ["", ".", ":absent", "absent:"] {
        assert!(resolve_exec(&exec(b"app"), Some(OsStr::new(path)), dir.path()).is_ok());
    }
    assert!(resolve_exec(&exec(b"./app"), None, dir.path()).is_ok());
    assert_eq!(
        resolve_exec(&exec(b"app"), None, dir.path()),
        Err(EngineError::CommandNotFound(
            NotFoundReason::PathSearchExhausted
        ))
    );
    let absent = dir.path().join("missing");
    assert_eq!(
        resolve_exec(&exec(absent.as_os_str().as_bytes()), None, dir.path()),
        Err(EngineError::CommandNotFound(
            NotFoundReason::ExecutableMissing
        ))
    );
}
#[test]
fn candidate_failures_and_search_bounds_are_finite() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("noexec"), b"data").unwrap();
    assert_eq!(
        resolve_exec(&exec(b"noexec"), Some(OsStr::new("")), dir.path()),
        Err(EngineError::CommandNotExecutable(
            NotExecutableReason::ExecuteBitMissing
        ))
    );
    std::fs::create_dir(dir.path().join("directory")).unwrap();
    assert_eq!(
        resolve_exec(&exec(b"directory"), Some(OsStr::new("")), dir.path()),
        Err(EngineError::CommandNotExecutable(
            NotExecutableReason::NotRegularFile
        ))
    );
    symlink("loop", dir.path().join("loop")).unwrap();
    assert_eq!(
        resolve_exec(&exec(b"loop"), Some(OsStr::new("")), dir.path()),
        Err(EngineError::CommandResolutionFailed(
            ResolutionReason::SymlinkLoop
        ))
    );
    for path in ["a".repeat(65537), ":".repeat(1024), "bad\0path".into()] {
        assert_eq!(
            resolve_exec(&exec(b"app"), Some(OsStr::new(&path)), dir.path()),
            Err(EngineError::CommandResolutionFailed(
                ResolutionReason::InvalidSearchPath
            ))
        );
    }
    assert_eq!(
        resolve_exec(&exec(b"bad\0name"), None, dir.path()),
        Err(EngineError::InvalidRequest)
    );
}
// Darwin filesystems reject invalid UTF-8 filenames before resolver lookup.
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_filesystem_paths_and_argument_bytes_are_preserved() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let name = std::ffi::OsString::from_vec(vec![b'a', 255]);
    let alias = std::ffi::OsString::from_vec(vec![b'l', 255]);
    program(&dir.path().join(&name));
    symlink(&name, dir.path().join(&alias)).unwrap();
    let result =
        resolve_exec(&exec(alias.as_bytes()), Some(OsStr::new("./.")), dir.path()).unwrap();
    let CommandRequest::ResolvedExec {
        executable,
        invoked_path,
        argv,
    } = result
    else {
        panic!()
    };
    assert_eq!(
        executable,
        std::fs::canonicalize(dir.path().join(&name))
            .unwrap()
            .as_os_str()
            .as_bytes()
    );
    assert_eq!(invoked_path, dir.path().join(&alias).as_os_str().as_bytes());
    assert_eq!(
        argv,
        vec![
            vec![b'l', 255],
            vec![],
            b"a ; $(echo bad) '*".to_vec(),
            vec![255]
        ]
    );
}

#[test]
fn lexical_symlink_alias_and_non_utf8_arguments_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    program(&dir.path().join("real"));
    symlink("real", dir.path().join("alias")).unwrap();
    let result = resolve_exec(&exec(b"alias"), Some(OsStr::new("./.")), dir.path()).unwrap();
    let CommandRequest::ResolvedExec {
        executable,
        invoked_path,
        argv,
    } = result
    else {
        panic!()
    };
    assert_eq!(
        executable,
        std::fs::canonicalize(dir.path().join("real"))
            .unwrap()
            .as_os_str()
            .as_bytes()
    );
    assert_eq!(
        invoked_path,
        dir.path().join("alias").as_os_str().as_bytes()
    );
    assert_ne!(executable, invoked_path);
    assert_eq!(
        argv,
        vec![
            b"alias".to_vec(),
            vec![],
            b"a ; $(echo bad) '*".to_vec(),
            vec![255]
        ]
    );
}

#[test]
fn selected_denied_path_candidate_does_not_fall_through_to_allowed_candidate() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("denied");
    let second = directory.path().join("allowed");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    program(&first.join("app"));
    program(&second.join("app"));
    let path = std::ffi::OsString::from(format!("{}:{}", first.display(), second.display()));
    let selected = resolve_exec(&exec(b"app"), Some(&path), directory.path()).unwrap();
    let principal = PrincipalId([1; 32]);
    let policy = EnginePolicy {
        state_root: directory.path().to_owned(),
        worker_executable: "/usr/bin/true".into(),
        server: ServerId([2; 32]),
        allowed_principals: vec![principal],
        allow_shell_command: false,
        command_policy: Some(crate::CommandPolicy {
            version: 1,
            defaults: crate::Restrictions {
                allow_exec: Some(vec![format!(
                    "{}/**",
                    std::fs::canonicalize(&second).unwrap().display()
                )]),
                deny_exec: vec![],
            },
            keys: vec![],
        }),
        cwd: None,
        limits: EngineLimits::default(),
    };
    assert_eq!(
        policy.check_exec_restrictions(principal, &selected),
        Err(EngineError::CommandDenied)
    );
    let allowed = resolve_exec(&exec(b"app"), Some(second.as_os_str()), directory.path()).unwrap();
    assert_eq!(
        policy.check_exec_restrictions(principal, &allowed),
        Ok(true)
    );
}
