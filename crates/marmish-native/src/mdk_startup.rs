//! Keep an admitted account root in custody across the rest of startup.

use super::{Journal, ProfileLease, SqlCipherKey, SqliteAccountStorage, mdk};
#[cfg(any(test, marmish_peer_receiver_candidate))]
use crate::command_engine_admission::Admission;
use crate::{Error, Result};
#[cfg(any(test, marmish_peer_receiver_candidate))]
use std::sync::Arc;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Step {
    Initialize,
    AccountSmoke,
    AccountSmokeClose,
    AccountOpen,
    JournalSmoke,
    JournalSmokeClose,
    JournalOpen,
    LifecycleRead,
    LifecycleWrite,
    Runtime,
    #[cfg(test)]
    RuntimeOpened,
    Binding,
    Revoked,
    ForgetGroup,
    Orphans,
    Maintain,
    Routes,
    Invalidations,
    Return,
}

pub(super) enum StartupMode {
    Stock,
    #[cfg(any(test, marmish_peer_receiver_candidate))]
    Candidate {
        lease: Arc<ProfileLease>,
        admission: Admission,
        #[cfg(test)]
        probe: Option<Box<dyn Fn(Step) -> Result<()>>>,
    },
}

impl StartupMode {
    #[cfg(any(test, marmish_peer_receiver_candidate))]
    pub(super) fn candidate(lease: Arc<ProfileLease>, admission: Admission) -> Self {
        Self::Candidate {
            lease,
            admission,
            #[cfg(test)]
            probe: None,
        }
    }

    #[cfg(test)]
    pub(super) fn probe(&self, step: Step) -> Result<()> {
        if let Self::Candidate {
            probe: Some(probe), ..
        } = self
        {
            probe(step)?;
        }
        Ok(())
    }

    pub(super) fn before(&self, _step: Step) -> Result<()> {
        #[cfg(test)]
        self.probe(_step)?;
        self.admit()
    }

    pub(super) fn admit(&self) -> Result<()> {
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        if let Self::Candidate { admission, .. } = self {
            return checked(admission);
        }
        Ok(())
    }

    pub(super) fn effect<T>(&self, step: Step, call: impl FnOnce() -> Result<T>) -> Result<T> {
        self.before(step)?;
        let result = call();
        self.admit().and(result)
    }
}

#[cfg(any(test, marmish_peer_receiver_candidate))]
fn checked(admission: &Admission) -> Result<()> {
    if admission.admit() {
        Ok(())
    } else {
        Err(Error::Storage("dispatcher_admission"))
    }
}

#[derive(Default)]
pub(super) struct StartupStorage {
    pub(super) journal: Option<Journal>,
    #[cfg(any(test, marmish_peer_receiver_candidate))]
    pub(super) custody: Option<AccountCustody>,
}

impl StartupStorage {
    pub(super) fn open_account(
        &mut self,
        mode: &StartupMode,
        step: Step,
        profile: &ProfileLease,
        key: &[u8; 32],
    ) -> Result<SqliteAccountStorage> {
        mode.before(step)?;
        let path = profile.mdk_database();
        let key = SqlCipherKey::raw(*key);
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        if let StartupMode::Candidate {
            lease, admission, ..
        } = mode
        {
            let opened = mdk(SqliteAccountStorage::open_encrypted_observed(
                path,
                &key,
                &|_| {},
            ));
            let root = match opened {
                Ok(root) => root,
                Err(error) => return mode.admit().and(Err(error)),
            };
            let handle = root.clone();
            self.custody = Some(AccountCustody::new(root, lease.clone(), admission.clone()));
            mode.admit()?;
            return Ok(handle);
        }
        mdk(SqliteAccountStorage::open_encrypted(path, &key))
    }

    pub(super) fn close_smoke_account(
        &mut self,
        mode: &StartupMode,
        smoke: SqliteAccountStorage,
    ) -> Result<()> {
        mode.before(Step::AccountSmokeClose)?;
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        if let Some(custody) = self.custody.take() {
            custody.close()?;
        }
        drop(smoke);
        Ok(())
    }

    pub(super) fn open_journal(
        &mut self,
        mode: &StartupMode,
        step: Step,
        profile: &ProfileLease,
        key: &[u8; 32],
        identity: &str,
    ) -> Result<()> {
        mode.before(step)?;
        let path = profile.journal_database();
        let result = match mode {
            StartupMode::Stock => Journal::open(path, key, identity),
            #[cfg(any(test, marmish_peer_receiver_candidate))]
            StartupMode::Candidate { admission, .. } => {
                Journal::open_with_fence(path, key, identity, Some(&admission.connection_fence()))
            }
        };
        match result {
            Ok(journal) => self.journal = Some(journal),
            Err(error) => return mode.admit().and(Err(error)),
        }
        mode.admit()
    }

    pub(super) fn close_smoke_journal(&mut self, mode: &StartupMode) -> Result<()> {
        mode.before(Step::JournalSmokeClose)?;
        if let Some(journal) = self.journal.take() {
            journal.close()?;
        }
        mode.admit()
    }

    pub(super) fn finish_error(&mut self, error: Error) -> Error {
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        if let Some(custody) = self.custody.take() {
            match self.journal.take().map_or(Ok(()), Journal::close) {
                Ok(()) => return custody.close().err().unwrap_or(error),
                Err(close_error) => {
                    drop(custody);
                    return close_error;
                }
            }
        }
        error
    }
}

impl Drop for StartupStorage {
    fn drop(&mut self) {
        #[cfg(any(test, marmish_peer_receiver_candidate))]
        drop(self.custody.take());
    }
}

#[cfg(any(test, marmish_peer_receiver_candidate))]
pub(super) struct AccountCustody {
    admission: Admission,
    held: Option<(
        SqliteAccountStorage,
        storage_sqlite::DatabaseCloser,
        Arc<ProfileLease>,
    )>,
}

#[cfg(any(test, marmish_peer_receiver_candidate))]
impl AccountCustody {
    fn new(root: SqliteAccountStorage, lease: Arc<ProfileLease>, admission: Admission) -> Self {
        let closer = root.closer();
        Self {
            admission,
            held: Some((root, closer, lease)),
        }
    }

    pub(super) fn close(self) -> Result<()> {
        self.close_with(|root| mdk(root.close()))
    }

    fn close_with(mut self, close: impl FnOnce(&SqliteAccountStorage) -> Result<()>) -> Result<()> {
        checked(&self.admission)?;
        if let Some((root, _, _)) = &self.held {
            if close(root).is_err() {
                return Err(Error::Storage("dispatcher_admission"));
            }
            self.held.take();
        }
        checked(&self.admission)
    }
}

#[cfg(any(test, marmish_peer_receiver_candidate))]
impl Drop for AccountCustody {
    fn drop(&mut self) {
        if let Some((root, closer, lease)) = self.held.take() {
            closer.begin_close();
            self.admission.report_custody_failure();
            std::mem::forget((root, lease));
        }
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "mdk_startup_tests.rs"]
pub(crate) mod tests;
