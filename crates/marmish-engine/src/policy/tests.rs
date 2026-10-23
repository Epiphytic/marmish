use super::*;

fn policy(defaults: Restrictions, keys: Vec<KeyRestrictions>) -> EnginePolicy {
    EnginePolicy {
        state_root: "/tmp/not-opened".into(),
        worker_executable: "/usr/bin/true".into(),
        server: ServerId([1; 32]),
        allowed_principals: vec![PrincipalId([2; 32]), PrincipalId([3; 32])],
        allow_shell_command: true,
        command_policy: Some(CommandPolicy {
            version: 1,
            defaults,
            keys,
        }),
        cwd: None,
        limits: Default::default(),
    }
}
fn allow(patterns: &[&str]) -> Restrictions {
    Restrictions {
        allow_exec: Some(patterns.iter().map(|p| p.to_string()).collect()),
        deny_exec: vec![],
    }
}
fn exec(path: &[u8]) -> CommandRequest {
    CommandRequest::Exec {
        argv: vec![path.to_vec(), vec![255, b'*', b';']],
    }
}

#[test]
fn globs_are_anchored_byte_patterns_with_only_two_wildcards() {
    for (pattern, candidate, expected) in [
        ("/usr/*", &b"/usr/echo"[..], true),
        ("/usr/*", &b"/usr/bin/echo"[..], false),
        ("/usr/**", &b"/usr/bin/echo"[..], true),
        ("/usr/***", &b"/usr/bin/echo"[..], true),
        ("/usr/**/echo", &b"/usr/echo"[..], false),
        ("/**/sh", &b"/sh"[..], false),
        ("/**/sh", &b"/bin/sh"[..], true),
        ("/a*", &b"/a"[..], true),
        ("/a", &b"/abc"[..], false),
        ("/a\\*", &b"/a*"[..], true),
        ("/a\\\\", &b"/a\\"[..], true),
        ("/a?[]{}", &b"/a?[]{}"[..], true),
        ("/a?", &b"/ab"[..], false),
        ("/a*", &[b'/', b'a', 255][..], true),
    ] {
        assert_eq!(
            matches(&compile(pattern).unwrap(), candidate),
            expected,
            "{pattern}"
        );
    }
    for bad in [
        "", "*", "relative", "\\/bin/a", "/a\\", "/a\\?", "/a\0", "/a\n",
    ] {
        assert!(compile(bad).is_err());
    }
}

#[test]
fn defaults_and_duplicate_key_layers_accumulate_and_denies_win() {
    let principal = PrincipalId([2; 32]);
    let mut p = policy(
        allow(&["/usr/**"]),
        vec![
            KeyRestrictions {
                principal,
                restrictions: allow(&["/**/echo", "/usr/bin/true"]),
            },
            KeyRestrictions {
                principal,
                restrictions: Restrictions {
                    deny_exec: vec!["/**/true".into()],
                    ..Default::default()
                },
            },
        ],
    );
    assert_eq!(
        check_command(&p, principal, &exec(b"/usr/bin/echo")),
        Ok(true)
    );
    assert_eq!(
        check_command(&p, principal, &exec(b"/usr/bin/false")),
        Err(EngineError::CommandDenied)
    );
    assert_eq!(
        check_command(&p, principal, &exec(b"/usr/bin/true")),
        Err(EngineError::CommandDenied)
    );
    assert_eq!(
        check_command(&p, PrincipalId([3; 32]), &exec(b"/usr/bin/false")),
        Ok(true)
    );
    assert_eq!(
        check_command(&p, PrincipalId([3; 32]), &exec(b"/bin/echo")),
        Err(EngineError::CommandDenied)
    );
    p.command_policy.as_mut().unwrap().keys.reverse();
    assert_eq!(
        check_command(&p, principal, &exec(b"/usr/bin/echo")),
        Ok(true)
    );
    assert_eq!(
        check_command(&p, principal, &exec(b"/usr/bin/true")),
        Err(EngineError::CommandDenied)
    );
}

#[test]
fn empty_and_missing_allows_and_effective_shell_restrictions_are_distinct() {
    let principal = PrincipalId([2; 32]);
    let shell = CommandRequest::Shell {
        command: b"echo ; anything".to_vec(),
    };
    let mut p = policy(Restrictions::default(), vec![]);
    assert_eq!(check_command(&p, principal, &shell), Ok(false));
    p.command_policy.as_mut().unwrap().defaults = allow(&[]);
    assert_eq!(
        check_command(&p, principal, &exec(b"/usr/bin/echo")),
        Err(EngineError::CommandDenied)
    );
    assert_eq!(
        check_command(&p, principal, &shell),
        Err(EngineError::CommandDenied)
    );
    p.command_policy.as_mut().unwrap().defaults = Restrictions {
        deny_exec: vec!["/never".into()],
        ..Default::default()
    };
    assert_eq!(
        check_command(&p, principal, &shell),
        Err(EngineError::CommandDenied)
    );
    for path in [&b"echo"[..], &b"./echo"[..], &vec![b'/'; 4096][..]] {
        assert_eq!(
            check_command(&p, principal, &exec(path)),
            Err(EngineError::CommandDenied)
        );
    }
    p.command_policy = None;
    assert_eq!(check_command(&p, principal, &exec(b"echo")), Ok(false));
}

#[test]
fn policy_bounds_and_strict_deserialization_reject_bad_configuration() {
    let p = policy(allow(&["/**"]), vec![]);
    let mut commands = p.command_policy.unwrap();
    assert!(commands.validate(&p.allowed_principals).is_ok());
    commands.version = 2;
    assert_eq!(
        commands.validate(&p.allowed_principals),
        Err(EngineError::InvalidPolicy)
    );
    commands.version = 1;
    commands.keys = vec![KeyRestrictions {
        principal: PrincipalId([9; 32]),
        restrictions: Default::default(),
    }];
    assert!(commands.validate(&p.allowed_principals).is_err());
    commands.keys = vec![
        KeyRestrictions {
            principal: p.allowed_principals[0],
            restrictions: Default::default()
        };
        129
    ];
    assert!(commands.validate(&p.allowed_principals).is_err());
    commands.keys.clear();
    commands.defaults = Restrictions {
        allow_exec: Some(vec!["/**".into(); 257]),
        ..Default::default()
    };
    assert!(commands.validate(&p.allowed_principals).is_err());
    assert!(compile(&format!("/{}", "x".repeat(1024))).is_err());
    for json in [
        r#"{"version":true}"#,
        r#"{"version":1,"version":1}"#,
        r#"{"version":1,"typo":[]}"#,
        r#"{"version":1,"defaults":{"allow_exec":[],"unknown":0}}"#,
        r#"{"version":1,"defaults":{"deny_exec":[],"deny_exec":[]}}"#,
    ] {
        assert!(serde_json::from_str::<CommandPolicy>(json).is_err());
    }
}

#[test]
fn adversarial_fuel_exhaustion_denies_independently_of_pattern_order() {
    let candidate = format!("/{}", "a".repeat(2000));
    let long = format!("/{}", "*a".repeat(500));
    let mut p = policy(allow(&["/**", &long]), vec![]);
    let principal = p.allowed_principals[0];
    assert_eq!(
        check_command(&p, principal, &exec(candidate.as_bytes())),
        Err(EngineError::CommandDenied)
    );
    p.command_policy
        .as_mut()
        .unwrap()
        .defaults
        .allow_exec
        .as_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        check_command(&p, principal, &exec(candidate.as_bytes())),
        Err(EngineError::CommandDenied)
    );
}

#[test]
fn canonical_check_refuses_aliases_but_preserves_argument_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("executable");
    std::fs::write(&file, b"content").unwrap();
    let canonical = std::fs::canonicalize(&file).unwrap();
    let command = exec(canonical.as_os_str().as_bytes());
    assert_eq!(require_canonical_exec(&command), Ok(()));
    let alias = directory.path().join("alias");
    std::os::unix::fs::symlink(&file, &alias).unwrap();
    for bad in [
        alias,
        directory.path().join("./executable"),
        directory.path().join("absent"),
    ] {
        assert_eq!(
            require_canonical_exec(&exec(bad.as_os_str().as_bytes())),
            Err(EngineError::CommandDenied)
        );
    }
}
