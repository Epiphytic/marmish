//! Local inventory and decision turns. No raw socket or new joins here.

use super::*;
use crate::peer_invitation::{
    prepared_consent::JoinedReadback,
    store::consent_checkpoint::provisioning::prepared_consent::custody::{
        CustodyState, HeldEvidence,
    },
};

type LocalResult<T> = std::result::Result<T, IpcFailure>;

impl Service<'_> {
    pub(super) fn handle(&mut self, lease: &ProfileLease, worker: &Worker, mut work: Work) {
        let reply = match *work.request() {
            Request::List {} => self.inventory(&work),
            Request::Show { selection } => self.show(&work, selection),
            Request::Approve { selection } => self.approve(worker, &mut work, lease, selection),
        };
        work.reply(reply);
    }

    fn inventory(&mut self, work: &Work) -> LocalResult<Reply> {
        work.can_reply()?;
        let wall = self
            .owner
            .available_sample()
            .map_err(local_failure)?
            .as_secs();
        let rows = self
            .owner
            .state
            .checkpoint
            .daemon_inventory(&self.owner.state.source, wall)
            .map_err(|error| local_failure(error.into()))?;
        let entries = rows
            .into_iter()
            .filter(|row| row.key.server == self.founder)
            .map(entry)
            .collect();
        work.can_reply()?;
        Ok(Reply::List { entries })
    }

    fn show(&mut self, work: &Work, selection: Selection) -> LocalResult<Reply> {
        work.can_reply()?;
        let (row, _) = self.proof(selection)?;
        let found = row.inventory().ok_or(IpcFailure::Unavailable)?;
        work.can_reply()?;
        Ok(Reply::Show {
            entry: entry(found),
        })
    }

    fn approve(
        &mut self,
        worker: &Worker,
        work: &mut Work,
        lease: &ProfileLease,
        selection: Selection,
    ) -> LocalResult<Reply> {
        let manual = work.begin_manual(worker, lease)?;
        let (row, joined) = self.proof(selection)?;
        let mut found = row.inventory().ok_or(IpcFailure::Unavailable)?;
        if found.state == CustodyState::Activating {
            // Readback answers what persisted. It does not claim the earlier
            // request performed the CAS, nor consume another write aperture.
            manual.authorize(work, lease, selection)?;
            return Ok(Reply::Approve {
                entry: entry(found),
            });
        }

        let mut denied = None;
        let mut cas_started = false;
        let committed = self
            .owner
            .commit_pending_decision(&row, &joined, |checkpoint| {
                let uid = match manual.authorize(work, lease, selection) {
                    Ok(uid) => uid,
                    Err(failure) => {
                        denied = Some(failure);
                        return Err(Error::Binding);
                    }
                };
                let local = checkpoint.authorize_local(uid)?;
                cas_started = true;
                Ok(CustodyDecision::Manual(local))
            });
        let at = committed.map_err(|error| {
            if cas_started
                && matches!(
                    error,
                    Error::Checkpoint(consent_checkpoint::Error::Cursor(store::Error::Storage))
                )
            {
                return IpcFailure::Uncertain;
            }
            denied.unwrap_or_else(|| local_failure(error))
        })?;
        if std::mem::take(&mut self.uncertain_approval) || work.can_reply().is_err() {
            return Err(IpcFailure::Uncertain);
        }
        found.state = CustodyState::Activating;
        found.activating_at = Some(at);
        Ok(Reply::Approve {
            entry: entry(found),
        })
    }

    fn proof(&mut self, selection: Selection) -> LocalResult<(HeldEvidence, JoinedReadback)> {
        if selection.server != self.founder {
            return Err(IpcFailure::Forbidden);
        }
        self.owner
            .read_pending_proof(
                Selector {
                    server: selection.server,
                    id: selection.offer_id,
                    revision: selection.revision,
                },
                selection.generation,
            )
            .map_err(local_failure)?
            .ok_or(IpcFailure::Stale)
    }

    pub(super) fn automatic(
        &mut self,
        worker: &Worker,
        key: Selector,
        generation: u64,
    ) -> Result<()> {
        let Some((row, joined)) = self.owner.read_pending_proof(key, generation)? else {
            return Ok(());
        };
        self.owner.commit_pending_decision(&row, &joined, |_| {
            if !worker.registered() || worker.stopped() {
                return Err(Error::Binding);
            }
            Ok(CustodyDecision::Automatic)
        })?;
        Ok(())
    }
}

fn local_failure(error: Error) -> IpcFailure {
    match error {
        Error::Checkpoint(consent_checkpoint::Error::Expired) => IpcFailure::Expired,
        Error::Checkpoint(
            consent_checkpoint::Error::Policy | consent_checkpoint::Error::LocalOwner,
        ) => IpcFailure::Forbidden,
        Error::Checkpoint(
            consent_checkpoint::Error::Missing
            | consent_checkpoint::Error::Cursor(store::Error::Stale),
        ) => IpcFailure::Stale,
        _ => IpcFailure::Unavailable,
    }
}

fn entry(found: Inventory) -> Entry {
    let state = if found.state == CustodyState::Pending {
        State::Pending
    } else {
        State::Activating
    };
    Entry {
        selection: Selection {
            server: found.key.server,
            offer_id: found.key.id,
            revision: found.key.revision,
            generation: found.generation,
        },
        state,
        held_at: found.held_at,
        qualified_at: found.qualified_at,
        expires_at: found.expires_at,
        activating_at: found.activating_at,
    }
}
