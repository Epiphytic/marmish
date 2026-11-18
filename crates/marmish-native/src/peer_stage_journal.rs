//! Durable ownership transitions for a private provisional stage.

use std::sync::Arc;

#[cfg(test)]
use super::super::super::CursorStore;
use super::super::super::{read_seconds, seconds};
#[cfg(test)]
use super::super::Policy;
use super::super::package_digest;
use super::{Binding, Checkpoint, CursorError, Error, Reservation, Result, Source};
use super::{generation, has_outbox, rows};
#[cfg(test)]
use nostr::key::Keys;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileWitness {
    pub(crate) device: [u8; 8],
    pub(crate) inode: [u8; 8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Artifact {
    pub(crate) id: [u8; 32],
    pub(crate) digest: [u8; 32],
    pub(crate) expires: u64,
}

impl Artifact {
    pub(crate) fn new(id: [u8; 32], exact_bytes: &[u8], expires: u64) -> Self {
        Self {
            id,
            digest: package_digest(exact_bytes),
            expires,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Bound,
    NativeStarted(FileWitness),
    Prepared(FileWitness, Artifact),
}

#[derive(Debug)]
pub(crate) struct BoundStage {
    owner: Arc<()>,
    binding: Binding,
    state: State,
    offer_expires: u64,
}

impl BoundStage {
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(crate) fn require_package_owner(&self, checkpoint: &Checkpoint) -> Result<()> {
        checkpoint.check_local_owner(&self.owner)
    }

    pub(crate) fn binding(&self) -> Binding {
        self.binding
    }

    pub(crate) fn state(&self) -> State {
        self.state
    }

    pub(crate) fn offer_expires(&self) -> u64 {
        self.offer_expires
    }
}

impl Reservation {
    pub(crate) fn stage_binding(&self) -> Binding {
        self.binding
    }
}

/// Intentionally not Clone. Only a newly committed NativeStarted grants generation.
pub(crate) struct NativeStart {
    owner: Arc<()>,
    binding: Binding,
    witness: FileWitness,
}

impl NativeStart {
    pub(crate) fn matches(self, stage: &BoundStage, witness: FileWitness) -> bool {
        self.same_stage(stage, witness)
    }

    pub(crate) fn same_stage(&self, stage: &BoundStage, witness: FileWitness) -> bool {
        Arc::ptr_eq(&self.owner, &stage.owner)
            && self.binding == stage.binding
            && self.witness == witness
    }
}

pub(super) fn load(conn: &Connection, binding: Binding) -> Result<Option<State>> {
    conn.query_row(
        "SELECT state, device, inode, package_id, package_digest, package_expires
         FROM peer_provisional_stage_v1 WHERE class=?1 AND generation=?2 AND server=?3
             AND offer_id=?4 AND revision=?5 AND origin=?6",
        params![
            binding.generation.known,
            seconds(binding.generation.count)?,
            binding.key.server.to_bytes(),
            binding.key.id,
            binding.key.revision,
            binding.origin,
        ],
        |row| {
            let witness = || -> rusqlite::Result<FileWitness> {
                Ok(FileWitness {
                    device: row.get(1)?,
                    inode: row.get(2)?,
                })
            };
            match row.get::<_, u8>(0)? {
                0 => Ok(State::Bound),
                1 => Ok(State::NativeStarted(witness()?)),
                2 => Ok(State::Prepared(
                    witness()?,
                    Artifact {
                        id: row.get(3)?,
                        digest: row.get(4)?,
                        expires: read_seconds(row, 5)?,
                    },
                )),
                _ => Err(rusqlite::Error::InvalidQuery),
            }
        },
    )
    .optional()
    .map_err(Error::from)
}

impl Checkpoint {
    #[cfg(test)]
    pub(crate) fn attach_for_provisional_proof(
        cursor: CursorStore,
        signer: Keys,
        owner_uid: u32,
        urls: &[&str],
        policy: Policy,
    ) -> Result<Self> {
        let mut checkpoint = Self::attach_with(
            cursor,
            signer,
            owner_uid,
            urls,
            policy,
            |cursor, account| rows::attach_with(cursor, account, super::stage_schema::attach),
        )?;
        checkpoint.provisional_schema = true;
        Ok(checkpoint)
    }

    #[cfg(test)]
    pub(crate) fn has_stage(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        wall: u64,
    ) -> Result<bool> {
        self.check_local_owner(&reservation.owner)?;
        let (tx, _) = self.stage_transaction(source, reservation.binding, wall)?;
        Ok(load(&tx, reservation.binding)?.is_some())
    }

    pub(crate) fn bind_stage(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        wall: u64,
    ) -> Result<BoundStage> {
        self.check_local_owner(&reservation.owner)?;
        let binding = reservation.binding;
        let owner = self.owner.clone();
        let (tx, offer_expires) = self.stage_transaction(source, binding, wall)?;
        let state = match load(&tx, binding)? {
            Some(state) => state,
            None => {
                tx.execute(
                    "INSERT INTO peer_provisional_stage_v1
                         (class, generation, server, offer_id, revision, origin, state)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
                    params![
                        binding.generation.known,
                        seconds(binding.generation.count)?,
                        binding.key.server.to_bytes(),
                        binding.key.id,
                        binding.key.revision,
                        binding.origin,
                    ],
                )?;
                State::Bound
            }
        };
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(BoundStage {
            owner,
            binding,
            state,
            offer_expires,
        })
    }

    pub(crate) fn start_stage(
        &mut self,
        source: &Source,
        stage: &BoundStage,
        witness: FileWitness,
        wall: u64,
    ) -> Result<NativeStart> {
        self.check_local_owner(&stage.owner)?;
        let binding = stage.binding;
        let owner = self.owner.clone();
        let (tx, _) = self.stage_transaction(source, binding, wall)?;
        if load(&tx, binding)? != Some(State::Bound) {
            return Err(Error::Bound);
        }
        tx.execute(
            "UPDATE peer_provisional_stage_v1 SET state=1, device=?2, inode=?3 WHERE class=?1",
            params![binding.generation.known, witness.device, witness.inode],
        )?;
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(NativeStart {
            owner,
            binding,
            witness,
        })
    }

    pub(crate) fn record_prepared(
        &mut self,
        source: &Source,
        stage: &BoundStage,
        witness: FileWitness,
        artifact: Artifact,
        wall: u64,
    ) -> Result<()> {
        self.check_local_owner(&stage.owner)?;
        let binding = stage.binding;
        let (tx, offer_expires) = self.stage_transaction(source, binding, wall)?;
        if artifact.expires <= wall
            || offer_expires.checked_add(600).ok_or(Error::Package)? > artifact.expires
        {
            return Err(Error::Package);
        }
        match load(&tx, binding)? {
            Some(State::NativeStarted(previous)) if previous == witness => {
                tx.execute(
                    "UPDATE peer_provisional_stage_v1
                     SET state=2, package_id=?2, package_digest=?3, package_expires=?4 WHERE class=?1",
                    params![
                        binding.generation.known,
                        artifact.id,
                        artifact.digest,
                        seconds(artifact.expires)?,
                    ],
                )?;
            }
            Some(State::Prepared(previous, known)) if previous == witness && known == artifact => {}
            _ => return Err(Error::Bound),
        }
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(())
    }

    pub(super) fn stage_transaction(
        &mut self,
        source: &Source,
        binding: Binding,
        wall: u64,
    ) -> Result<(Transaction<'_>, u64)> {
        if !self.provisional_schema || binding.origin != self.origin(source)? {
            return Err(Error::Source);
        }
        #[cfg(test)]
        if self.retirement_epoch {
            super::retirement::verify_epoch(&self.cursor.conn)?;
            if super::retirement::retiring(&self.cursor.conn, binding)? {
                return Err(Error::Retiring);
            }
        }
        let (tx, origin, offer_expires) =
            self.live_provisioning_offer(source, binding.key, wall)?;
        if generation(&tx, binding.key, origin)? != Some(binding.generation)
            || has_outbox(&tx, binding.key, origin)?
        {
            return Err(CursorError::Stale.into());
        }
        Ok((tx, offer_expires))
    }
}
