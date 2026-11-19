use super::{joined_port::Fixture, *};
use crate::peer_invitation::{
    consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::retry::retirement::ManagedSaved,
    socket::owner::recipient::managed_consent::Publication,
    consent_checkpoint::Error as CheckpointError,
    receiver_inventory::managed_delivery::SavedVisit,
    store::Error as CursorError,
};
use crate::peer_quota_vfs::{dispatch::welcome_counts, stage_native_opens};
use crate::peer_relay_trust::candidate_registration::ForwardingVerifier;

#[test]
#[ignore = "opt-in registered WSS saved Delivery retry and signed cutoff"]
fn saved_delivery_guarded_visit_private_wss() {
    run_cases(&["saved-waiting", "saved-live-cliff"]);
}

pub(super) async fn run(
    case: &str,
    mut fixture: Fixture<'_, '_>,
    mut owner: RecoveredOwner<'_>,
    queued: ManagedQueued,
    forwarding: &ForwardingVerifier,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
) {
    let (id, cutoff) = fixture.defer_delivery(&mut owner, &queued, true).await;
    let snapshot = |owner: &RecoveredOwner<'_>| {
        (
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            owner
                .checkpoint
                .fixture_retry_row(&owner.source, id)
                .unwrap(),
        )
    };
    if case == "saved-live-cliff" {
        let publication = fixture.port.publish_managed_consent(
            &mut owner,
            fixture.quota,
            Selector::from(fixture.offer),
            fixture.readiness.clone(),
            Arc::clone(&fixture.stop),
        );
        assert!(matches!(publication.await, Publication::Settled(_)));
    }
    let due = snapshot(&owner).1.0;
    wait_for_wall(due, cutoff).await;
    let before = snapshot(&owner);
    let coordinate = owner
        .probe_due_managed_delivery(fixture.port)
        .unwrap()
        .unwrap();
    assert_eq!(snapshot(&owner), before);

    let saved = fixture
        .port
        .use_current_recipient(|account, origin, wall| {
            owner.checkpoint.acquire_installed_delivery(
                &owner.source,
                (account, origin, wall),
                Some(coordinate),
                || Err(CheckpointError::Expired),
            )
        })
        .unwrap()
        .err();
    assert_eq!(saved, Some(CheckpointError::Expired));
    assert_eq!(snapshot(&owner), before);

    #[cfg(marmish_peer_receiver_candidate)]
    if case == "saved-waiting" {
        use crate::dispatcher_fatal::Trip;
        let (_signal, foreign_fatal) = Trip::new();
        let (admission, _drain) = ReceiverRouteAdmission::fixture(Arc::clone(lease), material)
            .unwrap()
            .fixture_guarded(foreign_fatal)
            .unwrap();
        let mut foreign = PeerRecipientPort::open(
            admission,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert_eq!(
            owner.service_saved_delivery(
                fixture.quota,
                &mut foreign,
                coordinate,
                &fixture.readiness,
                &fixture.stop,
                forwarding,
            ),
            Err(Error::Binding)
        );
        assert_eq!(snapshot(&owner), before);
    }
    #[cfg(not(marmish_peer_receiver_candidate))]
    let _ = (lease, material);

    let saved = fixture
        .port
        .use_current_recipient(|account, origin, wall| {
            owner.checkpoint.acquire_installed_delivery(
                &owner.source,
                (account, origin, wall),
                Some(coordinate),
                || Ok(()),
            )
        })
        .unwrap()
        .unwrap()
        .unwrap();
    let acquired = snapshot(&owner);
    assert_eq!(acquired.1, before.1);
    let effects = || (welcome_counts(), stage_native_opens());
    let native = effects();
    if case == "saved-waiting" {
        let observed = fixture
            .port
            .use_saved_delivery(&saved, |checked| {
                owner.checkpoint.observe_managed_saved_guarded(
                    &owner.source,
                    checked,
                    |signed_end| {
                        assert_eq!(signed_end, cutoff);
                        Err(CheckpointError::Cursor(CursorError::Stale))
                    },
                )
            })
            .unwrap()
            .err();
        assert_eq!(observed, Some(CheckpointError::Cursor(CursorError::Stale)));
        assert_eq!(snapshot(&owner), acquired);
        assert_eq!(effects(), native);
        assert_eq!(
            owner.service_saved_delivery(
                fixture.quota,
                fixture.port,
                coordinate,
                &fixture.readiness,
                &fixture.stop,
                forwarding,
            ),
            Ok(SavedVisit::Waiting)
        );
        let recorded = snapshot(&owner);
        assert!(recorded.1.0 > acquired.1.0);
        assert!(recorded.1.1 > acquired.1.1);
        assert_eq!(recorded.1.2, acquired.1.2);
        assert_eq!(recorded.0[4..], acquired.0[4..]);
    } else {
        let observed = fixture
            .port
            .use_saved_delivery(&saved, |checked| {
                owner.observe_managed_saved_delivery(checked)
            })
            .unwrap()
            .unwrap();
        let ManagedSaved::Live(prepared) = observed else {
            panic!("the queued exact Delivery did not become eligible");
        };
        wait_for_wall(cutoff - 1, cutoff).await;
        let window = fixture
            .port
            .use_current_native(|route| {
                route.manual_window(
                    fixture.readiness.clone(),
                    Arc::clone(&fixture.stop),
                    u64::MAX,
                    forwarding,
                )
            })
            .unwrap()
            .unwrap();
        wait_for_wall(cutoff, cutoff + 1).await;
        assert_eq!(
            owner.visit_live_saved(fixture.quota, fixture.port, &saved, prepared, &window),
            Ok(SavedVisit::Reobserve)
        );
        assert_eq!(snapshot(&owner), acquired);
    }
    assert_eq!(effects(), native);
}

async fn wait_for_wall(target: u64, exclusive: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let wall = Wall::System.sample().unwrap().as_secs();
        assert!(wall < exclusive);
        if wall >= target {
            return;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
