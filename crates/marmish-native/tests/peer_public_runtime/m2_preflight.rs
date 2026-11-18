//! Exercise the closed SQLCipher image boundary without a relay or the M2 registry.

use super::*;
use cgka_traits::{
    EventSizeClass, TransportEndpointFailure, TransportPublishTarget, TransportReadBack,
};
use std::{
    fs::{self, File, OpenOptions, Permissions},
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
};
use storage_sqlite::{SqliteJournalMode, SqliteStorageOptions};

const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Default)]
struct JoinIo(Quarantine);

#[async_trait]
impl TransportAdapter for JoinIo {
    async fn activate_account(
        &self,
        activation: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        self.0.activate_account(activation).await
    }

    async fn sync_account_groups(
        &self,
        sync: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        self.0.sync_account_groups(sync).await
    }

    async fn deactivate_account(&self, account: &MemberId) -> Result<(), TransportAdapterError> {
        self.0.deactivate_account(account).await
    }

    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        self.0.publish(request).await
    }

    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        self.0.receive().await
    }

    async fn publish_commit(
        &self,
        _: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        Err(self
            .0
            .deny(BlockedIo::PublishCommit, TransportAdapterError::Closed))
    }

    async fn read_back(
        &self,
        _: &MemberId,
        _: &TransportMessage,
        endpoints: &[TransportEndpoint],
    ) -> Vec<TransportReadBack> {
        self.0.deny(
            BlockedIo::ReadBack,
            vec![TransportReadBack::NoAnswer; endpoints.len()],
        )
    }

    async fn size_class_over_known_limits(
        &self,
        _: &TransportMessage,
        _: &TransportPublishTarget,
    ) -> Option<EventSizeClass> {
        self.0.deny(BlockedIo::SizeCheck, None)
    }

    async fn commit_size_refusals(
        &self,
        _: &TransportMessage,
        _: &TransportPublishTarget,
    ) -> Vec<TransportEndpointFailure> {
        self.0.deny(BlockedIo::CommitSizeCheck, Vec::new())
    }
}

#[async_trait]
impl KeyPackagePublisher for JoinIo {
    fn legacy_slot_id(&self, _: &MemberId) -> Result<Option<String>, KeyPackagePublishError> {
        Err(self.0.deny_package(BlockedIo::LookupKeyPackageSlot))
    }

    async fn prepare_key_package(
        &self,
        _: KeyPackagePublication,
    ) -> Result<SignedPublicationArtifact, KeyPackagePublishError> {
        Err(self.0.deny_package(BlockedIo::PrepareKeyPackage))
    }

    async fn publish_prepared_key_package(
        &self,
        _: &KeyPackagePublication,
        _: &SignedPublicationArtifact,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.0.deny_package(BlockedIo::PublishKeyPackage))
    }

    async fn withdraw_key_package_slot(
        &self,
        _: &KeyPackageSlotWithdrawal,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.0.deny_package(BlockedIo::WithdrawKeyPackage))
    }

    async fn delete_key_package_event(
        &self,
        _: &KeyPackageEventDeletion,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.0.deny_package(BlockedIo::DeleteKeyPackage))
    }
}

fn open_delete(path: &Path, store: &Store) -> SqliteAccountStorage {
    marmish_engine::initialize_storage().unwrap();
    SqliteAccountStorage::open_encrypted_with_options(
        path,
        &SqlCipherKey::raw(store.key),
        SqliteStorageOptions {
            journal_mode: SqliteJournalMode::Delete,
            ..SqliteStorageOptions::default()
        },
    )
    .unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct Image {
    identity: (u64, u64),
    digest: [u8; 32],
}

impl Image {
    fn open(path: &Path) -> File {
        require_no_sidecars(path);
        let original = fs::symlink_metadata(path).unwrap();
        assert!(original.is_file());
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .unwrap();
        let metadata = file.metadata().unwrap();
        assert_eq!(
            (metadata.dev(), metadata.ino()),
            (original.dev(), original.ino())
        );
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        assert!((1..=MAX_IMAGE_BYTES).contains(&metadata.len()));
        file
    }

    fn read(path: &Path) -> Self {
        let file = Self::open(path);
        let metadata = file.metadata().unwrap();
        let mut digest = Sha256::new();
        assert_eq!(
            io::copy(&mut file.take(MAX_IMAGE_BYTES + 1), &mut digest).unwrap(),
            metadata.len()
        );
        let current = Self::open(path).metadata().unwrap();
        assert_eq!(
            (metadata.dev(), metadata.ino(), metadata.len()),
            (current.dev(), current.ino(), current.len())
        );
        Self {
            identity: (metadata.dev(), metadata.ino()),
            digest: digest.finalize().into(),
        }
    }
}

fn require_no_sidecars(path: &Path) {
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        assert!(matches!(
            fs::symlink_metadata(sidecar),
            Err(e) if e.kind() == io::ErrorKind::NotFound
        ));
    }
}

fn private_dir() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    assert_eq!(dir.path().metadata().unwrap().mode() & 0o7777, 0o700);
    dir
}

fn snapshot_authority(
    client: &Store,
    closed: &Image,
    routes: &StaticTransportRouting,
    io: &JoinIo,
    expected: &ExpectedJoin<'_>,
) -> Authority {
    let private = private_dir();
    let path = private.path().join("image.sqlite");
    let mut snapshot = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .unwrap();
    let created = snapshot.metadata().unwrap();
    let copied = io::copy(
        &mut Image::open(&client.path).take(MAX_IMAGE_BYTES + 1),
        &mut snapshot,
    )
    .unwrap();
    drop(snapshot);
    assert!((1..=MAX_IMAGE_BYTES).contains(&copied));
    let before = Image::read(&path);
    assert_eq!(before.identity, (created.dev(), created.ino()));
    assert_ne!(before.identity, closed.identity);
    assert_eq!(before.digest, closed.digest);
    assert_eq!(Image::read(&client.path), *closed);

    let storage = open_delete(&path, client);
    assert_eq!(storage.migration_summary().0, 0);
    let runtime = client.runtime(&storage, routes, io.clone(), io.clone());
    let authority = expected.verify(&storage, &runtime, &io.0, true).unwrap();
    assert_eq!(
        runtime.own_leaf_index(expected.group).unwrap(),
        authority.local_leaf_index
    );
    drop((runtime, storage));
    // MDK opens this disposable image for writing. Its bytes may change
    // during readback; only the closed source remains authoritative.
    assert_eq!(Image::read(&path).identity, before.identity);
    assert_eq!(Image::read(&client.path), *closed);
    fs::remove_file(&path).unwrap();
    require_no_sidecars(&path);
    private.close().unwrap();
    authority
}

#[tokio::test]
#[ignore = "opt-in no-network MDK and closed DELETE-mode image qualification"]
async fn pending_delete_image_reopens_through_public_runtime() {
    let started = Instant::now();
    let private = private_dir();
    let client = Store::new(private.path(), "client.sqlite", Arc::new(Keys::generate()));
    let founder = Store::new(private.path(), "founder.sqlite", Arc::new(Keys::generate()));
    let endpoint = TransportEndpoint(RELAY.into());
    let routes = StaticTransportRouting::new(vec![endpoint.clone()]);
    let route = NostrRoutingV1::new(random(), vec![endpoint.0.clone()]).unwrap();
    let setup_guard = Quarantine::default();
    let storage = open_delete(&client.path, &client);
    let mut runtime = client.runtime(
        &storage,
        &routes,
        setup_guard.clone(),
        PackageSigner {
            keys: client.keys.clone(),
            slot: client.slot.clone(),
            guard: setup_guard.clone(),
        },
    );
    let (package, package_ref) =
        prepare_private_package(&storage, &mut runtime, &client, &endpoint).await;
    let package_event = package.source.as_ref().unwrap().event_id.clone();
    drop((runtime, storage));
    let prepared = Image::read(&client.path);

    let (founder_storage, mut founder_runtime) = founder.open(&routes, &setup_guard);
    let (group, mut welcomes) = found_room(&mut founder_runtime, vec![package], &route).await;
    assert_eq!(welcomes.len(), 1);
    let (_, welcome) = unwrapped_delivery(&founder, &client, &welcomes.pop().unwrap()).await;
    verify_hints(&welcome, &client, &founder, &package_event, &route)
        .await
        .unwrap();
    drop((founder_runtime, founder_storage));
    assert!(setup_guard.attempts().is_empty());
    let welcome_id = welcome.id.clone();
    let expected = ExpectedJoin {
        group: &group,
        welcome: &welcome_id,
        founder: founder.account(),
        client: client.account(),
        package_ref: &package_ref,
        route: &route,
    };
    let delivery = client.delivery(welcome, &endpoint);
    assert_eq!(Image::read(&client.path), prepared);
    let setup = started.elapsed();
    let quarantine = JoinIo::default();

    let ingest_started = Instant::now();
    let storage = open_delete(&client.path, &client);
    assert_eq!(storage.migration_summary().0, 0);
    let mut runtime = client.runtime(&storage, &routes, quarantine.clone(), quarantine.clone());
    let mut receipt = runtime.ingest_delivery(&delivery).await.unwrap();
    assert_eq!(receipt.outcome, IngestOutcome::Processed);
    assert!(!receipt.left_object_unpersisted);
    receipt.effects.local_accept_duration = None;
    receipt.effects.publish_duration = None;
    assert!(admissible_effects(
        &receipt.effects,
        &expected.event(),
        &quarantine.0
    ));
    let joined = expected
        .verify(&storage, &runtime, &quarantine.0, true)
        .unwrap();
    drop((runtime, storage));
    let closed = Image::read(&client.path);
    assert_eq!(closed.identity, prepared.identity);
    assert_ne!(closed.digest, prepared.digest);
    let ingest_and_close = ingest_started.elapsed();

    let read_started = Instant::now();
    for _ in 0..2 {
        assert_eq!(
            snapshot_authority(&client, &closed, &routes, &quarantine, &expected),
            joined
        );
    }
    assert!(quarantine.0.attempts().is_empty());
    eprintln!(
        "m2_native_preflight setup_ms={:.2} ingest_and_close_ms={:.2} two_snapshot_reads_ms={:.2} total_ms={:.2}",
        setup.as_secs_f64() * 1000.0,
        ingest_and_close.as_secs_f64() * 1000.0,
        read_started.elapsed().as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64() * 1000.0
    );
}
