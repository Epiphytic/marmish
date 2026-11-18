use std::sync::Arc;

use cgka_engine::EngineBuilder;
use cgka_engine::account_identity_proof::{
    AccountIdentityProofRequest, AccountIdentityProofSigner,
};
use cgka_traits::app_components::{
    NOSTR_ROUTING_COMPONENT_ID, NostrRoutingV1, PRIVATE_USE_APP_COMPONENT_ID_START,
    default_group_components,
};
use cgka_traits::engine::CgkaEngine;
use cgka_traits::{MemberId, TransportEndpoint};
use nostr::event::{EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::nips::nip44::{self, Version};
use nostr::types::Timestamp;
use storage_sqlite::{SqliteAccountStorage, SqliteStorageOptions};
use transport_nostr_adapter::NostrKeyPackagePublication;
use transport_nostr_peeler::NostrMlsPeeler;

use super::super::super::HardenedPeerConnection;
use super::*;

pub(super) const A: &str = "wss://consent-a.example/";
pub(super) const B: &str = "wss://consent-b.example/";
pub(super) const UID: u32 = 15200;
pub(super) const DB_KEY: [u8; 32] = [0x1d; 32];

pub(super) struct Fixture {
    pub(super) file: tempfile::NamedTempFile,
    pub(super) client: Keys,
    pub(super) server: Keys,
    pub(super) wall: u64,
}

impl Fixture {
    pub(super) fn new() -> Self {
        Self {
            file: tempfile::NamedTempFile::new().unwrap(),
            client: Keys::generate(),
            server: Keys::generate(),
            wall: Timestamp::now().as_secs(),
        }
    }

    pub(super) fn cursor(
        &self,
        key: [u8; 32],
        account: PublicKey,
    ) -> super::super::super::Result<CursorStore> {
        let connection = HardenedPeerConnection::open(self.file.path(), key)?;
        let origins = [
            Route::configured(A).unwrap().id,
            Route::configured(B).unwrap().id,
        ];
        CursorStore::attach(connection, account.to_bytes(), &origins)
    }

    pub(super) fn start(&self, policy: Policy) -> Checkpoint {
        let cursor = self.cursor(DB_KEY, self.client.public_key()).unwrap();
        Checkpoint::attach(cursor, self.client.clone(), UID, &[A, B], policy).unwrap()
    }

    pub(super) fn offer(&self, server: &Keys, route: &str, wall: u64) -> wire::Prepared<Offer> {
        let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
        let routing = NostrRoutingV1::new([0xc4; 32], vec![route.to_owned()]).unwrap();
        codec
            .prepare_offer(server, self.client.public_key(), [0x2a; 16], &routing, wall)
            .unwrap()
    }

    pub(super) fn submit(
        &self,
        checkpoint: &mut Checkpoint,
        source: &Source,
        offer: &wire::Prepared<Offer>,
    ) -> Status {
        let disposition = checkpoint
            .admit(source, raw(offer.frame()), self.wall)
            .unwrap();
        disposition.status
    }
}

pub(super) fn raw(frame: &[u8]) -> RawRecord<'_> {
    let outer: Event = serde_json::from_slice(frame).unwrap();
    RawRecord {
        id: outer.id.to_bytes(),
        stamp: outer.created_at.as_secs(),
        bytes: frame,
    }
}

pub(super) fn rewrap(frame: &[u8], recipient: &Keys) -> Vec<u8> {
    let outer: Event = serde_json::from_slice(frame).unwrap();
    let seal = Zeroizing::new(
        nip44::decrypt_to_bytes(recipient.secret_key(), &outer.pubkey, &outer.content).unwrap(),
    );
    let signer = Keys::generate();
    let ciphertext = nip44::encrypt(
        signer.secret_key(),
        &recipient.public_key(),
        seal.as_slice(),
        Version::V2,
    )
    .unwrap();
    let outer = EventBuilder::new(outer.kind, ciphertext)
        .tags(outer.tags.iter().cloned())
        .custom_created_at(outer.created_at)
        .finalize(&signer)
        .unwrap();
    serde_json::to_vec(&outer).unwrap()
}

struct ProofSigner(Keys);

impl AccountIdentityProofSigner for ProofSigner {
    fn sign_account_identity_proof(
        &self,
        req: &AccountIdentityProofRequest,
    ) -> std::result::Result<[u8; 64], String> {
        assert_eq!(
            req.account_identity.as_slice(),
            self.0.public_key().to_bytes()
        );
        req.signature_from_signed_event(req.proof_event()?.finalize(&self.0).unwrap())
    }
}

pub(super) async fn native_package(client: &Keys, route: &str) -> (Vec<u8>, u64) {
    marmish_engine::initialize_storage().unwrap();
    let storage = SqliteAccountStorage::in_memory_with_options(SqliteStorageOptions {
        cipher_memory_security: false,
        ..Default::default()
    })
    .unwrap();
    let account_id = MemberId::new(client.public_key().to_bytes().to_vec());
    let mut engine = EngineBuilder::new(storage)
        .identity(account_id.as_slice().to_vec())
        .account_identity_proof_signer(Arc::new(ProofSigner(client.clone())))
        .supported_app_components(
            default_group_components()
                .into_iter()
                .chain([NOSTR_ROUTING_COMPONENT_ID]),
        )
        .peeler(Box::new(NostrMlsPeeler::new()))
        .build()
        .unwrap();
    let key_package = engine.fresh_key_package().await.unwrap();
    let metadata = cgka_engine::key_package_metadata(&key_package).unwrap();
    assert!(metadata.last_resort);
    let now = Timestamp::now().as_secs();
    let event = NostrKeyPackagePublication {
        client_name: None,
        account_id,
        key_package,
        key_package_slot_id: hex::encode([0x1a; 16]),
        key_package_ref: metadata.key_package_ref_hex,
        mls_ciphersuite: format!("0x{:04x}", metadata.ciphersuite),
        mls_extensions: hex_numbers(metadata.mls_extensions),
        mls_proposals: hex_numbers(metadata.mls_proposals),
        app_components: hex_numbers(
            metadata
                .app_components
                .into_iter()
                .filter(|v| *v >= PRIVATE_USE_APP_COMPONENT_ID_START),
        ),
        publish_endpoints: vec![TransportEndpoint(route.to_owned())],
    }
    .to_event_at(now)
    .unwrap();
    let package = EventBuilder::new(Kind::Custom(30443), event.content)
        .tags(event.tags.into_iter().map(|tag| Tag::parse(tag).unwrap()))
        .custom_created_at(Timestamp::from_secs(now))
        .finalize(client)
        .unwrap();
    (serde_json::to_vec(&package).unwrap(), now)
}

fn hex_numbers(values: impl IntoIterator<Item = u16>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| format!("0x{value:04x}"))
        .collect()
}
