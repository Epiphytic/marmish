//! Exact custody epoch layered on the Prepared Consent inventory.

use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::OnceLock;

#[cfg(test)]
use super::super::super::super::super::MAX_ORIGINS;
use super::super::super::super::super::{CursorStore, HardenedPeerConnection, check_hardening};
use super::super::super::super::installed;
use super::super::super::layout_inventory::{Entry, inventory};
use super::super::super::stage_schema;
use super::*;
use nostr::key::{Keys, PublicKey};

const INSTALLED_LAYOUT: &str = "
CREATE TABLE peer_installed_custody_v2 (
    version INTEGER PRIMARY KEY CHECK (version=2)
) STRICT;
INSERT INTO peer_installed_custody_v2 VALUES (2);
CREATE TRIGGER peer_installed_custody_insert BEFORE INSERT ON peer_installed_custody_v2
BEGIN SELECT RAISE(ABORT,'immutable installed custody version'); END;
CREATE TRIGGER peer_installed_custody_update BEFORE UPDATE ON peer_installed_custody_v2
BEGIN SELECT RAISE(ABORT,'immutable installed custody version'); END;
CREATE TRIGGER peer_installed_custody_delete BEFORE DELETE ON peer_installed_custody_v2
BEGIN SELECT RAISE(ABORT,'immutable installed custody version'); END;
CREATE TRIGGER peer_installed_custody_pending_only BEFORE UPDATE ON peer_pending_custody
WHEN NEW.state>2
BEGIN SELECT RAISE(ABORT,'installed custody ends at Pending'); END;
";

#[cfg(test)]
const APPROVAL_LAYOUT: &str = "
CREATE TABLE peer_installed_custody_approval_v3 (
    version INTEGER PRIMARY KEY CHECK (version=3)
) STRICT;
INSERT INTO peer_installed_custody_approval_v3 VALUES (3);
CREATE TRIGGER peer_installed_custody_insert BEFORE INSERT ON peer_installed_custody_approval_v3
BEGIN SELECT RAISE(ABORT,'immutable installed custody version'); END;
CREATE TRIGGER peer_installed_custody_update BEFORE UPDATE ON peer_installed_custody_approval_v3
BEGIN SELECT RAISE(ABORT,'immutable installed custody version'); END;
CREATE TRIGGER peer_installed_custody_delete BEFORE DELETE ON peer_installed_custody_approval_v3
BEGIN SELECT RAISE(ABORT,'immutable installed custody version'); END;
CREATE TRIGGER peer_installed_custody_manual_approval BEFORE UPDATE ON peer_pending_custody
WHEN NEW.state=3 AND (OLD.class IS NOT 1 OR OLD.mode IS NOT 0 OR NEW.approval IS NOT 0)
BEGIN SELECT RAISE(ABORT,'installed custody requires Manual approval'); END;
";

const DDL: &str = "
CREATE TABLE peer_pending_custody_epoch (
    epoch INTEGER PRIMARY KEY CHECK (epoch=3)
) STRICT;
INSERT INTO peer_pending_custody_epoch VALUES (3);
CREATE TRIGGER peer_custody_epoch_insert BEFORE INSERT ON peer_pending_custody_epoch
BEGIN SELECT RAISE(ABORT,'immutable custody epoch'); END;
CREATE TRIGGER peer_custody_epoch_update BEFORE UPDATE ON peer_pending_custody_epoch
BEGIN SELECT RAISE(ABORT,'immutable custody epoch'); END;
CREATE TRIGGER peer_custody_epoch_delete BEFORE DELETE ON peer_pending_custody_epoch
BEGIN SELECT RAISE(ABORT,'immutable custody epoch'); END;

CREATE TABLE peer_pending_custody (
    class INTEGER PRIMARY KEY CHECK (class IN (0,1)),
    generation INTEGER NOT NULL CHECK (generation>0),
    account BLOB NOT NULL CHECK (length(account)=32),
    server BLOB NOT NULL UNIQUE CHECK (length(server)=32 AND server!=account),
    offer_id BLOB NOT NULL CHECK (length(offer_id)=16),
    offer_revision BLOB NOT NULL CHECK (length(offer_revision)=32),
    origin BLOB NOT NULL CHECK (length(origin)=32),
    consent_carrier BLOB NOT NULL UNIQUE CHECK (length(consent_carrier)=32)
        REFERENCES peer_prepared_consent_outbox(carrier) ON DELETE RESTRICT ON UPDATE RESTRICT,
    consent_revision BLOB NOT NULL CHECK (length(consent_revision)=32),
    delivery_carrier BLOB NOT NULL UNIQUE CHECK (length(delivery_carrier)=32),
    outer_stamp INTEGER NOT NULL CHECK (outer_stamp>0),
    flight_created INTEGER NOT NULL CHECK (flight_created>0),
    expires INTEGER NOT NULL CHECK (expires>flight_created),
    frame BLOB NOT NULL CHECK (length(frame) BETWEEN 1 AND 65536),
    replay_until INTEGER NOT NULL CHECK (replay_until>expires),
    welcome_id BLOB NOT NULL CHECK (length(welcome_id)=32),
    welcome_digest BLOB NOT NULL CHECK (length(welcome_digest)=32),
    package_id BLOB NOT NULL CHECK (length(package_id)=32),
    package_digest BLOB NOT NULL CHECK (length(package_digest)=32),
    package_ref BLOB NOT NULL CHECK (length(package_ref)=32),
    package_slot BLOB NOT NULL CHECK (length(package_slot)=16),
    package_not_before INTEGER NOT NULL CHECK (package_not_before>=0),
    package_not_after INTEGER NOT NULL CHECK (package_not_after>package_not_before),
    mode INTEGER NOT NULL CHECK (mode IN (0,1)),
    held_at INTEGER NOT NULL CHECK (held_at>0 AND held_at>=package_not_before),
    held_deadline INTEGER NOT NULL CHECK (held_deadline>held_at AND
        held_deadline=min(expires,package_not_after)-300),
    state INTEGER NOT NULL CHECK (state IN (0,1,2,3)),
    started_at INTEGER CHECK (started_at>=held_at AND started_at<held_deadline),
    group_id BLOB CHECK (length(group_id) BETWEEN 1 AND 255),
    epoch_number INTEGER CHECK (epoch_number>=0),
    epoch_identity BLOB CHECK (length(epoch_identity)=32),
    own_leaf INTEGER CHECK (own_leaf BETWEEN 0 AND 4294967295),
    qualified_at INTEGER CHECK (qualified_at>=started_at AND qualified_at<held_deadline),
    pending_deadline INTEGER CHECK (pending_deadline>qualified_at AND
        pending_deadline=min(held_deadline,qualified_at+86400)),
    approval INTEGER CHECK (approval IN (0,1) AND (approval=0 OR mode=1)),
    activating_at INTEGER CHECK (activating_at>=qualified_at AND activating_at<pending_deadline),
    CHECK ((state=0 AND started_at IS NULL) OR (state>0 AND started_at IS NOT NULL)),
    CHECK ((state<2 AND group_id IS NULL AND epoch_number IS NULL AND epoch_identity IS NULL
        AND own_leaf IS NULL AND qualified_at IS NULL AND pending_deadline IS NULL)
      OR (state>=2 AND group_id IS NOT NULL AND epoch_number IS NOT NULL
        AND epoch_identity IS NOT NULL AND own_leaf IS NOT NULL AND qualified_at IS NOT NULL
        AND pending_deadline IS NOT NULL)),
    CHECK ((state<3 AND approval IS NULL AND activating_at IS NULL)
      OR (state=3 AND approval IS NOT NULL AND activating_at IS NOT NULL))
) STRICT;

CREATE TRIGGER peer_custody_insert_held BEFORE INSERT ON peer_pending_custody
WHEN NEW.state!=0
BEGIN SELECT RAISE(ABORT,'custody starts Held'); END;

CREATE TRIGGER peer_custody_next BEFORE UPDATE ON peer_pending_custody
WHEN NEW.state!=OLD.state+1 OR OLD.state=3
    OR EXISTS (SELECT 1 FROM peer_pending_terminal t WHERE t.class=OLD.class AND t.generation=OLD.generation)
    OR OLD.class IS NOT NEW.class OR OLD.generation IS NOT NEW.generation
    OR OLD.account IS NOT NEW.account OR OLD.server IS NOT NEW.server
    OR OLD.offer_id IS NOT NEW.offer_id OR OLD.offer_revision IS NOT NEW.offer_revision
    OR OLD.origin IS NOT NEW.origin OR OLD.consent_carrier IS NOT NEW.consent_carrier
    OR OLD.consent_revision IS NOT NEW.consent_revision
    OR OLD.delivery_carrier IS NOT NEW.delivery_carrier OR OLD.outer_stamp IS NOT NEW.outer_stamp
    OR OLD.flight_created IS NOT NEW.flight_created OR OLD.expires IS NOT NEW.expires
    OR OLD.frame IS NOT NEW.frame OR OLD.replay_until IS NOT NEW.replay_until
    OR OLD.welcome_id IS NOT NEW.welcome_id OR OLD.welcome_digest IS NOT NEW.welcome_digest
    OR OLD.package_id IS NOT NEW.package_id OR OLD.package_digest IS NOT NEW.package_digest
    OR OLD.package_ref IS NOT NEW.package_ref OR OLD.package_slot IS NOT NEW.package_slot
    OR OLD.package_not_before IS NOT NEW.package_not_before
    OR OLD.package_not_after IS NOT NEW.package_not_after
    OR OLD.mode IS NOT NEW.mode OR OLD.held_at IS NOT NEW.held_at
    OR OLD.held_deadline IS NOT NEW.held_deadline
    OR (OLD.state>=1 AND OLD.started_at IS NOT NEW.started_at)
    OR (OLD.state>=2 AND (OLD.group_id IS NOT NEW.group_id
        OR OLD.epoch_number IS NOT NEW.epoch_number OR OLD.epoch_identity IS NOT NEW.epoch_identity
        OR OLD.own_leaf IS NOT NEW.own_leaf OR OLD.qualified_at IS NOT NEW.qualified_at
        OR OLD.pending_deadline IS NOT NEW.pending_deadline))
BEGIN SELECT RAISE(ABORT,'exact custody transition required'); END;
";

// Prepared epoch verification checks this outbox against the exact existing
// account, origin, slot, invitation and stage. This predicate pins the remaining
// custody coordinates and positive settlement to that same outbox.
const EXACT_OUTBOX: &str = "
    SELECT 1 FROM peer_prepared_consent_outbox o
    WHERE o.carrier=c.consent_carrier AND o.revision=c.consent_revision
        AND o.class=c.class AND o.generation=c.generation AND o.account=c.account
        AND o.server=c.server AND o.offer_id=c.offer_id AND o.offer_revision=c.offer_revision
        AND o.origin=c.origin AND o.package_id=c.package_id AND o.package_digest=c.package_digest
        AND o.package_ref=c.package_ref AND o.package_slot=c.package_slot
        AND c.expires<=o.expires AND c.flight_created>=o.created_at
        AND c.held_at<o.expires AND o.package_expires=c.package_not_after
        AND o.ack_at IS NOT NULL AND o.ack_at<=c.held_at";

#[derive(Clone, Copy)]
pub(super) enum Writer {
    Insert,
    Update,
    Delete,
}

impl Writer {
    fn verb(self) -> &'static str {
        match self {
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
            Self::Delete => "DELETE",
        }
    }

    // DDL is transactional. If the write or post-check fails, the owner turns
    // roll back both the write and the temporary removal of the denial trigger.
    pub(super) fn guard(self, tx: &Transaction<'_>, install: bool) -> Result<()> {
        let verb = self.verb();
        if install {
            install_guard(tx, verb)?;
        } else {
            tx.execute_batch(&format!("DROP TRIGGER peer_custody_deny_{verb}"))?;
        }
        Ok(())
    }
}

fn install_guard(conn: &Connection, verb: &str) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "CREATE TRIGGER peer_custody_deny_{verb} BEFORE {verb} ON peer_pending_custody
         BEGIN SELECT RAISE(ABORT,'custody owner required'); END"
    ))
}

fn install_common(conn: &Connection, ddl: &str) -> std::result::Result<(), CursorError> {
    conn.execute_batch(ddl)?;
    let predicate = EXACT_OUTBOX.replace("c.", "NEW.");
    for writer in [Writer::Insert, Writer::Update] {
        let verb = writer.verb();
        conn.execute_batch(&format!(
            "CREATE TRIGGER peer_custody_reference_{verb} BEFORE {verb} ON peer_pending_custody
             WHEN NOT EXISTS ({predicate})
             BEGIN SELECT RAISE(ABORT,'exact acknowledged Consent required'); END"
        ))?;
        install_guard(conn, verb)?;
    }
    install_guard(conn, "DELETE")?;
    Ok(())
}

#[cfg(test)]
pub(super) fn install_epoch_two_for_test(
    conn: &Connection,
) -> std::result::Result<(), CursorError> {
    let ddl = DDL.replace("epoch=3", "epoch=2").replace("VALUES (3);", "VALUES (2);").replace(
        "\n    OR EXISTS (SELECT 1 FROM peer_pending_terminal t WHERE t.class=OLD.class AND t.generation=OLD.generation)",
        "",
    );
    install_common(conn, &ddl)
}

fn install(conn: &Connection) -> std::result::Result<(), CursorError> {
    install_common(conn, DDL)?;
    super::super::super::cooldown::install(conn).map_err(super::schema::cursor_error)?;
    super::terminal_schema::install(conn).map_err(super::schema::cursor_error)?;
    Ok(())
}

fn expected(kind: Kind) -> std::result::Result<&'static [Entry], CursorError> {
    #[cfg(test)]
    static PUBLICATION: OnceLock<Vec<Entry>> = OnceLock::new();
    #[cfg(test)]
    static TERMINAL_LEAVE: OnceLock<Vec<Entry>> = OnceLock::new();
    #[cfg(test)]
    static LEAVE_EFFECT: OnceLock<Vec<Entry>> = OnceLock::new();
    static INSTALLED_V2: OnceLock<Vec<Entry>> = OnceLock::new();
    #[cfg(test)]
    static APPROVAL_V3: OnceLock<Vec<Entry>> = OnceLock::new();
    static CUSTODY: OnceLock<Vec<Entry>> = OnceLock::new();
    let expected = match kind {
        Kind::Custody => &CUSTODY,
        Kind::Installed(InstalledKind::V2) => &INSTALLED_V2,
        #[cfg(test)]
        Kind::Installed(InstalledKind::ApprovalV3) => &APPROVAL_V3,
        #[cfg(test)]
        Kind::Publication => &PUBLICATION,
        #[cfg(test)]
        Kind::TerminalLeave => &TERMINAL_LEAVE,
        #[cfg(test)]
        Kind::LeaveEffect => &LEAVE_EFFECT,
    };
    if let Some(entries) = expected.get() {
        return Ok(entries);
    }
    let template = super::schema::template()?;
    install(&template)?;
    if let Kind::Installed(installed_kind) = kind {
        installed::install(&template)?;
        template.execute_batch(installed_kind.ddl())?;
    }
    #[cfg(test)]
    if Epoch(kind).has_publication_inventory() {
        super::activation_schema::install(&template)?;
    }
    #[cfg(test)]
    if Epoch(kind).is_terminal_leave() {
        super::leave_schema::install(&template)?;
    }
    #[cfg(test)]
    if Epoch(kind).is_leave_effect() {
        super::leave_effect_schema::install(&template)?;
    }
    let entries = inventory(&template).map_err(super::schema::cursor_error)?;
    Ok(expected.get_or_init(|| entries))
}

#[derive(Clone, Copy)]
pub(crate) struct Epoch(Kind);

#[derive(Clone, Copy)]
enum Kind {
    Custody,
    Installed(InstalledKind),
    #[cfg(test)]
    Publication,
    #[cfg(test)]
    TerminalLeave,
    #[cfg(test)]
    LeaveEffect,
}

#[derive(Clone, Copy)]
pub(super) enum InstalledKind {
    V2,
    #[cfg(test)]
    ApprovalV3,
}

impl InstalledKind {
    pub(super) fn ddl(self) -> &'static str {
        match self {
            Self::V2 => INSTALLED_LAYOUT,
            #[cfg(test)]
            Self::ApprovalV3 => APPROVAL_LAYOUT,
        }
    }

    fn other_marker(self) -> &'static str {
        match self {
            Self::V2 => "peer_installed_custody_approval_v3",
            #[cfg(test)]
            Self::ApprovalV3 => "peer_installed_custody_v2",
        }
    }
}

impl Epoch {
    pub(crate) fn is_installed_v2(self) -> bool {
        matches!(self.0, Kind::Installed(InstalledKind::V2))
    }

    #[cfg(test)]
    pub(super) fn is_installed_family(self) -> bool {
        matches!(self.0, Kind::Installed(_))
    }

    #[cfg(test)]
    pub(super) fn is_publication(self) -> bool {
        matches!(self.0, Kind::Publication | Kind::TerminalLeave)
    }

    #[cfg(test)]
    fn has_publication_inventory(self) -> bool {
        self.is_publication() || self.is_leave_effect()
    }

    #[cfg(test)]
    pub(super) fn is_terminal_leave(self) -> bool {
        matches!(self.0, Kind::TerminalLeave | Kind::LeaveEffect)
    }

    #[cfg(test)]
    pub(super) fn is_leave_effect(self) -> bool {
        matches!(self.0, Kind::LeaveEffect)
    }

    pub(crate) fn verify(
        self,
        tx: &Transaction<'_>,
        account: [u8; 32],
        origins: &[Origin],
    ) -> std::result::Result<(), CursorError> {
        verify_hardening(tx)?;
        if inventory(tx).map_err(super::schema::cursor_error)? != expected(self.0)? {
            return Err(CursorError::Binding);
        }
        super::schema::verify_rows(tx, account, origins)?;
        let marker: i64 = tx.query_row(
            "SELECT count(*) FROM peer_pending_custody_epoch WHERE epoch=3",
            [],
            |row| row.get(0),
        )?;
        let broken: bool = tx.query_row(
            &format!(
                "SELECT EXISTS(
                    SELECT 1 FROM peer_pending_custody c
                    WHERE c.account!=?1 OR NOT EXISTS ({EXACT_OUTBOX})
                )"
            ),
            [account],
            |row| row.get(0),
        )?;
        if marker != 1 || broken {
            return Err(CursorError::Binding);
        }
        super::terminal_schema::verify_rows(tx, account).map_err(super::schema::cursor_error)?;
        if self.is_installed_v2() {
            let [origin] = origins else {
                return Err(CursorError::Binding);
            };
            let signer = PublicKey::from_slice(&account).map_err(|_| CursorError::Binding)?;
            installed::verify_rows(tx, signer, *origin)?;
            let closed: bool = tx.query_row(
                "SELECT (SELECT count(*) FROM peer_installed_custody_v2 WHERE version=2)=1
                    AND NOT EXISTS (SELECT 1 FROM peer_pending_custody WHERE state>2)
                    AND NOT EXISTS (SELECT 1 FROM peer_pending_terminal)",
                [],
                |row| row.get(0),
            )?;
            if !closed {
                return Err(CursorError::Binding);
            }
        }
        #[cfg(test)]
        if matches!(self.0, Kind::Installed(InstalledKind::ApprovalV3)) {
            let [origin] = origins else {
                return Err(CursorError::Binding);
            };
            let signer = PublicKey::from_slice(&account).map_err(|_| CursorError::Binding)?;
            installed::verify_rows(tx, signer, *origin)?;
            verify_approval_rows(tx)?;
        }
        #[cfg(test)]
        if self.has_publication_inventory() {
            super::activation_schema::verify_rows(tx, account)
                .map_err(super::schema::cursor_error)?;
        }
        #[cfg(test)]
        if self.is_terminal_leave() {
            super::leave_schema::verify_rows(tx, account).map_err(super::schema::cursor_error)?;
        }
        #[cfg(test)]
        if self.is_leave_effect() {
            super::leave_effect_schema::verify_rows(tx)?;
        }
        Ok(())
    }
}

#[cfg(all(
    test,
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
#[allow(
    dead_code,
    reason = "the standalone cursor target has no offline native stage"
)]
pub(crate) fn fixture_pending_stage(
    tx: &Transaction<'_>,
    account: PublicKey,
    origin: Origin,
    (key, generation, held_at, deadline): (Selector, u64, u64, u64),
) -> Result<(Binding, FileWitness)> {
    Epoch(Kind::Installed(InstalledKind::V2)).verify(tx, account.to_bytes(), &[origin])?;
    let exact: bool = tx.query_row(
        "SELECT (SELECT count(*) FROM peer_pending_custody)=1
           AND (SELECT count(*) FROM peer_provisional_stage_v1)=1
           AND EXISTS(SELECT 1 FROM peer_pending_custody WHERE class=0
               AND generation=?1 AND account=?2 AND server=?3 AND offer_id=?4
               AND offer_revision=?5 AND origin=?6 AND held_at=?7
               AND pending_deadline=?8 AND mode=0 AND state=2)",
        params![
            seconds(generation)?,
            account.to_bytes(),
            key.server.to_bytes(),
            key.id,
            key.revision,
            origin,
            seconds(held_at)?,
            seconds(deadline)?
        ],
        |row| row.get(0),
    )?;
    if !exact {
        return Err(CursorError::Binding.into());
    }
    let binding = Binding {
        key,
        origin,
        generation: Generation {
            known: false,
            count: generation,
        },
    };
    match stage_journal::load(tx, binding)? {
        Some(State::Prepared(witness, _))
            if witness.device != [0; 8] && witness.inode != [0; 8] =>
        {
            Ok((binding, witness))
        }
        _ => Err(CursorError::Binding.into()),
    }
}

#[cfg(test)]
pub(super) fn verify_approval_rows(tx: &Connection) -> std::result::Result<(), CursorError> {
    let valid: bool = tx.query_row(
        "SELECT (SELECT count(*) FROM peer_installed_custody_approval_v3 WHERE version=3)=1
            AND NOT EXISTS (SELECT 1 FROM peer_pending_terminal)
            AND NOT EXISTS (
                SELECT 1 FROM peer_pending_custody WHERE state=3
                    AND (class IS NOT 1 OR mode IS NOT 0 OR approval IS NOT 0
                        OR length(group_id) IS NULL OR length(group_id) NOT BETWEEN 1 AND 255
                        OR epoch_number IS NULL OR epoch_number<0
                        OR length(epoch_identity) IS NOT 32 OR own_leaf IS NULL
                        OR own_leaf NOT BETWEEN 0 AND 4294967295
                        OR qualified_at IS NULL OR pending_deadline IS NULL
                        OR activating_at IS NULL OR activating_at<qualified_at
                        OR activating_at>=pending_deadline)
            )",
        [],
        |row| row.get(0),
    )?;
    if !valid {
        return Err(CursorError::Binding);
    }
    Ok(())
}

fn verify_hardening(conn: &Connection) -> std::result::Result<(), CursorError> {
    check_hardening(conn)?;
    let enforcing: i64 = conn.query_row("PRAGMA ignore_check_constraints", [], |row| row.get(0))?;
    if enforcing != 0 {
        return Err(CursorError::Storage);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn open(
    HardenedPeerConnection(mut conn): HardenedPeerConnection,
    account: [u8; 32],
    mut origins: Vec<Origin>,
) -> std::result::Result<(CursorStore, Epoch), CursorError> {
    if !(1..=MAX_ORIGINS).contains(&origins.len()) {
        return Err(CursorError::Binding);
    }
    origins.sort_unstable();
    verify_hardening(&conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if inventory(&tx).map_err(super::schema::cursor_error)? != expected(Kind::Custody)? {
        super::schema::verify(&tx, account, &origins)?;
        install(&tx)?;
    }
    let epoch = Epoch(Kind::Custody);
    epoch.verify(&tx, account, &origins)?;
    tx.commit()?;
    Ok((CursorStore { conn, origins }, epoch))
}

pub(super) fn open_installed(
    HardenedPeerConnection(mut conn): HardenedPeerConnection,
    signer: &Keys,
    route: &super::super::super::super::Route,
    codec: &wire::Codec,
    kind: InstalledKind,
) -> Result<(CursorStore, Epoch)> {
    let account = signer.public_key();
    let origins = vec![route.id];
    verify_hardening(&conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let other_present: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_schema WHERE lower(name)=?1)",
        [kind.other_marker()],
        |row| row.get(0),
    )?;
    if other_present {
        return Err(Error::Layout);
    }
    let epoch = Epoch(Kind::Installed(kind));
    if inventory(&tx).map_err(super::schema::cursor_error)? != expected(epoch.0)? {
        installed::verify(&tx, account, route.id)?;
        installed::verify_recovered_parents(&tx, codec, signer, route)?;
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM peer_provisioning_slot
                WHERE generation!=0 OR next_start_at!=0 OR server IS NOT NULL
                   OR offer_id IS NOT NULL OR revision IS NOT NULL OR origin IS NOT NULL)",
            [],
            |row| row.get(0),
        )?;
        if used {
            return Err(CursorError::Binding.into());
        }
        stage_schema::attach(&tx, true)?;
        super::schema::install(&tx)?;
        install(&tx)?;
        tx.execute_batch(kind.ddl())?;
    }
    epoch.verify(&tx, account.to_bytes(), &origins)?;
    installed::verify_recovered_parents(&tx, codec, signer, route)?;
    tx.commit()?;
    Ok((CursorStore { conn, origins }, epoch))
}

#[cfg(test)]
pub(super) fn open_publication(
    HardenedPeerConnection(mut conn): HardenedPeerConnection,
    account: [u8; 32],
    mut origins: Vec<Origin>,
) -> std::result::Result<(CursorStore, Epoch), CursorError> {
    if !(1..=MAX_ORIGINS).contains(&origins.len()) {
        return Err(CursorError::Binding);
    }
    origins.sort_unstable();
    verify_hardening(&conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let epoch = Epoch(Kind::Publication);
    if inventory(&tx).map_err(super::schema::cursor_error)? != expected(Kind::Publication)? {
        Epoch(Kind::Custody).verify(&tx, account, &origins)?;
        super::activation_schema::install(&tx)?;
    }
    epoch.verify(&tx, account, &origins)?;
    tx.commit()?;
    Ok((CursorStore { conn, origins }, epoch))
}

#[cfg(test)]
pub(super) fn open_terminal_leave(
    conn: HardenedPeerConnection,
    account: [u8; 32],
    origin: Origin,
) -> std::result::Result<(CursorStore, Epoch), CursorError> {
    open_leave_epoch(conn, account, origin, Kind::TerminalLeave)
}

#[cfg(test)]
pub(super) fn open_leave_effect(
    conn: HardenedPeerConnection,
    account: [u8; 32],
    origin: Origin,
) -> std::result::Result<(CursorStore, Epoch), CursorError> {
    open_leave_epoch(conn, account, origin, Kind::LeaveEffect)
}

#[cfg(test)]
fn open_leave_epoch(
    HardenedPeerConnection(mut conn): HardenedPeerConnection,
    account: [u8; 32],
    origin: Origin,
    kind: Kind,
) -> std::result::Result<(CursorStore, Epoch), CursorError> {
    let origins = vec![origin];
    verify_hardening(&conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let epoch = Epoch(kind);
    if inventory(&tx).map_err(super::schema::cursor_error)? != expected(epoch.0)? {
        super::schema::verify(&tx, account, &origins)?;
        install(&tx)?;
        super::activation_schema::install(&tx)?;
        super::leave_schema::install(&tx)?;
        if epoch.is_leave_effect() {
            super::leave_effect_schema::install(&tx)?;
        }
    }
    epoch.verify(&tx, account, &origins)?;
    tx.commit()?;
    Ok((CursorStore { conn, origins }, epoch))
}
