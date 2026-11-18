//! Drive local retirement through a real SQLCipher journal and private filesystem.

use super::*;
use paths::retirement::{RetirementDir, Witness};
use store::consent_checkpoint::provisioning::{
    RetirementTarget,
    retirement::{Action, Outcome, Step},
};

#[path = "peer_stage_retirement_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cut {
    Intent,
    Unlinked,
    Synced,
    FilesRemoved,
}

impl<'a> InvitationState<'a> {
    fn open_retirement(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        policy: Policy,
        routes: &[Route],
        wall: Wall,
    ) -> Result<Self> {
        let paths = Paths::new(lease, material)?;
        let checked = paths.cursor()?;
        let conn = HardenedPeerConnection::open(checked.path(), *paths.cursor_key())?;
        checked.unchanged()?;
        let signer = material.signer();
        let origins = routes.iter().map(|route| route.id).collect::<Vec<_>>();
        let urls = routes
            .iter()
            .map(|route| route.urls[0].as_str())
            .collect::<Vec<_>>();
        let (Some(origin), Some(url)) = (origins.first().copied(), urls.first().copied()) else {
            return Err(Error::Binding);
        };
        let cursor = Checkpoint::retirement_cursor(conn, signer.public_key().to_bytes(), origins)?;
        // SAFETY: reads the current process's effective UID.
        let uid = unsafe { libc::geteuid() };
        let checkpoint =
            Checkpoint::attach_retirement_fixture(cursor, (*signer).clone(), uid, &urls, policy)?;
        let source = checkpoint.bind_source(url)?;
        checked.unchanged()?;
        Ok(Self {
            paths,
            signer,
            origin,
            checkpoint,
            source,
            wall,
            origin_url: url.to_owned(),
        })
    }

    fn decline_stage(
        &mut self,
        local: &Local,
        target: &RetirementTarget,
        after: impl FnMut(Cut) -> Result<()>,
    ) -> Result<Outcome> {
        self.retire_stage(local, target, Action::Decline, after)
    }

    fn expire_stage(
        &mut self,
        target: &RetirementTarget,
        after: impl FnMut(Cut) -> Result<()>,
    ) -> Result<Outcome> {
        // SAFETY: reads the effective UID of the profile executor. This method
        // is only called by the profile's internal clock-owned fixture.
        let uid = unsafe { libc::geteuid() };
        let local = self.checkpoint.authorize_local(uid)?;
        self.retire_stage(&local, target, Action::Expire, after)
    }

    fn retire_stage(
        &mut self,
        local: &Local,
        target: &RetirementTarget,
        action: Action,
        mut after: impl FnMut(Cut) -> Result<()>,
    ) -> Result<Outcome> {
        let initial_wall = self.provisional_sample()?.as_secs();
        let examined =
            match self
                .checkpoint
                .examine_retirement(local, target, action, initial_wall)?
            {
                Ok(examined) => examined,
                Err(receipt) => return Ok(receipt),
            };
        let (known, ..) = examined.binding.coordinates();
        let stage = examined.stage;
        let step = if let Some(step) = examined.step {
            step
        } else {
            let witness =
                match RetirementDir::open(&self.paths, known).and_then(|dir| dir.capture(stage)) {
                    Ok(witness) => Some(witness.encode()),
                    Err(Error::StageIdentityMismatch) => None,
                    Err(error) => return Err(error),
                };
            self.checkpoint.begin_retirement(
                local,
                target,
                stage,
                action,
                witness.as_deref(),
                initial_wall,
            )?;
            after(Cut::Intent)?;
            witness.map_or(Step::Blocked, Step::Retiring)
        };
        let (witness, needs_unlink) = match step {
            Step::Retiring(bytes) => (Witness::decode(&bytes)?, true),
            Step::FilesRemoved(bytes) => (Witness::decode(&bytes)?, false),
            Step::Blocked => return Ok(Outcome::Blocked),
            Step::Released(_) => return Err(Error::Binding),
        };
        let directory = RetirementDir::open(&self.paths, known)?;
        let check_absent = || {
            directory
                .ensure_absent(witness)
                .map_err(|_| consent_checkpoint::Error::Cursor(store::Error::Storage))
        };
        if needs_unlink {
            directory.unlink(witness, stage)?;
            after(Cut::Unlinked)?;
            directory.sync_removed(witness)?;
            after(Cut::Synced)?;
            // No methods from the native preparation path can run during this
            // turn. Use a fresh time sample after awaited/durable boundaries.
            let removed_wall = self.provisional_sample()?.as_secs();
            self.checkpoint
                .files_retired(local, target, stage, removed_wall, check_absent)?;
            after(Cut::FilesRemoved)?;
        }
        directory.ensure_absent(witness)?;
        let release_wall = self.provisional_sample()?.as_secs();
        self.checkpoint
            .release_retirement(local, target, stage, release_wall, check_absent)?;
        Ok(Outcome::Released)
    }
}
