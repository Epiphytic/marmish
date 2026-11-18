//! Local retirement ownership and receipts for the test-only native stage.

use super::super::super::{CursorStore, HardenedPeerConnection, after};
use super::stage_journal::{self, State};
use super::*;

#[path = "peer_stage_retirement_schema.rs"]
pub(super) mod schema;

use nostr::key::Keys;
use rusqlite::Connection;
use schema::Writers;
pub(crate) use schema::check_epoch as verify_epoch;

const DAY: u64 = 86_400;
const HISTORY_LIMIT: i64 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Expire = 0,
    Decline = 1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Retiring(Vec<u8>),
    FilesRemoved(Vec<u8>),
    Blocked,
    Released(u64),
}

pub(crate) struct Examination {
    pub(crate) binding: Binding,
    pub(crate) stage: State,
    pub(crate) step: Option<Step>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Released,
    AlreadyReleased,
    Blocked,
}

fn checkout(conn: &Connection, binding: Binding) -> Result<Option<Step>> {
    conn.query_row(
        "SELECT server, offer_id, revision, origin, step, witness, receipt_until
         FROM peer_retirement_intent WHERE class=?1 AND generation=?2",
        params![binding.generation.known, seconds(binding.generation.count)?],
        |r| {
            if r.get::<_, [u8; 32]>(0)? != binding.key.server.to_bytes()
                || r.get::<_, [u8; 16]>(1)? != binding.key.id
                || r.get::<_, [u8; 32]>(2)? != binding.key.revision
                || r.get::<_, Origin>(3)? != binding.origin
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
            match r.get::<_, u8>(4)? {
                0 => Ok(Step::Retiring(r.get(5)?)),
                1 => Ok(Step::FilesRemoved(r.get(5)?)),
                2 => Ok(Step::Blocked),
                3 => Ok(Step::Released(read_seconds(r, 6)?)),
                _ => Err(rusqlite::Error::InvalidQuery),
            }
        },
    )
    .optional()
    .map_err(Into::into)
}

pub(super) fn retiring(conn: &Connection, binding: Binding) -> Result<bool> {
    Ok(matches!(
        checkout(conn, binding)?,
        Some(Step::Retiring(_) | Step::FilesRemoved(_) | Step::Blocked)
    ))
}

pub(crate) fn under_cooldown(
    conn: &Connection,
    server: [u8; 32],
    wall: u64,
) -> super::super::super::Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_retirement_cooldown WHERE server=?1 AND until>?2)",
        params![server, seconds(wall)?],
        |r| r.get(0),
    )?)
}

fn guard_account(tx: &Transaction<'_>, binding: Binding, wall: u64) -> Result<Invitation> {
    if generation(tx, binding.key, binding.origin)? != Some(binding.generation)
        || has_outbox(tx, binding.key, binding.origin)?
    {
        return Err(CursorError::Stale.into());
    }
    let row = Invitation::load(tx, binding.key, binding.origin)?;
    if tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_consent_outbox WHERE server=?1)",
        [binding.key.server.to_bytes()],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(CursorError::Conflict.into());
    }
    let fence_deadline = tx
        .query_row(
            "SELECT replay_until FROM peer_consent_fence WHERE origin=?1 AND revision=?2",
            params![binding.origin, binding.key.revision],
            |r| read_seconds(r, 0),
        )
        .optional()?;
    if wall < row.replay_until && fence_deadline != Some(row.replay_until) {
        return Err(CursorError::Binding.into());
    }
    Ok(row)
}

#[derive(Clone, Copy)]
enum Change<'a> {
    Begin {
        action: Action,
        witness: Option<&'a [u8]>,
    },
    FilesRemoved,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    Intent,
    FilesRemoved,
    Release,
}

impl Change<'_> {
    fn fault(self) -> Fault {
        match self {
            Self::Begin { .. } => Fault::Intent,
            Self::FilesRemoved => Fault::FilesRemoved,
            Self::Release => Fault::Release,
        }
    }
}

impl Checkpoint {
    pub(crate) fn retirement_database_for_fixture(&self) -> &Connection {
        &self.cursor.conn
    }

    pub(crate) fn inject_retirement_fault(&mut self, fault: Fault) {
        self.retirement_fault = Some(fault);
    }

    pub(crate) fn probe_retirement_guard(
        &mut self,
        HardenedPeerConnection(second): HardenedPeerConnection,
    ) -> Result<()> {
        use rusqlite::ErrorCode;

        self.require_retirement_epoch()?;
        second.busy_timeout(std::time::Duration::ZERO)?;
        let tx = self
            .cursor
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        schema::check_epoch(&tx)?;
        schema::guard(&tx, Writers::Intent, false)?;
        let count = second.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='peer_retirement_intent_DELETE'",
            [],
            |r| r.get::<_, u8>(0),
        )?;
        let blocked = second.execute("DELETE FROM peer_retirement_intent", []);
        if count != 1
            || !matches!(blocked, Err(ref error) if error.sqlite_error_code() == Some(ErrorCode::DatabaseBusy))
        {
            return Err(CursorError::Storage.into());
        }
        // Intentionally omit reinstall and require rollback to restore the guard.
        tx.rollback()?;
        schema::check_epoch(&self.cursor.conn)
    }

    pub(crate) fn retirement_cursor(
        conn: HardenedPeerConnection,
        account: [u8; 32],
        origins: Vec<Origin>,
    ) -> Result<CursorStore> {
        schema::open(conn, account, origins)
    }

    pub(crate) fn attach_retirement_fixture(
        cursor: CursorStore,
        signer: Keys,
        owner_uid: u32,
        urls: &[&str],
        policy: super::super::Policy,
    ) -> Result<Self> {
        let mut checkpoint =
            Self::attach_with(cursor, signer, owner_uid, urls, policy, |store, _| {
                schema::check_epoch(&store.conn).map_err(|_| CursorError::Binding)
            })?;
        checkpoint.provisional_schema = true;
        checkpoint.retirement_epoch = true;
        Ok(checkpoint)
    }

    pub(crate) fn released_target(
        &self,
        local: &Local,
        key: Selector,
        known: bool,
        generation: u64,
    ) -> Result<RetirementTarget> {
        self.check_local(local)?;
        self.require_retirement_epoch()?;
        schema::check_epoch(&self.cursor.conn)?;
        let origin = self
            .cursor
            .conn
            .query_row(
                "SELECT origin FROM peer_retirement_intent WHERE class=?1 AND generation=?2
                AND server=?3 AND offer_id=?4 AND revision=?5 AND step=3",
                params![
                    known,
                    seconds(generation)?,
                    key.server.to_bytes(),
                    key.id,
                    key.revision
                ],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::Missing)?;
        self.route(origin)?;
        Ok(RetirementTarget {
            owner: self.owner.clone(),
            binding: Binding {
                key,
                origin,
                generation: Generation {
                    known,
                    count: generation,
                },
            },
        })
    }

    fn require_retirement_epoch(&self) -> Result<()> {
        if !self.provisional_schema || !self.retirement_epoch {
            return Err(CursorError::Binding.into());
        }
        Ok(())
    }

    pub(crate) fn examine_retirement(
        &mut self,
        local: &Local,
        target: &RetirementTarget,
        request: Action,
        wall: u64,
    ) -> Result<std::result::Result<Examination, Outcome>> {
        self.check_local(local)?;
        self.check_local_owner(&target.owner)?;
        self.require_retirement_epoch()?;
        let binding = target.binding;
        let route = self
            .routes
            .iter()
            .find(|r| r.id == binding.origin)
            .ok_or(Error::Source)?;
        let tx = self.cursor.begin(binding.origin)?;
        schema::check_epoch(&tx)?;
        rows::check_wall(&tx, wall)?;
        self::check_origin_wall(&tx, binding.origin, wall)?;
        let step = checkout(&tx, binding)?;
        if let Some(Step::Released(until)) = step {
            return if wall < until {
                Ok(Err(Outcome::AlreadyReleased))
            } else {
                Err(Error::Missing)
            };
        }
        let row = guard_account(&tx, binding, wall)?;
        Self::check_signed_offer(
            &self.codec,
            &self.signer,
            route,
            &row,
            row.expires.checked_sub(1).ok_or(CursorError::Range)?,
        )?;
        if step.is_none() && request == Action::Expire && wall < row.expires {
            return Err(Error::NotExpired);
        }
        let stage = stage_journal::load(&tx, binding)?.ok_or(Error::Missing)?;
        Ok(Ok(Examination {
            binding,
            stage,
            step,
        }))
    }

    pub(crate) fn begin_retirement(
        &mut self,
        local: &Local,
        target: &RetirementTarget,
        stage: State,
        request: Action,
        witness: Option<&[u8]>,
        wall: u64,
    ) -> Result<()> {
        self.change_retirement(
            local,
            target,
            stage,
            wall,
            Change::Begin {
                action: request,
                witness,
            },
            || Ok(()),
        )
    }

    pub(crate) fn files_retired(
        &mut self,
        local: &Local,
        target: &RetirementTarget,
        stage: State,
        wall: u64,
        verify_absent: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.change_retirement(
            local,
            target,
            stage,
            wall,
            Change::FilesRemoved,
            verify_absent,
        )
    }

    pub(crate) fn release_retirement(
        &mut self,
        local: &Local,
        target: &RetirementTarget,
        stage: State,
        wall: u64,
        verify_absent: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.change_retirement(local, target, stage, wall, Change::Release, verify_absent)
    }

    fn change_retirement(
        &mut self,
        local: &Local,
        target: &RetirementTarget,
        stage: State,
        wall: u64,
        change: Change<'_>,
        verify_absent: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.check_local(local)?;
        self.check_local_owner(&target.owner)?;
        self.require_retirement_epoch()?;
        let fail_before_commit = self.retirement_fault == Some(change.fault());
        if fail_before_commit {
            self.retirement_fault = None;
        }
        let binding = target.binding;
        let route = self
            .routes
            .iter()
            .find(|r| r.id == binding.origin)
            .ok_or(Error::Source)?;
        let tx = self.cursor.begin(binding.origin)?;
        schema::check_epoch(&tx)?;
        rows::check_wall(&tx, wall)?;
        check_origin_wall(&tx, binding.origin, wall)?;
        let row = guard_account(&tx, binding, wall)?;
        Self::check_signed_offer(
            &self.codec,
            &self.signer,
            route,
            &row,
            row.expires.checked_sub(1).ok_or(CursorError::Range)?,
        )?;
        if stage_journal::load(&tx, binding)? != Some(stage) {
            return Err(CursorError::Stale.into());
        }
        match (change, checkout(&tx, binding)?) {
            (Change::Begin { action, witness }, None) => {
                if action == Action::Expire && wall < row.expires {
                    return Err(Error::NotExpired);
                }
                start_intent(&tx, binding, action, witness, wall)?;
            }
            (Change::FilesRemoved, Some(Step::Retiring(_))) => {
                verify_absent()?;
                shift_removed(&tx, binding)?;
            }
            (Change::Release, Some(Step::FilesRemoved(_))) => {
                verify_absent()?;
                release(&tx, binding, row, wall)?;
            }
            _ => return Err(CursorError::Stale.into()),
        }
        rows::advance_wall(&tx, wall)?;
        rows::require_write(tx.execute(
            "UPDATE peer_cursor_origin SET wall=?2 WHERE origin=?1",
            params![binding.origin, seconds(wall)?],
        )?)?;
        schema::check_epoch(&tx)?;
        if fail_before_commit {
            return Err(CursorError::Storage.into());
        }
        tx.commit()?;
        Ok(())
    }
}

fn check_origin_wall(tx: &Transaction<'_>, origin: Origin, wall: u64) -> Result<()> {
    let saved = tx.query_row(
        "SELECT wall FROM peer_cursor_origin WHERE origin=?1",
        [origin],
        |r| read_seconds(r, 0),
    )?;
    if wall < saved {
        return Err(CursorError::ClockRegressed.into());
    }
    Ok(())
}

fn start_intent(
    tx: &Transaction<'_>,
    binding: Binding,
    action: Action,
    witness: Option<&[u8]>,
    wall: u64,
) -> Result<()> {
    schema::guard(tx, Writers::Intent, false)?;
    rows::require_write(tx.execute(
        "INSERT INTO peer_retirement_intent
            (class,generation,server,offer_id,revision,origin,action,step,started,witness)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            binding.generation.known,
            seconds(binding.generation.count)?,
            binding.key.server.to_bytes(),
            binding.key.id,
            binding.key.revision,
            binding.origin,
            action as u8,
            if witness.is_some() { 0 } else { 2 },
            seconds(wall)?,
            witness
        ],
    )?)?;
    schema::guard(tx, Writers::Intent, true)?;
    if action == Action::Decline {
        schema::guard(tx, Writers::Cooldown, false)?;
        tx.execute(
            "DELETE FROM peer_retirement_cooldown WHERE until<=?1",
            [seconds(wall)?],
        )?;
        capacity(tx, "SELECT count(*) FROM peer_retirement_cooldown")?;
        rows::require_write(tx.execute(
            "INSERT INTO peer_retirement_cooldown VALUES (?1,?2)",
            params![binding.key.server.to_bytes(), after(wall, DAY)?],
        )?)?;
        schema::guard(tx, Writers::Cooldown, true)?;
    }
    Ok(())
}

fn shift_removed(tx: &Transaction<'_>, binding: Binding) -> Result<()> {
    schema::guard(tx, Writers::Intent, false)?;
    rows::require_write(tx.execute(
        "UPDATE peer_retirement_intent SET step=1 WHERE class=?1 AND generation=?2 AND step=0",
        params![binding.generation.known, seconds(binding.generation.count)?],
    )?)?;
    schema::guard(tx, Writers::Intent, true)
}

fn capacity(tx: &Transaction<'_>, query: &str) -> Result<()> {
    let count: i64 = tx.query_row(query, [], |r| r.get(0))?;
    if count >= HISTORY_LIMIT {
        return Err(CursorError::Backlog.into());
    }
    Ok(())
}

fn release(tx: &Transaction<'_>, binding: Binding, row: Invitation, wall: u64) -> Result<()> {
    let Generation { known, count } = binding.generation;
    let generation = seconds(count)?;
    schema::guard(tx, Writers::Release, false)?;
    tx.execute(
        "DELETE FROM peer_retirement_intent WHERE step=3 AND receipt_until<=?1",
        [seconds(wall)?],
    )?;
    capacity(
        tx,
        "SELECT count(*) FROM peer_retirement_intent WHERE step=3",
    )?;
    rows::require_write(tx.execute(
        "DELETE FROM peer_provisional_stage_v1 WHERE class=?1 AND generation=?2",
        params![known, generation],
    )?)?;
    rows::require_write(tx.execute(
        "UPDATE peer_provisioning_slot SET server=NULL,offer_id=NULL,revision=NULL,origin=NULL
            WHERE class=?1 AND generation=?2 AND server=?3 AND offer_id=?4 AND revision=?5 AND origin=?6",
        params![known, generation, binding.key.server.to_bytes(), binding.key.id,
            binding.key.revision, binding.origin],
    )?)?;
    row.remove(tx)?;
    rows::require_write(tx.execute(
        "UPDATE peer_retirement_intent SET step=3,witness=NULL,receipt_until=?3
         WHERE class=?1 AND generation=?2 AND step=1",
        params![known, generation, after(wall, DAY)?],
    )?)?;
    schema::guard(tx, Writers::Release, true)
}
