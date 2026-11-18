//! Test-only client checkpoint. Source, UID and wall qualify outside this seam.

use std::collections::BTreeSet;
use std::sync::Arc;

use marmish_core::RelaySet;
use nostr::event::Event;
use nostr::key::{Keys, PublicKey};

use super::{
    Auth, CursorStore, Error as CursorError, MAX_ORIGINS, Metadata, Origin, RawRecord, Receipt,
    Sha256, TrustedOutcome,
};
use sha2::Digest as _;
use zeroize::Zeroizing;

#[path = "peer_consent_checkpoint_rows.rs"]
mod rows;
#[path = "peer_consent_checkpoint_tests.rs"]
mod tests;
#[path = "peer_control_wire.rs"]
mod wire;

use rows::{Invitation, Outbox};
use wire::{Codec, MAX_OUTER_BYTES, Offer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Error {
    Source,
    LocalOwner,
    Index,
    Policy,
    Missing,
    Expired,
    Package,
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

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Default)]
enum Mode {
    #[default]
    Manual,
    Automatic,
    Off,
}

#[derive(Default)]
struct Policy {
    mode: Mode,
    reserved: BTreeSet<PublicKey>,
    blocked: BTreeSet<PublicKey>,
}

impl Policy {
    fn permits(&self, server: PublicKey) -> bool {
        !matches!(self.mode, Mode::Off) && !self.blocked.contains(&server)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selector {
    server: PublicKey,
    id: [u8; 16],
    revision: [u8; 32],
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
enum Status {
    Accepted(TrustedOutcome),
    Deferred,
    Invalid,
}

struct Disposition {
    status: Status,
    receipt: Receipt,
}

struct Source {
    owner: Arc<()>,
    origin: Origin,
}

struct Local {
    owner: Arc<()>,
}

struct Route {
    id: Origin,
    urls: [String; 1],
}

impl Route {
    fn configured(value: &str) -> Result<Self> {
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

struct Material<'a> {
    origin: Origin,
    selector: Selector,
    package: &'a [u8],
    expires: u64,
}

enum Work {
    Pending,
    NeedPackage { origin: Origin },
    Queued(Outbox),
}

struct Checkpoint {
    cursor: CursorStore,
    signer: Keys,
    codec: Codec,
    owner: Arc<()>,
    owner_uid: u32,
    policy: Policy,
    routes: Vec<Route>,
}

impl Checkpoint {
    fn attach(
        mut cursor: CursorStore,
        signer: Keys,
        owner_uid: u32,
        urls: &[&str],
        policy: Policy,
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
        rows::attach(&mut cursor, signer.public_key())?;
        Ok(Self {
            cursor,
            signer,
            codec: Codec::new(MAX_OUTER_BYTES).map_err(|_| Error::Cursor(CursorError::Range))?,
            owner: Arc::new(()),
            owner_uid,
            policy,
            routes,
        })
    }

    // Only the test harness stands in for the authenticated socket owner.
    fn bind_source(&self, url: &str) -> Result<Source> {
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

    // The fixture supplies the verified peer UID, never data from an Offer.
    fn authorize_local(&self, verified_uid: u32) -> Result<Local> {
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
        if !Arc::ptr_eq(&self.owner, &local.owner) {
            return Err(Error::LocalOwner);
        }
        Ok(())
    }

    fn admit(&mut self, source: &Source, record: RawRecord<'_>, wall: u64) -> Result<Disposition> {
        let origin = self.origin(source)?;
        record.check()?;
        rows::check_wall(&self.cursor.conn, wall)?;
        let Some(offer) = self.verify_offer(origin, record, wall)? else {
            return Ok(Disposition {
                status: Status::Invalid,
                receipt: self.cursor.commit_invalid(origin, record, wall)?,
            });
        };

        let server = offer.flight().author;
        let reserved = self.policy.reserved.contains(&server);
        let decision_at = matches!(self.policy.mode, Mode::Automatic).then_some(wall);
        let result = self.cursor.commit_trusted(origin, record, wall, |tx| {
            rows::sweep(tx, wall)?;
            rows::admit(tx, origin, record, &offer, reserved, decision_at)
        });
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
        let Ok(offer) = self.codec.open_offer(
            &self.signer,
            record.bytes,
            None,
            &self.route(origin)?.urls,
            wall,
        ) else {
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

    fn approve(&mut self, local: &Local, key: Selector, wall: u64) -> Result<()> {
        let origin = self.authorized_row(local, key)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        let row = Invitation::load(&tx, key, origin)?;
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

    fn discard(&mut self, local: &Local, key: Selector, wall: u64) -> Result<()> {
        self.check_local(local)?;
        let origin = rows::stored_origin(&self.cursor.conn, key)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        let row = Invitation::load(&tx, key, origin)?;
        rows::advance_wall(&tx, wall)?;
        row.remove(&tx)?;
        tx.commit()?;
        Ok(())
    }

    fn sweep(&mut self, source: &Source, wall: u64) -> Result<()> {
        let origin = self.origin(source)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::sweep(&tx, wall)?;
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

    fn work(
        &mut self,
        local: &Local,
        key: Selector,
        wall: u64,
        material: Option<Material<'_>>,
    ) -> Result<Work> {
        let origin = self.authorized_row(local, key)?;
        let route = self
            .routes
            .iter()
            .find(|route| route.id == origin)
            .ok_or(Error::Source)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::check_wall(&tx, wall)?;
        let row = Invitation::load(&tx, key, origin)?;
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
        let signed_offer = codec
            .open_offer(signer, &row.frame, Some(row.key.server), &route.urls, wall)
            .map_err(|_| Error::Cursor(CursorError::Binding))?;
        let flight = signed_offer.flight();
        if Selector::from(&signed_offer) != row.key
            || flight.carrier_id.to_bytes() != row.carrier
            || flight.expires_at != row.expires
            || flight.replay_safe_until != row.replay_until
        {
            return Err(Error::Cursor(CursorError::Binding));
        }
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
        };
        row.save_outbox(tx, &queued)?;
        Ok(queued)
    }
}
