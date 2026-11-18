//! Exact custody journal for a single non-reconstructible native dispatch.

use crate::peer_invitation::LeaveSnapshot;

use super::{
    leave_effect_schema,
    terminal::advance_local_wall,
    terminal_leave::{Hold, Recovery},
    *,
};

pub(crate) struct NativeStart {
    stage: (Binding, FileWitness),
    image: [u8; 32],
}

impl NativeStart {
    pub(crate) fn check_image(self, stage: (Binding, FileWitness), image: [u8; 32]) -> Result<()> {
        if self.stage != stage || self.image != image {
            return Err(CursorError::Binding.into());
        }
        Ok(())
    }
}

impl Checkpoint {
    pub(crate) fn attach_leave_effect_fixture(
        conn: HardenedPeerConnection,
        signer: Keys,
        uid: u32,
        url: &str,
        policy: Policy,
    ) -> Result<Self> {
        let origin = super::super::super::super::Route::configured(url)?.id;
        let (cursor, epoch) =
            custody_schema::open_leave_effect(conn, signer.public_key().to_bytes(), origin)?;
        let mut checkpoint = Self::attach_with(cursor, signer, uid, &[url], policy, |_, _| Ok(()))?;
        checkpoint.provisional_schema = true;
        checkpoint.export_epoch = Some(super::super::Epoch::Custody(epoch));
        Ok(checkpoint)
    }

    pub(crate) fn reserve_native_leave(
        &mut self,
        source: &Source,
        prior: &Hold,
        verified: &LeaveSnapshot,
        wall: u64,
    ) -> Result<NativeStart> {
        let image = verified
            .pristine(prior.custody.stage())
            .ok_or(CursorError::Binding)?;
        self.check_local_owner(&prior.custody.owner)?;
        let cx = self.custody_context(source)?;
        if !cx.epoch.is_leave_effect() || cx.origin != prior.custody.stage().0.origin {
            return Err(CursorError::Binding.into());
        }
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let current = cx
            .leave_hold(&tx, prior.selector(), prior.terminal.generation())?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) || current.recovery() != Recovery::Idle {
            return Err(CursorError::Stale.into());
        }
        if wall >= current.deadline() {
            return Err(Error::Expired);
        }
        let stage = current.custody.stage();
        advance_local_wall(&tx, cx.origin, wall)?;
        leave_effect_schema::set_write(&tx, false, true)?;
        rows::require_write(tx.execute(
            "INSERT INTO peer_terminal_leave_effect (class,generation,armed_at,image_digest)
             VALUES (?1,?2,?3,?4) ON CONFLICT(class,generation) DO NOTHING",
            params![
                stage.0.generation.known,
                seconds(stage.0.generation.count)?,
                seconds(wall)?,
                image,
            ],
        )?)?;
        leave_effect_schema::set_write(&tx, false, false)?;
        cx.verify_local_terminal(&tx, wall)?;
        tx.commit()?;
        Ok(NativeStart { stage, image })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    Absent,
    Armed,
    Captured([u8; 32]),
}

type Retained = (u64, Option<([u8; 32], [u8; 32], Zeroizing<Vec<u8>>)>);

fn retained(tx: &Transaction<'_>, stage: Binding) -> Result<Option<Retained>> {
    let raw = tx
        .query_row(
            "SELECT armed_at,captured_at,event_id,event_digest,signed_event
         FROM peer_terminal_leave_effect WHERE class=?1 AND generation=?2",
            params![stage.generation.known, seconds(stage.generation.count)?],
            |row| {
                Ok((
                    read_seconds(row, 0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<Vec<u8>>>(2)?,
                    row.get::<_, Option<Vec<u8>>>(3)?,
                    row.get::<_, Option<Vec<u8>>>(4)?.map(Zeroizing::new),
                ))
            },
        )
        .optional()?;
    let Some((armed, captured, id, digest, bytes)) = raw else {
        return Ok(None);
    };
    match (captured, id, digest, bytes) {
        (None, None, None, None) => Ok(Some((armed, None))),
        (Some(_), Some(id), Some(digest), Some(bytes)) => {
            let expected: [u8; 32] = Sha256::digest(&bytes).into();
            if digest != expected || !(1..=65_536).contains(&bytes.len()) {
                return Err(CursorError::Binding.into());
            }
            let id = id.try_into().map_err(|_| CursorError::Binding)?;
            Ok(Some((armed, Some((id, expected, bytes)))))
        }
        _ => Err(CursorError::Binding.into()),
    }
}

impl Checkpoint {
    pub(crate) fn native_leave_effect(
        &mut self,
        source: &Source,
        prior: &Hold,
        wall: u64,
    ) -> Result<Effect> {
        let cx = self.custody_context(source)?;
        if !cx.epoch.is_leave_effect() {
            return Err(CursorError::Binding.into());
        }
        self.check_local_owner(&prior.custody.owner)?;
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let current = cx
            .leave_hold(&tx, prior.selector(), prior.terminal.generation())?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) || !current.is_idle() {
            return Err(CursorError::Stale.into());
        }
        if wall >= current.deadline() {
            return Err(Error::Expired);
        }
        Ok(match retained(&tx, current.custody.stage().0)? {
            Some((_, Some((id, _, _)))) => Effect::Captured(id),
            Some((_, None)) => Effect::Armed,
            None => Effect::Absent,
        })
    }

    pub(crate) fn capture_native_leave(
        &mut self,
        source: &Source,
        prior: &Hold,
        verified: LeaveSnapshot,
        wall: u64,
    ) -> Result<[u8; 32]> {
        let cx = self.custody_context(source)?;
        if !cx.epoch.is_leave_effect() {
            return Err(CursorError::Binding.into());
        }
        self.check_local_owner(&prior.custody.owner)?;
        let tx = self.cursor.begin(cx.origin)?;
        cx.verify_local_terminal(&tx, wall)?;
        let current = cx
            .leave_hold(&tx, prior.selector(), prior.terminal.generation())?
            .ok_or(CursorError::Stale)?;
        if !current.same_turn(prior) || !current.is_idle() {
            return Err(CursorError::Stale.into());
        }
        if wall >= current.deadline() {
            return Err(Error::Expired);
        }
        let stage = current.custody.stage();
        let (id, digest, signed) = verified.captured(stage).ok_or(CursorError::Binding)?;
        let (armed, saved) = retained(&tx, stage.0)?.ok_or(CursorError::Stale)?;
        if let Some((saved_id, saved_digest, saved_bytes)) = saved {
            return if (saved_id, saved_digest, saved_bytes.as_slice()) == (id, digest, signed) {
                Ok(id)
            } else {
                Err(CursorError::Binding.into())
            };
        }
        advance_local_wall(&tx, cx.origin, wall)?;
        leave_effect_schema::set_write(&tx, true, true)?;
        rows::require_write(tx.execute(
            "UPDATE peer_terminal_leave_effect
             SET captured_at=?1,event_id=?2,event_digest=?3,signed_event=?4
             WHERE class=?5 AND generation=?6 AND armed_at=?7 AND captured_at IS NULL",
            params![
                seconds(wall)?,
                id,
                digest,
                signed,
                stage.0.generation.known,
                seconds(stage.0.generation.count)?,
                seconds(armed)?
            ],
        )?)?;
        leave_effect_schema::set_write(&tx, true, false)?;
        cx.verify_local_terminal(&tx, wall)?;
        tx.commit()?;
        Ok(id)
    }
}
