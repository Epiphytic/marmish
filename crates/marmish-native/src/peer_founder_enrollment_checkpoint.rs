//! Independent founder enrollment coordinates retained in the private fixture.
use super::*;

#[derive(Eq, PartialEq)]
pub(crate) struct FounderEnrollment {
    pub(crate) receipt: Receipt,
    pub(crate) group: Vec<u8>,
    pub(crate) route: [u8; 32],
    pub(crate) leaf: u32,
    pub(crate) valid_from: u64,
    pub(crate) deadline: u64,
}

impl FounderEnrollment {
    pub(super) fn matches(&self, source: &ReceiptSourceCoordinates) -> bool {
        self.receipt == source.decoded.receipt
            && self.group == source.group
            && self.route == source.route
            && self.leaf == source.sender_leaf
            && self.deadline == source.deadline
            && source.decoded.created_at >= self.valid_from
    }
}

pub(super) fn load(tx: &Transaction<'_>, origin: Origin) -> Result<Option<FounderEnrollment>> {
    let saved = tx
        .query_row(
            "SELECT founder,recipient,expected,group_id,route,recipient_leaf,valid_from,deadline
         FROM peer_founder_enrollment WHERE singleton=1 AND origin=?1",
            [origin],
            |row| {
                Ok((
                    row.get::<_, [u8; 32]>(0)?,
                    row.get::<_, [u8; 32]>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    read_seconds(row, 6)?,
                    read_seconds(row, 7)?,
                ))
            },
        )
        .optional()?;
    let Some((founder, recipient, bytes, group, route, leaf, valid_from, deadline)) = saved else {
        return Ok(None);
    };
    let decoded = receipt_wire::decode(&bytes).map_err(|_| CursorError::Binding)?;
    if decoded.receipt.founder.to_bytes() != founder
        || decoded.receipt.recipient.to_bytes() != recipient
        || decoded.created_at != valid_from
    {
        return Err(CursorError::Binding.into());
    }
    Ok(Some(FounderEnrollment {
        receipt: decoded.receipt,
        group,
        route,
        leaf,
        valid_from,
        deadline,
    }))
}

impl Checkpoint {
    pub(crate) fn record_founder_enrollment(
        &mut self,
        source: &Source,
        enrollment: &FounderEnrollment,
        wall: u64,
    ) -> Result<()> {
        let cx = self.publication_context(source, enrollment.receipt.recipient)?;
        if enrollment.receipt.founder != cx.signer.public_key()
            || wall < enrollment.valid_from
            || wall >= enrollment.deadline
        {
            return Err(CursorError::Binding.into());
        }
        let prepared = receipt_wire::prepare(&enrollment.receipt, enrollment.valid_from)
            .map_err(|_| CursorError::Binding)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        if let Some(saved) = load(&tx, cx.origin)? {
            return if saved == *enrollment {
                Ok(())
            } else {
                Err(CursorError::Stale.into())
            };
        }
        Table::FounderEnrollment.checked_write(&tx, "INSERT", || {
            rows::require_write(tx.execute(
                "INSERT INTO peer_founder_enrollment
                 (singleton,origin,founder,recipient,expected,group_id,route,recipient_leaf,valid_from,deadline)
                 VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    cx.origin,
                    enrollment.receipt.founder.to_bytes(),
                    enrollment.receipt.recipient.to_bytes(),
                    prepared.payload(),
                    enrollment.group,
                    enrollment.route,
                    enrollment.leaf,
                    seconds(enrollment.valid_from)?,
                    seconds(enrollment.deadline)?,
                ],
            )?)?;
            Ok(())
        })?;
        if load(&tx, cx.origin)?.as_ref() != Some(enrollment) {
            return Err(CursorError::Binding.into());
        }
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn read_founder_enrollment(
        &mut self,
        source: &Source,
        recipient: PublicKey,
        wall: u64,
    ) -> Result<FounderEnrollment> {
        let cx = self.publication_context(source, recipient)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        let saved = load(&tx, cx.origin)?.ok_or(CursorError::Binding)?;
        if saved.receipt.founder != cx.signer.public_key() || saved.receipt.recipient != recipient {
            return Err(CursorError::Binding.into());
        }
        Ok(saved)
    }

    pub(crate) fn read_founder_acceptance(
        &mut self,
        source: &Source,
        recipient: PublicKey,
        wall: u64,
    ) -> Result<FounderAcceptance> {
        let cx = self.publication_context(source, recipient)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        let enrollment = load(&tx, cx.origin)?.ok_or(CursorError::Binding)?;
        let saved = load_acceptance(&tx, cx.origin)?.ok_or(CursorError::Binding)?;
        if enrollment.receipt.founder != cx.signer.public_key()
            || enrollment.receipt.recipient != recipient
            || !enrollment.matches(saved.source())
            || saved.accepted_at() > wall
        {
            return Err(CursorError::Binding.into());
        }
        Ok(saved)
    }
}
