//! One volatile Native MDK account and its sole permitted Terminal group.
use super::terminal_enrollment_transport::{
    Authority, GuardedAdapter, Lane, SingleRoomRouting, exact_selector,
};
use super::*;
use crate::terminal_bootstrap::enrollment::{Package, Plan, RETENTION};
use cgka_traits::app_components::ACCOUNT_IDENTITY_PROOF_COMPONENT_ID;
use cgka_traits::app_event::{AppMessageRetentionDecision, MarmotAppEvent};
use cgka_traits::{EpochId, TransportDeliveryPlane};
use nostr::event::Event;
use storage_sqlite::SqliteStorageOptions;

const PURPOSE: &str = "Marmish Terminal v1";
const WELCOME_CAP: usize = 48 * 1024;
const FRAME_CAP: usize = 32 * 1024;
const STATE: Error = Error::Mdk("terminal_enrollment_state");
const EXCHANGE: Error = Error::Mdk("terminal_enrollment_effect");
type VolatileRuntime = AccountDeviceRuntime<GuardedAdapter, SingleRoomRouting, WithheldPublisher>;

struct WithheldPublisher(Publisher);

#[async_trait]
impl KeyPackagePublisher for WithheldPublisher {
    async fn prepare_key_package(
        &self,
        package: KeyPackagePublication,
    ) -> std::result::Result<SignedPublicationArtifact, KeyPackagePublishError> {
        self.0.prepare_key_package(package).await
    }

    async fn publish_prepared_key_package(
        &self,
        _: &KeyPackagePublication,
        _: &SignedPublicationArtifact,
    ) -> std::result::Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(KeyPackagePublishError::unexposed(
            "terminal_package_withheld",
        ))
    }
}

pub(super) struct Room {
    pub(super) lane: Lane,
    routing: SingleRoomRouting,
    keys: Arc<Keys>,
    storage: SqliteAccountStorage,
    runtime: VolatileRuntime,
    client: MemberId,
    server: MemberId,
    room: Option<Installed>,
}

#[derive(Clone)]
struct Installed {
    group: GroupId,
    route: [u8; 32],
    digest: [u8; 32],
    subscription: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct RoomWitness {
    route: [u8; 32],
    subscription: String,
    local_leaf: u32,
    epoch: EpochId,
}

pub(super) struct HeldWelcome {
    delivery: TransportDelivery,
    peeled_id: MessageId,
}

pub(super) struct App {
    pub(super) bytes: Vec<u8>,
    pub(super) retention: AppMessageRetentionDecision,
}

pub(super) struct OpenFailure {
    pub(super) error: Error,
    pub(super) reaped: bool,
}

impl OpenFailure {
    fn before_open(error: Error) -> Self {
        Self {
            error,
            reaped: true,
        }
    }
}

impl Room {
    pub(super) async fn open(
        connector: RelayConnector,
        endpoint: TransportEndpoint,
        keys: Arc<Keys>,
        client: PublicKey,
        server: PublicKey,
        authority: Arc<Authority>,
        verified: bool,
    ) -> std::result::Result<Self, OpenFailure> {
        let own = keys.public_key();
        if client == server || (own != client && own != server) {
            return Err(OpenFailure::before_open(STATE));
        }
        let client = terminal_member(client);
        let server = terminal_member(server);
        let local = terminal_member(own);
        let peer = if client == local { &server } else { &client };
        let routing = SingleRoomRouting::new(endpoint.clone(), peer.clone());
        let mut lane = if verified {
            Lane::open_verified(
                connector,
                endpoint,
                keys.clone(),
                authority.clone(),
                WELCOME_CAP,
            )
            .await
        } else {
            Lane::open(
                connector,
                endpoint,
                keys.clone(),
                authority.clone(),
                WELCOME_CAP,
            )
            .await
        }
        .map_err(OpenFailure::before_open)?;
        lane.begin_active_enrollment();
        let setup = async {
            let (storage, runtime) = Self::prepare_runtime(&lane, keys.clone(), &local, &routing)?;
            authority
                .run(async { runtime.activate_transport(None).await.map_err(|_| EXCHANGE) })
                .await?;
            lane.pin_inbox().await?;
            Ok((storage, runtime))
        };
        let (storage, runtime) = match setup.await {
            Ok(setup) => setup,
            Err(error) => {
                return Err(OpenFailure {
                    error,
                    reaped: lane.close().await.is_ok(),
                });
            }
        };
        Ok(Self {
            lane,
            routing,
            keys,
            storage,
            runtime,
            client,
            server,
            room: None,
        })
    }

    fn prepare_runtime(
        lane: &Lane,
        keys: Arc<Keys>,
        local: &MemberId,
        routing: &SingleRoomRouting,
    ) -> Result<(SqliteAccountStorage, VolatileRuntime)> {
        let storage = SqliteAccountStorage::in_memory_with_options(SqliteStorageOptions {
            temp_store_memory: true,
            cipher_memory_security: true,
            ..SqliteStorageOptions::default()
        })
        .map_err(|_| STATE)?;
        let slot = hex::encode(entropy::<16>()?);
        storage
            .put_key_package_lifecycle(&KeyPackageLifecycleState::slot_only(slot))
            .map_err(|_| STATE)?;
        let config = SessionConfig::with_storage(
            storage.clone(),
            Duration::ZERO,
            local.as_slice().to_vec(),
            Box::new(NostrMlsPeeler::new().with_welcome_signer_arc(keys.clone())),
        )
        .account_identity_proof_signer(Arc::new(ProofSigner(keys.clone())))
        .supported_app_components(component_ids());
        let runtime = AccountDeviceRuntime::open(config, || {
            Ok::<_, marmot_session::SessionError>((
                lane.adapter.clone(),
                routing.clone(),
                WithheldPublisher(Publisher {
                    keys: keys.clone(),
                    relay: lane.adapter.relay.clone(),
                }),
            ))
        })
        .map_err(|_| STATE)?;
        Ok((storage, runtime))
    }

    pub(super) async fn prepare_package(&mut self) -> Result<Package> {
        if self.lane.adapter.account != self.client || self.room.is_some() {
            return Err(STATE);
        }
        let adapter = &self.lane.adapter;
        let package = adapter
            .authority
            .run(async {
                self.runtime
                    .prepare_fresh_key_package(vec![adapter.endpoint.clone()])
                    .await
                    .map_err(|_| STATE)
            })
            .await?;
        let lifecycle = self
            .storage
            .key_package_lifecycle()
            .map_err(|_| STATE)?
            .ok_or(STATE)?;
        if lifecycle.authored_event_id.is_some() || lifecycle.current_key_package.is_some() {
            return Err(STATE);
        }
        let pending = lifecycle.pending_replacement.as_ref().ok_or(STATE)?;
        if pending.attempt_count != 0
            || pending.consumed_at.is_some()
            || pending.key_package != package
        {
            return Err(STATE);
        }
        let artifact = pending.signed_event.as_ref().ok_or(STATE)?;
        let verified = serde_json::from_slice::<NostrTransportEvent>(&artifact.bytes)
            .map_err(|_| STATE)?
            .to_verified_nostr_event()
            .map_err(|_| STATE)?;
        if verified.kind != Kind::Custom(30443)
            || verified.pubkey != self.keys.public_key()
            || verified.id.to_bytes().as_slice() != artifact.id.as_slice()
        {
            return Err(STATE);
        }
        let wire = Package {
            event: verified.id.to_bytes(),
            reference: pending
                .key_package_ref
                .as_slice()
                .try_into()
                .map_err(|_| STATE)?,
            mls: package.bytes,
        };
        checked_package(&wire, &self.client)?;
        Ok(wire)
    }

    pub(super) async fn prepare_founder(
        &mut self,
        package: &Package,
        plan: &Plan,
    ) -> Result<marmot_session::CreateGroupEffects> {
        if self.lane.adapter.account != self.server || self.room.is_some() {
            return Err(STATE);
        }
        let group = GroupId::new(plan.group.to_vec());
        let route = encode_nostr_routing_v1(&expected_route(plan, &self.lane.adapter.endpoint)?)
            .map_err(|_| STATE)?;
        let package = checked_package(package, &self.client)?;
        let expires = unix_now()?.checked_add(RETENTION).ok_or(STATE)?;
        let authority = self.lane.adapter.authority.clone();
        let prepared = authority
            .run(async {
                self.runtime
                    .prepare_create_group_with_chosen_id_and_audit_context(
                        CreateGroupRequest {
                            name: PURPOSE.into(),
                            description: description(&plan.digest),
                            members: vec![package],
                            required_features: vec![],
                            initial_admins: vec![],
                            app_components: vec![
                                AppComponentData {
                                    component_id: NOSTR_ROUTING_COMPONENT_ID,
                                    data: route,
                                },
                                AppComponentData {
                                    component_id: GROUP_MESSAGE_RETENTION_COMPONENT_ID,
                                    data: RETENTION.to_be_bytes().to_vec(),
                                },
                            ],
                        },
                        group.clone(),
                        None,
                        Timestamp(expires),
                        Default::default(),
                    )
                    .await
                    .map_err(|_| EXCHANGE)
            })
            .await?;
        if prepared.group_id != group {
            return Err(STATE);
        }
        self.register(plan).await?;
        Ok(prepared)
    }

    pub(super) async fn publish_welcome(
        &mut self,
        prepared: marmot_session::CreateGroupEffects,
    ) -> Result<()> {
        self.check_live().await?;
        if self.room.as_ref().ok_or(STATE)?.group != prepared.group_id {
            return Err(STATE);
        }
        let authority = self.lane.adapter.authority.clone();
        authority
            .run(async {
                let effects = self
                    .runtime
                    .publish_prepared_session_effects_with_audit_context(
                        prepared.effects,
                        Default::default(),
                    )
                    .await
                    .map_err(|_| EXCHANGE)?;
                check_publication(&effects)
            })
            .await
    }

    pub(super) async fn receive_welcome(&mut self, package: &Package) -> Result<HeldWelcome> {
        let authority = self.lane.adapter.authority.clone();
        if self.lane.adapter.account != self.client || self.room.is_some() {
            return Err(STATE);
        }
        let delivery = self
            .lane
            .receive(TransportDeliveryPlane::AccountInbox, None, WELCOME_CAP)
            .await?;
        authority
            .run(async {
                let event = checked_outer(&delivery, Kind::GiftWrap)?;
                let expected = self.keys.public_key().to_hex();
                if !exact_selector(&event, "p", &expected)
                    || delivery.message.envelope
                        != (TransportEnvelope::Welcome {
                            recipient: self.client.clone(),
                        })
                {
                    return Err(STATE);
                }
                let peeler = NostrMlsPeeler::new().with_welcome_signer_arc(self.keys.clone());
                let (peeled, provenance) = peeler
                    .peel_welcome_with_provenance(&delivery.message)
                    .await
                    .map_err(|_| STATE)?;
                if peeled.sender.as_ref() != Some(&self.server)
                    || provenance.key_package_event_id.as_slice() != package.event
                {
                    return Err(STATE);
                }
                let held = HeldWelcome {
                    delivery,
                    peeled_id: peeled.id,
                };
                self.lane.acknowledge(&held.delivery.message.id).await;
                Ok(held)
            })
            .await
    }

    pub(super) async fn join(
        &mut self,
        held: HeldWelcome,
        package: &Package,
        plan: &Plan,
    ) -> Result<()> {
        let authority = self.lane.adapter.authority.clone();
        authority
            .run(async {
                if self.lane.adapter.account != self.client || self.room.is_some() {
                    return Err(STATE);
                }
                self.lane.adapter.check().await?;
                let incoming = self
                    .runtime
                    .ingest_delivery(&held.delivery)
                    .await
                    .map_err(|_| EXCHANGE)?;
                let group = GroupId::new(plan.group.to_vec());
                if incoming.outcome != IngestOutcome::Processed
                    || !incoming.effects.reports.is_empty()
                    || !matches!(incoming.effects.events.as_slice(), [
                    GroupEvent::GroupJoined {group_id, via_welcome, welcomer:Some(author), ..},
                    GroupEvent::GroupStateChanged {
                        group_id: state_group, epoch: EpochId(1), actor: Some(actor),
                        change: cgka_traits::engine::GroupStateChange::MessageRetentionChanged {
                            old_seconds: 0, new_seconds: RETENTION,
                        }, origin_commit_id: None,
                    },
                ] if group_id == &group && via_welcome == &held.peeled_id && author == &self.server
                    && state_group == &group && actor == &self.server)
                {
                    return Err(STATE);
                }
                let lifecycle = self
                    .storage
                    .key_package_lifecycle()
                    .map_err(|_| STATE)?
                    .ok_or(STATE)?;
                let used = lifecycle.pending_replacement.ok_or(STATE)?;
                if lifecycle.authored_event_id.is_some()
                    || lifecycle.current_key_package.is_some()
                    || used.attempt_count != 0
                    || used.consumed_at.is_none()
                    || used.key_package_ref != package.reference
                    || used
                        .signed_event
                        .as_ref()
                        .is_none_or(|event| event.id.as_slice() != package.event)
                {
                    return Err(STATE);
                }
                self.storage
                    .delete_pending_application_events(&[held.peeled_id])
                    .map_err(|_| STATE)?;
                self.register(plan).await
            })
            .await
    }

    async fn register(&mut self, plan: &Plan) -> Result<()> {
        self.lane.adapter.authority.check()?;
        let group = GroupId::new(plan.group.to_vec());
        let route = self.verify(&group, &plan.route, &plan.digest)?;
        self.routing.register(TransportGroupSubscription {
            group_id: group.clone(),
            transport_group_id: route.nostr_group_id.to_vec(),
            endpoints: route.relays.into_iter().map(TransportEndpoint).collect(),
            retained_since: None,
        })?;
        self.runtime
            .sync_transport_groups(None)
            .await
            .map_err(|_| EXCHANGE)?;
        let subscription = self
            .lane
            .adapter
            .native
            .group_route_subscription_id(&self.lane.adapter.account, &plan.route)
            .await
            .ok_or(STATE)?;
        self.lane.pin_group(&subscription, &plan.route, FRAME_CAP)?;
        self.room = Some(Installed {
            group,
            route: plan.route,
            digest: plan.digest,
            subscription,
        });
        self.check_live().await
    }

    fn verify(
        &self,
        group: &GroupId,
        route: &[u8; 32],
        digest: &[u8; 32],
    ) -> Result<NostrRoutingV1> {
        if self.runtime.live_group_ids().map_err(|_| STATE)?.as_slice()
            != std::slice::from_ref(group)
        {
            return Err(STATE);
        }
        let record = self.runtime.group_record(group).map_err(|_| STATE)?;
        // Stock Current requires only MLS AppDataUpdate (8) and AppDataDictionary (6).
        let local_install_epoch = EpochId(u64::from(self.lane.adapter.account == self.client));
        if record.id != *group
            || record.name != PURPOSE
            || record.description != description(digest)
            || record.protocol_profile != ProtocolProfile::Current
            || record.removed
            || record.unrecoverable
            || record.disbanded.is_some()
            || record.epoch != EpochId(1)
            || record.join_epoch != local_install_epoch
            || record.local_copy_install_epoch != local_install_epoch
            || record.required_capabilities.proposals != BTreeSet::from([8])
            || record.required_capabilities.extensions != BTreeSet::from([6])
        {
            return Err(Error::Mdk("terminal_enrollment_group_record"));
        }
        let membership = self
            .runtime
            .canonical_group_membership(group)
            .map_err(|_| STATE)?;
        if !membership.local_leaf_active
            || membership.member_leaves.len() != 2
            || !membership
                .member_leaves
                .iter()
                .any(|leaf| leaf.member == self.client)
            || !membership
                .member_leaves
                .iter()
                .any(|leaf| leaf.member == self.server)
            || self
                .runtime
                .admin_pubkeys(group)
                .map_err(|_| STATE)?
                .as_slice()
                != [self.server.as_slice()]
        {
            return Err(Error::Mdk("terminal_enrollment_membership"));
        }
        for leaf in membership.member_leaves {
            if self
                .runtime
                .member_device_tag_claim(
                    group,
                    record.epoch,
                    leaf.leaf_index,
                    &DeviceRosterV1::default(),
                )
                .map_err(|_| STATE)?
                != Some(DeviceTagClaim::Unclaimed)
            {
                return Err(STATE);
            }
        }
        let listed = self.component(group, APP_COMPONENTS_COMPONENT_ID)?;
        let expected: BTreeSet<_> = component_ids().collect();
        if record.required_capabilities.app_components.ids != expected
            || decode_components_list(&listed).map_err(|_| STATE)? != expected
            || self.component(group, GROUP_MESSAGE_RETENTION_COMPONENT_ID)?
                != RETENTION.to_be_bytes()
        {
            return Err(Error::Mdk("terminal_enrollment_components"));
        }
        let native = self.component(group, NOSTR_ROUTING_COMPONENT_ID)?;
        let native = decode_nostr_routing_v1(&native).map_err(|_| STATE)?;
        if native.nostr_group_id != *route
            || native.relays.as_slice() != std::slice::from_ref(&self.lane.adapter.endpoint.0)
        {
            return Err(STATE);
        }
        Ok(native)
    }

    fn component(&self, group: &GroupId, id: AppComponentId) -> Result<Vec<u8>> {
        self.runtime
            .app_component(group, id)
            .map_err(|_| STATE)?
            .ok_or(STATE)
    }

    pub(super) fn witness(&self) -> Result<RoomWitness> {
        let (route, subscription) = self.checked_route()?;
        let room = self.room.as_ref().ok_or(STATE)?;
        let membership = self
            .runtime
            .canonical_group_membership(&room.group)
            .map_err(|_| STATE)?;
        let leaf = membership
            .member_leaves
            .into_iter()
            .find(|leaf| leaf.member == self.lane.adapter.account)
            .ok_or(STATE)?;
        let record = self.runtime.group_record(&room.group).map_err(|_| STATE)?;
        Ok(RoomWitness {
            route,
            subscription: subscription.to_owned(),
            local_leaf: leaf.leaf_index,
            epoch: record.epoch,
        })
    }

    pub(super) fn checked_route(&self) -> Result<([u8; 32], &str)> {
        let room = self.room.as_ref().ok_or(STATE)?;
        self.verify(&room.group, &room.route, &room.digest)?;
        Ok((room.route, &room.subscription))
    }

    pub(super) fn verify_mismatches(&self) -> Result<()> {
        let room = self.room.as_ref().ok_or(STATE)?;
        let mut route = room.route;
        route[0] ^= 1;
        let mut digest = room.digest;
        digest[0] ^= 1;
        let mut group = room.group.as_slice().to_vec();
        group[0] ^= 1;
        if self.verify(&room.group, &route, &room.digest).is_ok()
            || self.verify(&room.group, &room.route, &digest).is_ok()
            || self
                .verify(&GroupId::new(group), &room.route, &room.digest)
                .is_ok()
        {
            return Err(STATE);
        }
        Ok(())
    }

    pub(super) async fn check_live(&self) -> Result<()> {
        self.lane.adapter.check().await?;
        let installed = self.room.as_ref().ok_or(STATE)?;
        if self
            .lane
            .adapter
            .native
            .group_route_subscription_id(&self.lane.adapter.account, &installed.route)
            .await
            .as_deref()
            != Some(installed.subscription.as_str())
        {
            self.lane.adapter.authority.retire();
            return Err(STATE);
        }
        self.verify(&installed.group, &installed.route, &installed.digest)?;
        self.lane.adapter.authority.check()
    }

    pub(super) async fn send_app(&mut self, payload: Vec<u8>) -> Result<()> {
        self.check_live().await?;
        let group = self.room.as_ref().ok_or(STATE)?.group.clone();
        let epoch = self.runtime.epoch_pin(&group).map_err(|_| STATE)?;
        let authority = self.lane.adapter.authority.clone();
        authority
            .run(async {
                let effects = self
                    .runtime
                    .send(SendIntent::AppMessage {
                        group_id: group,
                        payload,
                        expected_epoch: Some(epoch),
                    })
                    .await
                    .map_err(|_| EXCHANGE)?;
                check_publication(&effects)?;
                if effects.published_app_messages.len() != 1 {
                    return Err(EXCHANGE);
                }
                self.lane
                    .acknowledge(&effects.published_app_messages[0].message_id)
                    .await;
                self.runtime
                    .acknowledge_published_app_messages(&effects.published_app_messages)
                    .map_err(|_| EXCHANGE)
            })
            .await?;
        self.check_live().await
    }

    pub(super) async fn receive_app(&mut self) -> Result<App> {
        self.check_live().await?;
        let authority = self.lane.adapter.authority.clone();
        let installed = self.room.clone().ok_or(STATE)?;
        let delivery = self
            .lane
            .receive(
                TransportDeliveryPlane::Group,
                Some(&installed.subscription),
                FRAME_CAP,
            )
            .await?;
        authority
            .run(async {
                let event = checked_outer(&delivery, Kind::MlsGroupMessage)?;
                if !exact_selector(&event, "h", &hex::encode(installed.route))
                    || delivery.message.envelope
                        != (TransportEnvelope::GroupMessage {
                            transport_group_id: installed.route.to_vec(),
                        })
                {
                    return Err(STATE);
                }
                let group = &installed.group;
                let peer = if self.lane.adapter.account == self.server {
                    &self.client
                } else {
                    &self.server
                };
                let membership = self
                    .runtime
                    .canonical_group_membership(group)
                    .map_err(|_| STATE)?;
                let expected_leaf = membership
                    .member_leaves
                    .iter()
                    .find(|leaf| &leaf.member == peer)
                    .ok_or(STATE)?
                    .leaf_index;
                let incoming = self
                    .runtime
                    .ingest_delivery(&delivery)
                    .await
                    .map_err(|_| EXCHANGE)?;
                if incoming.outcome != IngestOutcome::Processed
                    || !incoming.effects.reports.is_empty()
                {
                    return Err(EXCHANGE);
                }
                let [
                    GroupEvent::MessageReceived {
                        group_id,
                        message_id,
                        sender,
                        sender_leaf_index: Some(leaf),
                        epoch: EpochId(1),
                        payload,
                        retention: Some(retention),
                        ..
                    },
                ] = incoming.effects.events.as_slice()
                else {
                    return Err(EXCHANGE);
                };
                if group_id != group || sender != peer || *leaf != expected_leaf {
                    return Err(EXCHANGE);
                }
                let app = App {
                    bytes: payload.clone(),
                    retention: *retention,
                };
                self.storage
                    .delete_pending_application_events(std::slice::from_ref(message_id))
                    .map_err(|_| STATE)?;
                let event = MarmotAppEvent::decode(&app.bytes).map_err(|_| STATE)?;
                if event.pubkey != hex::encode(peer.as_slice())
                    || app.retention
                        != AppMessageRetentionDecision::new(event.created_at, RETENTION)
                {
                    return Err(STATE);
                }
                self.lane.acknowledge(&delivery.message.id).await;
                self.check_live().await?;
                Ok(app)
            })
            .await
    }
}

fn check_publication(effects: &AccountDeviceEffects) -> Result<()> {
    if effects.reports.len() != 1
        || effects.reports[0].accepted.len() != 1
        || !effects.failures.is_empty()
        || !effects.welcome_failures.is_empty()
        || !effects.pending.is_empty()
        || !effects.pending_convergence.is_empty()
        || !effects.queued.is_empty()
        || !effects.unresolved_publishes.is_empty()
        || !effects.unresolved_app_messages.is_empty()
        || !effects.failed_app_messages.is_empty()
    {
        return Err(EXCHANGE);
    }
    Ok(())
}

fn component_ids() -> impl Iterator<Item = AppComponentId> {
    default_group_components().into_iter().chain([
        ACCOUNT_IDENTITY_PROOF_COMPONENT_ID,
        NOSTR_ROUTING_COMPONENT_ID,
        GROUP_MESSAGE_RETENTION_COMPONENT_ID,
    ])
}

fn description(digest: &[u8; 32]) -> String {
    format!("marmish/terminal/1/{}", hex::encode(digest))
}

fn expected_route(plan: &Plan, endpoint: &TransportEndpoint) -> Result<NostrRoutingV1> {
    NostrRoutingV1::new(plan.route, vec![endpoint.0.clone()]).map_err(|_| STATE)
}

fn checked_package(wire: &Package, client: &MemberId) -> Result<KeyPackage> {
    let package = KeyPackage {
        bytes: wire.mls.clone(),
        source: Some(KeyPackageSource {
            event_id: MessageId::new(wire.event.to_vec()),
        }),
        protocol_profile: ProtocolProfile::Current,
    };
    let meta = key_package_metadata(&package).map_err(|_| STATE)?;
    if meta.protocol_profile != ProtocolProfile::Current
        || meta.credential_identity_hex != hex::encode(client.as_slice())
        || meta.key_package_ref_hex != hex::encode(wire.reference)
    {
        return Err(STATE);
    }
    Ok(package)
}

fn checked_outer(delivery: &TransportDelivery, kind: Kind) -> Result<Event> {
    let event: Event = serde_json::from_slice(&delivery.message.payload).map_err(|_| STATE)?;
    event.verify().map_err(|_| STATE)?;
    if event.kind != kind || event.id.to_bytes().as_slice() != delivery.message.id.as_slice() {
        return Err(STATE);
    }
    Ok(event)
}

pub(super) fn terminal_member(key: PublicKey) -> MemberId {
    MemberId::new(key.to_bytes().to_vec())
}

pub(super) fn entropy<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| STATE)?;
    Ok(bytes)
}

pub(super) fn unix_now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| STATE)?
        .as_secs())
}
