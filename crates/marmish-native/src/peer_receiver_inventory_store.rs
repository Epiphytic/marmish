//! Exact durable reservation facts for the recipient quota owner.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Charge {
    pub(crate) binding: Binding,
    pub(crate) stage: Option<stage_journal::State>,
}

impl Checkpoint {
    pub(crate) fn receiver_charges(
        &mut self,
        source: &Source,
        wall: u64,
    ) -> Result<[Option<Charge>; 2]> {
        if !self.is_installed_custody() {
            return Err(Error::Layout);
        }
        let cx = self.custody_context(source)?;
        let tx = self.cursor.begin(cx.origin)?;
        let last_observed = tx.query_row(
            "SELECT wall FROM peer_cursor_origin WHERE origin=?1",
            [cx.origin],
            |row| read_seconds(row, 0),
        )?;
        if wall < last_observed {
            return Err(CursorError::ClockRegressed.into());
        }
        cx.verify(&tx, wall)?;
        Ok([
            class_charge(&tx, cx.origin, true)?,
            class_charge(&tx, cx.origin, false)?,
        ])
    }
}

fn class_charge(tx: &Transaction<'_>, origin: Origin, known: bool) -> Result<Option<Charge>> {
    let (count, server, offer, revision, stored_origin) = tx.query_row(
        "SELECT generation,server,offer_id,revision,origin
           FROM peer_provisioning_slot WHERE class=?1",
        [known],
        |row| {
            Ok((
                read_seconds(row, 0)?,
                row.get::<_, Option<[u8; 32]>>(1)?,
                row.get::<_, Option<[u8; 16]>>(2)?,
                row.get::<_, Option<[u8; 32]>>(3)?,
                row.get::<_, Option<Origin>>(4)?,
            ))
        },
    )?;
    let occupied: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_provisional_stage_v1 WHERE class=?1)",
        [known],
        |row| row.get(0),
    )?;
    match (server, offer, revision, stored_origin) {
        (None, None, None, None) if !occupied => Ok(None),
        (Some(server), Some(id), Some(revision), Some(stored)) if stored == origin && count > 0 => {
            let key = Selector {
                server: PublicKey::from_slice(&server).map_err(|_| CursorError::Binding)?,
                id,
                revision,
            };
            let binding = Binding {
                key,
                origin,
                generation: super::super::super::Generation { known, count },
            };
            Invitation::load(tx, key, origin)?;
            let stage = stage_journal::load(tx, binding)?;
            if stage.is_some() != occupied {
                return Err(CursorError::Binding.into());
            }
            Ok(Some(Charge { binding, stage }))
        }
        _ => Err(CursorError::Binding.into()),
    }
}
