//! Offline trust tests using production Current-profile creation and Welcome ingest.
use super::*;
use crate::identity::BackendChoice;
use crate::transport::{ArtiDirs, RelayPolicy, RouteMode, TransportLimits};
use cgka_traits::storage::OutboundIntentStorage;
use cgka_traits::{TransportDelivery, TransportDeliveryPlane, TransportDeliverySource};
use marmot_session::PublishWork;
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    owners: Vec<MdkOwner>,
    profiles: Vec<ProfileLease>,
    materials: Vec<IdentityMaterial>,
    _root: tempfile::TempDir,
}
impl Fixture {
    fn new(count: usize) -> Self {
        Self::accounts(count, false)
    }
    fn accounts(count: usize, sibling: bool) -> Self {
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let mut profiles = Vec::new();
        let mut materials = Vec::new();
        for n in 0..count {
            let mut profile = ProfileLease::acquire(root.path(), &format!("account{n}")).unwrap();
            let material = profile
                .prepare(
                    BackendChoice::File,
                    &std::env::current_exe().unwrap(),
                    std::time::Instant::now() + Duration::from_secs(5),
                )
                .unwrap();
            profiles.push(profile);
            materials.push(material);
        }
        let allowed: Vec<_> = materials.iter().map(|m| m.public_key().to_hex()).collect();
        let owners = profiles
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                // A second device uses the same real account signer but an
                // independent encrypted MLS store and fresh MLS leaf keys.
                // Profile readiness is not asserted for this adapter fixture.
                let material = &materials[if sibling && index == 2 { 1 } else { index }];
                let connector = RelayConnector::new(
                    RelayPolicy::parse(
                        &["wss://127.0.0.1:9/".into()],
                        RouteMode::Off,
                        TransportLimits::default(),
                    )
                    .unwrap(),
                    ArtiDirs {
                        state: profile.arti_state().into(),
                        cache: profile.arti_cache().into(),
                    },
                )
                .unwrap();
                MdkOwner::open(
                    profile,
                    material,
                    connector,
                    allowed
                        .iter()
                        .filter(|p| **p != material.public_key().to_hex())
                        .cloned()
                        .collect(),
                )
                .unwrap()
            })
            .collect();
        Self {
            owners,
            profiles,
            materials,
            _root: root,
        }
    }

    async fn join(
        &mut self,
        invitees: &[usize],
        description: &str,
        coadmin: bool,
    ) -> (GroupId, GroupEvent) {
        self.join_with_route(
            invitees,
            description,
            coadmin,
            foundation::fresh_id(now().unwrap()).unwrap(),
            NostrRoutingV1::new([42; 32], vec!["wss://127.0.0.1:9/".into()]).unwrap(),
        )
        .await
    }

    async fn join_with_route(
        &mut self,
        invitees: &[usize],
        description: &str,
        coadmin: bool,
        session: String,
        route: NostrRoutingV1,
    ) -> (GroupId, GroupEvent) {
        let mut members = Vec::new();
        for &index in invitees {
            let owner = &mut self.owners[index];
            let mut kp = owner
                .runtime
                .prepare_fresh_key_package(owner.routing.endpoints.clone())
                .await
                .unwrap();
            let pending = owner
                .storage
                .key_package_lifecycle()
                .unwrap()
                .unwrap()
                .pending_replacement
                .unwrap();
            kp.source = Some(KeyPackageSource {
                event_id: pending.signed_event.unwrap().id,
            });
            members.push(kp);
        }
        let admins = if coadmin {
            vec![self.owners[1].account.clone()]
        } else {
            vec![]
        };
        let prepared = self.owners[0]
            .runtime
            .prepare_create_group_with_audit_context(
                CreateGroupRequest {
                    name: format!(
                        "{}{session}",
                        if description == RoomPurpose::Command.description() {
                            RoomPurpose::Command.prefix()
                        } else {
                            PREFIX
                        }
                    ),
                    description: description.into(),
                    members,
                    required_features: vec![],
                    initial_admins: admins,
                    app_components: vec![AppComponentData {
                        component_id: NOSTR_ROUTING_COMPONENT_ID,
                        data: encode_nostr_routing_v1(&route).unwrap(),
                    }],
                },
                Default::default(),
            )
            .await
            .unwrap();
        let group = prepared.group_id;
        let welcomes: Vec<_> = prepared
            .effects
            .publish
            .into_iter()
            .flat_map(|work| match work {
                PublishWork::FoundingGroupCreated { welcomes } => welcomes,
                _ => panic!("Current founding group must not stage a legacy commit"),
            })
            .collect();
        let bob = &mut self.owners[1];
        bob.publication.pause();
        let welcome = welcomes
            .into_iter()
            .find(|v| {
                matches!(&v.envelope,
            TransportEnvelope::Welcome { recipient } if recipient == &bob.account)
            })
            .unwrap();
        let delivery = TransportDelivery {
            account_id: bob.account.clone(),
            group_id_hint: None,
            received_at: Timestamp(now().unwrap()),
            source: TransportDeliverySource {
                transport: welcome.source.clone(),
                plane: TransportDeliveryPlane::AccountInbox,
                endpoint: None,
                subscription_id: None,
                wire: None,
            },
            message: welcome,
        };
        let ingested = tokio::time::timeout(
            Duration::from_secs(5),
            bob.runtime.ingest_delivery(&delivery),
        )
        .await
        .unwrap()
        .unwrap();
        let event = ingested
            .effects
            .events
            .into_iter()
            .find(|e| matches!(e, GroupEvent::GroupJoined { .. }))
            .unwrap();
        (group, event)
    }
    async fn encrypted_event(
        &mut self,
        group: &GroupId,
        envelope: &Envelope,
        carrier: u64,
    ) -> GroupEvent {
        let alice = &mut self.owners[0];
        alice.publication.pause();
        let (_, _, route, _) = alice.check_group(group, None).unwrap();
        alice.routing.group(group.clone(), route).unwrap();
        let app = cgka_traits::app_event::MarmotAppEvent::new(
            alice.identity.clone(),
            now().unwrap() + carrier,
            foundation::KIND,
            vec![vec!["marmish".into(), "v2-foundation".into()]],
            String::from_utf8(envelope.encode().unwrap()).unwrap(),
        );
        self.encrypted_app(group, app).await
    }

    async fn encrypted_app(
        &mut self,
        group: &GroupId,
        app: cgka_traits::app_event::MarmotAppEvent,
    ) -> GroupEvent {
        let alice = &mut self.owners[0];
        alice.publication.pause();
        tokio::time::timeout(
            Duration::from_secs(5),
            alice.runtime.send(SendIntent::AppMessage {
                group_id: group.clone(),
                payload: app.encode().unwrap(),
                expected_epoch: None,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let message = alice
            .runtime
            .outbound_fanouts_for_group(group)
            .unwrap()
            .iter()
            .find(|f| {
                f.application_message()
                    .is_some_and(|m| m.app_event_id == app.id)
            })
            .expect("real application send must retain its exact native fanout")
            .request()
            .message
            .clone();
        let bob = &mut self.owners[1];
        let delivery = TransportDelivery {
            account_id: bob.account.clone(),
            group_id_hint: None,
            received_at: Timestamp(now().unwrap()),
            source: TransportDeliverySource {
                transport: message.source.clone(),
                plane: TransportDeliveryPlane::Group,
                endpoint: None,
                subscription_id: None,
                wire: None,
            },
            message,
        };
        let effects = tokio::time::timeout(
            Duration::from_secs(5),
            bob.runtime.ingest_delivery(&delivery),
        )
        .await
        .unwrap()
        .unwrap();
        effects
            .effects
            .events
            .into_iter()
            .find(|e| matches!(e, GroupEvent::MessageReceived { .. }))
            .expect(
                "authenticated encrypted application message must reach native durable projection",
            )
    }

    fn envelope(&self, group: &GroupId, timestamp: u64) -> Envelope {
        let session = self.owners[0].check_group(group, None).unwrap().0;
        Envelope::message(
            session,
            foundation::fresh_id(timestamp).unwrap(),
            self.owners[0].identity.clone(),
            self.owners[1].identity.clone(),
            b"binary\0\xfffixture".to_vec(),
        )
        .unwrap()
    }

    fn reopen_bob(&mut self) {
        drop(self.owners.remove(1));
        let profile = &self.profiles[1];
        let material = &self.materials[1];
        let connector = RelayConnector::new(
            RelayPolicy::parse(
                &["wss://127.0.0.1:9/".into()],
                RouteMode::Off,
                TransportLimits::default(),
            )
            .unwrap(),
            ArtiDirs {
                state: profile.arti_state().into(),
                cache: profile.arti_cache().into(),
            },
        )
        .unwrap();
        let owner = MdkOwner::open(
            profile,
            material,
            connector,
            vec![self.materials[0].public_key().to_hex()],
        )
        .unwrap();
        self.owners.insert(1, owner);
    }

    fn rejected(&self, group: &GroupId) {
        let bob = &self.owners[1];
        assert!(!bob.journal.group_rejected(group.as_slice()).unwrap());
        assert!(bob.conversations().unwrap().is_empty());
        assert!(bob.routing.group_subscriptions().is_empty());
        assert!(bob.runtime.group_record(group).is_err());
    }
    async fn mutate(&mut self, intent: SendIntent) {
        let expected_description = match &intent {
            SendIntent::UpdateGroupData { description, .. } => description.clone(),
            _ => None,
        };
        let group = match &intent {
            SendIntent::UpdateGroupData { group_id, .. }
            | SendIntent::UpdateAppComponents { group_id, .. } => group_id.clone(),
            _ => panic!("fixture mutation requires an explicit group update"),
        };
        let prepared = tokio::time::timeout(
            Duration::from_secs(10),
            self.owners[0]
                .runtime
                .confirm_commit_without_publish_with_audit_context(intent, Default::default()),
        )
        .await
        .unwrap()
        .unwrap();
        let marmot_session::PreparedSessionSend::Commit(commit) = prepared else {
            panic!("fresh founding group must permit an immediate admin commit");
        };
        let (effects, welcomes) = commit.into_effects_and_welcomes();
        assert!(welcomes.is_empty());
        let (message, pending) = effects
            .publish
            .into_iter()
            .find_map(|work| match work {
                PublishWork::GroupEvolution { msg, pending, .. } => Some((msg, pending)),
                _ => None,
            })
            .unwrap();
        let bob = &mut self.owners[1];
        let delivery = TransportDelivery {
            account_id: bob.account.clone(),
            group_id_hint: None,
            received_at: Timestamp(now().unwrap()),
            source: TransportDeliverySource {
                transport: message.source.clone(),
                plane: TransportDeliveryPlane::Group,
                endpoint: None,
                subscription_id: None,
                wire: None,
            },
            message,
        };
        let ingested = tokio::time::timeout(
            Duration::from_secs(5),
            bob.runtime.ingest_delivery(&delivery),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(
            ingested.outcome,
            IngestOutcome::Processed | IngestOutcome::Buffered { .. }
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            for _ in 0..100 {
                bob.runtime.advance_convergence(&group).await.unwrap();
                let changed = match &expected_description {
                    Some(expected) => {
                        bob.runtime.group_record(&group).unwrap().description == *expected
                    }
                    None => bob.check_group(&group, None).is_err(),
                };
                if changed {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("authenticated metadata commit failed to converge");
        })
        .await
        .unwrap();
        // The fixture delivered the commit directly to its recipient. Confirm
        // that staged sender commit before a subsequent metadata mutation.
        self.owners[0]
            .runtime
            .session_mut()
            .confirm_published(pending)
            .await
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.owners.clear();
        for profile in &self.profiles {
            profile.cleanup_runtime().unwrap();
        }
    }
}

#[tokio::test]
async fn real_two_party_current_welcome_accepts_only_authenticated_creator() {
    let mut f = Fixture::new(2);
    let (group, event) = f.join(&[1], PURPOSE, false).await;
    let alice = f.owners[0].identity.clone();
    assert!(f.owners[1].check_group(&group, Some(&alice)).is_ok());
    assert!(matches!(
        f.owners[1].check_group(&group, Some(&f.owners[1].identity)),
        Err(Error::Mdk("conversation_admin"))
    ));
    f.owners[1].project_join(event).unwrap();
    assert_eq!(f.owners[1].conversations().unwrap().len(), 1);
    assert_eq!(f.owners[1].routing.group_subscriptions().len(), 1);
    assert!(
        !f.owners[1]
            .journal
            .group_rejected(group.as_slice())
            .unwrap()
    );
}

#[tokio::test]
async fn signed_welcome_with_disjoint_routes_is_acknowledged_without_poisoning_account() {
    let mut f = Fixture::new(2);
    let (bad_group, event) = f
        .join_with_route(
            &[1],
            PURPOSE,
            false,
            foundation::fresh_id(now().unwrap()).unwrap(),
            NostrRoutingV1::new([43; 32], vec!["wss://127.0.0.1:10/".into()]).unwrap(),
        )
        .await;
    let GroupEvent::GroupJoined { via_welcome, .. } = &event else {
        unreachable!()
    };
    let welcome_id = via_welcome.clone();
    assert!(f.owners[1]
        .storage
        .list_pending_application_events()
        .unwrap()
        .iter()
        .any(|e| matches!(e, GroupEvent::GroupJoined { via_welcome, .. } if via_welcome == &welcome_id)));
    f.owners[1].project_join(event).unwrap();
    f.rejected(&bad_group);
    assert!(!f.owners[1]
        .storage
        .list_pending_application_events()
        .unwrap()
        .iter()
        .any(|e| matches!(e, GroupEvent::GroupJoined { via_welcome, .. } if via_welcome == &welcome_id)));
    f.reopen_bob();
    f.rejected(&bad_group);

    let (valid_group, event) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(event).unwrap();
    assert_eq!(f.owners[1].conversations().unwrap().len(), 1);
    assert!(f.owners[1].runtime.group_record(&valid_group).is_ok());
    f.reopen_bob();
    assert_eq!(f.owners[1].conversations().unwrap().len(), 1);
    assert_eq!(f.owners[1].routing.group_subscriptions().len(), 1);
    assert!(f.owners[1].runtime.group_record(&valid_group).is_ok());
}

#[tokio::test]
async fn duplicate_transport_group_id_cannot_alias_an_accepted_room() {
    let mut f = Fixture::new(2);
    let (original, event) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(event).unwrap();
    let (collision, event) = f
        .join_with_route(
            &[1],
            PURPOSE,
            false,
            foundation::fresh_id(now().unwrap()).unwrap(),
            NostrRoutingV1::new([42; 32], vec!["wss://127.0.0.1:9/".into()]).unwrap(),
        )
        .await;
    f.owners[1].project_join(event).unwrap();
    let bob = &f.owners[1];
    assert!(!bob.journal.group_rejected(collision.as_slice()).unwrap());
    assert!(bob.runtime.group_record(&collision).is_err());
    assert_eq!(bob.conversations().unwrap().len(), 1);
    assert_eq!(
        bob.conversations().unwrap()[0].group_id,
        hex::encode(original.as_slice())
    );
    assert_eq!(bob.routing.group_subscriptions().len(), 1);
    f.reopen_bob();
    assert_eq!(f.owners[1].conversations().unwrap().len(), 1);
    assert_eq!(f.owners[1].routing.group_subscriptions().len(), 1);
}

#[tokio::test]
async fn unsolicited_authenticated_peer_is_rejected_and_forgotten() {
    let mut f = Fixture::new(2);
    let (group, event) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].allowed.clear();
    assert!(matches!(
        f.owners[1].check_group(&group, None),
        Err(Error::Mdk("untrusted_peer"))
    ));
    f.owners[1].project_join(event).unwrap();
    f.rejected(&group);
}

#[tokio::test]
async fn valid_welcome_at_conversation_capacity_is_rejected_without_failing_account() {
    let mut f = Fixture::new(2);
    let (group, event) = f.join(&[1], PURPOSE, false).await;
    assert!(f.owners[1].check_group(&group, None).is_ok());
    let alice = f.owners[0].identity.clone();
    let bob = &mut f.owners[1];
    for n in 0_u8..64 {
        bob.journal
            .accept_conversation(
                &[n; 32],
                &format!("{:012x}{n:020x}", now().unwrap()),
                &alice,
            )
            .unwrap();
    }
    bob.project_join(event).unwrap();
    assert!(bob.journal.group_rejected(group.as_slice()).unwrap());
    assert_eq!(bob.conversations().unwrap().len(), 64);
    assert!(
        bob.conversations()
            .unwrap()
            .iter()
            .all(|c| !c.revoked && c.group_id != hex::encode(group.as_slice()))
    );
    assert!(bob.routing.group_subscriptions().is_empty());
    assert!(bob.runtime.group_record(&group).is_err());
}

#[tokio::test]
async fn third_account_is_rejected_even_when_all_accounts_are_allowed() {
    let mut f = Fixture::new(3);
    let (group, event) = f.join(&[1, 2], PURPOSE, false).await;
    assert!(matches!(
        f.owners[1].check_group(&group, None),
        Err(Error::Mdk("conversation_membership"))
    ));
    f.owners[1].project_join(event).unwrap();
    f.rejected(&group);
}

#[tokio::test]
async fn additional_leaf_of_an_allowed_account_is_rejected() {
    let mut f = Fixture::accounts(3, true);
    let (group, event) = f.join(&[1, 2], PURPOSE, false).await;
    let roster = f.owners[1]
        .runtime
        .canonical_group_membership(&group)
        .unwrap();
    assert_eq!(roster.member_leaves.len(), 3);
    assert_eq!(
        roster
            .member_leaves
            .iter()
            .map(|leaf| leaf.member.as_slice().to_vec())
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert!(matches!(
        f.owners[1].check_group(&group, None),
        Err(Error::Mdk("conversation_membership"))
    ));
    f.owners[1].project_join(event).unwrap();
    f.rejected(&group);
}

#[tokio::test]
async fn welcome_projection_rejects_missing_or_wrong_authenticated_creator() {
    for missing in [false, true] {
        let mut f = Fixture::new(2);
        let (group, mut event) = f.join(&[1], PURPOSE, false).await;
        assert!(f.owners[1].check_group(&group, None).is_ok());
        let GroupEvent::GroupJoined { welcomer, .. } = &mut event else {
            unreachable!()
        };
        // Exercise the application gate on trusted upstream event metadata;
        // this does not claim that a wire attacker can forge that metadata.
        *welcomer = if missing {
            None
        } else {
            Some(f.owners[1].account.clone())
        };
        f.owners[1].project_join(event).unwrap();
        f.rejected(&group);
    }
}

#[tokio::test]
async fn general_chat_purpose_is_rejected() {
    let mut f = Fixture::new(2);
    let (group, event) = f.join(&[1], "general purpose chat", false).await;
    assert!(matches!(
        f.owners[1].check_group(&group, None),
        Err(Error::Mdk("conversation_purpose"))
    ));
    f.owners[1].project_join(event).unwrap();
    f.rejected(&group);
}

#[tokio::test]
async fn extra_authenticated_admin_is_rejected() {
    let mut f = Fixture::new(2);
    let (group, event) = f.join(&[1], PURPOSE, true).await;
    assert!(matches!(
        f.owners[1].check_group(&group, None),
        Err(Error::Mdk("conversation_admin"))
    ));
    f.owners[1].project_join(event).unwrap();
    f.rejected(&group);
}

#[tokio::test]
async fn paused_publication_preserves_exact_native_welcome_and_application_obligations() {
    let mut f = Fixture::new(2);
    let (group, _) = f.join(&[1], PURPOSE, false).await;
    let alice = &mut f.owners[0];
    alice.publication.pause();
    let (_, _, route, _) = alice.check_group(&group, None).unwrap();
    alice.routing.group(group.clone(), route).unwrap();
    let original = alice.runtime.outstanding_welcome_deliveries().unwrap();
    assert_eq!(original.len(), 1);
    let effects = tokio::time::timeout(
        Duration::from_secs(5),
        alice.runtime.retry_welcome_messages_with_audit_context(
            original.iter().map(|(_, m)| m.clone()).collect(),
            Default::default(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!effects.welcome_failures.is_empty());
    assert_eq!(
        alice.runtime.outstanding_welcome_deliveries().unwrap(),
        original
    );
    let app = cgka_traits::app_event::MarmotAppEvent::new(
        alice.identity.clone(),
        now().unwrap(),
        foundation::KIND,
        vec![vec!["marmish".into(), "v2-foundation".into()]],
        "retained fixture",
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        alice.runtime.send(SendIntent::AppMessage {
            group_id: group.clone(),
            payload: app.encode().unwrap(),
            expected_epoch: None,
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let fanouts = alice.runtime.outbound_fanouts_for_group(&group).unwrap();
    let queued = alice.storage.list_queued_outbound_intents(&group).unwrap();
    assert!(!fanouts.is_empty() || !queued.is_empty());
    assert!(fanouts.iter().any(|f| f.application_message().is_some_and(|m| m.app_event_id == app.id)) || queued.iter().any(|q|
        matches!(&q.intent, SendIntent::AppMessage { payload, .. } if payload == &app.encode().unwrap())));
    let exact: Vec<_> = fanouts.iter().map(|f| f.request().clone()).collect();
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(5), alice.runtime.drain())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            alice.runtime.outstanding_welcome_deliveries().unwrap(),
            original
        );
        assert_eq!(
            alice
                .runtime
                .outbound_fanouts_for_group(&group)
                .unwrap()
                .iter()
                .map(|f| f.request().clone())
                .collect::<Vec<_>>(),
            exact
        );
        assert_eq!(
            alice.storage.list_queued_outbound_intents(&group).unwrap(),
            queued
        );
    }
}

#[tokio::test]
async fn accepted_group_is_revoked_after_authenticated_purpose_mutation() {
    let mut f = Fixture::new(2);
    let (group, event) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(event).unwrap();
    assert_eq!(f.owners[1].conversations().unwrap().len(), 1);
    f.mutate(SendIntent::UpdateGroupData {
        group_id: group.clone(),
        name: None,
        description: Some("general purpose chat".into()),
    })
    .await;
    assert!(matches!(
        f.owners[1].ensure_trusted(&group),
        Err(Error::Mdk("conversation_revoked"))
    ));
    assert!(f.owners[1].conversations().unwrap()[0].revoked);
    assert!(f.owners[1].routing.group_subscriptions().is_empty());
    assert!(
        f.owners[1]
            .journal
            .group_rejected(group.as_slice())
            .unwrap()
    );
}

#[tokio::test]
async fn accepted_group_is_revoked_after_authenticated_admin_mutation() {
    let mut f = Fixture::new(2);
    let (group, event) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(event).unwrap();
    let mut admins = [
        f.owners[0].account.as_slice().to_vec(),
        f.owners[1].account.as_slice().to_vec(),
    ];
    admins.sort();
    // The production component codec accepts one length-prefixed concatenated key vector.
    let data = encode_component_vectors(&[admins.concat().as_slice()]);
    f.mutate(SendIntent::UpdateAppComponents {
        group_id: group.clone(),
        updates: vec![AppComponentData {
            component_id: GROUP_ADMIN_POLICY_COMPONENT_ID,
            data,
        }],
    })
    .await;
    assert!(matches!(
        f.owners[1].ensure_trusted(&group),
        Err(Error::Mdk("conversation_revoked"))
    ));
    assert!(f.owners[1].conversations().unwrap()[0].revoked);
    assert!(f.owners[1].routing.group_subscriptions().is_empty());
    assert!(
        f.owners[1]
            .journal
            .group_rejected(group.as_slice())
            .unwrap()
    );
}

#[tokio::test]
async fn duplicate_application_reencrypted_in_distinct_native_messages_has_one_receipt() {
    let mut f = Fixture::new(2);
    let (group, join) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(join).unwrap();
    let envelope = f.envelope(&group, now().unwrap());
    let first = f.encrypted_event(&group, &envelope, 0).await;
    f.owners[1].project_message(first).unwrap();
    let receipt = foundation::receipt_id(group.as_slice(), &envelope).unwrap();
    let initial = f.owners[1].journal.unsubmitted().unwrap();
    assert_eq!(initial.len(), 1);
    assert_eq!(initial[0].envelope.id(), receipt);
    let second = f.encrypted_event(&group, &envelope, 1).await;
    f.owners[1].project_message(second).unwrap();
    assert_eq!(f.owners[1].journal.received().unwrap().len(), 1);
    let after = f.owners[1].journal.unsubmitted().unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].event_id, initial[0].event_id);
    assert_eq!(after[0].encoded, initial[0].encoded);
}

#[tokio::test]
async fn message_before_undecided_join_remains_pending_then_projects_after_join() {
    let mut f = Fixture::new(2);
    let (group, join) = f.join(&[1], PURPOSE, false).await;
    let envelope = f.envelope(&group, now().unwrap());
    let message = f.encrypted_event(&group, &envelope, 0).await;
    let GroupEvent::MessageReceived { message_id, .. } = &message else {
        unreachable!()
    };
    let id = message_id.clone();
    let _ = f.owners[1].project_message(message.clone());
    assert!(
        f.owners[1]
            .storage
            .list_pending_application_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, GroupEvent::MessageReceived {message_id,..} if message_id == &id))
    );
    assert!(f.owners[1].journal.received().unwrap().is_empty());
    f.owners[1].project_join(join).unwrap();
    f.owners[1].project_message(message).unwrap();
    assert_eq!(f.owners[1].journal.received().unwrap().len(), 1);
    assert!(
        !f.owners[1]
            .storage
            .list_pending_application_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, GroupEvent::MessageReceived {message_id,..} if message_id == &id))
    );
}

#[tokio::test]
async fn future_session_defers_without_ack_or_rejection_frontier() {
    let mut f = Fixture::new(2);
    let (group, join) = f
        .join_with_route(
            &[1],
            PURPOSE,
            false,
            foundation::fresh_id(now().unwrap() + foundation::FUTURE_ALLOWANCE + 60).unwrap(),
            NostrRoutingV1::new([44; 32], vec!["wss://127.0.0.1:9/".into()]).unwrap(),
        )
        .await;
    let _ = f.owners[1].project_join(join);
    let bob = &f.owners[1];
    assert!(bob.conversations().unwrap().is_empty());
    assert!(!bob.journal.group_rejected(group.as_slice()).unwrap());
    assert_eq!(bob.journal.admission_reject_through().unwrap(), None);
    assert!(bob.runtime.group_record(&group).is_ok());
    assert!(
        bob.storage
            .list_pending_application_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, GroupEvent::GroupJoined { group_id, .. } if group_id == &group))
    );
    f.reopen_bob();
    assert!(f.owners[1].runtime.group_record(&group).is_ok());
}

#[tokio::test]
async fn unsolicited_welcome_flood_does_not_allocate_rejection_or_frontier() {
    let mut f = Fixture::new(2);
    f.owners[1].allowed.clear();
    for _ in 0..3 {
        let (group, join) = f.join(&[1], PURPOSE, false).await;
        f.owners[1].project_join(join).unwrap();
        f.rejected(&group);
        assert_eq!(
            f.owners[1].journal.admission_reject_through().unwrap(),
            None
        );
        assert!(
            f.owners[1]
                .journal
                .pending_group_cleanup()
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn native_created_group_without_journal_or_pending_join_is_forgotten_on_reopen() {
    let mut f = Fixture::new(2);
    let (group, _) = f.join(&[1], PURPOSE, false).await;
    assert!(f.owners[0].conversations().unwrap().is_empty());
    assert_eq!(
        f.owners[0]
            .runtime
            .outstanding_welcome_deliveries()
            .unwrap()
            .len(),
        1
    );
    // Reopen the creator rather than the invitee: native creation has no pending
    // GroupJoined event and crashed before application journal acceptance.
    f.owners.swap(0, 1);
    f.profiles.swap(0, 1);
    f.materials.swap(0, 1);
    f.reopen_bob();
    assert!(f.owners[1].runtime.group_record(&group).is_err());
    assert!(
        f.owners[1]
            .runtime
            .outstanding_welcome_deliveries()
            .unwrap()
            .is_empty()
    );
    assert!(f.owners[1].conversations().unwrap().is_empty());
    assert!(
        f.owners[1]
            .journal
            .group_rejected(group.as_slice())
            .unwrap()
    );
}

#[tokio::test]
async fn full_room_pending_cohort_allows_other_room_within_two_bounded_turns() {
    let mut f = Fixture::new(2);
    let (full_group, join) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(join).unwrap();
    let (other_group, join) = f
        .join_with_route(
            &[1],
            PURPOSE,
            false,
            foundation::fresh_id(now().unwrap()).unwrap(),
            NostrRoutingV1::new([45; 32], vec!["wss://127.0.0.1:9/".into()]).unwrap(),
        )
        .await;
    f.owners[1].project_join(join).unwrap();
    // Fill only the application's authenticated projection journal. All 66
    // pending carrier messages below still come from real MLS send/ingest;
    // no upstream storage table is modified by this fixture.
    for n in 0_u64..512 {
        let envelope = f.envelope(&full_group, now().unwrap());
        f.owners[1]
            .journal
            .project_authenticated(
                full_group.as_slice(),
                &n.to_be_bytes(),
                &envelope,
                now().unwrap(),
                b"fixture-authority",
            )
            .unwrap();
    }
    let mut blocked_ids = Vec::new();
    for carrier in 0..65 {
        let envelope = f.envelope(&full_group, now().unwrap());
        let event = f.encrypted_event(&full_group, &envelope, carrier).await;
        let GroupEvent::MessageReceived { message_id, .. } = event else {
            unreachable!()
        };
        blocked_ids.push(message_id);
    }
    let accepted = f.envelope(&other_group, now().unwrap());
    let accepted = f.encrypted_event(&other_group, &accepted, 1).await;
    let GroupEvent::MessageReceived {
        message_id: accepted_id,
        ..
    } = accepted
    else {
        unreachable!()
    };
    let bob = &mut f.owners[1];
    // A finite cohort of 66 keys must be visited in ceil(66/64) turns.
    // The full room must not stall progress even if all its keys sort first.
    for _ in 0..2 {
        bob.pending_budget = 64;
        bob.apply_effects(
            AccountDeviceEffects::default(),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap();
    }
    let pending = bob.storage.list_pending_application_events().unwrap();
    for blocked_id in blocked_ids {
        assert!(pending.iter().any(|e| matches!(e, GroupEvent::MessageReceived { message_id, .. } if message_id == &blocked_id)));
    }
    assert!(!pending.iter().any(|e| matches!(e, GroupEvent::MessageReceived { message_id, .. } if message_id == &accepted_id)));
    assert_eq!(bob.journal.status_counts().unwrap().1, 513);
    assert_eq!(bob.journal.unsubmitted().unwrap().len(), 1);
}

#[tokio::test]
async fn expired_application_reencrypted_after_collection_and_reopen_cannot_create_receipt() {
    let mut f = Fixture::new(2);
    let (group, join) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(join).unwrap();
    let timestamp = now().unwrap();
    let envelope = f.envelope(&group, timestamp);
    let event = f.encrypted_event(&group, &envelope, 0).await;
    f.owners[1].project_message(event).unwrap();
    assert_eq!(f.owners[1].journal.received().unwrap().len(), 1);
    let advanced = timestamp + foundation::RETRY_HORIZON + 60;
    f.owners[1]
        .journal
        .maintain(advanced, 0, &BTreeSet::new())
        .unwrap();
    assert!(f.owners[1].journal.received().unwrap().is_empty());
    assert!(f.owners[1].journal.unsubmitted().unwrap().is_empty());
    let floor = f.owners[1].journal.replay_floor().unwrap();
    assert!(floor > timestamp);
    f.reopen_bob();
    assert_eq!(f.owners[1].journal.replay_floor().unwrap(), floor);
    let event = f.encrypted_event(&group, &envelope, 1).await;
    let GroupEvent::MessageReceived { message_id, .. } = &event else {
        unreachable!()
    };
    let id = message_id.clone();
    f.owners[1].project_message(event).unwrap();
    assert!(f.owners[1].journal.received().unwrap().is_empty());
    assert!(f.owners[1].journal.unsubmitted().unwrap().is_empty());
    assert!(
        !f.owners[1]
            .storage
            .list_pending_application_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, GroupEvent::MessageReceived {message_id,..} if message_id == &id))
    );
}

#[tokio::test]
async fn future_message_during_clock_behind_floor_remains_pending_without_receipt() {
    let mut f = Fixture::new(2);
    let (group, join) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(join).unwrap();
    let timestamp = now().unwrap() + foundation::FUTURE_ALLOWANCE + 60;
    f.owners[1]
        .journal
        .maintain(
            timestamp + foundation::RETRY_HORIZON,
            86400,
            &BTreeSet::new(),
        )
        .unwrap();
    let envelope = f.envelope(&group, timestamp);
    let event = f.encrypted_event(&group, &envelope, 0).await;
    let GroupEvent::MessageReceived { message_id, .. } = &event else {
        unreachable!()
    };
    let id = message_id.clone();
    let _ = f.owners[1].project_message(event);
    assert!(f.owners[1].journal.received().unwrap().is_empty());
    assert!(f.owners[1].journal.unsubmitted().unwrap().is_empty());
    assert!(
        f.owners[1]
            .storage
            .list_pending_application_events()
            .unwrap()
            .iter()
            .any(|e| matches!(e, GroupEvent::MessageReceived {message_id,..} if message_id == &id))
    );
}

#[tokio::test]
async fn reused_session_in_distinct_signed_room_is_acknowledged_without_poisoning_account() {
    let mut f = Fixture::new(2);
    let (original, join) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(join).unwrap();
    let session = f.owners[1].conversations().unwrap()[0].session.clone();
    let frontier = f.owners[1].journal.admission_reject_through().unwrap();
    let (collision, join) = f
        .join_with_route(
            &[1],
            PURPOSE,
            false,
            session.clone(),
            NostrRoutingV1::new([43; 32], vec!["wss://127.0.0.1:9/".into()]).unwrap(),
        )
        .await;
    assert_ne!(original, collision);
    assert!(f.owners[1].check_group(&collision, None).is_ok());
    let GroupEvent::GroupJoined { via_welcome, .. } = &join else {
        unreachable!()
    };
    let welcome_id = via_welcome.clone();
    f.owners[1].project_join(join).unwrap();
    let bob = &f.owners[1];
    assert!(bob.runtime.group_record(&collision).is_err());
    assert!(
        bob.journal
            .conversation_by_group(collision.as_slice())
            .unwrap()
            .is_none()
    );
    assert!(!bob.journal.group_rejected(collision.as_slice()).unwrap());
    assert_eq!(bob.journal.admission_reject_through().unwrap(), frontier);
    assert!(bob.journal.pending_group_cleanup().unwrap().is_empty());
    assert!(!bob.storage.list_pending_application_events().unwrap().iter().any(|event|
        matches!(event, GroupEvent::GroupJoined {via_welcome, ..} if via_welcome == &welcome_id)));
    assert_eq!(bob.conversations().unwrap().len(), 1);
    assert_eq!(
        bob.conversations().unwrap()[0].group_id,
        hex::encode(original.as_slice())
    );
    assert_eq!(bob.conversations().unwrap()[0].session, session);
    assert_eq!(bob.routing.group_subscriptions().len(), 1);
    f.reopen_bob();
    assert_eq!(f.owners[1].conversations().unwrap().len(), 1);
    assert!(f.owners[1].runtime.group_record(&original).is_ok());
    let (good, join) = f
        .join_with_route(
            &[1],
            PURPOSE,
            false,
            foundation::fresh_id(now().unwrap()).unwrap(),
            NostrRoutingV1::new([44; 32], vec!["wss://127.0.0.1:9/".into()]).unwrap(),
        )
        .await;
    f.owners[1].project_join(join).unwrap();
    f.reopen_bob();
    let bob = &f.owners[1];
    assert_eq!(bob.conversations().unwrap().len(), 2);
    assert_eq!(bob.routing.group_subscriptions().len(), 2);
    assert!(bob.runtime.group_record(&original).is_ok());
    assert!(bob.runtime.group_record(&good).is_ok());
    assert!(bob.runtime.group_record(&collision).is_err());
    assert!(
        bob.journal
            .conversation_by_group(collision.as_slice())
            .unwrap()
            .is_none()
    );
    assert!(!bob.journal.group_rejected(collision.as_slice()).unwrap());
    assert_eq!(bob.journal.admission_reject_through().unwrap(), frontier);
}

#[tokio::test]
async fn authenticated_receipt_requires_canonical_id_without_claiming_incoming_slot() {
    let mut f = Fixture::new(2);
    let (group, join) = f.join(&[1], PURPOSE, false).await;
    f.owners[1].project_join(join).unwrap();
    let session = f.owners[1].conversations().unwrap()[0].session.clone();
    let original = Envelope::message(
        session.clone(),
        foundation::fresh_id(now().unwrap()).unwrap(),
        f.owners[1].identity.clone(),
        f.owners[0].identity.clone(),
        b"original outgoing fixture".to_vec(),
    )
    .unwrap();
    let original_event = cgka_traits::app_event::MarmotAppEvent::new(
        f.owners[1].identity.clone(),
        now().unwrap(),
        foundation::KIND,
        vec![vec!["marmish".into(), "v2-foundation".into()]],
        String::from_utf8(original.encode().unwrap()).unwrap(),
    );
    f.owners[1]
        .journal
        .prepare_send(
            &Prepared {
                group: group.as_slice().to_vec(),
                envelope: original.clone(),
                event_id: original_event.id.clone(),
                encoded: original_event.encode().unwrap(),
            },
            now().unwrap(),
        )
        .unwrap();
    f.owners[1]
        .journal
        .mark(group.as_slice(), &original_event.id, "published")
        .unwrap();
    let expected = foundation::receipt_id(group.as_slice(), &original).unwrap();
    let wrong_id = foundation::fresh_id(now().unwrap()).unwrap();
    assert_ne!(wrong_id, expected);
    let digest = foundation::payload_digest(b"original outgoing fixture");
    let bad = Envelope::receipt(
        session.clone(),
        wrong_id.clone(),
        f.owners[0].identity.clone(),
        f.owners[1].identity.clone(),
        original.id().into(),
        digest.clone(),
    )
    .unwrap();
    let event = f.encrypted_event(&group, &bad, 0).await;
    let GroupEvent::MessageReceived { message_id, .. } = &event else {
        unreachable!()
    };
    let native_id = message_id.clone();
    f.owners[1].project_message(event).unwrap();
    assert_eq!(
        f.owners[1]
            .journal
            .outgoing(group.as_slice(), original.id())
            .unwrap()
            .unwrap()
            .state,
        "published"
    );
    assert!(!f.owners[1].storage.list_pending_application_events().unwrap().iter().any(|e|
        matches!(e, GroupEvent::MessageReceived {message_id, ..} if message_id == &native_id)));
    // A genuine message with the rejected receipt's application ID must still
    // project. A persisted receipt row would instead cause a binding conflict.
    let probe = Envelope::message(
        session.clone(),
        wrong_id,
        f.owners[0].identity.clone(),
        f.owners[1].identity.clone(),
        b"unclaimed incoming slot".to_vec(),
    )
    .unwrap();
    let event = f.encrypted_event(&group, &probe, 1).await;
    f.owners[1].project_message(event).unwrap();
    assert_eq!(f.owners[1].journal.received().unwrap().len(), 1);
    assert_eq!(
        f.owners[1].journal.received().unwrap()[0].data,
        b"unclaimed incoming slot"
    );
    let good = Envelope::receipt(
        session,
        expected,
        f.owners[0].identity.clone(),
        f.owners[1].identity.clone(),
        original.id().into(),
        digest,
    )
    .unwrap();
    let event = f.encrypted_event(&group, &good, 2).await;
    f.owners[1].project_message(event).unwrap();
    assert_eq!(
        f.owners[1]
            .journal
            .outgoing(group.as_slice(), original.id())
            .unwrap()
            .unwrap()
            .state,
        "received"
    );
}

#[tokio::test]
async fn failed_open_attempt_immediately_reconciles_existing_native_creation_orphan() {
    let mut f = Fixture::new(2);
    let (group, _) = f.join(&[1], PURPOSE, false).await;
    let peer = f.owners[1].identity.clone();
    let alice = &mut f.owners[0];
    assert!(alice.runtime.group_record(&group).is_ok());
    assert_eq!(
        alice
            .runtime
            .outstanding_welcome_deliveries()
            .unwrap()
            .len(),
        1
    );
    assert!(alice.conversations().unwrap().is_empty());
    // Native creation succeeded before application acceptance. The next public
    // open fails before networking because this offline owner is inactive.
    // This exercises immediate error-wrapper cleanup, not an injected failure
    // inside create_group or a claim of live discovery-failure coverage.
    assert!(matches!(
        alice
            .open_conversation(&peer, Instant::now() + Duration::from_secs(1))
            .await,
        Err(Error::Mdk("inactive"))
    ));
    assert!(alice.runtime.group_record(&group).is_err());
    assert!(
        alice
            .runtime
            .outstanding_welcome_deliveries()
            .unwrap()
            .is_empty()
    );
    assert!(alice.journal.group_rejected(group.as_slice()).unwrap());
    assert!(alice.journal.pending_group_cleanup().unwrap().is_empty());
}

#[tokio::test]
async fn command_rooms_bind_native_purpose_and_permanent_direction() {
    let mut f = Fixture::new(2);
    let (group, _join) = f
        .join(&[1], RoomPurpose::Command.description(), false)
        .await;
    let creator = f.owners[0].identity.clone();
    assert!(f.owners[0].check_group(&group, None).is_err());
    let client_room = f.owners[0].accept_command_group(&group, &creator).unwrap();
    let server_room = f.owners[1].accept_command_group(&group, &creator).unwrap();
    assert!(client_room.direction == CommandDirection::Client);
    assert!(server_room.direction == CommandDirection::Server);
    assert_eq!(client_room.session, server_room.session);
    assert_eq!(client_room.transport, server_room.transport);
    let client = f.owners[0].command_context(&group).unwrap();
    let server = f.owners[1].command_context(&group).unwrap();
    assert_eq!(client.peer_account(), server.local_account());
    assert_eq!(client.session(), server.session());
    assert_eq!(client.authority_digest(), server.authority_digest());
    assert!(
        f.owners[0]
            .journal
            .conversation_by_group(group.as_slice())
            .unwrap()
            .is_none()
    );
    f.owners[1]
        .journal
        .command_revoke_room(group.as_slice())
        .unwrap();
    assert!(matches!(
        f.owners[1].command_context(&group),
        Err(Error::Mdk("command_room_revoked"))
    ));
    assert!(f.owners[1].accept_command_group(&group, &creator).is_err());
}

#[tokio::test]
async fn foundation_rooms_cannot_mint_command_authority() {
    let mut f = Fixture::new(2);
    let (group, _join) = f.join(&[1], PURPOSE, false).await;
    let creator = f.owners[0].identity.clone();
    assert!(f.owners[0].accept_command_group(&group, &creator).is_err());
    assert!(f.owners[1].accept_command_group(&group, &creator).is_err());
    assert!(f.owners[0].journal.command_rooms().unwrap().is_empty());
}

#[tokio::test]
async fn encrypted_command_bytes_require_the_native_session_binding() {
    use crate::command_wire as wire;
    let mut f = Fixture::new(2);
    let (group, _) = f
        .join(&[1], RoomPurpose::Command.description(), false)
        .await;
    let creator = f.owners[0].identity.clone();
    let room = f.owners[0].accept_command_group(&group, &creator).unwrap();
    f.owners[1].accept_command_group(&group, &creator).unwrap();
    let client = f.materials[0].public_key().to_bytes();
    let server = f.materials[1].public_key().to_bytes();
    let id = hex::decode(foundation::fresh_id(now().unwrap()).unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let mut request = wire::Request {
        binding: wire::Binding {
            session: room.session,
            client,
            server,
        },
        id,
        op: wire::RequestOp::Submit(wire::Submit {
            invocation: id,
            argv: vec![b"/bin/printf".to_vec(), b"'\"$`\\;\n\xff".to_vec()],
            stdin: vec![0, 0xff, b'\n'],
        }),
    };
    let bytes = wire::encode_request(&request).unwrap();
    let app = cgka_traits::app_event::MarmotAppEvent::new(
        creator.clone(),
        now().unwrap(),
        wire::KIND,
        vec![wire::TAG.map(str::to_owned).to_vec()],
        String::from_utf8(bytes.clone()).unwrap(),
    );
    let event = f.encrypted_app(&group, app).await;
    let (context, source, envelope) = f.owners[1].decode_command_event(&event).unwrap();
    assert_eq!(context.peer_account(), client);
    assert_eq!(source.group, group.as_slice());
    let command_adapter::CommandEnvelope::Request(decoded) = envelope else {
        panic!("request expected")
    };
    assert_eq!(wire::encode_request(&decoded).unwrap(), bytes);

    request.binding.session[15] ^= 1;
    let app = cgka_traits::app_event::MarmotAppEvent::new(
        creator,
        now().unwrap(),
        wire::KIND,
        vec![wire::TAG.map(str::to_owned).to_vec()],
        String::from_utf8(wire::encode_request(&request).unwrap()).unwrap(),
    );
    let event = f.encrypted_app(&group, app).await;
    assert!(matches!(
        f.owners[1].decode_command_event(&event),
        Err(Error::Invalid("command_wire_binding"))
    ));
    assert!(f.owners[1].journal.conversations().unwrap().is_empty());
}

#[tokio::test]
async fn command_authority_rejects_extra_accounts_leaves_and_admins() {
    for (accounts, duplicate_account, coadmin) in
        [(3, false, false), (3, true, false), (2, false, true)]
    {
        let mut fixture = Fixture::accounts(accounts, duplicate_account);
        let invitees = if accounts == 3 { vec![1, 2] } else { vec![1] };
        let (group, _) = fixture
            .join(&invitees, RoomPurpose::Command.description(), coadmin)
            .await;
        let creator = fixture.owners[0].identity.clone();
        for owner in &mut fixture.owners[..2] {
            assert!(owner.accept_command_group(&group, &creator).is_err());
            assert!(owner.command_context(&group).is_err());
            assert!(owner.journal.command_rooms().unwrap().is_empty());
            assert!(
                owner
                    .journal
                    .command_historical_principals()
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[tokio::test]
async fn command_purpose_mutation_permanently_revokes_native_authority() {
    let mut fixture = Fixture::new(2);
    let (group, _) = fixture
        .join(&[1], RoomPurpose::Command.description(), false)
        .await;
    let creator = fixture.owners[0].identity.clone();
    fixture.owners[1]
        .accept_command_group(&group, &creator)
        .unwrap();
    assert!(fixture.owners[1].command_context(&group).is_ok());
    fixture
        .mutate(SendIntent::UpdateGroupData {
            group_id: group.clone(),
            name: None,
            description: Some("ordinary chat".into()),
        })
        .await;
    assert!(matches!(
        fixture.owners[1].command_context(&group),
        Err(Error::Mdk("command_room_revoked"))
    ));
    assert!(
        fixture.owners[1]
            .journal
            .command_room(group.as_slice())
            .unwrap()
            .unwrap()
            .revoked
    );
    assert!(fixture.owners[1].routing.group_subscriptions().is_empty());
    fixture
        .mutate(SendIntent::UpdateGroupData {
            group_id: group.clone(),
            name: None,
            description: Some(RoomPurpose::Command.description().into()),
        })
        .await;
    assert!(matches!(
        fixture.owners[1].command_context(&group),
        Err(Error::Mdk("command_room_revoked"))
    ));
    assert!(
        fixture.owners[1]
            .accept_command_group(&group, &creator)
            .is_err()
    );
}

#[tokio::test]
async fn command_source_replay_states_defer_without_revocation_or_ack() {
    use crate::command_wire as wire;
    use crate::journal::{AdmissionDecision, CommandProjection, Owner};
    let mut fixture = Fixture::new(2);
    let (group, _) = fixture
        .join(&[1], RoomPurpose::Command.description(), false)
        .await;
    let creator = fixture.owners[0].identity.clone();
    let room = fixture.owners[0]
        .accept_command_group(&group, &creator)
        .unwrap();
    fixture.owners[1]
        .accept_command_group(&group, &creator)
        .unwrap();
    let id = hex::decode(foundation::fresh_id(now().unwrap()).unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let request = wire::Request {
        binding: wire::Binding {
            session: room.session,
            client: fixture.materials[0].public_key().to_bytes(),
            server: fixture.materials[1].public_key().to_bytes(),
        },
        id,
        op: wire::RequestOp::Submit(wire::Submit {
            invocation: id,
            argv: vec![b"/bin/true".to_vec()],
            stdin: vec![],
        }),
    };
    let bytes = wire::encode_request(&request).unwrap();
    let event = fixture
        .encrypted_app(
            &group,
            cgka_traits::app_event::MarmotAppEvent::new(
                creator,
                now().unwrap(),
                wire::KIND,
                vec![wire::TAG.map(str::to_owned).to_vec()],
                String::from_utf8(bytes.clone()).unwrap(),
            ),
        )
        .await;
    let bob = &mut fixture.owners[1];
    let (context, source, _) = bob.decode_command_event(&event).unwrap();
    let refusal = wire::WireError {
        code: wire::ErrorCode::CommandDenied,
        detail: None,
    };
    let response = wire::Response {
        binding: request.binding.clone(),
        id,
        op: wire::Op::Submit,
        request_sha256: wire::request_digest(&bytes),
        result: Err(refusal.clone()),
    };
    let publication = prepare_command_publication(
        group.as_slice(),
        request.binding.clone(),
        id,
        response.request_sha256,
        wire::encode_response(&response).unwrap(),
        request.binding.server,
    )
    .unwrap();
    let plan = CommandProjection {
        owner: Owner::from_context(&context),
        source: source.clone(),
        request,
        decision: AdmissionDecision::Refused(refusal),
        response: Some(publication),
    };
    let key = bob
        .journal
        .command_project_request(&plan, now().unwrap())
        .unwrap()
        .key;
    let native_id = MessageId::new(source.native_id.clone());
    let original = bob.storage.get_message(&native_id).unwrap();
    for state in [
        cgka_traits::MessageState::Created,
        cgka_traits::MessageState::Retryable,
        cgka_traits::MessageState::ConvergenceDeferred,
        cgka_traits::MessageState::PeelDeferred,
    ] {
        bob.storage.update_message_state(&native_id, state).unwrap();
        bob.reconcile_command_sources().unwrap();
        assert!(
            bob.journal
                .command_get(&key)
                .unwrap()
                .unwrap()
                .authority_valid
        );
        let bindings = bob.journal.command_native_bindings(None, 64).unwrap();
        assert_eq!(bindings.rows.len(), 1);
        assert!(!bindings.rows[0].acked);
    }
    bob.storage.delete_message(&native_id).unwrap();
    bob.reconcile_command_sources().unwrap();
    assert!(
        bob.journal
            .command_get(&key)
            .unwrap()
            .unwrap()
            .authority_valid
    );
    assert!(!bob.journal.command_native_bindings(None, 64).unwrap().rows[0].acked);
    bob.storage.put_message(&original).unwrap();
    bob.storage
        .update_message_state(&native_id, cgka_traits::MessageState::EpochInvalidated)
        .unwrap();
    bob.reconcile_command_sources().unwrap();
    assert!(
        !bob.journal
            .command_get(&key)
            .unwrap()
            .unwrap()
            .authority_valid
    );
}

// Build owned response obligations from real authenticated command events. The
// effects below are controlled offline inputs, not evidence of relay delivery.
async fn owned_command_refusal(
    fixture: &mut Fixture,
    group: &GroupId,
) -> crate::journal::PreparedCommandPublication {
    use crate::command_wire as wire;
    use crate::journal::{AdmissionDecision, CommandProjection, Owner};
    let context = fixture.owners[1].command_context(group).unwrap();
    let id = hex::decode(foundation::fresh_id(now().unwrap()).unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let request = wire::Request {
        binding: wire::Binding {
            session: context.session(),
            client: fixture.materials[0].public_key().to_bytes(),
            server: fixture.materials[1].public_key().to_bytes(),
        },
        id,
        op: wire::RequestOp::Submit(wire::Submit {
            invocation: id,
            argv: vec![b"/bin/true".to_vec()],
            stdin: vec![],
        }),
    };
    let bytes = wire::encode_request(&request).unwrap();
    let event = fixture
        .encrypted_app(
            group,
            cgka_traits::app_event::MarmotAppEvent::new(
                fixture.owners[0].identity.clone(),
                now().unwrap(),
                wire::KIND,
                vec![wire::TAG.map(str::to_owned).to_vec()],
                String::from_utf8(bytes.clone()).unwrap(),
            ),
        )
        .await;
    let bob = &mut fixture.owners[1];
    let (context, source, _) = bob.decode_command_event(&event).unwrap();
    let refusal = wire::WireError {
        code: wire::ErrorCode::CommandDenied,
        detail: None,
    };
    let response = wire::Response {
        binding: request.binding.clone(),
        id,
        op: wire::Op::Submit,
        request_sha256: wire::request_digest(&bytes),
        result: Err(refusal.clone()),
    };
    let publication = prepare_command_publication(
        group.as_slice(),
        request.binding.clone(),
        id,
        response.request_sha256,
        wire::encode_response(&response).unwrap(),
        request.binding.server,
    )
    .unwrap();
    bob.journal
        .command_project_request(
            &CommandProjection {
                owner: Owner::from_context(&context),
                source,
                request,
                decision: AdmissionDecision::Refused(refusal),
                response: Some(publication.clone()),
            },
            now().unwrap(),
        )
        .unwrap();
    assert!(
        bob.journal
            .command_claim_publication(&publication.key)
            .unwrap()
            .is_some()
    );
    publication
}

fn command_published_effect(
    publication: &crate::journal::PreparedCommandPublication,
    native: &[u8],
) -> marmot_session::PublishedApplicationMessage {
    marmot_session::PublishedApplicationMessage {
        group_id: GroupId::new(publication.key.group.clone()),
        app_event_id: publication.key.app_event_id.clone(),
        message_id: MessageId::new(native.to_vec()),
        source_epoch: cgka_traits::EpochId(1),
        retention: cgka_traits::app_event::AppMessageRetentionDecision {
            retention_seconds: 86400,
            expires_at: None,
        },
        authority: None,
    }
}

#[tokio::test]
async fn unmatched_command_queue_refusal_does_not_fail_the_owner() {
    let mut fixture = Fixture::new(2);
    let (group, _) = fixture
        .join(&[1], RoomPurpose::Command.description(), false)
        .await;
    let creator = fixture.owners[0].identity.clone();
    fixture.owners[0]
        .accept_command_group(&group, &creator)
        .unwrap();
    fixture.owners[1]
        .accept_command_group(&group, &creator)
        .unwrap();
    let publication = owned_command_refusal(&mut fixture, &group).await;
    let published = command_published_effect(&publication, b"later-known-publication");
    let mut effects = AccountDeviceEffects::default();
    effects.events.push(GroupEvent::QueuedIntentRefused {
        group_id: group,
        intent_id: MessageId::new(b"unrecorded-intent".to_vec()),
        intent_kind: "app_message".into(),
        reason: "not_group_member".into(),
    });
    effects.published_app_messages.push(published.clone());
    let bob = &mut fixture.owners[1];
    bob.apply_effects(effects, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    let bindings = bob
        .journal
        .command_publication_bindings(&publication.key)
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].source.native_id,
        published.message_id.as_slice()
    );
    assert!(
        bindings[0].evidence
            == Some(crate::command_evidence::PublishedEvidence::from_native(
                &published
            ))
    );
    assert!(!bindings[0].acked);
    assert!(
        bob.journal
            .command_publications(crate::journal::PublicationState::Published, 64)
            .unwrap()[0]
            .key
            == publication.key
    );
}

#[tokio::test]
async fn command_invalidation_capacity_preserves_terminal_verdict_and_later_effects() {
    use crate::journal::{CommandNativeUpdate, PublicationState};
    let mut fixture = Fixture::new(2);
    let (group, _) = fixture
        .join(&[1], RoomPurpose::Command.description(), false)
        .await;
    let creator = fixture.owners[0].identity.clone();
    fixture.owners[0]
        .accept_command_group(&group, &creator)
        .unwrap();
    fixture.owners[1]
        .accept_command_group(&group, &creator)
        .unwrap();
    let invalidated = owned_command_refusal(&mut fixture, &group).await;
    let later = owned_command_refusal(&mut fixture, &group).await;
    let bob = &mut fixture.owners[1];
    for n in 0u16..256 {
        bob.journal
            .command_apply_native(&[CommandNativeUpdate::Invalidated {
                group: group.as_slice().to_vec(),
                native_id: n.to_be_bytes().to_vec(),
                possible_publication: Some(invalidated.key.clone()),
            }])
            .unwrap();
    }
    assert_eq!(
        bob.journal.command_pending_invalidations().unwrap().len(),
        256
    );
    let published = command_published_effect(&invalidated, b"new-invalidated-source");
    let surviving = command_published_effect(&later, b"later-surviving-source");
    let mut effects = AccountDeviceEffects::default();
    effects.events.push(GroupEvent::AppMessageInvalidated {
        group_id: group,
        message_id: published.message_id.clone(),
        epoch: cgka_traits::EpochId(1),
        reason: cgka_traits::engine::AppMessageInvalidationReason::LosingBranch,
        decrypted_payload_ref: None,
    });
    effects
        .published_app_messages
        .extend([published, surviving.clone()]);
    bob.apply_effects(effects, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    assert!(
        bob.journal
            .command_publications(PublicationState::Invalidated, 64)
            .unwrap()[0]
            .key
            == invalidated.key
    );
    // Skipped confirmation must neither allocate a native binding nor create an
    // ACK marker. No actual native fanout/network acknowledgement is fabricated.
    assert!(
        bob.journal
            .command_publication_bindings(&invalidated.key)
            .unwrap()
            .is_empty()
    );
    let bindings = bob
        .journal
        .command_publication_bindings(&later.key)
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].source.native_id,
        surviving.message_id.as_slice()
    );
    assert!(
        bindings[0].evidence
            == Some(crate::command_evidence::PublishedEvidence::from_native(
                &surviving
            ))
    );
    assert!(!bindings[0].acked);
    assert!(
        bob.journal
            .command_publications(PublicationState::Published, 64)
            .unwrap()[0]
            .key
            == later.key
    );
}
