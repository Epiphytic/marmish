//! Separate founder-owned use of the checked socket and exact send fence.

use super::*;
use crate::peer_invitation::store::consent_checkpoint::wire::Flight;
use crate::{
    command_config::CommandConfig,
    identity::{IdentityMaterial, ProfileLease},
    peer_invitation::founder_ledger::{Ledger, Outbound, Step, ledger_binding},
};
use std::num::NonZeroU64;

pub(in crate::peer_invitation) struct FounderPort {
    wire: PeerRecipientPort,
    grant: Grant,
    signer: Arc<Keys>,
}

struct Grant {
    lease: Arc<ProfileLease>,
    founder: PublicKey,
    recipient: PublicKey,
    origin: String,
    generation: NonZeroU64,
}

impl Grant {
    fn check(&self) -> Result<()> {
        let config = CommandConfig::load(self.lease.root()).map_err(|_| Failure::Retired)?;
        let recipient = self.recipient.to_bytes();
        if self.founder == self.recipient
            || self.lease.public_key() != Some(self.founder)
            || config.founder_origin(&recipient).map(|url| url.as_str())
                != Some(self.origin.as_str())
        {
            return Err(Failure::Retired);
        }
        Ok(())
    }
}

impl FounderPort {
    pub(in crate::peer_invitation) async fn open(
        lease: Arc<ProfileLease>,
        material: &IdentityMaterial,
        recipient: PublicKey,
        origin: &str,
        generation: NonZeroU64,
        deadline: Instant,
    ) -> Result<Self> {
        let grant = Grant {
            lease: lease.clone(),
            founder: material.public_key(),
            recipient,
            origin: origin.to_owned(),
            generation,
        };
        grant.check()?;
        let admission =
            ReceiverRouteAdmission::fixture(lease, material).map_err(|_| Failure::Retired)?;
        let wire = PeerRecipientPort::open(admission, deadline).await?;
        let mut port = Self {
            wire,
            grant,
            signer: material.signer(),
        };
        port.current()?;
        Ok(port)
    }

    pub(in crate::peer_invitation) fn current(&mut self) -> Result<u64> {
        if let Err(error) = self.grant.check() {
            self.wire.0.retire();
            return Err(error);
        }
        let (account, wall) = self.wire.check_observation()?;
        if account != self.grant.founder || self.wire.0.scope.origin != self.grant.origin {
            return Err(Failure::Binding);
        }
        Ok(wall)
    }

    pub(in crate::peer_invitation) async fn page(
        &mut self,
        bounds: [u64; 2],
        deadline: Instant,
    ) -> Result<Page> {
        self.current()?;
        self.wire.page(bounds, deadline).await
    }

    pub(in crate::peer_invitation) fn use_page<T>(
        &mut self,
        page: &Page,
        handle: impl FnOnce(CheckedPage<'_>) -> T,
    ) -> Result<T> {
        self.current()?;
        self.wire.use_page(page, handle)
    }

    pub(in crate::peer_invitation) async fn send(
        &mut self,
        ledger: &mut Ledger,
        which: Outbound,
        flight: Flight,
        bytes: &[u8],
        deadline: Instant,
        after_arm: impl FnOnce(),
    ) -> std::result::Result<Step, &'static str> {
        let wall = self.current().map_err(|_| "founder_grant")?;
        if self.grant.founder != flight.author
            || self.grant.recipient != flight.recipient
            || !ledger.owns(
                ledger_binding(self.grant.founder, self.grant.recipient, &self.grant.origin),
                self.grant.generation,
            )
        {
            return Err("founder_scope");
        }
        ledger.arm(which, flight, bytes, wall)?;
        after_arm();
        let operation = Operation {
            id: flight.revision.to_bytes(),
            generation: self.grant.generation.get(),
        };
        let mut current = |proof: Operation, use_wall: u64| {
            proof == operation
                && self.grant.check().is_ok()
                && ledger.current(which, which.armed(), flight, bytes, use_wall)
        };
        let permit = ControlSendPermit {
            scope: self.wire.0.scope.clone(),
            author: flight.author,
            remote: flight.recipient,
            carrier: flight.carrier_id.to_bytes(),
            digest: sealed_digest(bytes),
            operation,
            deadline: flight.expires_at,
            bytes,
            current: &mut current,
        };
        let observed = self.wire.publish_control(permit, deadline).await;
        let step = match observed {
            ControlOutcome::Accepted(ack) => {
                let valid = self.grant.check().is_ok()
                    && self
                        .wire
                        .use_accepted(&ack, |ack| {
                            ack.account == self.grant.founder
                                && ack.origin == self.grant.origin
                                && ack.operation == operation
                                && ack.carrier == flight.carrier_id.to_bytes()
                                && ledger.current(which, which.armed(), flight, bytes, ack.wall)
                        })
                        .unwrap_or(false);
                if valid {
                    which.accepted()
                } else {
                    Step::Uncertain
                }
            }
            ControlOutcome::NotSent(_) => Step::NotSent,
            ControlOutcome::Rejected => Step::Rejected,
            ControlOutcome::PossiblyExposed(_) => Step::Uncertain,
        };
        ledger.terminal(which, step)?;
        Ok(step)
    }
}

use crate::peer_invitation::{
    founder_ledger::FounderResult,
    paths::Paths,
    store::consent_checkpoint::wire::{Codec, Consent, Delivery, Offer, Prepared},
};
use crate::peer_quota_vfs::dispatch::founder::{Created, Expected, FounderQuota, Image};

// Only the checked, independently reopened operation can mint this witness.
pub(in crate::peer_invitation) struct ObservedImage {
    image: Image,
    file: (u64, u64),
}

impl ObservedImage {
    pub(in crate::peer_invitation) fn image(&self) -> &Image {
        &self.image
    }
}

// The WSS proof owns the checked route and all mutations through one leased
// turn. The lower-level ledger and quota also have isolated offline tests.
pub(in crate::peer_invitation) struct FounderTurn<'a> {
    port: FounderPort,
    journal: Ledger,
    quota: FounderQuota<'a>,
    paths: Paths<'a>,
}

impl<'a> FounderTurn<'a> {
    pub(in crate::peer_invitation) fn new(
        mut port: FounderPort,
        quota: FounderQuota<'a>,
        paths: Paths<'a>,
    ) -> FounderResult<Self> {
        port.current().map_err(|_| "founder_grant")?;
        if !quota.owns(&port.grant.lease) || !paths.owns_lease(&port.grant.lease) {
            return Err("founder_scope");
        }
        let file = quota.ledger();
        paths
            .create_founder_file(&file)
            .map_err(|_| "founder_ledger_path")?;
        port.current().map_err(|_| "founder_grant")?;
        let journal = Ledger::open(
            &file,
            *paths.founder_ledger_key(),
            ledger_binding(port.grant.founder, port.grant.recipient, &port.grant.origin),
            port.grant.generation,
            true,
        )?;
        Ok(Self {
            port,
            journal,
            quota,
            paths,
        })
    }

    pub(in crate::peer_invitation) fn journal(&self) -> &Ledger {
        &self.journal
    }

    pub(in crate::peer_invitation) fn current(&mut self) -> FounderResult<u64> {
        self.port.current().map_err(|_| "founder_grant")
    }

    pub(in crate::peer_invitation) fn claim(
        &mut self,
        offer: &Prepared<Offer>,
    ) -> FounderResult<()> {
        let scope = &self.port.grant;
        let flight = offer.flight.flight();
        if flight.author != scope.founder
            || flight.recipient != scope.recipient
            || offer.flight.routing().relays != [scope.origin.as_str()]
        {
            return Err("founder_scope");
        }
        if self.current()? >= flight.expires_at {
            return Err("founder_stale");
        }
        self.journal.claim(offer)
    }

    pub(in crate::peer_invitation) async fn page(
        &mut self,
        bounds: [u64; 2],
        deadline: Instant,
    ) -> FounderResult<Page> {
        self.port
            .page(bounds, deadline)
            .await
            .map_err(|_| "founder_page")
    }

    pub(in crate::peer_invitation) fn receive_consent(
        &mut self,
        page: &Page,
        codec: &Codec,
        offer: &Offer,
    ) -> FounderResult<Consent> {
        let signer = self.port.signer.clone();
        let candidate = self
            .port
            .use_page(page, |page| {
                let [record] = page.records else {
                    return Err("founder_consent");
                };
                let bytes = record.body.get().as_bytes();
                let consent = codec
                    .open_consent(&signer, bytes, offer, page.wall)
                    .map_err(|_| "founder_consent")?;
                Ok((consent, bytes.to_vec()))
            })
            .map_err(|_| "founder_page")??;
        let (consent, bytes) = candidate;
        if self.current()? >= consent.flight().expires_at {
            return Err("founder_stale");
        }
        self.journal.receive_consent(&consent, &bytes)?;
        Ok(consent)
    }

    pub(in crate::peer_invitation) fn reserve(&mut self, consent: &Consent) -> FounderResult<()> {
        consent_fence(&mut self.port, &self.journal, consent, Step::Consent)?;
        self.quota.inventory(None)?;
        let wall = consent_fence(&mut self.port, &self.journal, consent, Step::Consent)?;
        self.journal.reserve(wall)
    }

    pub(in crate::peer_invitation) fn create_file(
        &mut self,
        consent: &Consent,
    ) -> FounderResult<(u64, u64)> {
        consent_fence(&mut self.port, &self.journal, consent, Step::Reserved)?;
        let file = self.quota.main(self.port.grant.generation);
        self.paths
            .create_founder_file(&file)
            .map_err(|_| "founder_file")
    }

    pub(in crate::peer_invitation) fn create(
        &mut self,
        consent: &Consent,
        file: (u64, u64),
    ) -> FounderResult<(Created, u64)> {
        let binding = consent.binding();
        let expected = expected(self.port.signer.clone(), consent, &binding.reference);
        consent_fence(&mut self.port, &self.journal, consent, Step::Reserved)?;
        let ticket = self.journal.start(file)?;
        let key = self.paths.founder_image_key(self.port.grant.generation);
        self.quota.create(
            ticket,
            &key,
            &expected,
            consent.key_package().clone(),
            || consent_fence(&mut self.port, &self.journal, consent, Step::Started).map(|_| ()),
        )
    }

    pub(in crate::peer_invitation) fn inspect(
        &mut self,
        consent: &Consent,
        file: (u64, u64),
        image: &Image,
    ) -> FounderResult<ObservedImage> {
        let binding = consent.binding();
        let expected = expected(self.port.signer.clone(), consent, &binding.reference);
        let generation = self.port.grant.generation;
        let key = self.paths.founder_image_key(generation);
        self.quota
            .inspect(generation, file, &key, &expected, image, || {
                consent_fence(&mut self.port, &self.journal, consent, Step::Started).map(|_| ())
            })
            .map(|(image, _)| ObservedImage { image, file })
    }

    pub(in crate::peer_invitation) fn freeze(
        &mut self,
        consent: &Consent,
        delivery: &Prepared<Delivery>,
        image: &ObservedImage,
    ) -> FounderResult<()> {
        let wall = consent_fence(&mut self.port, &self.journal, consent, Step::Started)?;
        if wall >= delivery.flight.flight().expires_at
            || self.journal.persisted_file()? != Some(image.file)
        {
            return Err("founder_stale");
        }
        self.journal.freeze(
            delivery,
            &image.image,
            consent.offer().room_id(),
            &consent.binding().reference,
        )
    }

    pub(in crate::peer_invitation) fn arm_before_write_cut(
        &mut self,
        delivery: &Prepared<Delivery>,
    ) -> FounderResult<()> {
        let wall = self.current()?;
        self.journal.arm(
            Outbound::Delivery,
            delivery.flight.flight(),
            delivery.frame(),
            wall,
        )
    }

    pub(in crate::peer_invitation) async fn send(
        &mut self,
        which: Outbound,
        flight: Flight,
        bytes: &[u8],
        deadline: Instant,
        after_arm: impl FnOnce(),
    ) -> FounderResult<Step> {
        self.port
            .send(&mut self.journal, which, flight, bytes, deadline, after_arm)
            .await
    }
}

fn consent_fence(
    port: &mut FounderPort,
    journal: &Ledger,
    consent: &Consent,
    step: Step,
) -> FounderResult<u64> {
    let wall = port.current().map_err(|_| "founder_grant")?;
    if wall >= consent.flight().expires_at || !journal.at(step) || !journal.consent_matches(consent)
    {
        return Err("founder_stale");
    }
    Ok(wall)
}

fn expected<'a>(signer: Arc<Keys>, consent: &'a Consent, reference: &'a [u8]) -> Expected<'a> {
    Expected {
        signer,
        recipient: consent.flight().author,
        routing: consent.offer().routing(),
        room: consent.offer().room_id(),
        reference,
    }
}
