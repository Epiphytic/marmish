//! Durable client consent checkpoint. Source, UID and wall qualify outside this seam.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::peer_control_version::ControlVersion;
use marmish_core::RelaySet;
use nostr::event::Event;
use nostr::key::{Keys, PublicKey};

use super::{
    Auth, CursorStore, Error as CursorError, MAX_ORIGINS, Metadata, Origin, RawRecord, Receipt,
    Sha256, TrustedOutcome,
};
use sha2::Digest as _;
use zeroize::Zeroizing;

#[path = "peer_installed_offer_store.rs"]
mod installed;
#[path = "peer_provisioning.rs"]
pub(crate) mod provisioning;
#[path = "peer_consent_checkpoint_rows.rs"]
mod rows;
#[cfg(test)]
#[path = "peer_consent_checkpoint_tests.rs"]
mod tests;
#[path = "peer_control_wire.rs"]
pub(crate) mod wire;

use rows::Invitation;
pub(crate) use rows::Outbox;
use wire::{Codec, MAX_OUTER_BYTES, Offer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Source,
    LocalOwner,
    Index,
    Policy,
    Missing,
    Expired,
    Layout,
    Package,
    #[cfg(test)]
    Bound,
    #[cfg(test)]
    Custodied,
    #[cfg(test)]
    Retiring,
    #[cfg(test)]
    NotExpired,
    Cooldown,
    Cursor(CursorError),
}

impl From<CursorError> for Error {
    fn from(error: CursorError) -> Self {
        Self::Cursor(error)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        CursorError::from(error).into()
    }
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Default)]
pub(crate) enum Mode {
    #[default]
    Manual,
    Automatic,
    Off,
}

#[derive(Default)]
pub(crate) struct Policy {
    pub(crate) mode: Mode,
    pub(crate) reserved: BTreeSet<PublicKey>,
    pub(crate) blocked: BTreeSet<PublicKey>,
}

impl Policy {
    fn permits(&self, server: PublicKey) -> bool {
        !matches!(self.mode, Mode::Off) && !self.blocked.contains(&server)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selector {
    pub(crate) server: PublicKey,
    pub(crate) id: [u8; 16],
    pub(crate) revision: [u8; 32],
}

impl From<&Offer> for Selector {
    fn from(offer: &Offer) -> Self {
        let flight = offer.flight();
        Self {
            server: flight.author,
            id: offer.offer_id(),
            revision: flight.revision.to_bytes(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Accepted(TrustedOutcome),
    Deferred,
    Invalid,
}

pub(crate) struct Disposition {
    pub(crate) status: Status,
    pub(crate) receipt: Receipt,
}

pub(crate) struct Source {
    owner: Arc<()>,
    pub(crate) origin: Origin,
}

pub(crate) struct Local {
    owner: Arc<()>,
}

pub(crate) struct Route {
    pub(crate) id: Origin,
    pub(crate) urls: [String; 1],
}

impl Route {
    pub(crate) fn configured(value: &str) -> Result<Self> {
        let urls = [value.to_owned()];
        let canonical = RelaySet::parse(&urls, true).map_err(|_| Error::Source)?;
        if canonical.as_slice() != urls || !value.starts_with("wss://") {
            return Err(Error::Source);
        }
        let mut hash = Sha256::new();
        hash.update(b"marmish/peer/checkpoint/origin/1\0");
        hash.update(value.as_bytes());
        Ok(Self {
            id: hash.finalize().into(),
            urls,
        })
    }
}

pub(crate) struct Material<'a> {
    pub(crate) origin: Origin,
    pub(crate) selector: Selector,
    pub(crate) package: &'a [u8],
    pub(crate) expires: u64,
}

pub(crate) enum Work {
    Pending,
    NeedPackage { origin: Origin },
    Queued(Outbox),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Legacy,
    InstalledV2,
}

pub(crate) struct Checkpoint {
    pub(crate) cursor: CursorStore,
    signer: Keys,
    codec: Codec,
    layout: Layout,
    owner: Arc<()>,
    owner_uid: u32,
    policy: Policy,
    routes: Vec<Route>,
    #[cfg(test)]
    provisional_schema: bool,
    #[cfg(test)]
    retirement_epoch: bool,
    #[cfg(test)]
    export_epoch: Option<provisioning::prepared_consent::Epoch>,
    #[cfg(test)]
    retirement_fault: Option<provisioning::retirement::Fault>,
}

impl Checkpoint {
    #[cfg(test)]
    pub(crate) fn fixture_statement(&self, sql: &str) {
        self.cursor.conn.execute_batch(sql).unwrap();
    }

    #[cfg(test)]
    pub(crate) fn fixture_try_statement(
        &self,
        sql: &str,
        identity: Option<&[u8; 32]>,
    ) -> rusqlite::Result<usize> {
        match identity {
            Some(identity) => self.cursor.conn.execute(sql, [identity]),
            None => self.cursor.conn.execute(sql, []),
        }
    }

    pub(crate) fn attach(
        cursor: CursorStore,
        signer: Keys,
        owner_uid: u32,
        urls: &[&str],
        policy: Policy,
    ) -> Result<Self> {
        Self::attach_with(cursor, signer, owner_uid, urls, policy, rows::attach)
    }

    fn attach_with(
        mut cursor: CursorStore,
        signer: Keys,
        owner_uid: u32,
        urls: &[&str],
        policy: Policy,
        open_rows: impl FnOnce(&mut CursorStore, PublicKey) -> std::result::Result<(), CursorError>,
    ) -> Result<Self> {
        if !(1..=MAX_ORIGINS).contains(&urls.len()) {
            return Err(Error::Source);
        }
        let routes: Vec<_> = urls
            .iter()
            .map(|url| Route::configured(url))
            .collect::<Result<_>>()?;
        let ids = routes.iter().map(|route| route.id).collect::<BTreeSet<_>>();
        if ids.len() != routes.len() || !ids.iter().eq(&cursor.origins) {
            return Err(Error::Source);
        }
        open_rows(&mut cursor, signer.public_key())?;
        Ok(Self {
            cursor,
            signer,
            codec: Codec::new(MAX_OUTER_BYTES).map_err(|_| Error::Cursor(CursorError::Range))?,
            layout: Layout::Legacy,
            owner: Arc::new(()),
            owner_uid,
            policy,
            routes,
            #[cfg(test)]
            provisional_schema: false,
            #[cfg(test)]
            retirement_epoch: false,
            #[cfg(test)]
            export_epoch: None,
            #[cfg(test)]
            retirement_fault: None,
        })
    }

    // Callers must already hold the authenticated socket owner.
    pub(crate) fn bind_source(&self, url: &str) -> Result<Source> {
        let route = self
            .routes
            .iter()
            .find(|route| route.urls[0] == url)
            .ok_or(Error::Source)?;
        Ok(Source {
            origin: route.id,
            owner: self.owner.clone(),
        })
    }

    // Callers must obtain this UID from a connected Unix stream.
    pub(crate) fn authorize_local(&self, verified_uid: u32) -> Result<Local> {
        if verified_uid != self.owner_uid {
            return Err(Error::LocalOwner);
        }
        Ok(Local {
            owner: self.owner.clone(),
        })
    }

    fn origin(&self, source: &Source) -> Result<Origin> {
        if !Arc::ptr_eq(&self.owner, &source.owner) {
            return Err(Error::Source);
        }
        self.route(source.origin)?;
        Ok(source.origin)
    }

    fn route(&self, origin: Origin) -> Result<&Route> {
        self.routes
            .iter()
            .find(|route| route.id == origin)
            .ok_or(Error::Source)
    }

    fn check_local(&self, local: &Local) -> Result<()> {
        self.check_local_owner(&local.owner)
    }

    fn check_local_owner(&self, owner: &Arc<()>) -> Result<()> {
        if !Arc::ptr_eq(&self.owner, owner) {
            return Err(Error::LocalOwner);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn admit_unclassified(
        &mut self,
        source: &Source,
        record: RawRecord<'_>,
        wall: u64,
    ) -> Result<Disposition> {
        let origin = self.origin(source)?;
        record.check()?;
        rows::check_wall(&self.cursor.conn, wall)?;
        Ok(Disposition {
            status: Status::Invalid,
            receipt: self.cursor.commit_invalid(origin, record, wall)?,
        })
    }

    fn require_legacy(&self) -> Result<()> {
        if self.layout != Layout::Legacy {
            return Err(Error::Layout);
        }
        Ok(())
    }

    fn cooldown_attached(&self) -> bool {
        #[cfg(test)]
        {
            self.retirement_epoch
                || matches!(
                    self.export_epoch,
                    Some(provisioning::prepared_consent::Epoch::Custody(_))
                )
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    pub(crate) fn admit(
        &mut self,
        source: &Source,
        record: RawRecord<'_>,
        wall: u64,
    ) -> Result<Disposition> {
        let origin = self.origin(source)?;
        record.check()?;
        rows::check_wall(&self.cursor.conn, wall)?;
        if self.layout == Layout::InstalledV2 {
            installed::verify(&self.cursor.conn, self.signer.public_key(), origin)?;
        }
        let Some(offer) = self.verify_offer(origin, record, wall)? else {
            return Ok(Disposition {
                status: Status::Invalid,
                receipt: self.cursor.commit_invalid(origin, record, wall)?,
            });
        };

        match self.layout {
            Layout::Legacy => self.admit_legacy(origin, record, wall, offer),
            Layout::InstalledV2 => self.admit_installed(origin, record, wall, offer),
        }
    }

    fn admit_legacy(
        &mut self,
        origin: Origin,
        record: RawRecord<'_>,
        wall: u64,
        offer: Offer,
    ) -> Result<Disposition> {
        let server = offer.flight().author;
        let reserved = self.policy.reserved.contains(&server);
        let automatic = matches!(self.policy.mode, Mode::Automatic);
        #[cfg(test)]
        let custody_epoch = matches!(
            self.export_epoch,
            Some(provisioning::prepared_consent::Epoch::Custody(_))
        );
        #[cfg(test)]
        let automatic = automatic && !custody_epoch;
        let decision_at = automatic.then_some(wall);
        #[cfg(test)]
        let retirement_epoch = self.retirement_epoch;
        #[cfg(test)]
        if retirement_epoch {
            provisioning::retirement::verify_epoch(&self.cursor.conn)?;
        }
        #[cfg(test)]
        let custody_context = match self.export_epoch {
            Some(provisioning::prepared_consent::Epoch::Custody(epoch)) => Some((
                epoch,
                self.signer.public_key().to_bytes(),
                self.cursor.origins.clone(),
            )),
            _ => None,
        };
        let cooldown_attached = self.cooldown_attached();
        let commit = CursorStore::commit_trusted;
        #[cfg(test)]
        let commit = if custody_epoch {
            CursorStore::commit_trusted_exact
        } else {
            commit
        };
        let result = commit(
            &mut self.cursor,
            origin,
            record,
            wall,
            |tx: &rusqlite::Transaction<'_>| {
                #[cfg(test)]
                if let Some((epoch, account, origins)) = &custody_context {
                    epoch.verify(tx, *account, origins)?;
                }
                if cooldown_attached
                    && provisioning::cooldown::under_cooldown(tx, server.to_bytes(), wall)?
                {
                    return Ok(TrustedOutcome::SkippedCooldown);
                }
                rows::sweep(tx, wall)?;
                rows::admit(tx, origin, record, &offer, reserved, decision_at)
            },
        );
        self.finish_admission(origin, record, wall, &offer, result)
    }

    fn finish_admission(
        &mut self,
        origin: Origin,
        record: RawRecord<'_>,
        wall: u64,
        offer: &Offer,
        result: std::result::Result<(Receipt, TrustedOutcome), CursorError>,
    ) -> Result<Disposition> {
        match result {
            Ok((receipt, outcome)) => Ok(Disposition {
                status: Status::Accepted(outcome),
                receipt,
            }),
            Err(CursorError::Backlog) => Ok(Disposition {
                status: Status::Deferred,
                receipt: self.cursor.defer(
                    origin,
                    record,
                    Metadata {
                        auth: Auth::FlightVerified,
                        expires: offer.flight().expires_at,
                        prerequisite: None,
                    },
                    wall,
                )?,
            }),
            Err(error) => Err(error.into()),
        }
    }

    fn verify_offer(
        &self,
        origin: Origin,
        record: RawRecord<'_>,
        wall: u64,
    ) -> Result<Option<Offer>> {
        let Ok(outer) = serde_json::from_slice::<Event>(record.bytes) else {
            return Ok(None);
        };
        if outer.verify().is_err() {
            return Ok(None);
        }
        if outer.id.to_bytes() != record.id || outer.created_at.as_secs() != record.stamp {
            return Err(Error::Index);
        }
        let route = self.route(origin)?;
        let Ok(offer) = self
            .codec
            .open_offer(&self.signer, record.bytes, None, &route.urls, wall)
        else {
            return Ok(None);
        };
        let flight = offer.flight();
        if flight.carrier_id != outer.id {
            return Err(Error::Index);
        }
        Ok((flight.author != flight.recipient
            && wall < flight.expires_at
            && self.policy.permits(flight.author))
        .then_some(offer))
    }

    pub(crate) fn approve(&mut self, local: &Local, key: Selector, wall: u64) -> Result<()> {
        self.require_legacy()?;
        let origin = self.authorized_row(local, key)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        let row = Invitation::load_unowned(&tx, key, origin)?;
        if wall >= row.expires {
            return Err(Error::Expired);
        }
        rows::sweep(&tx, wall)?;
        if row.decision_at.is_none() {
            row.approve(&tx, wall)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn discard(&mut self, local: &Local, key: Selector, wall: u64) -> Result<()> {
        self.require_legacy()?;
        self.check_local(local)?;
        let origin = rows::stored_origin(&self.cursor.conn, key)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        let row = Invitation::load_unowned(&tx, key, origin)?;
        rows::advance_wall(&tx, wall)?;
        row.remove(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn sweep(&mut self, source: &Source, wall: u64) -> Result<()> {
        let origin = self.origin(source)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        if self.layout == Layout::InstalledV2 {
            installed::verify(&tx, self.signer.public_key(), origin)?;
            installed::sweep(&tx, wall)?;
            installed::verify(&tx, self.signer.public_key(), origin)?;
        } else {
            rows::sweep(&tx, wall)?;
        }
        tx.commit()?;
        Ok(())
    }

    fn authorized_row(&self, local: &Local, key: Selector) -> Result<Origin> {
        self.check_local(local)?;
        if !self.policy.permits(key.server) {
            return Err(Error::Policy);
        }
        rows::stored_origin(&self.cursor.conn, key)
    }

    #[cfg(test)]
    fn work(
        &mut self,
        local: &Local,
        key: Selector,
        wall: u64,
        material: Option<Material<'_>>,
    ) -> Result<Work> {
        self.check_local(local)?;
        self.approved_work(key, wall, material)
    }

    pub(crate) fn approved_work(
        &mut self,
        key: Selector,
        wall: u64,
        material: Option<Material<'_>>,
    ) -> Result<Work> {
        self.require_legacy()?;
        if !self.policy.permits(key.server) {
            return Err(Error::Policy);
        }
        let origin = rows::stored_origin(&self.cursor.conn, key)?;
        let route = self
            .routes
            .iter()
            .find(|route| route.id == origin)
            .ok_or(Error::Source)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::check_wall(&tx, wall)?;
        let row = Invitation::load_unowned(&tx, key, origin)?;
        let outcome = if let Some(queued) = row.outbox(&tx)? {
            if wall >= queued.expires {
                return Err(Error::Expired);
            }
            Work::Queued(queued)
        } else {
            if wall >= row.expires {
                return Err(Error::Expired);
            }
            match (row.decision_at, material) {
                (None, _) => Work::Pending,
                (Some(_), None) => Work::NeedPackage { origin },
                (Some(_), Some(material)) => Work::Queued(Self::queue(
                    &self.codec,
                    &self.signer,
                    route,
                    &tx,
                    &row,
                    wall,
                    material,
                )?),
            }
        };
        rows::sweep(&tx, wall)?;
        tx.commit()?;
        Ok(outcome)
    }

    fn queue(
        codec: &Codec,
        signer: &Keys,
        route: &Route,
        tx: &rusqlite::Transaction<'_>,
        row: &Invitation,
        wall: u64,
        material: Material<'_>,
    ) -> Result<Outbox> {
        if material.origin != route.id || material.selector != row.key {
            return Err(Error::Package);
        }
        let signed_offer = Self::check_signed_offer(codec, signer, route, row, wall)?;
        let consent = codec
            .prepare_consent(
                signer,
                &signed_offer,
                material.package,
                wall,
                material.expires,
            )
            .map_err(|_| Error::Package)?;
        let flight = consent.flight.flight();
        let queued = Outbox {
            origin: route.id,
            carrier: flight.carrier_id.to_bytes(),
            revision: flight.revision.to_bytes(),
            expires: flight.expires_at,
            frame: Zeroizing::new(consent.frame().to_vec()),
            package_id: consent.flight.binding().event_id,
            package_digest: package_digest(consent.flight.package_bytes()),
            attempt: 0,
            retry_at: None,
            acknowledged_at: None,
        };
        row.save_outbox(tx, &queued)?;
        Ok(queued)
    }

    fn check_signed_offer(
        codec: &Codec,
        signer: &Keys,
        route: &Route,
        row: &Invitation,
        wall: u64,
    ) -> Result<Offer> {
        let signed_offer = codec
            .open_offer(signer, &row.frame, Some(row.key.server), &route.urls, wall)
            .map_err(|_| Error::Cursor(CursorError::Binding))?;
        let flight = signed_offer.flight();
        if flight.author == flight.recipient
            || Selector::from(&signed_offer) != row.key
            || flight.carrier_id.to_bytes() != row.carrier
            || flight.expires_at != row.expires
            || flight.replay_safe_until != row.replay_until
        {
            return Err(Error::Cursor(CursorError::Binding));
        }
        Ok(signed_offer)
    }
}

#[path = "peer_consent_publication.rs"]
mod publication;

fn package_digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
