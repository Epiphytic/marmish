//! Sign an isolated stage's KeyPackage without granting MDK any transport access.

use super::*;
use cgka_engine::key_package::durably_owned_key_packages;
use cgka_traits::maintenance::{
    KEY_PACKAGE_GENERATION_REVISION, MaintenancePhase, PendingKeyPackageReplacement,
    TransportFanoutAttemptState, WallClock,
};
use marmot_session::{
    AccountError, OsMaintenanceRandom, StaticTransportRouting, SystemMonotonicClock,
};
use nostr::event::Event;
use std::path::Path;
use transport_nostr_adapter::{NostrPublishOutcome, NostrSubscription};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StageError {
    Invalid,
    Storage,
    Unavailable,
}

type StageResult<T> = std::result::Result<T, StageError>;

pub(crate) struct NativePackage {
    pub(crate) id: [u8; 32],
    pub(crate) bytes: Zeroizing<Vec<u8>>,
    pub(crate) expires: u64,
}

pub(crate) struct OpenStage {
    storage: SqliteAccountStorage,
    signer: Arc<Keys>,
    endpoint: TransportEndpoint,
    slot: String,
}

impl OpenStage {
    pub(crate) fn open(
        path: &Path,
        key: [u8; 32],
        signer: Arc<Keys>,
        origin: &str,
        slot: String,
    ) -> StageResult<Self> {
        marmish_engine::initialize_storage().map_err(|_| StageError::Storage)?;
        let storage = SqliteAccountStorage::open_encrypted(path, &SqlCipherKey::raw(key))
            .map_err(|_| StageError::Storage)?;
        Ok(Self {
            storage,
            signer,
            endpoint: TransportEndpoint(origin.to_owned()),
            slot,
        })
    }

    pub(crate) async fn package(
        &self,
        sample: Duration,
        generation_allowed: bool,
    ) -> StageResult<NativePackage> {
        let empty = KeyPackageLifecycleState::slot_only(self.slot.clone());
        let mut lifecycle = match self.load()? {
            Some(lifecycle) => lifecycle,
            None if generation_allowed => {
                self.save(&empty)?;
                empty.clone()
            }
            None => return Err(StageError::Invalid),
        };
        if lifecycle == empty && generation_allowed {
            self.prepare_new(sample).await?;
            lifecycle = self.load()?.ok_or(StageError::Invalid)?;
        }
        let mut baseline = lifecycle.clone();
        let mut pending = baseline
            .pending_replacement
            .take()
            .ok_or(StageError::Invalid)?;
        if baseline.phase != MaintenancePhase::PendingPublication {
            return Err(StageError::Invalid);
        }
        baseline.phase = MaintenancePhase::Complete;
        if baseline != empty {
            return Err(StageError::Invalid);
        }
        self.check_pending(&pending, sample.as_secs())?;
        let publication = self.publication(&pending);
        if pending.signed_event.is_none() {
            if !generation_allowed {
                return Err(StageError::Invalid);
            }
            pending.signed_event = Some(
                self.publisher()
                    .prepare_key_package(publication.clone())
                    .await
                    .map_err(|_| StageError::Invalid)?,
            );
            lifecycle.pending_replacement = Some(pending.clone());
            self.save(&lifecycle)?;
        }
        self.exact_artifact(&pending, &publication)
    }

    fn load(&self) -> StageResult<Option<KeyPackageLifecycleState>> {
        self.storage
            .key_package_lifecycle()
            .map_err(|_| StageError::Storage)
    }

    fn save(&self, state: &KeyPackageLifecycleState) -> StageResult<()> {
        self.storage
            .put_key_package_lifecycle(state)
            .map_err(|_| StageError::Storage)
    }

    async fn prepare_new(&self, sample: Duration) -> StageResult<()> {
        let role = SessionConfig::with_storage(
            self.storage.clone(),
            Duration::ZERO,
            self.signer.public_key().to_bytes().to_vec(),
            Box::new(NostrMlsPeeler::new().with_welcome_signer_arc(self.signer.clone())),
        )
        .account_identity_proof_signer(Arc::new(ProofSigner(self.signer.clone())))
        .supported_app_components(components());
        let endpoints = vec![self.endpoint.clone()];
        let mut runtime = AccountDeviceRuntime::open(role, || {
            Ok::<_, marmot_session::SessionError>((
                NoIo,
                StaticTransportRouting::new(endpoints.clone()),
                self.publisher(),
            ))
        })
        .map_err(|_| StageError::Storage)?
        .with_maintenance_sources(
            Arc::new(StageWall(sample)),
            Arc::new(SystemMonotonicClock::default()),
            Arc::new(OsMaintenanceRandom),
        );
        runtime
            .prepare_fresh_key_package(endpoints)
            .await
            .map_err(|err| match err {
                AccountError::KeyPackageAnnouncementPending | AccountError::ClockSkewBlocked => {
                    StageError::Unavailable
                }
                _ => StageError::Invalid,
            })?;
        Ok(())
    }

    fn publisher(&self) -> WithheldPublisher {
        WithheldPublisher(Publisher {
            keys: self.signer.clone(),
            relay: Arc::new(NoIo),
        })
    }

    fn check_pending(&self, pending: &PendingKeyPackageReplacement, wall: u64) -> StageResult<()> {
        let [target] = pending.targets.as_slice() else {
            return Err(StageError::Invalid);
        };
        let meta = key_package_metadata(&pending.key_package).map_err(|_| StageError::Invalid)?;
        // MDK adds these mandatory components alongside the configured profile.
        let expected_components = components()
            .chain([
                APP_COMPONENTS_COMPONENT_ID,
                ACCOUNT_IDENTITY_PROOF_COMPONENT_ID,
            ])
            .collect::<BTreeSet<_>>();
        let native_components = meta.app_components.iter().copied().collect::<BTreeSet<_>>();
        if pending.generation_revision != KEY_PACKAGE_GENERATION_REVISION
            || pending.authored_created_at.0 > wall
            || !(pending.not_before.0..pending.not_after.0).contains(&wall)
            || pending.attempt_count != 0
            || pending.last_failure_code.is_some()
            || pending.consumed_at.is_some()
            || target.endpoint != self.endpoint
            || target.state != TransportFanoutAttemptState::Unattempted
            || target.attempt_count != 0
            || target.last_attempt_at.is_some()
            || target.failure_code.is_some()
            || meta.protocol_profile != ProtocolProfile::Current
            || meta.credential_identity_hex != self.signer.public_key().to_hex()
            || meta.key_package_ref_hex != hex::encode(&pending.key_package_ref)
            || !meta.last_resort
            || meta.not_before != pending.not_before.0
            || meta.not_after != pending.not_after.0
            || native_components != expected_components
        {
            return Err(StageError::Invalid);
        }
        let owned = durably_owned_key_packages(&self.storage, ProtocolProfile::Current)
            .map_err(|_| StageError::Storage)?;
        if !matches!(owned.as_slice(), [package] if package.bytes() == pending.key_package.bytes())
        {
            return Err(StageError::Invalid);
        }
        Ok(())
    }

    fn publication(&self, pending: &PendingKeyPackageReplacement) -> KeyPackagePublication {
        KeyPackagePublication {
            account_id: MemberId::new(self.signer.public_key().to_bytes().to_vec()),
            key_package: pending.key_package.clone(),
            slot_id: self.slot.clone(),
            created_at: pending.authored_created_at,
            endpoints: vec![self.endpoint.clone()],
        }
    }

    fn exact_artifact(
        &self,
        pending: &PendingKeyPackageReplacement,
        publication: &KeyPackagePublication,
    ) -> StageResult<NativePackage> {
        let artifact = pending.signed_event.as_ref().ok_or(StageError::Invalid)?;
        let native: NostrTransportEvent =
            serde_json::from_slice(&artifact.bytes).map_err(|_| StageError::Invalid)?;
        let event: Event =
            serde_json::from_slice(&artifact.bytes).map_err(|_| StageError::Invalid)?;
        let dto = Publisher::publication(publication).map_err(|_| StageError::Invalid)?;
        if artifact.id.as_slice() != event.id.to_bytes()
            || event.pubkey != self.signer.public_key()
            || artifact.created_at != pending.authored_created_at
            || event.created_at.as_secs() != pending.authored_created_at.0
            || event.verify().is_err()
            || serde_json::to_vec(&event).map_err(|_| StageError::Invalid)? != artifact.bytes
            || !dto
                .matches_prepared_event(&native)
                .map_err(|_| StageError::Invalid)?
        {
            return Err(StageError::Invalid);
        }
        Ok(NativePackage {
            id: event.id.to_bytes(),
            bytes: Zeroizing::new(artifact.bytes.clone()),
            expires: pending.not_after.0,
        })
    }
}

fn components() -> impl Iterator<Item = u16> {
    default_group_components()
        .into_iter()
        .chain([NOSTR_ROUTING_COMPONENT_ID])
}

struct StageWall(Duration);

impl WallClock for StageWall {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.as_secs())
    }
    fn now_ms(&self) -> u64 {
        u64::try_from(self.0.as_millis()).unwrap_or(u64::MAX)
    }
}

struct WithheldPublisher(Publisher);

#[async_trait]
impl KeyPackagePublisher for WithheldPublisher {
    async fn prepare_key_package(
        &self,
        publication: KeyPackagePublication,
    ) -> std::result::Result<SignedPublicationArtifact, KeyPackagePublishError> {
        let mut artifact = self.0.prepare_key_package(publication).await?;
        let event: Event = serde_json::from_slice(&artifact.bytes)
            .map_err(|_| KeyPackagePublishError::unexposed("peer_package_decode"))?;
        artifact.bytes = serde_json::to_vec(&event)
            .map_err(|_| KeyPackagePublishError::unexposed("peer_package_encode"))?;
        Ok(artifact)
    }

    async fn publish_prepared_key_package(
        &self,
        _: &KeyPackagePublication,
        _: &SignedPublicationArtifact,
    ) -> std::result::Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(KeyPackagePublishError::unexposed("peer_package_withheld"))
    }
}

struct NoIo;

#[async_trait]
impl TransportAdapter for NoIo {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
    async fn deactivate_account(
        &self,
        _: &MemberId,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
    async fn receive(
        &self,
    ) -> std::result::Result<Option<TransportDelivery>, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
    async fn publish(
        &self,
        _: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
}

#[async_trait]
impl NostrRelayClient for NoIo {
    async fn subscribe(
        &self,
        _: NostrSubscription,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
    async fn unsubscribe(
        &self,
        _: NostrSubscription,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
    async fn unsubscribe_account(
        &self,
        _: &MemberId,
        _: &[NostrSubscription],
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
    async fn publish_event(
        &self,
        _: &[TransportEndpoint],
        _: &NostrTransportEvent,
        _: usize,
    ) -> std::result::Result<NostrPublishOutcome, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
    async fn publish_event_for_account(
        &self,
        _: &MemberId,
        _: &[TransportEndpoint],
        _: &NostrTransportEvent,
        _: usize,
    ) -> std::result::Result<NostrPublishOutcome, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn peer_invitation_native_unsigned_recovery_does_not_rotate() {
        let database = tempfile::NamedTempFile::new().unwrap();
        let signer = Arc::new(Keys::generate());
        let slot = "a3".repeat(16);
        let stage = OpenStage::open(
            database.path(),
            [0x25; 32],
            signer,
            "wss://fixture.example/",
            slot,
        )
        .unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let original = stage.package(now, true).await.unwrap();
        let mut unsigned = stage.load().unwrap().unwrap();
        let original_bundle = {
            let pending = unsigned.pending_replacement.as_mut().unwrap();
            pending.signed_event = None;
            pending.key_package.clone()
        };
        stage.save(&unsigned).unwrap();
        assert!(matches!(
            stage.package(now, false).await,
            Err(StageError::Invalid)
        ));
        assert_eq!(stage.load().unwrap().unwrap(), unsigned);
        let signed = stage.package(now, true).await.unwrap();
        assert_eq!(original.id, signed.id);
        let mut obsolete = stage.load().unwrap().unwrap();
        assert_eq!(
            obsolete.pending_replacement.as_ref().unwrap().key_package,
            original_bundle
        );
        assert_eq!(
            stage.package(now, false).await.unwrap().bytes.as_slice(),
            signed.bytes.as_slice()
        );

        obsolete
            .pending_replacement
            .as_mut()
            .unwrap()
            .generation_revision = 0;
        stage.save(&obsolete).unwrap();
        assert!(matches!(
            stage.package(now, true).await,
            Err(StageError::Invalid)
        ));
        assert_eq!(stage.load().unwrap().unwrap(), obsolete);
        let owned = durably_owned_key_packages(&stage.storage, ProtocolProfile::Current).unwrap();
        assert!(matches!(owned.as_slice(), [owned] if owned.bytes() == original_bundle.bytes()));
    }
}
