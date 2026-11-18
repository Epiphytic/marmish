use super::*;
use std::{
    panic::AssertUnwindSafe,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn fence() -> Arc<AttachmentFence> {
    AttachmentFence::new(NonZeroU64::new(3).unwrap(), [7; 32])
}

#[test]
fn terminal_fence_core_resumes_only_equal_active_generation() {
    let fence = fence();
    assert_eq!(fence.generation().get(), 3);
    assert_eq!(fence.with_effect(|| 4), Effect::Applied(4));
    let update = fence.begin_update().unwrap();
    assert_eq!(fence.state(), FenceState::Suspect);
    assert_eq!(fence.with_effect(|| panic!()), Effect::<()>::Suspect);
    assert!(fence.begin_update().is_none());
    assert_eq!(update.resolve(Some([7; 32])), FenceState::Live);
    assert_eq!(fence.with_effect(|| 2), Effect::Applied(2));
    assert_eq!(
        fence.begin_update().unwrap().resolve(Some([8; 32])),
        FenceState::Retired
    );
    assert_eq!(fence.state(), FenceState::Retired);
    assert_eq!(fence.with_effect(|| panic!()), Effect::<()>::Retired);
    assert!(fence.begin_update().is_none());
}

#[test]
fn terminal_fence_core_drop_absence_and_explicit_retirement_are_sticky() {
    let dropped = fence();
    drop(dropped.begin_update().unwrap());
    assert_eq!(dropped.state(), FenceState::Retired);
    let absent = fence();
    assert_eq!(
        absent.begin_update().unwrap().resolve(None),
        FenceState::Retired
    );
    let retired = fence();
    let update = retired.begin_update().unwrap();
    retired.retire();
    assert_eq!(update.resolve(Some([7; 32])), FenceState::Retired);
}

#[test]
fn terminal_fence_core_token_drop_during_unwind_copes_with_poison() {
    let fence = fence();
    let update = fence.begin_update().unwrap();
    let error = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _update = update;
        let _guard = fence.state.lock().unwrap();
        panic!("fixture poisons guard before dropping update");
    }));
    assert!(error.is_err());
    assert_eq!(fence.state(), FenceState::Retired);
    assert_eq!(fence.with_effect(|| panic!()), Effect::<()>::Retired);
    let poisoned = self::fence();
    assert!(std::panic::catch_unwind(|| poisoned.with_effect(|| panic!())).is_err());
    assert_eq!(poisoned.state(), FenceState::Retired);
    assert!(poisoned.begin_update().is_none());
}

#[test]
fn terminal_fence_core_pending_update_precedes_further_effects() {
    let fence = fence();
    let (entered, effect_entered) = mpsc::channel();
    let (release, effect_release) = mpsc::channel();
    let (updated, update_done) = mpsc::channel();
    let timeout = Duration::from_secs(3);
    thread::scope(|scope| {
        scope.spawn(|| {
            assert_eq!(
                fence.with_effect(move || {
                    entered.send(()).unwrap();
                    effect_release.recv_timeout(timeout).unwrap()
                }),
                Effect::Applied(())
            );
        });
        effect_entered.recv_timeout(timeout).unwrap();
        assert_eq!(fence.with_effect(|| panic!()), Effect::<()>::Contended);
        scope.spawn(|| updated.send(fence.begin_update().unwrap()).unwrap());
        let deadline = Instant::now() + timeout;
        loop {
            match fence.with_effect(|| panic!()) {
                Effect::Suspect => break,
                Effect::Contended if Instant::now() < deadline => thread::yield_now(),
                other => panic!("no requested update before deadline: {other:?}"),
            }
        }
        for _ in 0..256 {
            assert_eq!(fence.with_effect(|| panic!()), Effect::<()>::Suspect);
        }
        assert!(update_done.try_recv().is_err());
        release.send(()).unwrap();
        let update = update_done.recv_timeout(timeout).unwrap();
        assert_eq!(update.resolve(Some([7; 32])), FenceState::Live);
    });
}

#[test]
#[ignore = "local microbenchmark"]
fn terminal_fence_bench() {
    let fence = fence();
    let mut nanos = Vec::with_capacity(20_000);
    for _ in 0..nanos.capacity() {
        let start = Instant::now();
        assert_eq!(
            fence.with_effect(|| std::hint::black_box(1)),
            Effect::Applied(1)
        );
        nanos.push(start.elapsed().as_nanos());
    }
    nanos.sort_unstable();
    eprintln!(
        "fence_ns p50={} p95={}",
        nanos[nanos.len() / 2],
        nanos[nanos.len() * 95 / 100]
    );
}
