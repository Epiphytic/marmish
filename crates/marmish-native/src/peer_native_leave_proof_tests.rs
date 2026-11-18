//! Private two-account probe for native SelfRemove through terminal readback.

use super::*;

use cgka_traits::capabilities::{Capability, CapabilityRequirement, Feature, RequirementLevel};
use cgka_traits::engine::{GroupEvent, GroupMemberLeaf};
use cgka_traits::ingest::IngestOutcome;
use cgka_traits::transport_adapter::TransportEndpointReceipt;
use cgka_traits::{
    MemberId, MessageId, Timestamp, TransportAccountActivation, TransportAdapter,
    TransportAdapterError, TransportDelivery, TransportDeliveryPlane, TransportGroupSync,
};
use marmot_session::FeatureRegistry;
use nostr::event::Kind;
use openmls::prelude::ProposalType;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use storage_sqlite::{SqlCipherKey, SqliteJournalMode, SqliteStorageOptions};
use transport_nostr_peeler::NostrTransportEvent;

const MAX_LEAVE_EVENT: usize = 65_536;
const CONVERGENCE_BOUND: Duration = Duration::from_secs(6);
const CONVERGENCE_POLL: Duration = Duration::from_millis(20);

enum Expected {
    Proposal(TransportPublishRequest),
    Founder {
        account: MemberId,
        route: TransportPublishTarget,
    },
}

#[derive(Clone)]
struct LocalSink {
    expected: Arc<Expected>,
    accepted: Arc<Mutex<Option<TransportPublishRequest>>>,
}

impl LocalSink {
    fn new(expected: Expected) -> Self {
        Self {
            expected: Arc::new(expected),
            accepted: Arc::default(),
        }
    }

    fn accepted(&self) -> Option<TransportPublishRequest> {
        self.accepted.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl TransportAdapter for LocalSink {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        panic!("native leave fixture does not activate accounts")
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        panic!("native leave fixture does not subscribe")
    }

    async fn deactivate_account(&self, _: &MemberId) -> Result<(), TransportAdapterError> {
        panic!("native leave fixture does not deactivate accounts")
    }

    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        panic!("native leave fixture admits only explicit sealed deliveries")
    }

    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        let matches = match &*self.expected {
            Expected::Proposal(expected) => &request == expected,
            Expected::Founder { account, route } => {
                request.account_id == *account && request.target == *route
            }
        };
        if !matches || request.required_acks != 1 || !authenticated(&request.message) {
            return Err(TransportAdapterError::Closed);
        }
        let [endpoint] = request.target.endpoints() else {
            return Err(TransportAdapterError::Closed);
        };
        if endpoint.0 != RELAY || request.validate_envelope_matches_target().is_err() {
            return Err(TransportAdapterError::Closed);
        }
        let report = TransportPublishReport {
            message_id: request.message.id.clone(),
            accepted: vec![TransportEndpointReceipt {
                endpoint: endpoint.clone(),
                accepted_at: Some(Timestamp(super::super::super::now())),
                ack_kind: None,
            }],
            failed: vec![],
            required_acks: request.required_acks,
        };
        let mut accepted = self.accepted.lock().unwrap();
        if accepted.is_some() {
            return Err(TransportAdapterError::Closed);
        }
        *accepted = Some(request);
        Ok(report)
    }
}

fn authenticated(message: &TransportMessage) -> bool {
    if !(1..=MAX_LEAVE_EVENT).contains(&message.payload.len()) {
        return false;
    }
    let Some(event) = NostrTransportEvent::from_transport_message(message).ok() else {
        return false;
    };
    event.to_verified_nostr_event().is_ok_and(|nostr| {
        nostr.kind == Kind::Custom(445)
            && nostr.id.to_bytes().as_slice() == message.id.as_slice()
            && event.to_transport_message().ok().as_ref() == Some(message)
    })
}

fn open_delete<A: TransportAdapter>(
    store: &Store,
    routing: StaticTransportRouting,
    adapter: A,
    wall: u64,
) -> (
    SqliteAccountStorage,
    AccountDeviceRuntime<A, StaticTransportRouting, Publisher>,
) {
    use cgka_traits::maintenance::KeyPackageLifecycleState;
    use cgka_traits::storage::MaintenanceStorage;

    marmish_engine::initialize_storage().unwrap();
    let options = SqliteStorageOptions {
        journal_mode: SqliteJournalMode::Delete,
        ..Default::default()
    };
    let storage = SqliteAccountStorage::open_encrypted_with_options(
        &store.path,
        &SqlCipherKey::raw(store.key),
        options,
    )
    .unwrap();
    if storage.key_package_lifecycle().unwrap().is_none() {
        storage
            .put_key_package_lifecycle(&KeyPackageLifecycleState::slot_only(hex::encode(
                random::<16>().unwrap(),
            )))
            .unwrap();
    }
    // OpenMLS requires every remaining leaf to advertise support for this
    // non-default proposal. The empty MDK registry cannot form this commit.
    let mut features = FeatureRegistry::new();
    features.register(
        Feature("self-remove"),
        CapabilityRequirement {
            requires: Capability::Proposal(u16::from(ProposalType::SelfRemove)),
            level: RequirementLevel::Required,
            description: "Native MLS SelfRemove",
        },
    );
    let config =
        super::super::super::postjoin_session_config(storage.clone(), store.signer.clone())
            .feature_registry(features);
    let runtime = AccountDeviceRuntime::open(config, || {
        Ok::<_, SessionError>((
            adapter,
            routing,
            Publisher {
                keys: store.signer.clone(),
                relay: Arc::new(NoIo),
            },
        ))
    })
    .unwrap();
    let runtime = runtime.with_maintenance_sources(
        Arc::new(FixedWall(wall)),
        Arc::new(SystemMonotonicClock::default()),
        Arc::new(OsMaintenanceRandom),
    );
    (storage, runtime)
}

fn open_delete_pending(
    store: &Store,
    routing: StaticTransportRouting,
    wall: u64,
) -> (SqliteAccountStorage, Runtime, DeniedTransport) {
    let denied = DeniedTransport::default();
    let (storage, runtime) = open_delete(store, routing, denied.clone(), wall);
    (storage, runtime, denied)
}

fn closed_image(path: &Path) -> (u64, u64) {
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        assert!(
            matches!(fs::symlink_metadata(sidecar), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        );
    }
    let metadata = fs::symlink_metadata(path).unwrap();
    assert!(metadata.is_file() && metadata.nlink() == 1 && metadata.mode() & 0o7777 == 0o600);
    assert!((1..=128 * 1024 * 1024).contains(&metadata.len()));
    (metadata.dev(), metadata.ino())
}

struct SealedRemoval {
    group: GroupId,
    source: EpochPin,
    founder_target: EpochPin,
    founder: MemberId,
    recipient: MemberId,
    retained_leaf: u32,
    commit: TransportMessage,
    digest: [u8; 32],
}

impl SealedRemoval {
    fn delivery<A: TransportAdapter>(
        &self,
        runtime: &AccountDeviceRuntime<A, StaticTransportRouting, Publisher>,
        store: &Store,
    ) -> Option<TransportDelivery> {
        if store.account() != self.recipient
            || runtime.self_id() != self.recipient
            || !authenticated(&self.commit)
            || self.digest != digest(&self.commit)
            || runtime.epoch_pin(&self.group).ok()? != self.source
            || runtime.own_leaf_index(&self.group).ok()? != self.retained_leaf
        {
            return None;
        }
        Some(group_delivery(store, &self.group, self.commit.clone()))
    }

    fn absent<A: TransportAdapter>(
        &self,
        runtime: &AccountDeviceRuntime<A, StaticTransportRouting, Publisher>,
        expected_commit: &MessageId,
    ) -> bool {
        if runtime.self_id() != self.recipient
            || &self.commit.id != expected_commit
            || self.digest != digest(&self.commit)
            || self.source.epoch.0.checked_add(1) != Some(self.founder_target.epoch.0)
        {
            return false;
        }
        runtime
            .group_authority_view()
            .with_group_authority_snapshot(&self.group, |storage, authority| {
                let record = storage.get_group(&self.group)?;
                let facts = authority.facts;
                Ok::<_, SessionError>(
                    record.id == self.group
                        && facts.epoch == self.founder_target.epoch
                        && record.epoch == self.founder_target.epoch
                        && facts.removed
                        && !facts.is_member
                        && !facts.account_listed
                        && !facts.is_admin
                        && !facts.disbanded
                        && !authority.disbanding
                        && !facts.unrecoverable
                        && record.removed
                        && !record.unrecoverable
                        && record.disbanded.is_none()
                        && record.members.len() == 1
                        && record.members[0].id == self.founder
                        && storage.has_processed_transport_id(expected_commit)?
                        && storage.leave_request(&self.group)?.is_none(),
                )
            })
            .unwrap_or(false)
    }
}

fn digest(message: &TransportMessage) -> [u8; 32] {
    Sha256::digest(&message.payload).into()
}

fn group_delivery(store: &Store, group: &GroupId, message: TransportMessage) -> TransportDelivery {
    let mut delivery = store.delivery(message);
    delivery.group_id_hint = Some(group.clone());
    delivery.source.plane = TransportDeliveryPlane::Group;
    delivery
}

fn leaves<A: TransportAdapter>(
    runtime: &AccountDeviceRuntime<A, StaticTransportRouting, Publisher>,
    group: &GroupId,
) -> Vec<GroupMemberLeaf> {
    let membership = runtime.canonical_group_membership(group).unwrap();
    assert!(membership.local_leaf_active);
    let mut leaves = membership.member_leaves;
    leaves.sort_by_key(|leaf| leaf.leaf_index);
    leaves
}

#[test]
#[ignore = "opt-in native leave and cold SQLCipher removal without a remote relay"]
fn peer_native_leave_auto_commit_and_cold_remove() {
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(120), prove_leave())
                        .await
                        .expect("bounded native leave proof")
                })
        })
        .unwrap()
        .join();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

async fn prove_leave() {
    let dir = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    assert_eq!(fs::metadata(dir.path()).unwrap().mode() & 0o7777, 0o700);
    let founder = Store::new(
        dir.path(),
        "leave-founder.sqlite",
        Arc::new(Keys::generate()),
    );
    let recipient = Store::new(
        dir.path(),
        "leave-recipient.sqlite",
        Arc::new(Keys::generate()),
    );
    let at = super::super::super::now();
    let founder_inbox =
        StaticTransportRouting::new(endpoints()).with_inbox_route(recipient.account(), endpoints());
    let recipient_inbox =
        StaticTransportRouting::new(endpoints()).with_inbox_route(founder.account(), endpoints());
    let (founder_storage, mut founder_runtime) = open_delete(&founder, founder_inbox, NoIo, at);
    let (storage, mut staging) = open_delete(&recipient, recipient_inbox, NoIo, at);
    let (package, _) = prepare_private_package(&storage, &mut staging, &recipient.account()).await;
    drop(staging);
    drop(storage);
    let image = closed_image(&recipient.path);
    let route =
        super::super::super::NostrRoutingV1::new(random().unwrap(), vec![RELAY.into()]).unwrap();
    let (group, welcomes) =
        found_room(&mut founder_runtime, vec![package], &route, PURPOSE, vec![]).await;
    let source = founder_runtime.epoch_pin(&group).unwrap();
    let before = leaves(&founder_runtime, &group);
    assert!(
        founder_runtime
            .group_authority_view()
            .with_group_authority_snapshot(&group, |_, authority| Ok::<_, SessionError>(authority))
            .unwrap()
            .facts
            .is_admin
    );
    let [welcome]: [_; 1] = welcomes.try_into().ok().unwrap();
    drop(founder_runtime);
    drop(founder_storage);

    let unrouted = StaticTransportRouting::new(endpoints());
    let (storage, mut staged, denied) = open_delete_pending(&recipient, unrouted, at);
    let joined = staged
        .ingest_delivery(&recipient.delivery(welcome))
        .await
        .unwrap();
    assert!(joined.effects.events.iter().any(|event| matches!(
        event,
        GroupEvent::GroupJoined { group_id, welcomer: Some(who), .. }
            if group_id == &group && who == &founder.account()
    )));
    assert!(staged.epoch_pin(&group).unwrap() == source && leaves(&staged, &group) == before);
    staged
        .group_authority_view()
        .with_group_authority_snapshot(&group, |_, authority| {
            assert!(authority.lifecycle == GroupLifecycleState::Stable);
            assert!(authority.facts.is_member && !authority.facts.is_admin);
            assert!(!authority.disbanding && !authority.facts.unrecoverable);
            Ok::<_, SessionError>(())
        })
        .unwrap();
    let recipient_leaf = before
        .iter()
        .find(|leaf| leaf.member == recipient.account())
        .unwrap();
    assert_eq!(before.len(), 2);
    let target = TransportPublishTarget::Group {
        group_id: group.clone(),
        transport_group_id: route.nostr_group_id.to_vec(),
        endpoints: endpoints(),
    };
    let routed = StaticTransportRouting::new(endpoints()).with_group_route(
        group.clone(),
        route.nostr_group_id.to_vec(),
        endpoints(),
    );
    let started = Instant::now();
    let effects = staged
        .send_host_change_with_audit_context(
            SendIntent::Leave {
                group_id: group.clone(),
            },
            Default::default(),
        )
        .await
        .unwrap();
    let [failure] = effects.failures.as_slice() else {
        panic!("expected one missing-route refusal");
    };
    assert_eq!(failure.reason, "missing group route for transport group id");
    let pre_send_id = failure.message_id.clone();
    assert!(effects.queued.is_empty() && effects.reports.is_empty() && effects.fanout.is_empty());
    assert!(denied.take().is_empty());
    assert!(staged.outbound_fanouts().unwrap().is_empty());
    drop(staged);
    drop(storage);
    assert_eq!(closed_image(&recipient.path), image);

    let (storage, mut staged, denied) = open_delete_pending(&recipient, routed.clone(), at);
    let retained = retained_leave(&staged, &group);
    assert!(
        retained.sent.len() <= 4
            && retained
                .sent
                .iter()
                .all(|record| record.payload.len() <= MAX_LEAVE_EVENT)
    );
    let candidate =
        exact_candidate(&retained.sent, &group, source.epoch, &route.nostr_group_id).unwrap();
    assert!(candidate.id == pre_send_id);
    let proposal = TransportPublishRequest {
        account_id: recipient.account(),
        target: target.clone(),
        message: candidate.clone(),
        required_acks: 1,
    };
    check_positive(
        &mut staged,
        &group,
        &recipient.account(),
        &route.nostr_group_id,
        &proposal,
        false,
    )
    .await;
    assert!(authenticated(&proposal.message));
    let capture = started.elapsed();
    let denied_seed = staged
        .publish_session_effects(SessionEffects {
            events: vec![],
            publish: vec![PublishWork::Proposal {
                msg: candidate,
                queued_intent: None,
            }],
            queued: vec![],
            pending_convergence: vec![],
        })
        .await
        .unwrap();
    assert_denied_report(&denied_seed.reports, &proposal);
    assert!(denied.take().as_slice() == std::slice::from_ref(&proposal));
    let frozen = exact_fanout(&staged, &group, &proposal);
    drop(staged);
    drop(storage);
    assert_eq!(closed_image(&recipient.path), image);

    let recipient_sink = LocalSink::new(Expected::Proposal(proposal.clone()));
    let mut wrong_request = proposal.clone();
    wrong_request.required_acks = 2;
    assert!(recipient_sink.publish(wrong_request).await.is_err());
    let (storage, mut staged) =
        open_delete(&recipient, routed.clone(), recipient_sink.clone(), at + 5);
    assert!(staged.outbound_fanouts().unwrap().as_slice() == std::slice::from_ref(&frozen));
    let result = staged.resume_outbound_fanouts().await.unwrap();
    let [report] = result.reports.as_slice() else {
        panic!("expected only the exact resumed proposal report");
    };
    assert!(result.failures.is_empty());
    assert!(report.message_id == proposal.message.id && report.met_required_acks());
    assert!(recipient_sink.accepted().as_ref() == Some(&proposal));
    assert!(recipient_sink.publish(proposal.clone()).await.is_err());
    assert!(staged.epoch_pin(&group).unwrap() == source);
    assert!(leaves(&staged, &group) == before);
    let accepted = started.elapsed();

    let founder_sink = LocalSink::new(Expected::Founder {
        account: founder.account(),
        route: target,
    });
    let (_storage, mut founder_runtime) =
        open_delete(&founder, routed.clone(), founder_sink.clone(), at + 5);
    let proposal_delivery = group_delivery(&founder, &group, proposal.message.clone());
    let ingested = founder_runtime
        .ingest_delivery(&proposal_delivery)
        .await
        .unwrap();
    assert!(!ingested.left_object_unpersisted);
    assert!(matches!(
        ingested.outcome,
        IngestOutcome::Processed | IngestOutcome::Buffered { .. }
    ));
    assert!(
        founder_runtime
            .scheduled_self_remove_auto_commit_delay_ms(&group)
            .unwrap()
            .is_some()
    );
    let (commit, successor) = tokio::time::timeout(CONVERGENCE_BOUND, async {
        loop {
            founder_runtime.advance_convergence(&group).await.unwrap();
            let pin = founder_runtime.epoch_pin(&group).unwrap();
            if let Some(commit) = founder_sink.accepted() {
                assert!(source.epoch.0.checked_add(1) == Some(pin.epoch.0));
                assert!(pin.identity != source.identity);
                break (commit, pin);
            }
            tokio::time::sleep(CONVERGENCE_POLL).await;
        }
    })
    .await
    .expect("founder queued SelfRemove auto-commit");
    let after = leaves(&founder_runtime, &group);
    let remaining: Vec<_> = before
        .iter()
        .filter(|leaf| *leaf != recipient_leaf)
        .cloned()
        .collect();
    assert!(after == remaining);
    founder_runtime
        .group_authority_view()
        .with_group_authority_snapshot(&group, |store, authority| {
            assert!(authority.lifecycle == GroupLifecycleState::Stable && authority.facts.is_admin);
            assert!(store.has_processed_transport_id(&proposal.message.id)?);
            Ok::<_, SessionError>(())
        })
        .unwrap();

    // This private value survives the native handles' close/reopen. The Phase B
    // owner must persist its equivalent before enabling ingress.
    let mut sealed = SealedRemoval {
        group: group.clone(),
        source,
        founder_target: successor,
        founder: founder.account(),
        recipient: recipient.account(),
        retained_leaf: recipient_leaf.leaf_index,
        digest: digest(&commit.message),
        commit: commit.message,
    };
    assert!(!sealed.absent(&staged, &sealed.commit.id));
    sealed.digest[0] ^= 1;
    assert!(sealed.delivery(&staged, &recipient).is_none());
    sealed.digest[0] ^= 1;
    assert!(sealed.delivery(&staged, &founder).is_none());
    let delivery = sealed
        .delivery(&staged, &recipient)
        .expect("exact retained leaf");
    let result = staged.ingest_delivery(&delivery).await.unwrap();
    let terminated = |events: &[GroupEvent]| {
        events.iter().any(|event| matches!(
        event, GroupEvent::LocalGroupCopyTerminated { group_id, voluntary: true } if group_id == &group
    ))
    };
    let mut observed = terminated(&result.effects.events);
    tokio::time::timeout(CONVERGENCE_BOUND, async {
        while !observed {
            let effects = staged.advance_convergence(&group).await.unwrap();
            observed = terminated(&effects.events);
            if !observed {
                tokio::time::sleep(CONVERGENCE_POLL).await;
            }
        }
    })
    .await
    .expect("recipient native termination");
    assert!(sealed.absent(&staged, &sealed.commit.id));
    let removal = started.elapsed();
    drop(staged);
    drop(storage);
    assert_eq!(closed_image(&recipient.path), image);
    let reopening = Instant::now();
    let (storage, reopened, denied) = open_delete_pending(&recipient, routed, at + 6);
    assert!(
        sealed.absent(&reopened, &sealed.commit.id),
        "cold exact native removal"
    );
    assert!(!sealed.absent(&reopened, &proposal.message.id));
    assert!(denied.take().is_empty());
    let reopen_time = reopening.elapsed();
    drop(reopened);
    drop(storage);
    assert_eq!(closed_image(&recipient.path), image);
    eprintln!(
        "native leave fixture capture_ms={} accepted_ms={} removal_ms={} reopen_ms={}",
        capture.as_millis(),
        accepted.as_millis(),
        removal.as_millis(),
        reopen_time.as_millis()
    );
}
