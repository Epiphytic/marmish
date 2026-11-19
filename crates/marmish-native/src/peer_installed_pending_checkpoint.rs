//! Exact opt-in Manual v2 native Pending transitions; no relay publication.

use super::*;

#[cfg(marmish_peer_receiver_candidate)]
#[path = "peer_installed_approval_checkpoint.rs"]
mod approval;
use crate::{
    mdk_adapter::peer_native_signer::WelcomeFence,
    peer_invitation::{
        receiver_inventory::v2_pending::PreparedWelcome,
        socket::owner::recipient::PeerRecipientPort,
    },
    peer_quota_vfs::dispatch::ReceiverQuota,
};
#[cfg(marmish_peer_receiver_candidate)]
pub(in crate::peer_invitation) use approval::{ApprovalFailure, ApprovalGuard, ApprovalScope};

#[cfg(marmish_peer_receiver_candidate)]
use super::manual_work::require_sole_pending;

#[cfg(test)]
use crate::peer_invitation::receiver_inventory::v2_pending::record_custody_changes;

fn require_current(fence: &WelcomeFence<'_>) -> Result<()> {
    fence.sample().map(drop).map_err(|_| Error::Source)
}

#[cfg(test)]
impl HeldEvidence {
    pub(in crate::peer_invitation) fn same_saved_pending(&self, other: &Self) -> bool {
        self.state() == CustodyState::Pending
            && !self.is_active()
            && !other.is_active()
            && self.stage == other.stage
            && self.row == other.row
            && self.saved == other.saved
    }
}

impl JoinStarted {
    pub(in crate::peer_invitation) fn matches_installed_turn(&self, held: &HeldEvidence) -> bool {
        std::sync::Arc::ptr_eq(&self.owner, &held.owner)
            && self.stage == held.stage
            && self.carrier == held.delivery_carrier()
            && held.state() == CustodyState::JoinStarted
    }
}

impl Checkpoint {
    pub(in crate::peer_invitation) fn read_installed_join_custody(
        &mut self,
        source: &Source,
        port: &mut PeerRecipientPort,
        selector: Selector,
        generation: u64,
    ) -> Result<(HeldEvidence, Delivery, u64)> {
        port.use_current_recipient(|account, origin, wall| {
            let held = self
                .read_installed_custody(source, account, origin, wall, selector, generation)?
                .ok_or(Error::Missing)?;
            let delivery = self.current_installed_delivery(&held, wall)?;
            Ok((held, delivery, wall))
        })
        .map_err(|_| Error::Source)?
    }

    pub(in crate::peer_invitation) fn start_installed_join(
        &mut self,
        source: &Source,
        port: &mut PeerRecipientPort,
        prior: &HeldEvidence,
        prepared: &PreparedWelcome<'_>,
        quota: &ReceiverQuota<'_>,
        fence: &WelcomeFence<'_>,
    ) -> Result<JoinStarted> {
        port.use_current_recipient(|account, origin, wall| {
            self.check_installed_consent_scope(source, account, origin, wall)?;
            let delivery = self.current_installed_delivery(prior, wall)?;
            if !prepared.matches_quota(quota) || !prepared.matches(prior, &delivery) {
                return Err(CursorError::Binding.into());
            }
            let _changed =
                self.transition_custody_guarded(source, prior, wall, Change::Start, || {
                    require_current(fence)
                })?;
            #[cfg(test)]
            record_custody_changes(_changed);
            Ok(JoinStarted {
                owner: self.owner.clone(),
                stage: prior.stage,
                carrier: prior.saved.carrier,
            })
        })
        .map_err(|_| Error::Source)?
    }

    /// A current Pending replay verifies the same native coordinates and
    /// leaves custody untouched. JoinStarted can advance exactly once.
    pub(in crate::peer_invitation) fn commit_installed_pending(
        &mut self,
        source: &Source,
        port: &mut PeerRecipientPort,
        prior: &HeldEvidence,
        joined: &InstalledJoinedReadback<'_>,
        quota: &ReceiverQuota<'_>,
        fence: &WelcomeFence<'_>,
    ) -> Result<()> {
        port.use_current_recipient(|account, origin, wall| {
            self.check_installed_consent_scope(source, account, origin, wall)?;
            let delivery = self.current_installed_delivery(prior, wall)?;
            if !joined.matches_quota(quota) || !joined.matches(prior, &delivery) {
                return Err(CursorError::Binding.into());
            }
            if prior.state() == CustodyState::Pending {
                let selector = Selector::from(prior.offer());
                let current = self
                    .read_installed_custody(
                        source,
                        account,
                        origin,
                        wall,
                        selector,
                        prior.stage().0.coordinates().1,
                    )?
                    .ok_or(Error::Missing)?;
                require_same_pending(&current, prior, joined)?;
                require_current(fence)?;
                return Ok(());
            }
            let _changed = self.transition_custody_guarded(
                source,
                prior,
                wall,
                Change::QualifyInstalled(joined),
                || require_current(fence),
            )?;
            #[cfg(test)]
            record_custody_changes(_changed);
            Ok(())
        })
        .map_err(|_| Error::Source)?
    }

    #[cfg(marmish_peer_receiver_candidate)]
    pub(in crate::peer_invitation) fn observe_installed_pending(
        &mut self,
        source: &Source,
        port: &mut PeerRecipientPort,
        prior: &HeldEvidence,
        joined: &InstalledJoinedReadback<'_>,
        quota: &ReceiverQuota<'_>,
    ) -> Result<(crate::ipc::peer_read::Entry, u64, u64)> {
        use crate::ipc::peer_read::{Entry, Selection, State};

        port.use_current_recipient(|account, origin, wall| {
            let delivery = self.current_installed_delivery(prior, wall)?;
            if !joined.matches_quota(quota) || !joined.matches(prior, &delivery) {
                return Err(CursorError::Binding.into());
            }
            let selector = Selector::from(prior.offer());
            let generation = prior.stage().0.coordinates().1;
            self.read_installed_consent(source, account, origin, wall, |tx, cx| {
                let current = read_pending_custody_row(tx, cx, selector, generation)?
                    .ok_or(Error::Missing)?;
                require_same_pending(&current, prior, joined)?;
                require_sole_pending(tx)?;
                let (origin_floor, global_floor) = tx.query_row(
                    "SELECT origin.wall, version.wall
                     FROM peer_cursor_origin origin
                     JOIN peer_consent_version version ON version.singleton=1
                     WHERE origin.origin=?1",
                    [cx.origin],
                    |row| Ok((read_seconds(row, 0)?, read_seconds(row, 1)?)),
                )?;
                let floor = origin_floor.max(global_floor);
                let held_at = current.held_at();
                let expires_at = current.pending_deadline().ok_or(Error::Missing)?;
                if generation == 0 || wall < floor || wall < held_at || wall >= expires_at {
                    return Err(CursorError::Stale.into());
                }
                Ok((
                    Entry {
                        selection: Selection {
                            server: selector.server,
                            offer_id: selector.id,
                            revision: selector.revision,
                            generation,
                        },
                        state: State::Pending,
                        held_at,
                        expires_at,
                    },
                    wall,
                    floor,
                ))
            })
        })
        .map_err(|_| Error::Source)?
    }

    fn read_installed_custody(
        &mut self,
        source: &Source,
        account: PublicKey,
        origin: &str,
        wall: u64,
        selector: Selector,
        generation: u64,
    ) -> Result<Option<HeldEvidence>> {
        if !matches!(self.custody_mode(selector.server)?, Mode::Manual) {
            return Err(Error::Policy);
        }
        self.read_installed_consent(source, account, origin, wall, |tx, cx| {
            read_pending_custody_row(tx, cx, selector, generation)
        })
    }

    pub(in crate::peer_invitation) fn current_installed_delivery(
        &self,
        held: &HeldEvidence,
        wall: u64,
    ) -> Result<Delivery> {
        let permitted = matches!(
            held.state(),
            CustodyState::Held | CustodyState::JoinStarted | CustodyState::Pending
        );
        self.reopen_installed_delivery(held, wall, permitted)
    }

    fn reopen_installed_delivery(
        &self,
        held: &HeldEvidence,
        wall: u64,
        permitted: bool,
    ) -> Result<Delivery> {
        self.check_local_owner(&held.owner)?;
        if held.is_active()
            || held.saved.automatic
            || held.stage().0.coordinates().0
            || held.offer().version() != ControlVersion::V2
            || !matches!(
                self.custody_mode(held.offer().flight().author)?,
                Mode::Manual
            )
            || !permitted
        {
            return Err(Error::Policy);
        }
        if wall
            >= held
                .pending_deadline()
                .unwrap_or_else(|| held.held_deadline())
        {
            return Err(Error::Expired);
        }
        self.codec
            .reopen_custodied_delivery(&self.signer, held, wall)
            .map_err(|_| Error::Package)
    }
}

fn require_same_pending(
    current: &HeldEvidence,
    prior: &HeldEvidence,
    joined: &InstalledJoinedReadback<'_>,
) -> Result<()> {
    let Progress::Pending { joined: saved, .. } = &current.saved.progress else {
        return Err(CursorError::Stale.into());
    };
    if !current.same_turn(prior)
        || (
            saved.native.group.as_slice(),
            saved.native.epoch,
            saved.native.epoch_identity,
            saved.native.own_leaf,
        ) != joined.coordinates()
    {
        return Err(CursorError::Stale.into());
    }
    Ok(())
}

fn read_pending_custody_row(
    tx: &Transaction<'_>,
    cx: &Context,
    selector: Selector,
    generation: u64,
) -> Result<Option<HeldEvidence>> {
    if terminalized(tx, selector, cx.origin)? {
        return Ok(None);
    }
    let Some(queued) = cx.exact_consent(tx, selector)? else {
        return Ok(None);
    };
    if queued.prepared.binding.generation.count != generation {
        return Ok(None);
    }
    cx.read_evidence(tx, queued)
}

// This module is mounted for Linux AMD64 native tests. Change
// one field in its disposable profile while preserving the exact schema.
#[cfg(test)]
impl Checkpoint {
    pub(in crate::peer_invitation) fn fixture_change_custody_frame(
        &mut self,
        carrier: [u8; 32],
    ) -> Result<()> {
        self.fixture_change_custody(
            carrier,
            "UPDATE peer_pending_custody SET frame=zeroblob(length(frame))
             WHERE delivery_carrier=?1 AND state=0",
        )
    }

    pub(in crate::peer_invitation) fn fixture_change_pending_epoch(
        &mut self,
        carrier: [u8; 32],
    ) -> Result<()> {
        self.fixture_change_custody(
            carrier,
            "UPDATE peer_pending_custody SET epoch_identity=zeroblob(32)
             WHERE delivery_carrier=?1 AND state=2 AND epoch_identity!=zeroblob(32)",
        )
    }

    fn fixture_change_custody(&mut self, carrier: [u8; 32], statement: &str) -> Result<()> {
        use super::super::super::retirement::schema::inventory;
        use rusqlite::TransactionBehavior;

        if !self.is_installed_custody() {
            return Err(Error::Layout);
        }
        let tx = self
            .cursor
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = inventory(&tx).map_err(super::schema::cursor_error)?;
        let trigger_sql = |name| -> rusqlite::Result<String> {
            tx.query_row(
                "SELECT sql FROM main.sqlite_schema WHERE type='trigger' AND name=?1",
                [name],
                |row| row.get(0),
            )
        };
        let next = trigger_sql("peer_custody_next")?;
        let deny = trigger_sql("peer_custody_deny_UPDATE")?;
        tx.execute_batch("DROP TRIGGER peer_custody_next; DROP TRIGGER peer_custody_deny_UPDATE;")?;
        if tx.execute(statement, [carrier])? != 1 {
            return Err(Error::Missing);
        }
        tx.execute_batch(&next)?;
        tx.execute_batch(&deny)?;
        if inventory(&tx).map_err(super::schema::cursor_error)? != before {
            return Err(CursorError::Binding.into());
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(all(test, marmish_peer_receiver_candidate))]
#[test]
fn pending_observation_requires_one_pending_slot() {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    let tx = conn.transaction().unwrap();
    tx.execute_batch(
        "CREATE TABLE peer_pending_custody (
          class INTEGER PRIMARY KEY CHECK (class IN (0,1)),
          state INTEGER NOT NULL
        ) STRICT;
        INSERT INTO peer_pending_custody VALUES (0,0);",
    )
    .unwrap();
    assert!(require_sole_pending(&tx).is_err());
    tx.execute("UPDATE peer_pending_custody SET state=2 WHERE class=0", [])
        .unwrap();
    require_sole_pending(&tx).unwrap();
    tx.execute("INSERT INTO peer_pending_custody VALUES (1,2)", [])
        .unwrap();
    assert!(require_sole_pending(&tx).is_err());
}
