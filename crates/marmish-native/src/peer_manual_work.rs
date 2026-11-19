//! Checked physical coordinates and one visit's owned package work.

#![allow(dead_code, reason = "mounted by the separate private Manual scheduler")]

use super::*;
use nostr::key::PublicKey;
use socket::owner::recipient::PeerRecipientPort;

pub(in crate::peer_invitation) use consent_checkpoint::provisioning::prepared_consent::custody::manual_work::ManualDue;

impl RecoveredOwner<'_> {
    pub(in crate::peer_invitation) fn due_manual_work(
        &mut self,
        port: &mut PeerRecipientPort,
    ) -> Result<Option<ManualDue>> {
        self.read_manual_work(port, Self::due_manual_at)
    }

    pub(in crate::peer_invitation) fn current_manual_pending(
        &mut self,
        port: &mut PeerRecipientPort,
    ) -> Result<Option<(Selector, u64)>> {
        self.read_manual_work(port, |owner, route, class, charge| {
            let (
                Class::Identified { .. },
                Some(
                    charge @ Charge {
                        stage: Some(State::Prepared(..)),
                        ..
                    },
                ),
            ) = (class, charge)
            else {
                return Ok(None);
            };
            Ok(owner
                .checkpoint
                .select_manual_pending(&owner.source, route, charge)?)
        })
    }

    fn due_manual_at(
        &mut self,
        route: (PublicKey, &str, u64),
        class: Class,
        charge: Option<Charge>,
    ) -> Result<Option<ManualDue>> {
        let selected = self
            .checkpoint
            .select_manual_due(&self.source, route, charge)?;
        Ok(physical_work(class, charge, selected))
    }

    fn read_manual_work<T>(
        &mut self,
        port: &mut PeerRecipientPort,
        select: impl FnOnce(&mut Self, (PublicKey, &str, u64), Class, Option<Charge>) -> Result<T>,
    ) -> Result<T> {
        let selected = port.use_current_recipient(|account, url, wall| {
            if self.signer.public_key() != account || self.relay != url {
                return Err(Error::Binding);
            }
            let (charges, files) = self.observe(wall)?;
            let class = Inventory::calculate(charges, files)?.classes[1];
            let selected = select(self, (account, url, wall), class, charges[1])?;
            if inspect(&self.paths, &self.cursor)? != files {
                return Err(Error::StageIdentityMismatch);
            }
            Ok(selected)
        })??;
        port.use_current_recipient(|_, _, _| ())?;
        Ok(selected)
    }
}

fn physical_work(
    class: Class,
    charge: Option<Charge>,
    selected: Option<ManualDue>,
) -> Option<ManualDue> {
    match (class, selected) {
        (Class::Vacant, work @ Some(ManualDue::Provision(_))) => work,
        (Class::Create(_) | Class::Identified { .. }, work @ Some(ManualDue::Resume(_))) => work,
        (
            Class::Identified { .. },
            work @ Some(ManualDue::Publish(_) | ManualDue::Held(..) | ManualDue::JoinStarted(..)),
        ) => work,
        (Class::JournalCandidate { binding, .. }, Some(work)) => match (charge, work) {
            (
                Some(Charge {
                    binding: charged,
                    stage: Some(State::NativeStarted(_)),
                }),
                ManualDue::Resume(key),
            ) if binding == charged && binding.coordinates().3 == key => Some(work),
            (
                Some(Charge {
                    binding: charged,
                    stage: Some(State::Prepared(..)),
                }),
                ManualDue::JoinStarted(key, generation),
            ) if binding == charged
                && binding.coordinates().3 == key
                && binding.coordinates().1 == generation =>
            {
                Some(work)
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(marmish_peer_receiver_candidate)]
use crate::{
    peer_candidate_startup::Readiness, peer_relay_trust::candidate_registration::ForwardingVerifier,
};
#[cfg(marmish_peer_receiver_candidate)]
use std::sync::atomic::AtomicBool;

#[cfg(marmish_peer_receiver_candidate)]
pub(in crate::peer_invitation) struct ManualPackageGuard<'a> {
    pub(in crate::peer_invitation) readiness: &'a Readiness,
    pub(in crate::peer_invitation) stop: &'a Arc<AtomicBool>,
    pub(in crate::peer_invitation) forwarding: &'a ForwardingVerifier,
}

#[cfg(marmish_peer_receiver_candidate)]
impl RecoveredOwner<'_> {
    pub(in crate::peer_invitation) fn manual_package_turn(
        &mut self,
        quota: &ReceiverQuota<'_>,
        port: &mut PeerRecipientPort,
        selector: Selector,
        fresh: bool,
        guard: ManualPackageGuard<'_>,
    ) -> Result<()> {
        let expected = if fresh {
            ManualDue::Provision(selector)
        } else {
            ManualDue::Resume(selector)
        };
        if self.due_manual_work(port)? != Some(expected) {
            return Err(store::Error::Stale.into());
        }
        let window = self.manual_package_window(port, selector, &guard)?;
        let wall = window.sample()?.as_secs();
        let reservation =
            self.checkpoint
                .reserve_manual_package(&self.source, selector, wall, fresh, || {
                    window
                        .sample()
                        .map(drop)
                        .map_err(|_| consent_checkpoint::Error::Source)
                })?;
        let window = self.manual_package_window(port, selector, &guard)?;
        let artifact = if let Some(started) = self.start_package(quota, &reservation, &window)? {
            let prestaged = self.prestage_package(quota, &reservation, started)?;
            Some(self.issue_package(quota, &reservation, prestaged)?)
        } else {
            None
        };

        let window = self.manual_package_window(port, selector, &guard)?;
        let proof = self.attest_package(quota, &reservation, artifact, &window)?;
        self.queue_managed_consent(quota, &reservation, &proof, &window)?;
        Ok(())
    }

    fn manual_package_window(
        &mut self,
        port: &mut PeerRecipientPort,
        selector: Selector,
        guard: &ManualPackageGuard<'_>,
    ) -> Result<NativeWindow> {
        port.use_current_native(|scope| {
            if self.signer.public_key() != scope.account() || self.relay != scope.origin() {
                return Err(Error::Binding);
            }
            let offer = self.checkpoint.installed_live_offer(
                &self.source,
                selector,
                scope.wall().as_secs(),
            )?;
            scope
                .manual_window(
                    guard.readiness.clone(),
                    guard.stop.clone(),
                    offer.flight().expires_at,
                    guard.forwarding,
                )
                .map_err(|_| Error::Binding)
        })?
    }
}

#[cfg(test)]
#[test]
fn cold_join_started_journal_only_selects_exact_readback() {
    use super::super::provisional::tests::Fixture;
    use super::tests::start_original;
    use consent_checkpoint::provisioning::stage_journal::Artifact;

    let fixture = Fixture::new();
    let (_owner, binding, witness) = start_original(&fixture, 4096);
    let (_, generation, _, selector) = binding.coordinates();
    let stage = State::Prepared(witness, Artifact::new([3; 32], b"test", fixture.now + 3600));
    let main = FileExtent {
        identity: (
            u64::from_be_bytes(witness.device),
            u64::from_be_bytes(witness.inode),
        ),
        logical: 4096,
        charged: 4096,
    };
    let journal = FileExtent {
        identity: (0, 1),
        ..main
    };
    let charge = Some(Charge {
        binding,
        stage: Some(stage),
    });
    let class = super::classify_unknown(charge, [Some(main), Some(journal), None, None]);
    assert!(matches!(class, Class::JournalCandidate { .. }));

    let exact = ManualDue::JoinStarted(selector, generation);
    assert_eq!(physical_work(class, charge, Some(exact)), Some(exact));
    for ineligible in [
        ManualDue::JoinStarted(selector, generation + 1),
        ManualDue::Held(selector, generation),
        ManualDue::Publish(selector),
        ManualDue::Resume(selector),
        ManualDue::Provision(selector),
    ] {
        assert_eq!(physical_work(class, charge, Some(ineligible)), None);
    }
    assert_eq!(physical_work(class, None, Some(exact)), None);
}
