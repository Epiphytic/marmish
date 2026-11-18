//! Custody and native artifact authority for the bounded installed-port proof.

use nostr::key::PublicKey;

use super::*;
use crate::peer_invitation::socket::owner::recipient::CheckedAck;
use consent_checkpoint::provisioning::prepared_consent::{
    Queued, custody::ReceivePolicy, publication::installed_settlement::SettledConsent,
};

pub(in crate::peer_invitation) struct Fixture<'a> {
    owner: PreparedConsentOwner<'a>,
    cursor: paths::CheckedFile,
    cut: Option<Cut>,
    wake_failure: Option<Error>,
}

#[derive(PartialEq, Eq)]
enum Cut {
    ReplaceStagedImage,
    ReadOnlySettlement,
}

#[path = "peer_installed_consent_wss_tests.rs"]
mod tests;

#[path = "peer_installed_delivery_fixture.rs"]
mod delivery;

pub(in crate::peer_invitation) struct Reserved {
    pub(in crate::peer_invitation) attempt: Attempt,
    native: NativeAttestation,
}

impl<'a> Fixture<'a> {
    fn open(lease: &'a ProfileLease, material: &IdentityMaterial, route: &Route) -> Result<Self> {
        let paths = Paths::new(lease, material)?;
        let cursor = paths.cursor()?;
        let url = &route.urls[0];
        let signer = material.signer();
        // SAFETY: reads this process's own effective UID.
        let uid = unsafe { libc::geteuid() };
        let open = || {
            cursor.unchanged()?;
            let conn = HardenedPeerConnection::open_installed(cursor.path(), *paths.cursor_key())?;
            cursor.unchanged()?;
            Ok::<_, Error>(conn)
        };
        if fs::metadata(cursor.path()).map_err(|_| Error::Path)?.len() == 0 {
            Checkpoint::attach_installed_v2(
                open()?,
                (*signer).clone(),
                uid,
                url,
                Policy::default(),
            )?;
        }
        let checkpoint = Checkpoint::attach_installed_custody(
            open()?,
            (*signer).clone(),
            uid,
            url,
            Policy::default(),
            ReceivePolicy::Manual,
        )?;
        let source = checkpoint.bind_source(url)?;
        cursor.unchanged()?;
        Ok(Self {
            owner: PreparedConsentOwner {
                state: InvitationState {
                    paths,
                    signer,
                    origin: route.id,
                    checkpoint,
                    source,
                    wall: Wall::System,
                    origin_url: url.clone(),
                },
                role: Role::Client,
                custody: true,
                snapshot: SnapshotArea::new()?,
            },
            cursor,
            cut: None,
            wake_failure: None,
        })
    }

    fn attest(
        &self,
        (binding, witness): (Binding, FileWitness),
        wall: u64,
    ) -> Result<NativeAttestation> {
        let (package, image) = self.owner.snapshot.read(
            &self.owner.state,
            binding,
            witness,
            Duration::from_secs(wall),
            |_| Ok(()),
        )?;
        Ok(NativeAttestation {
            binding,
            witness,
            package,
            image,
        })
    }

    fn queue(
        &mut self,
        reservation: &Reservation,
        wall: u64,
    ) -> Result<consent_checkpoint::wire::Transcript> {
        let state = &mut self.owner.state;
        let candidate =
            state
                .checkpoint
                .inspect_prepared_consent(&state.source, reservation, wall)?;
        let native = self.attest(candidate.stage(), wall)?;
        let state = &self.owner.state;
        let signed = state
            .checkpoint
            .sign_prepared_consent(&candidate, &native, wall)?;
        self.owner.verify_custody(&native)?;
        let state = &mut self.owner.state;
        let queued = state.checkpoint.queue_prepared_consent(
            &state.source,
            reservation,
            signed,
            wall,
            |_| Ok(()),
        )?;
        let transcript = state
            .checkpoint
            .verify_queued_consent(&queued, &native, wall)?;
        self.owner.verify_custody(&native)?;
        Ok(transcript)
    }

    fn queued(&mut self, selector: Selector, wall: u64) -> Result<Option<Queued>> {
        let state = &mut self.owner.state;
        Ok(state
            .checkpoint
            .recover_queued_consent(&state.source, selector, wall)?)
    }

    pub(in crate::peer_invitation) fn reserve(
        &mut self,
        account: PublicKey,
        origin: &str,
        selector: Selector,
        wall: u64,
    ) -> Result<Option<Reserved>> {
        self.cursor.unchanged()?;
        let state = &mut self.owner.state;
        if Route::configured(origin)?.id != state.origin {
            return Err(Error::Binding);
        }
        state
            .checkpoint
            .check_installed_consent_scope(&state.source, account, origin, wall)?;
        let Some(queued) = self.queued(selector, wall)? else {
            return Ok(None);
        };
        let native = self.attest(queued.stage(), wall)?;
        self.owner.verify_custody(&native)?;
        self.cursor.unchanged()?;
        let state = &mut self.owner.state;
        let attempt =
            state
                .checkpoint
                .reserve_prepared_attempt(&state.source, queued, &native, wall)?;
        Ok(attempt.map(|attempt| Reserved { attempt, native }))
    }

    pub(in crate::peer_invitation) fn before_flush(
        &mut self,
        reserved: &Reserved,
        wall: u64,
    ) -> Result<()> {
        if self.cut == Some(Cut::ReplaceStagedImage) {
            self.cut = None;
            let file = self
                .owner
                .state
                .paths
                .reopen_provisional(reserved.native.binding, reserved.native.witness)?;
            let path = file.path();
            fs::rename(path, path.with_extension("saved")).map_err(|_| Error::Path)?;
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|_| Error::Path)?;
        }
        self.check(reserved, wall)
    }

    fn check(&mut self, reserved: &Reserved, wall: u64) -> Result<()> {
        self.cursor.unchanged()?;
        let state = &mut self.owner.state;
        state.checkpoint.check_installed_prepared_attempt(
            &state.source,
            &reserved.attempt,
            wall,
        )?;
        // This bounded synchronous digest is permitted only in this test child.
        self.owner.verify_custody(&reserved.native)?;
        self.cursor.unchanged()?;
        Ok(())
    }

    pub(in crate::peer_invitation) fn settle(
        &mut self,
        reserved: Reserved,
        ack: CheckedAck<'_>,
    ) -> Result<SettledConsent> {
        self.cursor.unchanged()?;
        self.owner.verify_custody(&reserved.native)?;
        let state = &mut self.owner.state;
        if self.cut == Some(Cut::ReadOnlySettlement) {
            self.cut = None;
            state.checkpoint.fixture_statement("PRAGMA query_only=ON");
        }
        Ok(state.checkpoint.settle_installed_prepared_attempt(
            &state.source,
            reserved.attempt,
            ack,
        )?)
    }

    pub(in crate::peer_invitation) fn wake_delivery(
        &mut self,
        settled: SettledConsent,
        account: PublicKey,
        origin: &str,
        wall: u64,
    ) -> Result<usize> {
        self.cursor.unchanged()?;
        let state = &mut self.owner.state;
        Ok(state.checkpoint.wake_installed_delivery(
            &state.source,
            settled,
            account,
            origin,
            wall,
        )?)
    }

    pub(in crate::peer_invitation) fn record_wake_failure(&mut self, error: Error) {
        self.wake_failure = Some(error);
    }
}
