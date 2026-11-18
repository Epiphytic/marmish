//! Checked admission and restart material for an installed recipient's v2 Offer.

use std::sync::OnceLock;

use rusqlite::{Connection, Transaction, TransactionBehavior, params};

use super::super::{HardenedPeerConnection, check_hardening};
use super::*;

type DbResult<T> = std::result::Result<T, CursorError>;
type Entry = (String, String, String, Option<String>);

const TABLE: &str = "
CREATE TABLE peer_installed_offer_v2 (
    server BLOB NOT NULL CHECK (length(server)=32),
    origin BLOB NOT NULL CHECK (length(origin)=32),
    offer_id BLOB NOT NULL CHECK (length(offer_id)=16),
    revision BLOB NOT NULL CHECK (length(revision)=32),
    version INTEGER NOT NULL CHECK (version=2),
    PRIMARY KEY (server, origin, offer_id),
    FOREIGN KEY (server, offer_id, revision, origin)
        REFERENCES peer_consent_invite (server, offer_id, revision, origin)
        ON DELETE CASCADE ON UPDATE RESTRICT
) STRICT";
const DENY_INSERT: &str = "
CREATE TRIGGER peer_installed_offer_deny_insert BEFORE INSERT ON peer_installed_offer_v2
BEGIN SELECT RAISE(ABORT,'installed Offer owner required'); END";
const DENY_UPDATE: &str = "
CREATE TRIGGER peer_installed_offer_deny_update BEFORE UPDATE ON peer_installed_offer_v2
BEGIN SELECT RAISE(ABORT,'installed Offer is immutable'); END";
const DENY_DELETE: &str = "
CREATE TRIGGER peer_installed_offer_deny_delete BEFORE DELETE ON peer_installed_offer_v2
BEGIN SELECT RAISE(ABORT,'installed Offer owner required'); END";

pub(super) fn present(conn: &Connection) -> DbResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_schema WHERE lower(name) GLOB 'peer_installed_*')",
        [],
        |row| row.get(0),
    )?)
}

pub(super) fn install(conn: &Connection) -> DbResult<()> {
    for sql in [TABLE, DENY_INSERT, DENY_UPDATE, DENY_DELETE] {
        conn.execute_batch(sql)?;
    }
    Ok(())
}

fn inventory(conn: &Connection) -> DbResult<Vec<Entry>> {
    let temp: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_temp_schema)",
        [],
        |row| row.get(0),
    )?;
    if temp {
        return Err(CursorError::Binding);
    }
    let mut statement = conn.prepare(
        "SELECT type,name,tbl_name,sql FROM main.sqlite_schema ORDER BY type,name,tbl_name LIMIT 64",
    )?;
    let entries = statement.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    Ok(entries.collect::<rusqlite::Result<_>>()?)
}

fn expected(installed: bool) -> DbResult<&'static [Entry]> {
    static FRESH: OnceLock<Vec<Entry>> = OnceLock::new();
    static INSTALLED: OnceLock<Vec<Entry>> = OnceLock::new();
    let cache = if installed { &INSTALLED } else { &FRESH };
    if let Some(saved) = cache.get() {
        return Ok(saved);
    }
    let mut template = Connection::open_in_memory()?;
    template.execute_batch(super::super::SCHEMA)?;
    if installed {
        let tx = template.transaction()?;
        tx.execute_batch(rows::SCHEMA)?;
        provisioning::attach(&tx, false)?;
        install(&tx)?;
        tx.commit()?;
    }
    let schema = inventory(&template)?;
    Ok(cache.get_or_init(|| schema))
}

fn verify_open_inventory(conn: &Connection) -> DbResult<()> {
    let existing = inventory(conn)?;
    let installed = present(conn)?;
    if existing != expected(installed)? && (installed || !existing.is_empty()) {
        return Err(CursorError::Binding);
    }
    Ok(())
}

fn attach(cursor: &mut CursorStore, codec: &Codec, signer: &Keys, route: &Route) -> DbResult<()> {
    let account = signer.public_key();
    let origin = route.id;
    let tx = cursor
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !present(&tx)? {
        if inventory(&tx)? != expected(false)?
            || tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM peer_cursor_deferred)
                    OR EXISTS(SELECT 1 FROM peer_cursor_origin
                        WHERE wall!=0 OR revision!=0 OR retry_revision!=0
                           OR anchor IS NOT NULL OR lower IS NOT NULL OR upper IS NOT NULL
                           OR boundary IS NOT NULL OR ties!=X'' OR visible!=0
                           OR completed_anchor IS NOT NULL OR completed_wall IS NOT NULL
                           OR quarantine_until IS NOT NULL)",
                [],
                |row| row.get::<_, bool>(0),
            )?
        {
            return Err(CursorError::Binding);
        }
        rows::attach_rows(&tx, account, provisioning::attach)?;
        install(&tx)?;
    }
    verify(&tx, account, origin)?;
    verify_recovered_parents(&tx, codec, signer, route)?;
    tx.commit()?;
    Ok(())
}

pub(super) fn verify(conn: &Connection, account: PublicKey, origin: Origin) -> DbResult<()> {
    check_hardening(conn)?;
    let ignore_checks: bool =
        conn.query_row("PRAGMA ignore_check_constraints", [], |row| row.get(0))?;
    if ignore_checks || inventory(conn)? != expected(true)? {
        return Err(CursorError::Binding);
    }
    verify_rows(conn, account, origin)
}

pub(super) fn verify_rows(conn: &Connection, account: PublicKey, origin: Origin) -> DbResult<()> {
    // Legacy decisions and outbox are forbidden in both installed layouts.
    // The successor verifies its separate Prepared Consent outbox and custody chain.
    let broken: bool = conn.query_row(
        "SELECT (SELECT count(*) FROM peer_consent_version WHERE version=2 AND account=?1)!=1
             OR (SELECT count(*) FROM peer_cursor_version WHERE version=1 AND account=?1)!=1
             OR (SELECT count(*) FROM peer_cursor_origin)!=1
             OR NOT EXISTS(SELECT 1 FROM peer_cursor_origin WHERE origin=?2)
             OR EXISTS(SELECT 1 FROM peer_consent_outbox)
             OR (SELECT count(*) FROM peer_provisioning_slot)!=2
             OR EXISTS(SELECT 1 FROM peer_installed_offer_v2 m
                       WHERE m.version!=2 OR m.origin!=?2 OR NOT EXISTS(
                           SELECT 1 FROM peer_consent_invite i
                           WHERE i.server=m.server AND i.origin=m.origin
                               AND i.offer_id=m.offer_id AND i.revision=m.revision))
             OR EXISTS(SELECT 1 FROM peer_consent_invite i
                       WHERE i.origin!=?2 OR i.server=?1 OR i.decision_at IS NOT NULL
                          OR NOT EXISTS(SELECT 1 FROM peer_installed_offer_v2 m
                              WHERE m.server=i.server AND m.origin=i.origin
                                 AND m.offer_id=i.offer_id AND m.revision=i.revision AND m.version=2)
                          OR (i.replay_until>(SELECT wall FROM peer_consent_version)
                              AND NOT EXISTS(SELECT 1 FROM peer_consent_fence f
                              WHERE f.origin=i.origin AND f.revision=i.revision
                                AND f.reserved=i.reserved AND f.replay_until=i.replay_until)))
             OR EXISTS(SELECT 1 FROM peer_consent_invite GROUP BY reserved HAVING count(*)>8)",
        params![account.to_bytes(), origin],
        |row| row.get(0),
    )?;
    if broken {
        return Err(CursorError::Binding);
    }
    Ok(())
}

pub(super) fn mark(tx: &Transaction<'_>, origin: Origin, offer: &Offer) -> DbResult<()> {
    if offer.version() != ControlVersion::V2 {
        return Err(CursorError::Binding);
    }
    let key = Selector::from(offer);
    tx.execute_batch("DROP TRIGGER peer_installed_offer_deny_insert")?;
    rows::require_write(tx.execute(
        "INSERT INTO peer_installed_offer_v2(server, origin, offer_id, revision, version)
         VALUES(?1,?2,?3,?4,2)",
        params![key.server.to_bytes(), origin, key.id, key.revision],
    )?)?;
    tx.execute_batch(DENY_INSERT)?;
    Ok(())
}

pub(super) fn sweep(tx: &Transaction<'_>, wall: u64) -> DbResult<()> {
    tx.execute_batch("DROP TRIGGER peer_installed_offer_deny_delete")?;
    rows::sweep_with(tx, wall, true)?;
    tx.execute_batch(DENY_DELETE)?;
    Ok(())
}

fn verify_saved(codec: &Codec, signer: &Keys, route: &Route, row: &Invitation) -> Result<Offer> {
    let signed_wall = row.expires.checked_sub(1).ok_or(CursorError::Binding)?;
    Checkpoint::check_signed_offer(codec, signer, route, row, signed_wall)
}

pub(super) fn check_parent(
    tx: &Transaction<'_>,
    codec: &Codec,
    signer: &Keys,
    route: &Route,
    key: Selector,
) -> DbResult<()> {
    let row = Invitation::load(tx, key, route.id).map_err(|_| CursorError::Binding)?;
    verify_saved(codec, signer, route, &row).map_err(|_| CursorError::Binding)?;
    Ok(())
}

pub(super) fn verify_recovered_parents(
    tx: &Transaction<'_>,
    codec: &Codec,
    signer: &Keys,
    route: &Route,
) -> DbResult<()> {
    let mut statement =
        tx.prepare("SELECT server, offer_id, revision FROM peer_installed_offer_v2")?;
    let mut retained = statement.query([])?;
    while let Some(row) = retained.next()? {
        let server =
            PublicKey::from_slice(&row.get::<_, [u8; 32]>(0)?).map_err(|_| CursorError::Binding)?;
        let key = Selector {
            server,
            id: row.get(1)?,
            revision: row.get(2)?,
        };
        check_parent(tx, codec, signer, route, key)?;
    }
    Ok(())
}

/// Historical provenance cannot be passed to an API that expects an actionable Offer.
pub(crate) struct HistoricalOffer {
    pub(crate) selector: Selector,
    pub(crate) expires: u64,
}

impl Checkpoint {
    /// The caller owns the lease and qualified direct route, before any installed wiring.
    pub(crate) fn attach_installed_v2(
        conn: HardenedPeerConnection,
        signer: Keys,
        owner_uid: u32,
        url: &str,
        policy: Policy,
    ) -> Result<Self> {
        let route = Route::configured(url)?;
        let codec = Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V2)
            .map_err(|_| CursorError::Range)?;
        verify_open_inventory(&conn.0)?;
        let cursor = CursorStore::attach(conn, signer.public_key().to_bytes(), &[route.id])?;
        let mut checkpoint =
            Self::attach_with(cursor, signer, owner_uid, &[url], policy, |_, _| Ok(()))?;
        attach(&mut checkpoint.cursor, &codec, &checkpoint.signer, &route)?;
        checkpoint.codec = codec;
        checkpoint.layout = Layout::InstalledV2;
        Ok(checkpoint)
    }

    pub(super) fn admit_installed(
        &mut self,
        origin: Origin,
        record: RawRecord<'_>,
        wall: u64,
        offer: Offer,
    ) -> Result<Disposition> {
        let signer = &self.signer;
        let codec = &self.codec;
        let [route] = self.routes.as_slice() else {
            return Err(Error::Source);
        };
        let key = Selector::from(&offer);
        let reserved = self.policy.reserved.contains(&key.server);
        let result = self
            .cursor
            .commit_trusted_exact(origin, record, wall, |tx| {
                verify(tx, signer.public_key(), origin)?;
                sweep(tx, wall)?;
                let outcome = rows::admit(tx, origin, record, &offer, reserved, None)?;
                if outcome == TrustedOutcome::Applied {
                    mark(tx, origin, &offer)?;
                }
                verify(tx, signer.public_key(), origin)?;
                check_parent(tx, codec, signer, route, key)?;
                Ok(outcome)
            });
        self.finish_admission(origin, record, wall, &offer, result)
    }

    pub(crate) fn installed_live_offer(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
    ) -> Result<Offer> {
        self.with_installed_offer(source, key, wall, |codec, signer, route, row| {
            if wall >= row.expires {
                return Err(Error::Expired);
            }
            Self::check_signed_offer(codec, signer, route, &row, wall)
        })
    }

    pub(crate) fn installed_historical_offer(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
    ) -> Result<HistoricalOffer> {
        self.with_installed_offer(source, key, wall, |codec, signer, route, row| {
            verify_saved(codec, signer, route, &row)?;
            Ok(HistoricalOffer {
                selector: row.key,
                expires: row.expires,
            })
        })
    }

    fn with_installed_offer<T>(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
        read: impl FnOnce(&Codec, &Keys, &Route, Invitation) -> Result<T>,
    ) -> Result<T> {
        if self.layout != Layout::InstalledV2 {
            return Err(Error::Layout);
        }
        let origin = self.origin(source)?;
        let route = self.routes.first().ok_or(Error::Source)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::check_wall(&tx, wall)?;
        verify(&tx, self.signer.public_key(), origin)?;
        let row = Invitation::load(&tx, key, origin)?;
        if wall >= row.replay_until || !self.policy.permits(row.key.server) {
            return Err(if wall >= row.replay_until {
                Error::Expired
            } else {
                Error::Policy
            });
        }
        read(&self.codec, &self.signer, route, row)
    }
}
