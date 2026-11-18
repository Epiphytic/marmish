//! Distinct native operations for fresh, pending-unsigned and exact recovery.

use storage_sqlite::{SqliteJournalMode, SqliteStorageOptions};

use super::*;

pub(crate) struct ProvisionalPackage {
    pub(crate) event: NativePackage,
    authored: u64,
    not_before: u64,
}

impl ProvisionalPackage {
    pub(crate) fn check_lifetime(&self, wall: u64) -> StageResult<()> {
        if self.authored > wall
            || !(self.not_before..self.event.expires).contains(&wall)
            || self.event.expires.saturating_sub(self.authored) > 31 * 86_400
        {
            return Err(StageError::Invalid);
        }
        Ok(())
    }
}

impl OpenStage {
    #[cfg(test)]
    pub(crate) fn open_provisional(
        path: &Path,
        key: &SqlCipherKey,
        signer: Arc<Keys>,
        origin: &str,
        slot: String,
        first_open: bool,
    ) -> StageResult<Self> {
        Self::open_provisional_with_options(
            path,
            key,
            signer,
            origin,
            slot,
            first_open,
            SqliteStorageOptions::default(),
        )
    }

    fn open_provisional_with_options(
        path: &Path,
        key: &SqlCipherKey,
        signer: Arc<Keys>,
        origin: &str,
        slot: String,
        first_open: bool,
        options: SqliteStorageOptions,
    ) -> StageResult<Self> {
        let storage = open_provisional_storage(path, key, options)?;
        if !first_open && storage.migration_summary().0 != 0 {
            return Err(StageError::Storage);
        }
        Ok(Self {
            storage,
            signer,
            endpoint: TransportEndpoint(origin.to_owned()),
            slot,
        })
    }

    #[cfg(test)]
    pub(crate) async fn generate_provisional(
        &self,
        sample: Duration,
        withhold_signing: bool,
    ) -> StageResult<ProvisionalPackage> {
        if self.load()?.is_some()
            || !durably_owned_key_packages(&self.storage, ProtocolProfile::Current)
                .map_err(|_| StageError::Storage)?
                .is_empty()
        {
            return Err(StageError::Invalid);
        }
        if withhold_signing {
            self.save(&KeyPackageLifecycleState::slot_only(self.slot.clone()))?;
            let mut publisher = self.publisher();
            publisher.withhold_signing = true;
            self.prepare_new_with_publisher(sample, publisher).await?;
            return Err(StageError::Invalid);
        }
        let event = self.package(sample, true).await?;
        self.provisional_artifact(event, sample)
    }

    #[cfg(test)]
    pub(crate) fn provisional_is_signed(&self, sample: Duration) -> StageResult<bool> {
        self.load_provisional_pending(sample)
            .map(|(_, pending)| pending.signed_event.is_some())
    }

    #[cfg(test)]
    pub(crate) async fn finalize_provisional(
        &self,
        sample: Duration,
    ) -> StageResult<ProvisionalPackage> {
        let (mut lifecycle, mut pending) = self.load_provisional_pending(sample)?;
        if pending.signed_event.is_some() {
            return Err(StageError::Invalid);
        }
        let publication = self.publication(&pending);
        pending.signed_event = Some(
            self.publisher()
                .prepare_key_package(publication.clone())
                .await
                .map_err(|_| StageError::Invalid)?,
        );
        let event = self.exact_artifact(&pending, &publication)?;
        lifecycle.pending_replacement = Some(pending);
        self.save(&lifecycle)?;
        self.provisional_artifact(event, sample)
    }

    #[cfg(all(
        target_os = "linux",
        target_arch = "x86_64",
        any(test, marmish_peer_receiver_candidate)
    ))]
    pub(crate) fn prestage_managed_provisional(
        path: &Path,
        key: &SqlCipherKey,
        window: &crate::peer_invitation::SchemaWindow,
    ) -> StageResult<()> {
        let check = || crate::peer_invitation::NativeFence::Schema(window).check();
        check()?;
        #[cfg(test)]
        let point = std::time::Instant::now();
        let storage = open_provisional_storage(path, key, managed_options())?;
        #[cfg(test)]
        {
            let (count, elapsed) = storage.migration_summary();
            println!(
                "package_fixture_timing schema_open_ms={:.3} migrations={count} migration_ms={:.3}",
                point.elapsed().as_secs_f64() * 1000.0,
                elapsed.as_secs_f64() * 1000.0,
            );
        }
        check()?;
        Self::require_unjoined_storage(&storage)?;
        Self::require_empty_package_storage(&storage)?;
        drop(storage);
        check()
    }

    #[cfg(all(
        target_os = "linux",
        target_arch = "x86_64",
        any(test, marmish_peer_receiver_candidate)
    ))]
    pub(crate) fn open_managed_provisional(
        path: &Path,
        key: &SqlCipherKey,
        signer: Arc<Keys>,
        origin: &str,
        slot: String,
        window: &crate::peer_invitation::NativeWindow,
    ) -> StageResult<Self> {
        window.check()?;
        Self::open_provisional_with_options(
            path,
            key,
            signer,
            origin,
            slot,
            false,
            managed_options(),
        )
    }

    #[cfg(all(
        target_os = "linux",
        target_arch = "x86_64",
        any(test, marmish_peer_receiver_candidate)
    ))]
    pub(crate) async fn issue_managed_provisional(
        &self,
        window: &crate::peer_invitation::NativeWindow,
    ) -> StageResult<ProvisionalPackage> {
        #[cfg(test)]
        let point = std::time::Instant::now();
        self.require_unjoined_provisional()?;
        Self::require_empty_package_storage(&self.storage)?;
        window.check()?;
        self.save(&KeyPackageLifecycleState::slot_only(self.slot.clone()))?;
        #[cfg(test)]
        window.timing("native_bootstrap", point);
        let mut publisher = self.publisher();
        publisher.window = Some(window.clone());
        let sample = window.stage_sample()?;
        self.prepare_new_with_publisher(sample, publisher).await?;
        window.check()?;
        self.require_unjoined_provisional()?;
        let artifact = self.read_provisional(window.stage_sample()?)?;
        window.check()?;
        Ok(artifact)
    }

    pub(crate) fn read_provisional(&self, sample: Duration) -> StageResult<ProvisionalPackage> {
        let (_, pending) = self.load_provisional_pending(sample)?;
        let event = self.exact_artifact(&pending, &self.publication(&pending))?;
        Self::check_provisional_artifact(&pending, event, sample)
    }

    pub(crate) fn require_unjoined_provisional(&self) -> StageResult<()> {
        Self::require_unjoined_storage(&self.storage)
    }

    fn require_unjoined_storage(storage: &SqliteAccountStorage) -> StageResult<()> {
        use cgka_traits::storage::GroupStorage;

        if !storage
            .list_groups()
            .map_err(|_| StageError::Storage)?
            .is_empty()
        {
            return Err(StageError::Invalid);
        }
        Ok(())
    }

    #[cfg(all(
        target_os = "linux",
        target_arch = "x86_64",
        any(test, marmish_peer_receiver_candidate)
    ))]
    fn require_empty_package_storage(storage: &SqliteAccountStorage) -> StageResult<()> {
        if storage
            .key_package_lifecycle()
            .map_err(|_| StageError::Storage)?
            .is_some()
            || !durably_owned_key_packages(storage, ProtocolProfile::Current)
                .map_err(|_| StageError::Storage)?
                .is_empty()
        {
            return Err(StageError::Invalid);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fixture_replace_with_slot_only(&self) -> StageResult<()> {
        self.save(&KeyPackageLifecycleState::slot_only(self.slot.clone()))
    }

    #[cfg(test)]
    fn provisional_artifact(
        &self,
        event: NativePackage,
        sample: Duration,
    ) -> StageResult<ProvisionalPackage> {
        let (_, pending) = self.load_provisional_pending(sample)?;
        Self::check_provisional_artifact(&pending, event, sample)
    }

    fn check_provisional_artifact(
        pending: &PendingKeyPackageReplacement,
        event: NativePackage,
        sample: Duration,
    ) -> StageResult<ProvisionalPackage> {
        let artifact = ProvisionalPackage {
            event,
            authored: pending.authored_created_at.0,
            not_before: pending.not_before.0,
        };
        artifact.check_lifetime(sample.as_secs())?;
        Ok(artifact)
    }

    pub(super) fn load_provisional_pending(
        &self,
        sample: Duration,
    ) -> StageResult<(KeyPackageLifecycleState, PendingKeyPackageReplacement)> {
        let lifecycle = self.load()?.ok_or(StageError::Invalid)?;
        let mut baseline = lifecycle.clone();
        let pending = baseline
            .pending_replacement
            .take()
            .ok_or(StageError::Invalid)?;
        if baseline.phase != MaintenancePhase::PendingPublication {
            return Err(StageError::Invalid);
        }
        baseline.phase = MaintenancePhase::Complete;
        if baseline != KeyPackageLifecycleState::slot_only(self.slot.clone()) {
            return Err(StageError::Invalid);
        }
        self.check_pending(&pending, sample.as_secs())?;
        Ok((lifecycle, pending))
    }
}

fn open_provisional_storage(
    path: &Path,
    key: &SqlCipherKey,
    options: SqliteStorageOptions,
) -> StageResult<SqliteAccountStorage> {
    marmish_engine::initialize_storage().map_err(|_| StageError::Storage)?;
    SqliteAccountStorage::open_encrypted_with_options(
        path,
        key,
        SqliteStorageOptions {
            journal_mode: SqliteJournalMode::Delete,
            ..options
        },
    )
    .map_err(|_| StageError::Storage)
}

#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    any(test, marmish_peer_receiver_candidate)
))]
fn managed_options() -> SqliteStorageOptions {
    SqliteStorageOptions {
        busy_timeout_ms: 0,
        ..SqliteStorageOptions::default()
    }
}
