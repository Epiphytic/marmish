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

fn listener_candidates(counts: &[u8]) -> BTreeMap<Account, VecDeque<AuthenticatedCommandContext>> {
    let mut id = 0;
    counts
        .iter()
        .enumerate()
        .map(|(peer, count)| {
            let peer = [peer as u8; 32];
            let rooms = (0..*count)
                .map(|_| {
                    let mut room = context(id);
                    room.peer_account = peer;
                    room.direction = CommandDirection::Server;
                    id += 1;
                    room
                })
                .collect();
            (peer, rooms)
        })
        .collect()
}

fn bound_rooms(bindings: &[Binding]) -> Vec<u8> {
    bindings
        .iter()
        .map(|binding| binding.context.mls_group()[0])
        .collect()
}

fn listener_snapshot(authority: &mut Authority, counts: &[u8], now: Instant) -> Vec<Binding> {
    authority.prune_unused();
    authority
        .register_listeners(listener_candidates(counts), now)
        .unwrap()
}

#[test]
fn terminal_runtime_core_listener_pass_is_bounded_and_reaches_later_rooms() {
    let now = Instant::now();
    let mut authority = Authority::default();
    let rooms = [2; 32];
    let first = listener_snapshot(&mut authority, &rooms, now);
    let first_rooms = (0..64).step_by(2).collect::<Vec<_>>();
    assert_eq!(bound_rooms(&first), first_rooms);
    assert_eq!(authority.listeners.next_pass, Some(now + LISTENER_PASS));
    drop(first);
    let early = now + LISTENER_PASS - Duration::from_millis(1);
    assert_eq!(
        bound_rooms(&listener_snapshot(&mut authority, &rooms, early)),
        first_rooms
    );
    assert_eq!(authority.listeners.next_pass, Some(now + LISTENER_PASS));
    let boundary = now + LISTENER_PASS;
    assert_eq!(
        bound_rooms(&listener_snapshot(&mut authority, &rooms, boundary)),
        (1..64).step_by(2).collect::<Vec<_>>()
    );
    assert_eq!(
        authority.listeners.next_pass,
        Some(boundary + LISTENER_PASS)
    );
    assert_eq!(
        bound_rooms(&listener_snapshot(&mut authority, &[2; 2], boundary)),
        [0, 2, 1, 3]
    );
    assert!(authority.listeners.next_pass.is_none());
    assert!(authority.listeners.start_peer.is_none());
    assert!(authority.listeners.after_room.is_empty());
    assert!(authority.listeners.credited.is_empty());
    assert_eq!(
        bound_rooms(&listener_snapshot(&mut authority, &rooms, boundary)),
        first_rooms
    );
}

#[test]
fn terminal_runtime_core_listener_pass_is_peer_fair_with_skew_and_full_capacity() {
    let start = Instant::now();
    let mut authority = Authority::default();
    let mut skew = [1; 32];
    skew[0] = 33;
    for pass in 0..33 {
        let snapshot = listener_snapshot(&mut authority, &skew, start + LISTENER_PASS * pass);
        assert_eq!(snapshot.len(), CAPACITY);
        assert_eq!(bound_rooms(&snapshot)[0], pass as u8);
        assert_eq!(bound_rooms(&snapshot)[1..], (33u8..64).collect::<Vec<_>>());
    }

    let mut authority = Authority::default();
    let mut slots = [0; 20];
    for pass in 0..5 {
        let snapshot = listener_snapshot(&mut authority, &[3; 20], start + LISTENER_PASS * pass);
        assert_eq!(snapshot.len(), CAPACITY);
        for binding in &snapshot {
            slots[binding.context.peer_account()[0] as usize] += 1;
        }
    }
    assert_eq!(slots, [8; 20]);
}

#[test]
fn terminal_runtime_core_listener_rotation_keeps_held_fences_and_reaches_tail() {
    let now = Instant::now();
    let mut authority = Authority::default();
    let initial = listener_snapshot(&mut authority, &[64], now);
    let held = initial[..16].to_vec();
    assert_eq!(initial.len(), CAPACITY);
    assert!(listener_snapshot(&mut authority, &[64], now + LISTENER_PASS).is_empty());
    assert_eq!(authority.entries.len(), CAPACITY);
    assert!(authority.listeners.credited.is_empty());
    drop(initial);
    let next = listener_snapshot(&mut authority, &[64], now + LISTENER_PASS);
    assert_eq!(bound_rooms(&next), (32..48).collect::<Vec<_>>());
    assert_eq!(authority.entries.len(), CAPACITY);
    drop(next);
    let tail = listener_snapshot(&mut authority, &[64], now + LISTENER_PASS * 2);
    assert_eq!(bound_rooms(&tail)[..16], (48..64).collect::<Vec<_>>());
    assert_eq!(bound_rooms(&tail)[16..], (0..16).collect::<Vec<_>>());
    assert_eq!(authority.entries.len(), CAPACITY);
    for (saved, returned) in held.iter().zip(&tail[16..]) {
        assert!(Arc::ptr_eq(&saved.fence, &returned.fence));
        assert_eq!(saved.fence.state(), FenceState::Live);
    }
}

#[test]
fn terminal_runtime_core_listener_uses_identity_to_resume_after_removal_and_zero_credit() {
    let now = Instant::now();
    let mut authority = Authority::default();
    let initial = listener_snapshot(&mut authority, &[3; 20], now);
    let mut schedule = ListenerSchedule::default();
    schedule.select(listener_candidates(&[3; 20]), now);
    schedule.record(&initial);
    let selected = schedule.select(listener_candidates(&[3; 20]), now + LISTENER_PASS);
    assert_eq!(selected[0].peer_account(), [12; 32]);
    assert_eq!(selected[0].mls_group(), [37; 32]);

    let mut changed = listener_candidates(&[3; 20]);
    changed.remove(&[12; 32]);
    changed.get_mut(&[13; 32]).unwrap().pop_front();
    let selected = schedule.select(changed.clone(), now + LISTENER_PASS);
    assert_eq!(selected[0].peer_account(), [13; 32]);
    assert_eq!(selected[0].mls_group(), [40; 32]);
    assert!(schedule.credited.is_empty());
    let after_room = schedule.after_room.clone();
    let selected = schedule.select(changed, now + LISTENER_PASS * 2);
    assert_eq!(selected[0].peer_account(), [14; 32]);
    assert_eq!(schedule.after_room, after_room);
}

#[test]
fn terminal_runtime_core_listener_prefix_uses_high_water_and_survives_failure() {
    let now = Instant::now();
    let mut authority = Authority::default();
    let first = listener_snapshot(&mut authority, &[2; 32], now);
    let full = authority.listeners.credited.clone();
    authority.listeners.record(&first[..31]);
    assert_eq!(authority.listeners.credited, full);

    let mut schedule = ListenerSchedule::default();
    schedule.select(listener_candidates(&[2; 32]), now);
    schedule.record(&first[..3]);
    let previous = schedule.credited.clone();
    schedule.record(&first[5..7]);
    assert_eq!(schedule.credited, previous);
    schedule.record(&first[5..8]);
    assert_ne!(schedule.credited, previous);
    assert_eq!(schedule.credited[0].0, [5; 32]);

    drop(first);
    authority.prune_unused();
    authority.generation = u64::MAX;
    assert!(matches!(
        authority.register_listeners(listener_candidates(&[2; 32]), now + LISTENER_PASS),
        Err(Error::Invalid("terminal_authority_generation"))
    ));
    assert_eq!(authority.listeners.credited, full);
    assert_eq!(authority.listeners.next_pass, Some(now + LISTENER_PASS));
}

#[test]
fn terminal_runtime_core_listener_soft_refusal_preserves_later_registrations() {
    let now = Instant::now();
    let mut authority = Authority::default();
    let mut held = listener_snapshot(&mut authority, &[33], now);
    drop(held.remove(0));
    authority.prune_unused();
    let mut all = listener_candidates(&[33]);
    let candidate = all.values_mut().next().unwrap().pop_back().unwrap();
    let following = authority.register(candidate).unwrap();
    let refreshed = listener_snapshot(&mut authority, &[33], now + LISTENER_PASS);
    assert_eq!(
        bound_rooms(&refreshed),
        [32].into_iter().chain(1..31).collect::<Vec<_>>()
    );
    assert!(Arc::ptr_eq(&following.fence, &refreshed[0].fence));
    assert!(Arc::ptr_eq(&held[0].fence, &refreshed[1].fence));
    assert_eq!(authority.listeners.credited.len(), 1);
    assert_eq!(authority.listeners.credited[0].1.1, [32; 32]);
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
