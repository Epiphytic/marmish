use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) enum ApprovalFailure {
    Stale,
    Expired,
    Unavailable,
    Clock,
    Integrity,
    Ambiguous,
}

impl From<Error> for ApprovalFailure {
    fn from(error: Error) -> Self {
        match error {
            Error::Expired => Self::Expired,
            Error::Source | Error::LocalOwner => Self::Unavailable,
            Error::Policy | Error::Missing | Error::Cursor(CursorError::Stale) => Self::Stale,
            Error::Cursor(CursorError::ClockRegressed) => Self::Clock,
            _ => Self::Integrity,
        }
    }
}

impl From<CursorError> for ApprovalFailure {
    fn from(error: CursorError) -> Self {
        Error::from(error).into()
    }
}

impl From<rusqlite::Error> for ApprovalFailure {
    fn from(error: rusqlite::Error) -> Self {
        Error::from(error).into()
    }
}

pub(in crate::peer_invitation) type Result<T> = std::result::Result<T, ApprovalFailure>;

#[derive(Clone, Copy)]
pub(in crate::peer_invitation) struct ApprovalScope<'a> {
    pub(in crate::peer_invitation) source: &'a Source,
    pub(in crate::peer_invitation) account: PublicKey,
    pub(in crate::peer_invitation) origin: &'a str,
    pub(in crate::peer_invitation) selector: Selector,
    pub(in crate::peer_invitation) generation: u64,
}

#[derive(Clone, Copy)]
pub(in crate::peer_invitation) enum ApprovalGuard {
    Consume,
    Verify,
}

pub(super) fn floors(tx: &Transaction<'_>, origin: Origin) -> Result<(u64, u64)> {
    Ok(tx.query_row(
        "SELECT o.wall,v.wall FROM peer_cursor_origin o CROSS JOIN peer_consent_version v
         WHERE o.origin=?1 AND v.singleton=1",
        [origin],
        |row| Ok((read_seconds(row, 0)?, read_seconds(row, 1)?)),
    )?)
}

pub(super) fn check_wall(wall: u64, floor: u64, end: u64) -> Result<()> {
    if wall < floor {
        return Err(ApprovalFailure::Clock);
    }
    if wall >= end {
        return Err(ApprovalFailure::Expired);
    }
    Ok(())
}

fn write(
    tx: &Transaction<'_>,
    origin: Origin,
    carrier: [u8; 32],
    generation: u64,
    at: u64,
) -> Result<usize> {
    let writer = custody_schema::Writer::Update;
    writer.guard(tx, false)?;
    let changed = tx.execute(
        "UPDATE peer_pending_custody SET state=3,approval=0,activating_at=?3
         WHERE delivery_carrier=?1 AND generation=?2 AND mode=0 AND state=2 AND approval IS NULL",
        params![carrier, seconds(generation)?, seconds(at)?],
    )?;
    rows::require_write(changed)?;
    writer.guard(tx, true)?;
    if tx.execute(
        "UPDATE peer_cursor_origin SET wall=?2 WHERE origin=?1 AND wall<=?2",
        params![origin, seconds(at)?],
    )? != 1
    {
        return Err(ApprovalFailure::Clock);
    }
    rows::advance_wall(tx, at)?;
    Ok(changed)
}

fn commit(tx: Transaction<'_>) -> Result<()> {
    tx.commit().map_err(|_| ApprovalFailure::Ambiguous)
}

pub(super) fn record(
    tx: Transaction<'_>,
    origin: Origin,
    carrier: [u8; 32],
    generation: u64,
    at: u64,
    verify: impl FnOnce(&Transaction<'_>) -> Result<()>,
) -> Result<usize> {
    let changed = write(&tx, origin, carrier, generation, at)?;
    verify(&tx)?;
    commit(tx)?;
    Ok(changed)
}

#[cfg(test)]
#[path = "peer_installed_approval_decision_tests.rs"]
mod tests;
