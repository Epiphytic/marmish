use super::*;

#[test]
fn command_bounds_are_byte_oriented_and_shell_is_explicit() {
    let exec = |argv| CommandRequest::Exec { argv };
    assert!(validate_request(&exec(vec![b"/bin/echo".to_vec(), vec![255]]), false).is_ok());
    for argv in [
        vec![],
        vec![vec![]],
        vec![vec![b'x'; 4097]],
        vec![b"x".to_vec(), vec![0]],
        vec![vec![b'x']; 257],
        vec![b"x".to_vec(), vec![b'x'; 65535]],
    ] {
        assert_eq!(
            validate_request(&exec(argv), true),
            Err(EngineError::InvalidRequest)
        );
    }
    let shell = CommandRequest::Shell {
        command: b"true".to_vec(),
    };
    assert_eq!(
        validate_request(&shell, false),
        Err(EngineError::InvalidRequest)
    );
    assert!(validate_request(&shell, true).is_ok());
    assert_eq!(
        validate_request(&CommandRequest::Shell { command: vec![0] }, true),
        Err(EngineError::InvalidRequest)
    );
}

#[test]
fn limits_reject_overflow_inputs_before_arithmetic_and_enforce_retention_floor() {
    let valid = EngineLimits::default();
    assert!(validate_limits(&valid).is_ok());
    let mut invalid = valid.clone();
    invalid.spool_bytes_per_stream = u64::MAX;
    assert_eq!(validate_limits(&invalid), Err(EngineError::InvalidPolicy));
    invalid = valid.clone();
    invalid.tombstone_retention = Duration::from_secs(48 * 3600 - 1);
    assert_eq!(validate_limits(&invalid), Err(EngineError::InvalidPolicy));
    invalid = valid;
    invalid.aggregate_spool_bytes = invalid.spool_bytes_per_stream;
    assert_eq!(validate_limits(&invalid), Err(EngineError::InvalidPolicy));
}

#[test]
fn authorization_binds_server_account_and_current_allowlist() {
    let account = launcher::local_account(None).unwrap();
    let mut policy = EnginePolicy {
        state_root: "/tmp/not-opened".into(),
        worker_executable: "/usr/bin/true".into(),
        server: ServerId([1; 32]),
        allowed_principals: vec![PrincipalId([2; 32])],
        allow_shell_command: false,
        cwd: None,
        limits: EngineLimits::default(),
    };
    let ctx = authorize(
        &policy,
        &account,
        PrincipalId([2; 32]),
        EndpointId([3; 32]),
        policy.server,
        account.uid,
    )
    .unwrap();
    assert!(context_owner(&policy, &account, &ctx).is_ok());
    policy.allowed_principals.clear();
    assert!(matches!(
        context_owner(&policy, &account, &ctx),
        Err(EngineError::Unauthorized)
    ));
    policy.allowed_principals.push(PrincipalId([2; 32]));
    assert!(matches!(
        authorize(
            &policy,
            &account,
            PrincipalId([2; 32]),
            EndpointId([3; 32]),
            ServerId([4; 32]),
            account.uid
        ),
        Err(EngineError::Unauthorized)
    ));
}
