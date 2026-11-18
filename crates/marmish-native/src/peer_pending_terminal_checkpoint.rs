//! Local-only terminal decisions and guarded durable progress.

use nostr::key::PublicKey;

use super::super::super::{cooldown, retirement::check_origin_wall, stage_journal, stage_schema};
use super::terminal_schema::Writer;
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalReason {
    Declined = 0,
    Expired = 1,
}

#[derive(Clone, Copy)]
enum Decision {
    Local(TerminalReason),
    #[cfg(test)]
    Held(TerminalReason),
}

impl Decision {
    fn reason(self) -> TerminalReason {
        match self {
            Self::Local(reason) => reason,
            #[cfg(test)]
            Self::Held(reason) => reason,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TerminalStep {
    Fenced,
    Intent(Vec<u8>),
    FilesRemoved(Vec<u8>),
    Released,
    Blocked,
}

pub(crate) struct TerminalRecord {
    owner: Arc<()>,
    binding: Binding,
    reason: TerminalReason,
    decided_at: u64,
    step: TerminalStep,
    stage: Option<State>,
    delivery_carrier: [u8; 32],
    replay_until: u64,
    offer_replay_until: u64,
}

impl TerminalRecord {
    pub(crate) fn selector(&self) -> Selector {
        self.binding.key
    }

    pub(crate) fn generation(&self) -> u64 {
        self.binding.generation.count
    }

    pub(crate) fn reason(&self) -> TerminalReason {
        self.reason
    }

    pub(crate) fn decided_at(&self) -> u64 {
        self.decided_at
    }

    pub(crate) fn step(&self) -> &TerminalStep {
        &self.step
    }

    pub(crate) fn stage(&self) -> Option<(Binding, State)> {
        self.stage.map(|stage| (self.binding, stage))
    }

    pub(super) fn same_turn(&self, prior: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &prior.owner)
            && self.binding == prior.binding
            && self.reason == prior.reason
            && self.decided_at == prior.decided_at
            && self.step == prior.step
            && self.stage == prior.stage
            && self.delivery_carrier == prior.delivery_carrier
            && self.replay_until == prior.replay_until
            && self.offer_replay_until == prior.offer_replay_until
    }
}

impl Context {
    pub(super) fn verify_local_terminal(&self, tx: &Transaction<'_>, wall: u64) -> Result<()> {
        if self.epoch.is_installed_v2() {
            return Err(Error::Layout);
        }
        self.epoch.verify(tx, self.account, &self.origins)?;
        rows::check_wall(tx, wall)?;
        check_origin_wall(tx, self.origin, wall)
    }

    pub(super) fn terminal_record(
        &self,
        tx: &Transaction<'_>,
        selector: Selector,
        generation: u64,
    ) -> Result<Option<TerminalRecord>> {
        let row = tx
            .query_row(
                "SELECT class,reason,decided_at,step,witness,delivery_carrier,replay_until,
                        offer_replay_until,delivery_digest,device,inode
                 FROM peer_pending_terminal
                 WHERE origin=?1 AND account=?2 AND server=?3 AND offer_id=?4
                    AND offer_revision=?5 AND generation=?6",
                params![
                    self.origin,
                    self.account,
                    selector.server.to_bytes(),
                    selector.id,
                    selector.revision,
                    seconds(generation)?
                ],
                |r| {
                    let reason = match r.get::<_, u8>(1)? {
                        0 => TerminalReason::Declined,
                        1 => TerminalReason::Expired,
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    let step = match r.get::<_, u8>(3)? {
                        0 => TerminalStep::Fenced,
                        1 => TerminalStep::Intent(r.get(4)?),
                        2 => TerminalStep::FilesRemoved(r.get(4)?),
                        3 => TerminalStep::Released,
                        4 => TerminalStep::Blocked,
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    let row = TerminalRecord {
                        owner: self.owner.clone(),
                        binding: Binding {
                            key: selector,
                            origin: self.origin,
                            generation: Generation {
                                known: r.get(0)?,
                                count: generation,
                            },
                        },
                        reason,
                        decided_at: read_seconds(r, 2)?,
                        step,
                        stage: None,
                        delivery_carrier: r.get(5)?,
                        replay_until: read_seconds(r, 6)?,
                        offer_replay_until: read_seconds(r, 7)?,
                    };
                    let file = FileWitness {
                        device: r.get(9)?,
                        inode: r.get(10)?,
                    };
                    Ok((row, r.get::<_, [u8; 32]>(8)?, file))
                },
            )
            .optional()?;
        let Some((mut row, digest, file)) = row else {
            return Ok(None);
        };
        if row.step != TerminalStep::Released {
            let stage = stage_journal::load(tx, row.binding)?.ok_or(CursorError::Binding)?;
            if !matches!(stage, State::Prepared(saved, _) if saved == file) {
                return Err(CursorError::Binding.into());
            }
            let frame = Zeroizing::new(tx.query_row(
                "SELECT frame FROM peer_pending_custody WHERE delivery_carrier=?1",
                [row.delivery_carrier],
                |r| r.get::<_, Vec<u8>>(0),
            )?);
            if <[u8; 32]>::from(Sha256::digest(frame.as_slice())) != digest {
                return Err(CursorError::Binding.into());
            }
            row.stage = Some(stage);
        }
        Ok(Some(row))
    }
}

impl Checkpoint {
    pub(crate) fn terminal_lookup(
        &mut self,
        source: &Source,
        selector: Selector,
        generation: u64,
        wall: u64,
    ) -> Result<Option<TerminalRecord>> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        cx.terminal_record(&tx, selector, generation)
    }

    pub(crate) fn terminal_due(
        &mut self,
        source: &Source,
        founder: PublicKey,
        wall: u64,
    ) -> Result<Option<(Selector, u64)>> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        tx.query_row(
            "SELECT c.server,c.offer_id,c.offer_revision,c.generation
             FROM peer_pending_custody c
             WHERE c.origin=?1 AND c.server=?2 AND c.state=2 AND c.pending_deadline<=?3
               AND NOT EXISTS (SELECT 1 FROM peer_pending_terminal t
                   WHERE t.class=c.class AND t.generation=c.generation)",
            params![cx.origin, founder.to_bytes(), seconds(wall)?],
            selection,
        )
        .optional()
        .map_err(Into::into)
    }

    pub(crate) fn terminal_cleanup_due(
        &mut self,
        source: &Source,
        wall: u64,
    ) -> Result<Option<TerminalRecord>> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let query = "SELECT server,offer_id,offer_revision,generation FROM peer_pending_terminal
                     WHERE origin=?1 AND account=?2 AND step IN (0,1,2)
                     ORDER BY decided_at,class LIMIT 1";
        #[cfg(test)]
        let query = if cx.epoch.is_terminal_leave() {
            "SELECT t.server,t.offer_id,t.offer_revision,t.generation FROM peer_pending_terminal t
             WHERE t.origin=?1 AND t.account=?2 AND t.step IN (0,1,2)
               AND NOT EXISTS (SELECT 1 FROM peer_terminal_native_leave h
                               WHERE h.class=t.class AND h.generation=t.generation)
             ORDER BY t.decided_at,t.class LIMIT 1"
        } else {
            query
        };
        let due = tx
            .query_row(query, params![cx.origin, cx.account], selection)
            .optional()?;
        let Some((selector, generation)) = due else {
            return Ok(None);
        };
        cx.terminal_record(&tx, selector, generation)?
            .map(Some)
            .ok_or(CursorError::Binding.into())
    }

    pub(crate) fn decline_pending_checked(
        &mut self,
        source: &Source,
        selector: Selector,
        generation: u64,
        wall: u64,
        local: &Local,
        authorize: impl FnOnce() -> Result<()>,
    ) -> Result<TerminalRecord> {
        self.check_local(local)?;
        self.terminal_decision(
            source,
            selector,
            generation,
            wall,
            Decision::Local(TerminalReason::Declined),
            authorize,
        )
    }

    pub(crate) fn expire_pending(
        &mut self,
        source: &Source,
        selector: Selector,
        generation: u64,
        wall: u64,
    ) -> Result<TerminalRecord> {
        self.terminal_decision(
            source,
            selector,
            generation,
            wall,
            Decision::Local(TerminalReason::Expired),
            || Ok(()),
        )
    }

    #[cfg(test)]
    pub(crate) fn hold_pending_decline(
        &mut self,
        source: &Source,
        selector: Selector,
        generation: u64,
        wall: u64,
        local: &Local,
        authorize: impl FnOnce() -> Result<()>,
    ) -> Result<TerminalRecord> {
        self.check_local(local)?;
        self.terminal_decision(
            source,
            selector,
            generation,
            wall,
            Decision::Held(TerminalReason::Declined),
            authorize,
        )
    }

    #[cfg(test)]
    pub(super) fn hold_pending_expiry(
        &mut self,
        source: &Source,
        selector: Selector,
        generation: u64,
        wall: u64,
    ) -> Result<TerminalRecord> {
        self.terminal_decision(
            source,
            selector,
            generation,
            wall,
            Decision::Held(TerminalReason::Expired),
            || Ok(()),
        )
    }

    fn terminal_decision(
        &mut self,
        source: &Source,
        selector: Selector,
        generation: u64,
        wall: u64,
        decision: Decision,
        authorize: impl FnOnce() -> Result<()>,
    ) -> Result<TerminalRecord> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let reason = decision.reason();
        #[cfg(test)]
        let hold = matches!(decision, Decision::Held(_));
        #[cfg(test)]
        if hold && !cx.epoch.is_terminal_leave() {
            return Err(CursorError::Binding.into());
        }
        if let Some(existing) = cx.terminal_record(&tx, selector, generation)? {
            #[cfg(test)]
            if hold && cx.leave_hold(&tx, selector, generation)?.is_none() {
                return Err(CursorError::Stale.into());
            }
            authorize()?;
            return Ok(existing);
        }
        let queued = cx.exact_consent(&tx, selector)?.ok_or(CursorError::Stale)?;
        let prior = cx.read_evidence(&tx, queued)?.ok_or(CursorError::Stale)?;
        if prior.stage.0.generation.count != generation {
            return Err(CursorError::Stale.into());
        }
        let Progress::Pending { joined, .. } = &prior.saved.progress else {
            return Err(CursorError::Stale.into());
        };
        if wall < joined.qualified_at {
            return Err(CursorError::ClockRegressed.into());
        }
        match (reason, wall < joined.deadline) {
            (TerminalReason::Declined, false) => return Err(Error::Expired),
            (TerminalReason::Expired, true) => return Err(Error::NotExpired),
            _ => {}
        }
        advance_local_wall(&tx, cx.origin, wall)?;
        prune_terminal_history(&tx, cx.origin, wall)?;
        if reason == TerminalReason::Declined {
            cooldown::decline(&tx, selector.server.to_bytes(), wall)?;
        }
        let insert_parent = || insert_terminal_parent(&tx, &prior, reason, wall, authorize);
        #[cfg(test)]
        if hold {
            terminal_leave::insert_terminal_and_hold(&tx, &prior, wall, insert_parent)?;
        } else {
            insert_parent()?;
        }
        #[cfg(not(test))]
        insert_parent()?;
        cx.verify_local_terminal(&tx, wall)?;
        #[cfg(test)]
        if hold {
            cx.leave_hold(&tx, selector, generation)?
                .ok_or(CursorError::Binding)?;
        }
        let committed = cx
            .terminal_record(&tx, selector, generation)?
            .ok_or(CursorError::Binding)?;
        tx.commit()?;
        Ok(committed)
    }

    pub(crate) fn terminal_check(
        &mut self,
        source: &Source,
        prior: &TerminalRecord,
        wall: u64,
    ) -> Result<()> {
        let current = self
            .terminal_lookup(source, prior.selector(), prior.generation(), wall)?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }

    pub(crate) fn terminal_retirement_check(
        &mut self,
        source: &Source,
        prior: &TerminalRecord,
        wall: u64,
    ) -> Result<()> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let current = cx
            .terminal_record(&tx, prior.selector(), prior.generation())?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) {
            return Err(CursorError::Stale.into());
        }
        #[cfg(test)]
        if cx.epoch.is_terminal_leave() && terminal_leave::exists(&tx, current.binding)? {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }

    pub(crate) fn terminal_intent(
        &mut self,
        source: &Source,
        prior: &TerminalRecord,
        wall: u64,
        witness: &[u8],
    ) -> Result<TerminalRecord> {
        self.terminal_shift(source, prior, wall, Shift::Intent(witness))
    }

    pub(crate) fn terminal_blocked(
        &mut self,
        source: &Source,
        prior: &TerminalRecord,
        wall: u64,
    ) -> Result<TerminalRecord> {
        self.terminal_shift(source, prior, wall, Shift::Blocked)
    }

    pub(crate) fn terminal_files_removed(
        &mut self,
        source: &Source,
        prior: &TerminalRecord,
        wall: u64,
    ) -> Result<TerminalRecord> {
        self.terminal_shift(source, prior, wall, Shift::FilesRemoved)
    }

    pub(crate) fn terminal_release(
        &mut self,
        source: &Source,
        prior: &TerminalRecord,
        wall: u64,
    ) -> Result<TerminalRecord> {
        self.terminal_shift(source, prior, wall, Shift::Release)
    }

    fn terminal_shift(
        &mut self,
        source: &Source,
        prior: &TerminalRecord,
        wall: u64,
        shift: Shift<'_>,
    ) -> Result<TerminalRecord> {
        self.check_local_owner(&prior.owner)?;
        let cx = self.custody_context(source)?;
        if cx.origin != prior.binding.origin {
            return Err(Error::Source);
        }
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let current = cx
            .terminal_record(&tx, prior.selector(), prior.generation())?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) {
            return Err(CursorError::Stale.into());
        }
        #[cfg(test)]
        if cx.epoch.is_terminal_leave()
            && matches!(shift, Shift::Intent(_) | Shift::Blocked)
            && terminal_leave::exists(&tx, current.binding)?
        {
            return Err(CursorError::Stale.into());
        }
        advance_local_wall(&tx, cx.origin, wall)?;
        shift.apply(&tx, &current, wall)?;
        cx.verify_local_terminal(&tx, wall)?;
        let committed = cx
            .terminal_record(&tx, prior.selector(), prior.generation())?
            .ok_or(CursorError::Binding)?;
        tx.commit()?;
        Ok(committed)
    }
}

pub(super) fn selection(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Selector, u64)> {
    let key = row.get::<_, [u8; 32]>(0)?;
    Ok((
        Selector {
            server: PublicKey::from_slice(&key).map_err(|_| rusqlite::Error::InvalidQuery)?,
            id: row.get(1)?,
            revision: row.get(2)?,
        },
        read_seconds(row, 3)?,
    ))
}

pub(super) fn prune_terminal_history(
    tx: &Transaction<'_>,
    origin: Origin,
    wall: u64,
) -> Result<()> {
    Writer::Prune.guard(tx, false)?;
    tx.execute(
        "DELETE FROM peer_pending_terminal WHERE origin=?1 AND step=3 AND retain_until<=?2",
        params![origin, seconds(wall)?],
    )?;
    Writer::Prune.guard(tx, true)?;
    let count: i64 = tx.query_row("SELECT count(*) FROM peer_pending_terminal", [], |row| {
        row.get(0)
    })?;
    if count >= cooldown::HISTORY_LIMIT {
        return Err(CursorError::Backlog.into());
    }
    Ok(())
}

pub(super) fn advance_local_wall(tx: &Transaction<'_>, origin: Origin, wall: u64) -> Result<()> {
    rows::advance_wall(tx, wall)?;
    rows::require_write(tx.execute(
        "UPDATE peer_cursor_origin SET wall=?2 WHERE origin=?1 AND wall<=?2",
        params![origin, seconds(wall)?],
    )?)?;
    Ok(())
}

enum Shift<'a> {
    Intent(&'a [u8]),
    FilesRemoved,
    Release,
    Blocked,
}

impl Shift<'_> {
    fn apply(self, tx: &Transaction<'_>, row: &TerminalRecord, wall: u64) -> Result<()> {
        let (step, assignment) = match (self, &row.step) {
            (Self::Intent(witness), TerminalStep::Fenced) => {
                let Some(State::Prepared(file, _)) = row.stage else {
                    return Err(CursorError::Binding.into());
                };
                if witness.len() != 48
                    || witness[32..40] != file.device
                    || witness[40..] != file.inode
                {
                    return Err(CursorError::Binding.into());
                }
                (0, (1, Some(witness), None))
            }
            (Self::FilesRemoved, TerminalStep::Intent(witness)) => {
                (1, (2, Some(witness.as_slice()), None))
            }
            (Self::Release, TerminalStep::FilesRemoved(_)) => {
                release_rows(tx, row)?;
                let until = wall
                    .checked_add(cooldown::DAY)
                    .ok_or(CursorError::Range)?
                    .max(row.replay_until)
                    .max(row.offer_replay_until);
                (2, (3, None, Some(seconds(until)?)))
            }
            (Self::Blocked, TerminalStep::Fenced) => (0, (4, None, None)),
            (Self::Blocked, TerminalStep::Intent(_)) => (1, (4, None, None)),
            _ => return Err(CursorError::Stale.into()),
        };
        let (next, witness, retain_until) = assignment;
        let progress = Writer::Progress;
        progress.guard(tx, false)?;
        rows::require_write(tx.execute(
            "UPDATE peer_pending_terminal SET step=?4,witness=?5,retain_until=?6,
                 outcome=CASE WHEN ?4=1 THEN 0 ELSE outcome END
             WHERE class=?1 AND generation=?2 AND step=?3",
            params![
                row.binding.generation.known,
                seconds(row.generation())?,
                step,
                next,
                witness,
                retain_until
            ],
        )?)?;
        progress.guard(tx, true)
    }
}

fn release_rows(tx: &Transaction<'_>, row: &TerminalRecord) -> Result<()> {
    let binding = row.binding;
    let class = binding.generation.known;
    let generation = seconds(binding.generation.count)?;
    let custody = custody_schema::Writer::Delete;
    let consent = schema::Writer::Delete;
    custody.guard(tx, false)?;
    consent.guard(tx, false)?;
    tx.execute_batch("DROP TRIGGER peer_stage_delete_guard")?;
    // The hardened connection keeps foreign keys enabled. In particular, the
    // slot restricts invitation deletion until the exact claim is cleared.
    let carrier: [u8; 32] = tx.query_row(
        "SELECT consent_carrier FROM peer_pending_custody
         WHERE class=?1 AND generation=?2 AND delivery_carrier=?3 AND state=2",
        params![class, generation, row.delivery_carrier],
        |r| r.get(0),
    )?;
    rows::require_write(tx.execute(
        "DELETE FROM peer_pending_custody WHERE class=?1 AND generation=?2 AND delivery_carrier=?3",
        params![class, generation, row.delivery_carrier],
    )?)?;
    rows::require_write(tx.execute(
        "DELETE FROM peer_prepared_consent_outbox WHERE class=?1 AND generation=?2 AND carrier=?3",
        params![class, generation, carrier],
    )?)?;
    rows::require_write(tx.execute(
        "DELETE FROM peer_provisional_stage_v1 WHERE class=?1 AND generation=?2",
        params![class, generation],
    )?)?;
    rows::require_write(tx.execute(
        "UPDATE peer_provisioning_slot SET server=NULL,offer_id=NULL,revision=NULL,origin=NULL
         WHERE class=?1 AND generation=?2 AND server=?3 AND offer_id=?4 AND revision=?5 AND origin=?6",
        params![
            class,
            generation,
            binding.key.server.to_bytes(),
            binding.key.id,
            binding.key.revision,
            binding.origin
        ],
    )?)?;
    Invitation::load(tx, binding.key, binding.origin)?.remove(tx)?;
    tx.execute_batch(stage_schema::DELETE_STAGE)?;
    consent.guard(tx, true)?;
    custody.guard(tx, true)
}

pub(super) fn insert_terminal_parent(
    tx: &Transaction<'_>,
    prior: &HeldEvidence,
    reason: TerminalReason,
    wall: u64,
    authorize: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let insert = Writer::Insert;
    insert.guard(tx, false)?;
    let digest = <[u8; 32]>::from(Sha256::digest(prior.signed_delivery()));
    authorize()?;
    rows::require_write(tx.execute(
        "INSERT INTO peer_pending_terminal (
                class,generation,account,server,origin,offer_id,offer_revision,
                consent_carrier,delivery_carrier,delivery_digest,outer_stamp,welcome_digest,
                device,inode,group_id,epoch_number,epoch_identity,own_leaf,qualified_at,
                pending_deadline,replay_until,offer_replay_until,reason,decided_at,step
             )
             SELECT c.class,c.generation,c.account,c.server,c.origin,c.offer_id,c.offer_revision,
                c.consent_carrier,c.delivery_carrier,?3,c.outer_stamp,c.welcome_digest,
                ?4,?5,c.group_id,c.epoch_number,c.epoch_identity,c.own_leaf,c.qualified_at,
                c.pending_deadline,c.replay_until,i.replay_until,?6,?7,0
             FROM peer_pending_custody c JOIN peer_consent_invite i
                ON i.server=c.server AND i.offer_id=c.offer_id
                    AND i.revision=c.offer_revision AND i.origin=c.origin
             WHERE c.delivery_carrier=?1 AND c.generation=?2 AND c.state=2",
        params![
            prior.delivery_carrier(),
            seconds(prior.stage().0.generation.count)?,
            digest,
            prior.stage.1.device,
            prior.stage.1.inode,
            reason as u8,
            seconds(wall)?
        ],
    )?)?;
    insert.guard(tx, true)?;
    Ok(())
}
