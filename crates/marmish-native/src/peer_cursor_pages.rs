//! Inclusive, origin-specific catch-up pagination after carrier attribution.

use super::*;

const PAGE_SIZE: usize = 4;
const TIE_CAP: usize = PAGE_SIZE - 1;
const LOOKBACK: u64 = 259_500;
const SKEW: u64 = 300;
const CADENCE: u64 = 600;
const QUARANTINE: u64 = 60;

#[derive(Clone, Copy)]
pub(crate) enum ScanStart {
    Recovery,
    Periodic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub(crate) origin: Origin,
    pub(crate) revision: u64,
    pub(crate) anchor: u64,
    pub(crate) lower: u64,
    pub(crate) upper: u64,
    pub(crate) until: u64,
    pub(crate) ties: Vec<EventId>,
    pub(crate) visible: usize,
}

pub(crate) struct Page<'a> {
    pub(crate) origin: Origin,
    pub(crate) since: u64,
    pub(crate) until: u64,
    pub(crate) records: &'a [RawRecord<'a>],
}

struct ScanState {
    revision: u64,
    active: Option<Cursor>,
    completed_wall: Option<u64>,
    quarantine_until: Option<u64>,
}

fn load(tx: &Transaction<'_>, origin: Origin) -> Result<ScanState> {
    let (revision, cutoffs, ties, visible, completed_wall, quarantine_until) = tx.query_row(
        "SELECT revision, anchor, lower, upper, boundary, ties, visible, completed_wall,
            quarantine_until FROM peer_cursor_origin WHERE origin=?1",
        [origin],
        |row| {
            Ok((
                read_seconds(row, 0)?,
                [
                    read_optional_seconds(row, 1)?,
                    read_optional_seconds(row, 2)?,
                    read_optional_seconds(row, 3)?,
                    read_optional_seconds(row, 4)?,
                ],
                row.get::<_, Vec<u8>>(5)?,
                usize::from(row.get::<_, u16>(6)?),
                read_optional_seconds(row, 7)?,
                read_optional_seconds(row, 8)?,
            ))
        },
    )?;
    let active = match cutoffs {
        [None, None, None, None] if ties.is_empty() && visible == 0 => None,
        [Some(anchor), Some(lower), Some(upper), Some(until)] => {
            let (chunks, remainder) = ties.as_chunks::<32>();
            if !remainder.is_empty()
                || chunks.len() > TIE_CAP
                || chunks.len() > visible
                || chunks.windows(2).any(|pair| pair[0] >= pair[1])
                || visible > VISIBLE
                || lower != u64::saturating_sub(anchor, LOOKBACK)
                || Some(upper) != anchor.checked_add(SKEW)
                || !(lower..=upper).contains(&until)
            {
                return Err(Error::Storage);
            }
            Some(Cursor {
                origin,
                revision,
                anchor,
                lower,
                upper,
                until,
                ties: chunks.to_vec(),
                visible,
            })
        }
        _ => return Err(Error::Storage),
    };
    Ok(ScanState {
        revision,
        active,
        completed_wall,
        quarantine_until,
    })
}

impl CursorStore {
    pub(crate) fn scan(
        &mut self,
        origin: Origin,
        start: ScanStart,
        wall: u64,
    ) -> Result<Option<Cursor>> {
        let tx = self.with_wall(origin, wall)?;
        let state = load(&tx, origin)?;
        if state.quarantine_until.is_some() {
            return Err(Error::RelayContract);
        }
        if state.active.is_some() {
            tx.commit()?;
            return Ok(state.active);
        }
        if matches!(start, ScanStart::Periodic)
            && state
                .completed_wall
                .is_some_and(|done| wall.saturating_sub(done) < CADENCE)
        {
            tx.commit()?;
            return Ok(None);
        }
        let lower = wall.saturating_sub(LOOKBACK);
        let upper = after(wall, SKEW)?;
        tx.execute(
            "UPDATE peer_cursor_origin SET revision=revision+1, anchor=?2, lower=?3, upper=?4,
                boundary=?4 WHERE origin=?1",
            params![origin, seconds(wall)?, seconds(lower)?, upper],
        )?;
        let cursor = load(&tx, origin)?.active.ok_or(Error::Storage)?;
        tx.commit()?;
        Ok(Some(cursor))
    }

    pub(crate) fn commit_page(
        &mut self,
        cursor: &Cursor,
        page: &Page<'_>,
        receipts: &[Receipt],
        wall: u64,
    ) -> Result<Option<Cursor>> {
        if page.origin != cursor.origin || page.since != cursor.lower || page.until != cursor.until
        {
            return Err(Error::Page);
        }
        let origin = cursor.origin;
        let tx = self.with_wall(origin, wall)?;
        let state = load(&tx, origin)?;
        if state.quarantine_until.is_some() {
            return Err(Error::RelayContract);
        }
        if state.revision != cursor.revision || state.active.as_ref() != Some(cursor) {
            return Err(Error::Stale);
        }
        let Ok(visible) = count_page(cursor, page.records) else {
            tx.execute(
                "UPDATE peer_cursor_origin SET quarantine_until=?2 WHERE origin=?1",
                params![origin, after(wall, QUARANTINE)?],
            )?;
            tx.commit()?;
            return Err(Error::RelayContract);
        };
        check_receipts(&tx, origin, page.records, receipts)?;
        let remaining = if page.records.len() == PAGE_SIZE {
            advance(&tx, cursor, page.records, visible)?;
            load(&tx, origin)?.active
        } else {
            tx.execute(
                "UPDATE peer_cursor_origin SET revision=revision+1, anchor=NULL, lower=NULL,
                    upper=NULL, boundary=NULL, ties=X'', visible=0,
                    completed_anchor=?2, completed_wall=?3 WHERE origin=?1",
                params![origin, seconds(cursor.anchor)?, seconds(wall)?],
            )?;
            None
        };
        tx.commit()?;
        Ok(remaining)
    }

    pub(crate) fn requalify(&mut self, origin: Origin, wall: u64) -> Result<()> {
        let tx = self.with_wall(origin, wall)?;
        let state = load(&tx, origin)?;
        match state.quarantine_until {
            Some(until) if wall < until => return Err(Error::RelayContract),
            Some(_) => {
                reset_active(&tx, origin)?;
                tx.execute(
                    "UPDATE peer_cursor_origin SET quarantine_until=NULL WHERE origin=?1",
                    [origin],
                )?;
            }
            None => return Err(Error::Stale),
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn rebase_wall(&mut self, origin: Origin, wall: u64) -> Result<()> {
        let tx = self.begin(origin)?;
        let state = load(&tx, origin)?;
        let quarantine = state
            .quarantine_until
            .map(|_| after(wall, QUARANTINE))
            .transpose()?;
        reset_active(&tx, origin)?;
        tx.execute(
            "UPDATE peer_cursor_origin SET wall=?2, completed_wall=NULL, quarantine_until=?3
                WHERE origin=?1",
            params![origin, seconds(wall)?, quarantine],
        )?;
        retry::rebase_due(&tx, origin, wall)?;
        tx.commit()?;
        Ok(())
    }
}

fn reset_active(tx: &Transaction<'_>, origin: Origin) -> Result<()> {
    tx.execute(
        "UPDATE peer_cursor_origin SET revision=revision+1, anchor=NULL, lower=NULL,
            upper=NULL, boundary=NULL, ties=X'', visible=0 WHERE origin=?1",
        [origin],
    )?;
    Ok(())
}

fn count_page(cursor: &Cursor, records: &[RawRecord<'_>]) -> Result<usize> {
    if records.len() > PAGE_SIZE {
        return Err(Error::RelayContract);
    }
    let mut run = 0;
    let mut previous = None;
    let mut visible = cursor.visible;
    for (index, record) in records.iter().enumerate() {
        record.check()?;
        if !(cursor.lower..=cursor.until).contains(&record.stamp)
            || previous.is_some_and(|stamp| stamp < record.stamp)
            || records[..index].iter().any(|seen| seen.id == record.id)
        {
            return Err(Error::RelayContract);
        }
        run = if previous == Some(record.stamp) {
            run + 1
        } else {
            1
        };
        if run > TIE_CAP {
            return Err(Error::RelayContract);
        }
        previous = Some(record.stamp);
        if record.stamp != cursor.until || cursor.ties.binary_search(&record.id).is_err() {
            visible += 1;
        }
    }
    if visible > VISIBLE
        || records.len() == PAGE_SIZE && previous.is_none_or(|stamp| stamp >= cursor.until)
    {
        return Err(Error::RelayContract);
    }
    Ok(visible)
}

fn check_receipts(
    tx: &Transaction<'_>,
    origin: Origin,
    records: &[RawRecord<'_>],
    receipts: &[Receipt],
) -> Result<()> {
    if records.len() != receipts.len() {
        return Err(Error::Page);
    }
    for record in records {
        let digest = record.digest();
        let mut matching = receipts
            .iter()
            .filter(|receipt| receipt.matches(origin, *record, digest));
        let receipt = matching.next().ok_or(Error::Page)?;
        if matching.next().is_some() {
            return Err(Error::Page);
        }
        if receipt.kind == ReceiptKind::Deferred && !retry::record_exists(tx, origin, *record)? {
            return Err(Error::Stale);
        }
    }
    Ok(())
}

fn advance(
    tx: &Transaction<'_>,
    cursor: &Cursor,
    records: &[RawRecord<'_>],
    visible: usize,
) -> Result<()> {
    let until = records.last().ok_or(Error::Page)?.stamp;
    let mut ties: Vec<_> = records
        .iter()
        .rev()
        .take_while(|record| record.stamp == until)
        .map(|record| record.id)
        .collect();
    ties.sort_unstable();
    tx.execute(
        "UPDATE peer_cursor_origin SET revision=revision+1, boundary=?2, ties=?3, visible=?4 WHERE origin=?1",
        params![cursor.origin, seconds(until)?, ties.as_flattened(), i64::try_from(visible).map_err(|_| Error::Range)?],
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Turn {
    Retry,
    Page,
    Yield,
}

#[derive(Default)]
pub(crate) struct ServiceTurn {
    pages: u8,
    retries: u8,
    prefer_page: bool,
}

impl ServiceTurn {
    pub(crate) fn choose(&mut self, page_ready: bool, retry_due: bool) -> Turn {
        let page = page_ready && self.pages < 8;
        let retry = retry_due && self.retries < 8;
        match (page, retry, self.prefer_page) {
            (true, false, _) | (true, true, true) => {
                self.pages += 1;
                self.prefer_page = false;
                Turn::Page
            }
            (_, true, _) => {
                self.retries += 1;
                self.prefer_page = true;
                Turn::Retry
            }
            _ => Turn::Yield,
        }
    }
}

impl CursorStore {
    pub(crate) fn require_available(&mut self, origin: Origin, wall: u64) -> Result<()> {
        let tx = self.with_wall(origin, wall)?;
        if load(&tx, origin)?.quarantine_until.is_some() {
            return Err(Error::RelayContract);
        }
        Ok(())
    }
}
