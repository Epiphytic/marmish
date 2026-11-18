//! Single-owner, zero-egress native Leave qualification and sealed readback.

use std::{
    cell::Cell,
    num::NonZeroU64,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use crate::{
    command_limits::continuous_time,
    mdk_adapter::peer_native_signer::CapturedLeave,
    peer_quota_vfs::dispatch::{LeaveTurn, ProfileSlots, RawBinding},
};

use super::*;
use consent_checkpoint::provisioning::prepared_consent::custody::leave_effect::Effect;
use consent_checkpoint::wire::{Codec, MAX_OUTER_BYTES};

pub(crate) struct TurnFence<'a> {
    clock: &'a Wall,
    stop: &'a AtomicBool,
    injected_ticks: Option<&'a AtomicU64>,
    wall_limit: u64,
    time_limit: Duration,
    last_wall: Cell<u64>,
    last_time: Cell<Duration>,
}

impl<'a> TurnFence<'a> {
    fn new(
        clock: &'a Wall,
        stop: &'a AtomicBool,
        injected_ticks: Option<&'a AtomicU64>,
        wall_now: u64,
        wall_limit: u64,
    ) -> Result<Self> {
        let start = Self::ticks(injected_ticks)?;
        let remaining = wall_limit
            .checked_sub(wall_now)
            .filter(|remaining| *remaining > 0)
            .ok_or(Error::Clock)?;
        Ok(Self {
            clock,
            stop,
            injected_ticks,
            wall_limit,
            time_limit: start
                .checked_add(Duration::from_secs(remaining.min(5)))
                .ok_or(Error::Clock)?,
            last_wall: Cell::new(wall_now),
            last_time: Cell::new(start),
        })
    }

    fn ticks(injected: Option<&AtomicU64>) -> Result<Duration> {
        if let Some(ticks) = injected {
            Ok(Duration::from_millis(ticks.load(Ordering::Relaxed)))
        } else {
            continuous_time().map_err(|_| Error::Clock)
        }
    }

    fn wall(&self) -> Result<u64> {
        let wall = self.clock.sample()?.as_secs();
        let ticks = Self::ticks(self.injected_ticks)?;
        if self.stop.load(Ordering::Acquire)
            || wall < self.last_wall.get()
            || wall >= self.wall_limit
            || ticks < self.last_time.get()
            || ticks >= self.time_limit
        {
            return Err(Error::Clock);
        }
        self.last_wall.set(wall);
        self.last_time.set(ticks);
        Ok(wall)
    }

    pub(crate) fn check(&self) -> std::result::Result<(), StageError> {
        self.wall().map(|_| ()).map_err(|_| StageError::Unavailable)
    }
}

pub(crate) struct LeaveSnapshot {
    stage: (Binding, FileWitness),
    authority: JoinedAuthority,
    sealed: Option<CapturedLeave>,
    image: [u8; 32],
}

impl LeaveSnapshot {
    pub(crate) fn pristine(&self, stage: (Binding, FileWitness)) -> Option<[u8; 32]> {
        (self.stage == stage && self.sealed.is_none()).then_some(self.image)
    }

    pub(crate) fn captured(
        &self,
        stage: (Binding, FileWitness),
    ) -> Option<([u8; 32], [u8; 32], &[u8])> {
        (self.stage == stage).then_some(())?;
        self.sealed.as_ref().map(CapturedLeave::event)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Cut {
    Armed,
    NativeClosed,
    Captured,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Disposition {
    Indeterminate,
    Captured([u8; 32]),
}

pub(super) struct LeaveOwner<'a> {
    owner: PreparedConsentOwner<'a>,
    slots: ProfileSlots<'a>,
}

impl<'a> LeaveOwner<'a> {
    pub(super) fn open(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        policy: Policy,
        route: &Route,
        wall: Wall,
    ) -> Result<Self> {
        let owner = PreparedConsentOwner::open_with_epoch(
            lease,
            material,
            Role::Client,
            policy,
            route,
            wall,
            Epoch::LeaveEffect,
        )?;
        let slots = ProfileSlots::reserve(lease, 0).map_err(|_| Error::Path)?;
        Ok(Self { owner, slots })
    }

    pub(super) fn crash_inside_original(mut self) -> Self {
        self.slots = self.slots.fixture_stop_in_synced_transaction();
        self
    }

    pub(super) fn run(
        &mut self,
        selector: Selector,
        generation: u64,
        stop: &AtomicBool,
        ticks: Option<&AtomicU64>,
        mut cut: impl FnMut(Cut) -> Result<()>,
    ) -> Result<Disposition> {
        let initialized = self.owner.available_sample()?.as_secs();
        let state = &mut self.owner.state;
        let hold = state
            .checkpoint
            .read_leave_hold(&state.source, selector, generation, initialized)?
            .ok_or(Error::Binding)?;
        if !hold.is_idle() {
            return Err(Error::Binding);
        }
        let fence = TurnFence::new(&state.wall, stop, ticks, initialized, hold.deadline())?;
        let wall = fence.wall()?;
        let effect = state
            .checkpoint
            .native_leave_effect(&state.source, &hold, wall)?;
        let evidence = hold.evidence();
        let delivery = Codec::new(MAX_OUTER_BYTES)
            .map_err(|_| Error::Binding)?
            .reopen_custodied_delivery(&state.signer, evidence, evidence.held_at())
            .map_err(|_| Error::Binding)?;
        let stage = evidence.stage();
        let original = state.paths.reopen_provisional(stage.0, stage.1)?;
        let join = JoinWitness::new(&delivery, &state.origin_url, evidence.package_digest())?;
        let key = SqlCipherKey::raw(*state.paths.provisional_key(stage.0));
        let operation = || LeaveTurn {
            key: &key,
            signer: state.signer.clone(),
            origin: &state.origin_url,
            label: state.paths.provisional_label(stage.0),
            join: &join,
            fence: &fence,
        };
        let mut result = Self::inspect_slots(
            &self.slots,
            &original,
            stage,
            operation(),
            match effect {
                Effect::Captured(id) => Some(id),
                _ => None,
            },
        )?;
        let exact = |proof: &LeaveSnapshot| {
            evidence.matches_pending_readback(&JoinedReadback {
                stage,
                carrier: delivery.flight().carrier_id.to_bytes(),
                authority: proof.authority.clone(),
            })
        };
        if !exact(&result) {
            return Err(Error::Binding);
        }
        let mut local_id = None;
        if effect == Effect::Absent {
            let start = state.checkpoint.reserve_native_leave(
                &state.source,
                &hold,
                &result,
                fence.wall()?,
            )?;
            cut(Cut::Armed)?;
            fence.wall()?;
            start.check_image(stage, original.digest()?)?;
            let id = self
                .slots
                .leave_turn(
                    RawBinding {
                        path: original.path(),
                        generation: NonZeroU64::new(generation).ok_or(Error::Binding)?,
                        known: stage.0.coordinates().0,
                        witness: (
                            u64::from_be_bytes(stage.1.device),
                            u64::from_be_bytes(stage.1.inode),
                        ),
                    },
                    operation(),
                    result.authority.coordinates(),
                )
                .map_err(|_| Error::Path)??;
            local_id = Some(<[u8; 32]>::try_from(id.as_slice()).map_err(|_| Error::Binding)?);
            original.ensure_unchanged()?;
            cut(Cut::NativeClosed)?;
            fence.wall()?;
            result = Self::inspect_slots(&self.slots, &original, stage, operation(), None)?;
            if !exact(&result) {
                return Err(Error::Binding);
            }
        }
        let Some((id, _, _)) = result.captured(stage) else {
            return Ok(Disposition::Indeterminate);
        };
        if local_id.is_some_and(|returned| returned != id) || original.digest()? != result.image {
            return Err(Error::Binding);
        }
        let id =
            state
                .checkpoint
                .capture_native_leave(&state.source, &hold, result, fence.wall()?)?;
        cut(Cut::Captured)?;
        Ok(Disposition::Captured(id))
    }

    fn inspect_slots(
        slots: &ProfileSlots<'_>,
        original: &paths::ProvisionalFile,
        stage: (Binding, FileWitness),
        operation: LeaveTurn<'_>,
        expected: Option<[u8; 32]>,
    ) -> Result<LeaveSnapshot> {
        let image = original.digest()?;
        let reader = original.read_file()?;
        let length = reader.metadata().map_err(|_| Error::Path)?.len();
        let name = slots.leave_snapshot_path(length).map_err(|_| Error::Path)?;
        let mut copy = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(name)
            .map_err(|_| Error::Path)?;
        let inode = snapshot_inode(&copy.metadata().map_err(|_| Error::Path)?)?;
        if inode.0 != u64::from_be_bytes(stage.1.device)
            || inode.1 == u64::from_be_bytes(stage.1.inode)
        {
            return Err(Error::StageIdentityMismatch);
        }
        let copied = io::copy(&mut reader.take(length), &mut copy).map_err(|_| Error::Path)?;
        drop(copy);
        if copied != length || read_image(name)? != (image, inode) || original.digest()? != image {
            return Err(Error::StageIdentityMismatch);
        }
        let inspected = slots
            .inspect_leave_snapshot(name, inode, operation, expected)
            .map_err(|_| Error::Path)?;
        if read_image(name)?.1 != inode || original.digest()? != image {
            return Err(Error::StageIdentityMismatch);
        }
        fs::remove_file(name).map_err(|_| Error::Path)?;
        snapshot_sidecars(name)?;
        let checked = inspected?;
        Ok(LeaveSnapshot {
            stage,
            authority: checked.authority,
            sealed: checked.retained,
            image,
        })
    }
}

#[test]
fn peer_terminal_leave_fence_checks_stop_regression_and_exact_expiry() {
    let wall = Arc::new(AtomicU64::new(100));
    let clock = Wall::Fixture(wall.clone());
    let stop = AtomicBool::new(false);
    let ticks = AtomicU64::new(10);
    let exact = TurnFence::new(&clock, &stop, Some(&ticks), 100, 120).unwrap();
    ticks.store(5_009, Ordering::Relaxed);
    assert_eq!(exact.wall(), Ok(100));
    ticks.store(5_010, Ordering::Relaxed);
    assert_eq!(exact.wall(), Err(Error::Clock));
    ticks.store(0, Ordering::Relaxed);
    assert_eq!(exact.wall(), Err(Error::Clock));

    let wall_only = TurnFence::new(&clock, &stop, Some(&ticks), 100, 102).unwrap();
    wall.store(101, Ordering::Relaxed);
    assert_eq!(wall_only.wall(), Ok(101));
    wall.store(100, Ordering::Relaxed);
    assert_eq!(wall_only.wall(), Err(Error::Clock));
    wall.store(102, Ordering::Relaxed);
    assert_eq!(wall_only.wall(), Err(Error::Clock));
    wall.store(103, Ordering::Relaxed);
    assert_eq!(wall_only.wall(), Err(Error::Clock));

    wall.store(101, Ordering::Relaxed);
    let stopped = TurnFence::new(&clock, &stop, Some(&ticks), 101, 102).unwrap();
    assert_eq!(stopped.wall(), Ok(101));
    stop.store(true, Ordering::Release);
    assert_eq!(stopped.wall(), Err(Error::Clock));
}
