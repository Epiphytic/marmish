use super::tests::{allow, exec, policy};
use super::*;

#[test]
fn inventory_reports_only_selected_layers_in_admission_order() {
    let target = PrincipalId([2; 32]);
    let defaults = allow(&["/usr/**"]);
    let allow_layer = allow(&["/usr/bin/*"]);
    let deny_layer = Restrictions {
        allow_exec: None,
        deny_exec: vec!["/usr/bin/forbidden".into()],
    };
    let engine = policy(
        defaults.clone(),
        vec![
            KeyRestrictions {
                principal: PrincipalId([3; 32]),
                restrictions: allow(&["/private/**"]),
            },
            KeyRestrictions {
                principal: target,
                restrictions: allow_layer.clone(),
            },
            KeyRestrictions {
                principal: target,
                restrictions: Restrictions::default(),
            },
            KeyRestrictions {
                principal: target,
                restrictions: deny_layer.clone(),
            },
        ],
    );
    let description = engine
        .command_policy
        .as_ref()
        .unwrap()
        .describe_for_principal(target);
    assert_eq!(description.version, 1);
    assert_eq!(description.layers, vec![defaults, allow_layer, deny_layer]);
    assert_eq!(
        check_command(&engine, target, &exec(b"/usr/bin/echo")),
        Ok(true)
    );
    assert_eq!(
        check_command(&engine, target, &exec(b"/usr/bin/subdir/echo")),
        Err(EngineError::CommandDenied)
    );
    assert_eq!(
        check_command(
            &engine,
            target,
            &CommandRequest::ResolvedExec {
                argv: vec![b"echo".to_vec()],
                executable: b"/usr/bin/echo".to_vec(),
                invoked_path: b"/usr/bin/forbidden".to_vec(),
            },
        ),
        Err(EngineError::CommandDenied)
    );
}

#[test]
fn inventory_distinguishes_empty_allows_and_does_not_expend_match_budget() {
    let target = PrincipalId([2; 32]);
    let mut engine = policy(Restrictions::default(), vec![]);
    let command = exec(b"/usr/bin/echo");
    let describe = |engine: &EnginePolicy| {
        engine
            .command_policy
            .as_ref()
            .unwrap()
            .describe_for_principal(target)
    };
    assert!(describe(&engine).layers.is_empty());
    assert_eq!(check_command(&engine, target, &command), Ok(false));
    engine.command_policy.as_mut().unwrap().defaults = allow(&[]);
    assert_eq!(describe(&engine).layers, vec![allow(&[])]);
    assert_eq!(
        check_command(&engine, target, &command),
        Err(EngineError::CommandDenied)
    );

    let maximal = format!("/{}", "x".repeat(MAX_PATTERN_BYTES - 1));
    let defaults = Restrictions {
        allow_exec: None,
        deny_exec: vec![maximal; MAX_PATTERNS],
    };
    engine.command_policy.as_mut().unwrap().defaults = defaults.clone();
    assert_eq!(describe(&engine).layers, vec![defaults]);
    assert_eq!(
        check_command(&engine, target, &command),
        Err(EngineError::InvalidPolicy)
    );
}
