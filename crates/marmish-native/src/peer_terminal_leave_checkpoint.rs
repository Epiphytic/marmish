//! Local-only hold observations and bounded recovery reservation accounting.

use super::leave_schema::Writer;
use super::terminal::{TerminalRecord, TerminalStep, advance_local_wall, selection};
use super::*;

pub(super) const WINDOW: u64 = 86_400;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Recovery {
    Idle,
    Reserved { attempts: u8, at: u64 },
    Parked { at: u64 },
}

#[derive(Debug, Eq, PartialEq)]
struct SavedHold {
    recovery: Recovery,
    not_before: u64,
    deadline: u64,
    route: [u8; 32],
}

pub(crate) struct Hold {
    pub(super) terminal: TerminalRecord,
    pub(super) custody: HeldEvidence,
    saved: SavedHold,
}

impl Hold {
    pub(crate) fn evidence(&self) -> &HeldEvidence {
        &self.custody
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.saved.recovery == Recovery::Idle
    }

    pub(super) fn selector(&self) -> Selector {
        self.terminal.selector()
    }

    pub(crate) fn deadline(&self) -> u64 {
        self.saved.deadline
    }

    pub(super) fn recovery(&self) -> Recovery {
        self.saved.recovery
    }

    pub(super) fn same_turn(&self, prior: &Self) -> bool {
        self.terminal.same_turn(&prior.terminal)
            && self.custody.same_turn(&prior.custody)
            && self.saved == prior.saved
    }

    pub(super) fn not_before(&self) -> u64 {
        self.saved.not_before
    }
}

pub(super) fn deadline(wall: u64) -> Result<u64> {
    let deadline = wall.checked_add(WINDOW).ok_or(CursorError::Range)?;
    seconds(deadline)?;
    Ok(deadline)
}

pub(super) fn exists(tx: &Transaction<'_>, who: Binding) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM peer_terminal_native_leave WHERE class=?1 AND generation=?2)",
        params![who.generation.known, seconds(who.generation.count)?],
        |row| row.get(0),
    )?)
}

/// The closure inserts the parent through its ordinary authorized aperture.
/// This helper refuses a pre-existing parent before opening either aperture.
pub(super) fn insert_terminal_and_hold(
    tx: &Transaction<'_>,
    prior: &HeldEvidence,
    wall: u64,
    insert_parent: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let binding = prior.stage().0;
    let coordinates = params![binding.generation.known, seconds(binding.generation.count)?];
    let preexisting = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM peer_pending_terminal WHERE class=?1 AND generation=?2)",
        coordinates,
        |row| row.get::<_, bool>(0),
    )?;
    if preexisting {
        return Err(CursorError::Stale.into());
    }

    let deadline = deadline(wall)?;
    insert_parent()?;
    Writer::Insert.guard(tx, false)?;
    rows::require_write(tx.execute(
        "INSERT INTO peer_terminal_native_leave (
             class,generation,origin,account,founder,offer_id,offer_revision,consent_carrier,
             consent_revision,delivery_carrier,delivery_digest,welcome_id,welcome_digest,device,
             inode,group_id,epoch_number,epoch_identity,own_leaf,route,hold_deadline,
             recovery_state,recovery_attempts,recovery_not_before
         )
         SELECT t.class,t.generation,t.origin,t.account,t.server,t.offer_id,t.offer_revision,
             t.consent_carrier,c.consent_revision,t.delivery_carrier,t.delivery_digest,
             c.welcome_id,t.welcome_digest,t.device,t.inode,t.group_id,t.epoch_number,
             t.epoch_identity,t.own_leaf,?3,?4,0,0,?5
         FROM peer_pending_terminal t JOIN peer_pending_custody c
           ON c.class=t.class AND c.generation=t.generation
         WHERE t.class=?1 AND t.generation=?2 AND t.delivery_carrier=?6",
        params![
            binding.generation.known,
            seconds(binding.generation.count)?,
            prior.offer().routing().nostr_group_id,
            seconds(deadline)?,
            seconds(wall)?,
            prior.delivery_carrier(),
        ],
    )?)?;
    Writer::Insert.guard(tx, true)
}

impl Context {
    pub(super) fn leave_hold(
        &self,
        tx: &Transaction<'_>,
        selector: Selector,
        generation: u64,
    ) -> Result<Option<Hold>> {
        let saved = tx
            .query_row(
                "SELECT class,route,hold_deadline,recovery_state,recovery_attempts,
                        recovery_reserved_at,recovery_not_before
                 FROM peer_terminal_native_leave
                 WHERE account=?1 AND origin=?2 AND founder=?3 AND offer_id=?4
                   AND offer_revision=?5 AND generation=?6",
                params![
                    self.account,
                    self.origin,
                    selector.server.to_bytes(),
                    selector.id,
                    selector.revision,
                    seconds(generation)?,
                ],
                |row| {
                    let recovery = match (row.get::<_, u8>(3)?, row.get::<_, u8>(4)?) {
                        (0, 0) => Recovery::Idle,
                        (1, attempts @ 1..=3) => Recovery::Reserved {
                            attempts,
                            at: read_seconds(row, 5)?,
                        },
                        (2, 3) => Recovery::Parked {
                            at: read_seconds(row, 5)?,
                        },
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    Ok((
                        row.get(0)?,
                        SavedHold {
                            route: row.get(1)?,
                            deadline: read_seconds(row, 2)?,
                            recovery,
                            not_before: read_seconds(row, 6)?,
                        },
                    ))
                },
            )
            .optional()?;
        let Some((known, saved)) = saved else {
            return Ok(None);
        };
        let binding = Binding {
            key: selector,
            origin: self.origin,
            generation: Generation {
                known,
                count: generation,
            },
        };
        let terminal = self
            .terminal_record(tx, selector, generation)?
            .ok_or(CursorError::Binding)?;
        let queued = self
            .exact_consent(tx, selector)?
            .ok_or(CursorError::Binding)?;
        let prior = self
            .read_evidence(tx, queued)?
            .ok_or(CursorError::Binding)?;
        if terminal.step() != &TerminalStep::Fenced
            || prior.stage().0 != binding
            || prior.state() != CustodyState::Pending
            || saved.route != prior.offer().routing().nostr_group_id
            || saved.deadline != deadline(terminal.decided_at())?
        {
            return Err(CursorError::Binding.into());
        }

        Ok(Some(Hold {
            terminal,
            custody: prior,
            saved,
        }))
    }
}

impl Checkpoint {
    pub(super) fn attach_terminal_leave_fixture(
        conn: HardenedPeerConnection,
        signer: Keys,
        uid: u32,
        url: &str,
        policy: Policy,
    ) -> Result<Self> {
        let origin = super::super::super::super::Route::configured(url)?.id;
        let (cursor, epoch) =
            custody_schema::open_terminal_leave(conn, signer.public_key().to_bytes(), origin)?;
        let mut checkpoint = Self::attach_with(cursor, signer, uid, &[url], policy, |_, _| Ok(()))?;
        checkpoint.provisional_schema = true;
        checkpoint.export_epoch = Some(super::super::Epoch::Custody(epoch));
        Ok(checkpoint)
    }

    fn require_leave_context(&self, source: &Source) -> Result<Context> {
        let cx = self.custody_context(source)?;
        if !cx.epoch.is_terminal_leave() {
            return Err(CursorError::Binding.into());
        }
        Ok(cx)
    }

    pub(crate) fn read_leave_hold(
        &mut self,
        source: &Source,
        selector: Selector,
        generation: u64,
        wall: u64,
    ) -> Result<Option<Hold>> {
        let cx = self.require_leave_context(source)?;
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        cx.leave_hold(&tx, selector, generation)
    }

    pub(super) fn leave_recovery_due(
        &mut self,
        source: &Source,
        wall: u64,
    ) -> Result<Option<Hold>> {
        let cx = self.require_leave_context(source)?;
        if cx.epoch.is_leave_effect() {
            return Err(CursorError::Binding.into());
        }
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let selected = tx
            .query_row(
                "SELECT h.founder,h.offer_id,h.offer_revision,h.generation
                 FROM peer_terminal_native_leave h JOIN peer_pending_terminal t
                   ON t.class=h.class AND t.generation=h.generation
                 WHERE h.account=?1 AND h.origin=?2 AND t.step=0
                   AND h.recovery_state IN (0,1) AND h.recovery_not_before<=?3
                 ORDER BY t.decided_at,h.class LIMIT 1",
                params![cx.account, cx.origin, seconds(wall)?],
                selection,
            )
            .optional()?;
        let Some((selector, generation)) = selected else {
            return Ok(None);
        };
        cx.leave_hold(&tx, selector, generation)?
            .map(Some)
            .ok_or(CursorError::Binding.into())
    }

    pub(super) fn reserve_or_park_leave_recovery(
        &mut self,
        source: &Source,
        prior: &Hold,
        wall: u64,
    ) -> Result<Hold> {
        self.check_local_owner(&prior.custody.owner)?;
        let cx = self.require_leave_context(source)?;
        if cx.epoch.is_leave_effect() {
            return Err(CursorError::Binding.into());
        }
        if cx.origin != prior.custody.stage().0.origin {
            return Err(Error::Source);
        }
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let current = cx
            .leave_hold(&tx, prior.selector(), prior.terminal.generation())?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) || wall < prior.not_before() {
            return Err(CursorError::Stale.into());
        }
        let (state, attempts, at, floor) = match current.recovery() {
            Recovery::Idle => (1, 1, wall, after(wall, 1)?),
            Recovery::Reserved { attempts: 1, .. } => (1, 2, wall, after(wall, 2)?),
            Recovery::Reserved { attempts: 2, .. } => (1, 3, wall, current.not_before()),
            Recovery::Reserved { attempts: 3, at } => (2, 3, at, current.not_before()),
            _ => return Err(CursorError::Stale.into()),
        };
        advance_local_wall(&tx, cx.origin, wall)?;
        Writer::Update.guard(&tx, false)?;
        rows::require_write(tx.execute(
            "UPDATE peer_terminal_native_leave
             SET recovery_state=?3,recovery_attempts=?4,recovery_reserved_at=?5,
                 recovery_not_before=?6
             WHERE class=?1 AND generation=?2",
            params![
                current.custody.stage().0.generation.known,
                seconds(current.terminal.generation())?,
                state,
                attempts,
                seconds(at)?,
                seconds(floor)?,
            ],
        )?)?;
        Writer::Update.guard(&tx, true)?;
        cx.verify_local_terminal(&tx, wall)?;
        let saved = cx
            .leave_hold(&tx, prior.selector(), prior.terminal.generation())?
            .ok_or(CursorError::Binding)?;
        tx.commit()?;
        Ok(saved)
    }
}

fn after(wall: u64, delta: u64) -> Result<u64> {
    let next = wall.checked_add(delta).ok_or(CursorError::Range)?;
    seconds(next)?;
    Ok(next)
}
