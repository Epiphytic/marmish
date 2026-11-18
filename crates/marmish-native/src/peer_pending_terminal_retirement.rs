//! Local filesystem retirement for a fenced joined Pending stage.

use super::*;
use consent_checkpoint::provisioning::{
    prepared_consent::custody::terminal::{TerminalRecord, TerminalStep},
    stage_journal::State,
};
use paths::retirement::{RetirementDir, Witness};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) enum Cut {
    Intent,
    Unlinked,
    Synced,
    FilesRemoved,
}

impl PreparedConsentOwner<'_> {
    pub(in crate::peer_invitation) fn retire_terminal(
        &mut self,
        record: TerminalRecord,
    ) -> Result<TerminalRecord> {
        self.retire_terminal_at_cut(record, |_| Ok(()))
    }

    pub(in crate::peer_invitation) fn retire_terminal_at_cut(
        &mut self,
        mut record: TerminalRecord,
        mut after: impl FnMut(Cut) -> Result<()>,
    ) -> Result<TerminalRecord> {
        if !self.custody {
            return Err(Error::Binding);
        }
        let wall = self.sample()?.as_secs();
        self.state
            .checkpoint
            .terminal_check(&self.state.source, &record, wall)?;
        if matches!(
            record.step(),
            TerminalStep::Released | TerminalStep::Blocked
        ) {
            return Ok(record);
        }
        let (binding, stage) = record.stage().ok_or(Error::Binding)?;
        let (known, ..) = binding.coordinates();
        if matches!(record.step(), TerminalStep::Fenced) {
            let witness = RetirementDir::open(&self.state.paths, known)
                .and_then(|dir| dir.capture(stage))
                .and_then(|witness| {
                    witness.require_prepared(stage)?;
                    Ok(witness)
                });
            let bytes = match witness {
                Ok(witness) => witness.encode(),
                Err(Error::StageIdentityMismatch) => {
                    return self.block_terminal_retirement(&record);
                }
                Err(error) => return Err(error),
            };
            let wall = self.sample()?.as_secs();
            record =
                self.state
                    .checkpoint
                    .terminal_intent(&self.state.source, &record, wall, &bytes)?;
            after(Cut::Intent)?;
        }
        if let TerminalStep::Intent(bytes) = record.step() {
            let witness = Witness::decode(bytes)?;
            let result = self.remove_terminal_files(&record, known, stage, witness, &mut after);
            match result {
                Ok(()) => {}
                Err(Error::StageIdentityMismatch) => {
                    return self.block_terminal_retirement(&record);
                }
                Err(error) => return Err(error),
            }
            let wall = self.sample()?.as_secs();
            record =
                self.state
                    .checkpoint
                    .terminal_files_removed(&self.state.source, &record, wall)?;
            after(Cut::FilesRemoved)?;
        }
        let TerminalStep::FilesRemoved(bytes) = record.step() else {
            return Err(Error::Binding);
        };
        let witness = Witness::decode(bytes)?;
        witness.require_prepared(stage)?;
        let wall = self.sample()?.as_secs();
        self.state
            .checkpoint
            .terminal_check(&self.state.source, &record, wall)?;
        RetirementDir::open(&self.state.paths, known)?.ensure_absent(witness)?;
        let wall = self.sample()?.as_secs();
        Ok(self
            .state
            .checkpoint
            .terminal_release(&self.state.source, &record, wall)?)
    }

    fn remove_terminal_files(
        &mut self,
        record: &TerminalRecord,
        known: bool,
        stage: State,
        witness: Witness,
        after: &mut impl FnMut(Cut) -> Result<()>,
    ) -> Result<()> {
        witness.require_prepared(stage)?;
        let dir = RetirementDir::open(&self.state.paths, known)?;
        let wall = self.sample()?.as_secs();
        self.state
            .checkpoint
            .terminal_check(&self.state.source, record, wall)?;
        // This owner holds the profile lease. Its joined stage access is scoped
        // to completed calls, so no native writer survives into this turn.
        dir.unlink(witness, stage)?;
        after(Cut::Unlinked)?;
        let wall = self.sample()?.as_secs();
        self.state
            .checkpoint
            .terminal_check(&self.state.source, record, wall)?;
        dir.sync_removed(witness)?;
        after(Cut::Synced)
    }

    fn block_terminal_retirement(&mut self, record: &TerminalRecord) -> Result<TerminalRecord> {
        let wall = self.sample()?.as_secs();
        Ok(self
            .state
            .checkpoint
            .terminal_blocked(&self.state.source, record, wall)?)
    }
}
