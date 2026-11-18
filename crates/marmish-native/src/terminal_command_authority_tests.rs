use super::*;
use crate::terminal_fence::Effect;

fn context(id: u8) -> AuthenticatedCommandContext {
    let mut context = AuthenticatedCommandContext::test_fixture(
        [1; 32],
        [2; 32],
        vec![id; 32],
        [id; 16],
        CommandDirection::Client,
    );
    context.relay_endpoints = vec!["wss://relay.example/".into()];
    context
}

#[test]
fn terminal_command_authority_core_bounds_and_never_reuses_generation() {
    let mut authority = Authority::default();
    assert!(authority.stage(None).entries.is_empty());
    let first = authority.register(context(0)).unwrap();
    assert_eq!(first.context.relay_endpoints(), ["wss://relay.example/"]);
    assert_eq!(first.fence.generation().get(), 1);
    assert!(Arc::ptr_eq(
        &authority.register(context(0)).unwrap().fence,
        &first.fence,
    ));
    for id in 1..CAPACITY as u8 {
        authority.register(context(id)).unwrap();
    }
    assert!(matches!(
        authority.register(context(CAPACITY as u8)),
        Err(Error::Invalid("terminal_authority_capacity"))
    ));
    first.fence.retire();
    let renewed = authority.register(context(0)).unwrap();
    assert_eq!(renewed.fence.generation().get(), CAPACITY as u64 + 1);
    assert_eq!(first.fence.state(), FenceState::Retired);
    assert_eq!(renewed.fence.state(), FenceState::Live);
    drop(authority);
    assert_eq!(renewed.fence.state(), FenceState::Retired);
}

#[test]
fn terminal_command_authority_core_exact_groups_and_failed_update() {
    use cgka_traits::MemberId;

    let mut authority = Authority::default();
    let first = authority.register(context(1)).unwrap();
    let other = authority.register(context(2)).unwrap();
    let targeted = TransportEnvelope::GroupMessage {
        transport_group_id: vec![1; 32],
    };
    let group = authority.delivery_group(&targeted).unwrap();
    let mut pending = authority.stage(Some(&group));
    assert_eq!(pending.entries.len(), 1);
    assert_eq!(first.fence.with_effect(|| true), Effect::Suspect);
    assert_eq!(other.fence.with_effect(|| true), Effect::Applied(true));
    let (entry, token) = pending.entries.pop().unwrap();
    assert_eq!(entry.context.mls_group(), [1; 32]);
    assert_eq!(
        token.resolve(Some(entry.context.authority_digest())),
        FenceState::Live
    );

    let unrelated = TransportEnvelope::Welcome {
        recipient: MemberId::new(vec![1; 32]),
    };
    assert!(authority.delivery_group(&unrelated).is_none());
    let pending = authority.stage(None);
    assert_eq!(pending.entries.len(), 2);
    drop(pending);
    assert_eq!(first.fence.state(), FenceState::Retired);
    assert_eq!(other.fence.state(), FenceState::Retired);
    assert!(authority.stage(None).entries.is_empty());
}

#[test]
fn terminal_command_authority_core_changed_route_and_generation_overflow() {
    let mut authority = Authority::default();
    let first = authority.register(context(1)).unwrap();
    let mut changed = context(1);
    changed.relay_endpoints = vec!["wss://alternate.example/".into()];
    let replacement = authority.register(changed).unwrap();
    assert_eq!(first.fence.state(), FenceState::Retired);
    assert_eq!(replacement.fence.generation().get(), 2);
    authority.generation = u64::MAX;
    assert!(matches!(
        authority.register(context(2)),
        Err(Error::Invalid("terminal_authority_generation"))
    ));
    assert_eq!(replacement.fence.state(), FenceState::Live);
}

#[test]
#[ignore = "local timing measurements"]
fn terminal_command_authority_bench() {
    let mut authority = Authority::default();
    let mut empty = Authority::default();
    for id in 0..CAPACITY as u8 {
        authority.register(context(id)).unwrap();
    }
    let report = |label: &str, mut run: Box<dyn FnMut() + '_>| {
        let mut samples = Vec::with_capacity(10_000);
        for _ in 0..10_000 {
            let before = std::time::Instant::now();
            run();
            samples.push(before.elapsed().as_nanos());
        }
        samples.sort_unstable();
        eprintln!("{label}: p50={}ns p95={}ns", samples[4999], samples[9499]);
    };
    report(
        "empty table stage",
        Box::new(|| assert!(empty.stage(None).entries.is_empty())),
    );
    report(
        "stage and resume 32 bindings",
        Box::new(|| {
            for (entry, token) in authority.stage(None).entries {
                assert_eq!(
                    token.resolve(Some(entry.context.authority_digest())),
                    FenceState::Live
                );
            }
        }),
    );
}
