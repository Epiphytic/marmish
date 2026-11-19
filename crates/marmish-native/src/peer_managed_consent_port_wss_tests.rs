use nostr::event::EventId;
use sha2::{Digest, Sha256};

use super::*;
use crate::peer_invitation::store::consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::retry::retirement::{ManagedSaved, Retirement};
use crate::{
    mdk_adapter::{
        peer_staged_proof_tests::peer_enrollment_welcome, terminal_relay_fixture::Relay,
    },
    peer_candidate_startup::Readiness,
    peer_invitation::{
        consent_checkpoint::{
            Status, provisioning::prepared_consent::custody::delivery_ingress::Observation,
        },
        socket::{
            owner::recipient::managed_consent::{Cut, Publication, Wake},
            wire::CARRIER,
        },
    },
    peer_quota_vfs::{dispatch::welcome_counts, stage_native_opens},
};

#[test]
#[ignore = "opt-in registered single-port publication and exact deferred Delivery wake"]
fn managed_consent_single_port_private_wss() {
    run_cases(&[
        "port-positive",
        "port-wake-cut",
        "port-wake-stop",
        "port-staged",
        "port-reject",
        "port-lost",
        "port-stop",
    ]);
}

#[test]
#[ignore = "opt-in registered signed expired Delivery retirement"]
fn managed_expired_delivery_private_wss() {
    run_cases(&["port-expired", "port-expired-edge"]);
}

pub(super) struct Fixture<'a, 'profile> {
    pub(super) relay: &'a Relay,
    pub(super) port: &'a mut PeerRecipientPort,
    pub(super) sender: &'a mut Owner,
    pub(super) founder: &'a Keys,
    pub(super) offer: &'a wire::Offer,
    pub(super) quota: &'a ReceiverQuota<'profile>,
    pub(super) readiness: Readiness,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) profile: &'a Path,
}

fn wall() -> u64 {
    Wall::System.sample().unwrap().as_secs()
}

fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(15)
}

impl Fixture<'_, '_> {
    async fn defer_delivery(
        &mut self,
        owner: &mut RecoveredOwner<'_>,
        queued: &ManagedQueued,
        short_lived: bool,
    ) -> ([u8; 32], u64) {
        let (_, frame) = queued.fixture_frame();
        let codec = wire::Codec::for_version(wire::MAX_OUTER_BYTES, ControlVersion::V2).unwrap();
        let consent = codec
            .open_consent(self.founder, frame, self.offer, wall())
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let welcome = peer_enrollment_welcome(
            root.path(),
            Arc::new(self.founder.clone()),
            self.offer.flight().recipient,
            consent.offer().routing(),
            consent.offer().room_id(),
            consent.key_package().clone(),
        )
        .await;
        let now = wall();
        let expires = if short_lived {
            now + 300 + 7
        } else {
            consent.flight().expires_at - 1
        };
        let delivery = codec
            .prepare_delivery(self.founder, &consent, &welcome, now, expires)
            .unwrap();
        let flight = delivery.flight.flight();
        assert!(
            self.sender
                .publish(&delivery, flight, deadline())
                .await
                .unwrap()
        );
        let live = self.port.wait_live(deadline()).await.unwrap().unwrap();
        let prepare = |port: &mut PeerRecipientPort, owner: &mut RecoveredOwner<'_>| {
            let observation = port
                .use_live_carrier(&live, |checked| owner.observe_managed_delivery(checked))
                .unwrap()
                .unwrap();
            let Observation::Inspect(prepared) = observation else {
                panic!("authenticated Delivery did not reach inspection");
            };
            prepared
        };
        let prepared = prepare(self.port, owner);
        let other_turn = prepare(self.port, owner);
        let measure_refusal = self
            .port
            .use_delivery_native(prepared.challenge().unwrap(), |checked| {
                owner.measure_delivery(
                    self.quota,
                    checked,
                    self.readiness.clone(),
                    Arc::clone(&self.stop),
                )
            })
            .unwrap()
            .unwrap();
        let before = owner
            .checkpoint
            .fixture_delivery_state(&owner.source)
            .unwrap();
        assert_eq!(
            self.port
                .use_live_carrier(&live, |checked| {
                    owner.finish_measured_delivery(
                        self.quota,
                        checked,
                        *other_turn,
                        measure_refusal,
                    )
                })
                .unwrap()
                .err(),
            Some(Error::Checkpoint(
                crate::peer_invitation::store::consent_checkpoint::Error::Source
            )),
        );
        assert_eq!(
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            before
        );
        let proof = self
            .port
            .use_delivery_native(prepared.challenge().unwrap(), |checked| {
                owner.measure_delivery(
                    self.quota,
                    checked,
                    self.readiness.clone(),
                    Arc::clone(&self.stop),
                )
            })
            .unwrap()
            .unwrap();
        let result = self
            .port
            .use_live_carrier(&live, |checked| {
                owner.finish_measured_delivery(self.quota, checked, *prepared, proof)
            })
            .unwrap()
            .unwrap();
        assert_eq!(result.status, Status::Deferred);

        for _ in 0..if short_lived { 0 } else { 3 } {
            let (due, _, _) = owner
                .checkpoint
                .fixture_retry_row(&owner.source, flight.carrier_id.to_bytes())
                .unwrap();
            while wall() < due {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let saved = self
                .port
                .use_current_recipient(|account, origin, wall| {
                    owner
                        .checkpoint
                        .due_installed_delivery(&owner.source, account, origin, wall)
                })
                .unwrap()
                .unwrap()
                .unwrap();
            let observation = self
                .port
                .use_saved_delivery(&saved, |checked| {
                    owner
                        .checkpoint
                        .observe_saved_delivery(&owner.source, checked)
                })
                .unwrap()
                .unwrap();
            assert!(observation.is_none());
        }
        (flight.carrier_id.to_bytes(), expires - 300)
    }
}

async fn retire_expired(
    mut fixture: Fixture<'_, '_>,
    mut owner: RecoveredOwner<'_>,
    queued: ManagedQueued,
    past_lifetimes: bool,
) {
    let total = Instant::now();
    let (expired_id, cutoff) = fixture.defer_delivery(&mut owner, &queued, true).await;
    assert!(
        wall() < cutoff,
        "the signed Delivery was deferred before its cutoff"
    );
    let other = [0xe3; 32];
    let other_prerequisite = [0x75; 32];
    owner
        .checkpoint
        .fixture_later_retry(&owner.source, expired_id, other, other_prerequisite, cutoff)
        .unwrap();
    let original = fixture.profile.join("peer/provisional-unknown.db");
    let bytes = fs::read(&original).unwrap();
    let native = (welcome_counts(), stage_native_opens());
    let relay_attempts = fixture.relay.publication_attempts(CARRIER);
    let relay_frames = fixture.relay.peer_transcript().len();
    let sleep_deadline = Instant::now() + Duration::from_secs(10);
    while wall() < cutoff {
        assert!(Instant::now() < sleep_deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let saved = fixture
        .port
        .use_current_recipient(|account, origin, wall| {
            owner.due_managed_delivery(account, origin, wall)
        })
        .unwrap()
        .unwrap()
        .unwrap();
    let initial = owner.recover(wall()).unwrap();
    let mut before = owner
        .checkpoint
        .fixture_delivery_state(&owner.source)
        .unwrap();
    let retire = |fixture: &mut Fixture<'_, '_>, owner: &mut RecoveredOwner<'_>, stop| {
        let observation = fixture
            .port
            .use_saved_delivery(&saved, |checked| {
                owner.observe_managed_saved_delivery(checked)
            })
            .unwrap()
            .unwrap();
        let ManagedSaved::Expired(expired) = observation else {
            panic!("the signed saved Delivery should be expired");
        };
        assert_eq!(
            expired.historical_sample().stage_sample().as_secs(),
            cutoff - 1
        );
        assert_eq!(
            expired.check_route(fixture.founder.public_key(), "", cutoff),
            Err(Retirement::Stop),
        );
        let future_wall = expired.fixture_after_lifetimes();
        fixture
            .port
            .use_expired_delivery_native(expired, |route| {
                if past_lifetimes {
                    return owner.fixture_retire_expired_at(
                        fixture.quota,
                        route,
                        fixture.readiness.clone(),
                        stop,
                        future_wall,
                    );
                }
                owner.retire_expired_delivery(fixture.quota, route, fixture.readiness.clone(), stop)
            })
            .unwrap()
    };

    let retired_row = owner
        .checkpoint
        .fixture_retry_row(&owner.source, expired_id)
        .unwrap();
    let other_row = owner
        .checkpoint
        .fixture_retry_row(&owner.source, other)
        .unwrap();
    let stop = Arc::clone(&fixture.stop);
    if !past_lifetimes {
        use crate::peer_invitation::{
            consent_checkpoint::Error as CheckpointError, store::Error as CursorError,
        };
        owner
            .checkpoint
            .fixture_retry_metadata(&owner.source, expired_id, retired_row.1 + 1, cutoff)
            .unwrap();
        let stale = fixture
            .port
            .use_saved_delivery(&saved, |checked| {
                owner.observe_managed_saved_delivery(checked)
            })
            .unwrap()
            .err();
        assert_eq!(
            stale,
            Some(Error::Checkpoint(CheckpointError::Cursor(
                CursorError::Stale
            )))
        );
        owner
            .checkpoint
            .fixture_retry_metadata(&owner.source, expired_id, retired_row.1, cutoff - 1)
            .unwrap();
        let drift = fixture
            .port
            .use_current_recipient(|account, origin, wall| {
                owner.due_managed_delivery(account, origin, wall)
            })
            .unwrap()
            .unwrap()
            .unwrap();
        let drift_state = owner
            .checkpoint
            .fixture_delivery_state(&owner.source)
            .unwrap();
        let observed = fixture
            .port
            .use_saved_delivery(&drift, |checked| {
                owner.observe_managed_saved_delivery(checked)
            })
            .unwrap()
            .unwrap();
        let ManagedSaved::Expired(expired) = observed else {
            panic!("the changed cutoff still classifies as expired");
        };
        assert_eq!(
            fixture
                .port
                .use_expired_delivery_native(expired, |route| {
                    owner.retire_expired_delivery(
                        fixture.quota,
                        route,
                        fixture.readiness.clone(),
                        Arc::clone(&stop),
                    )
                })
                .unwrap(),
            Retirement::Stop
        );
        assert_eq!(
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            drift_state
        );
        owner
            .checkpoint
            .fixture_retry_metadata(&owner.source, expired_id, retired_row.1, cutoff)
            .unwrap();
        before = owner
            .checkpoint
            .fixture_delivery_state(&owner.source)
            .unwrap();
        assert_eq!(
            retire(&mut fixture, &mut owner, Arc::new(AtomicBool::new(true))),
            Retirement::Transient
        );
        assert_eq!(
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            before
        );
        assert_eq!(
            owner
                .checkpoint
                .fixture_retry_row(&owner.source, expired_id)
                .unwrap(),
            retired_row
        );

        let saved_stage = original.with_extension("held-out");
        fs::rename(&original, &saved_stage).unwrap();
        assert_eq!(
            retire(&mut fixture, &mut owner, Arc::clone(&stop)),
            Retirement::Stop
        );
        fs::rename(saved_stage, &original).unwrap();
        assert_eq!(
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            before,
        );

        change_image(&original, 0, bytes[0] ^ 1);
        assert_eq!(
            retire(&mut fixture, &mut owner, Arc::clone(&stop)),
            Retirement::Stop
        );
        change_image(&original, 0, bytes[0]);
        assert_eq!(
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            before
        );
        assert_eq!(
            owner
                .checkpoint
                .fixture_retry_row(&owner.source, expired_id)
                .unwrap(),
            retired_row
        );

        let observed = fixture
            .port
            .use_saved_delivery(&saved, |checked| {
                owner.observe_managed_saved_delivery(checked)
            })
            .unwrap()
            .unwrap();
        let ManagedSaved::Expired(expired) = observed else {
            panic!("the saved row should still be expired");
        };
        assert_eq!(
            fixture.port.fixture_foreign_saved_scope(expired),
            Err(crate::peer_invitation::socket::wire::Failure::Binding),
        );
        let later_limit = Instant::now() + Duration::from_secs(2);
        while wall() <= u64::try_from(before[2]).unwrap() {
            assert!(Instant::now() < later_limit);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let observed = fixture
            .port
            .use_saved_delivery(&saved, |checked| {
                owner.observe_managed_saved_delivery(checked)
            })
            .unwrap()
            .unwrap();
        let ManagedSaved::Expired(expired) = observed else {
            panic!("the saved row should still be expired");
        };
        let halt = Arc::new(AtomicBool::new(false));
        let mut reached = false;
        let before_commit = fixture
            .port
            .use_expired_delivery_native(expired, |route| {
                owner.fixture_retire_expired_before_commit(
                    fixture.quota,
                    route,
                    fixture.readiness.clone(),
                    halt.clone(),
                    || {
                        reached = true;
                        halt.store(true, Ordering::Release);
                    },
                )
            })
            .unwrap();
        assert!(
            reached,
            "the stop cut must follow the DELETE and both tentative clock updates"
        );
        assert_eq!(before_commit, Retirement::Transient);
        assert_eq!(
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            before
        );
        assert_eq!(
            owner
                .checkpoint
                .fixture_retry_row(&owner.source, expired_id)
                .unwrap(),
            retired_row
        );
        let observed = fixture
            .port
            .use_saved_delivery(&saved, |checked| {
                owner.observe_managed_saved_delivery(checked)
            })
            .unwrap()
            .unwrap();
        let ManagedSaved::Expired(expired) = observed else {
            panic!("the saved row should still be expired");
        };
        let cursor = owner.cursor.path().to_owned();
        let held_cursor = cursor.with_extension("held-out");
        let mut moved = false;
        let refused = fixture
            .port
            .use_expired_delivery_native(expired, |route| {
                owner.fixture_retire_expired_before_commit(
                    fixture.quota,
                    route,
                    fixture.readiness.clone(),
                    Arc::clone(&stop),
                    || {
                        fs::rename(&cursor, &held_cursor).unwrap();
                        moved = true;
                    },
                )
            })
            .unwrap();
        assert!(moved);
        fs::rename(held_cursor, cursor).unwrap();
        assert_eq!(refused, Retirement::Stop);
        assert_eq!(
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            before
        );
        assert_eq!(
            owner
                .checkpoint
                .fixture_retry_row(&owner.source, expired_id)
                .unwrap(),
            retired_row,
        );
    }
    let measured = Instant::now();
    assert_eq!(retire(&mut fixture, &mut owner, stop), Retirement::Retired);
    let elapsed = measured.elapsed();
    let after = owner
        .checkpoint
        .fixture_delivery_state(&owner.source)
        .unwrap();
    assert_eq!(after[..2], before[..2]);
    assert!(after[2] >= before[2]);
    assert_eq!(after[2], after[3]);
    assert_eq!(after[4], 1);
    assert_eq!(after[5], before[5]);
    assert!(
        owner
            .checkpoint
            .fixture_retry_row(&owner.source, expired_id)
            .is_err()
    );
    assert_eq!(
        owner
            .checkpoint
            .fixture_retry_row(&owner.source, other)
            .unwrap(),
        other_row
    );
    if !past_lifetimes {
        let next = fixture
            .port
            .use_current_recipient(|account, origin, wall| {
                owner.due_managed_delivery(account, origin, wall)
            })
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(next.fixture_id(), other);
    } else {
        assert!(after[2] > before[2] + 300);
    }
    let inventory = owner.recover(u64::try_from(after[2]).unwrap()).unwrap();
    assert_eq!(inventory.charged, initial.charged);
    assert_eq!(inventory.extents, initial.extents);
    assert_eq!((welcome_counts(), stage_native_opens()), native);
    assert_eq!(fixture.relay.publication_attempts(CARRIER), relay_attempts);
    assert_eq!(fixture.relay.peer_transcript().len(), relay_frames);
    assert_eq!(fs::read(&original).unwrap(), bytes);
    eprintln!(
        "managed_expired_delivery native_turn_ms={:.3} total_ms={:.3}",
        elapsed.as_secs_f64() * 1000.0,
        total.elapsed().as_secs_f64() * 1000.0
    );
}

fn change_image(path: &Path, offset: u64, byte: u8) {
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .write_all_at(&[byte], offset)
        .unwrap();
}

pub(super) async fn run<'profile>(
    case: &str,
    mut fixture: Fixture<'_, 'profile>,
    mut owner: RecoveredOwner<'profile>,
    queued: ManagedQueued,
    reopen: impl FnOnce() -> RecoveredOwner<'profile>,
) {
    if matches!(case, "expired" | "expired-edge") {
        retire_expired(fixture, owner, queued, case == "expired-edge").await;
        return;
    }
    let native = welcome_counts();
    let (carrier, bytes) = queued.fixture_frame();
    let bytes = bytes.to_vec();
    let selector = Selector::from(fixture.offer);
    let wake = matches!(case, "positive" | "wake-cut" | "wake-stop");
    let related = if wake {
        let (delivery, _) = fixture.defer_delivery(&mut owner, &queued, false).await;
        let other = [0xa8; 32];
        let other_prerequisite = [0x55; 32];
        let prior = owner
            .checkpoint
            .fixture_retry_row(&owner.source, delivery)
            .unwrap();
        assert_eq!(prior.2, carrier);
        assert!(prior.0 >= wall() + 7);
        owner
            .checkpoint
            .fixture_other_retry(&owner.source, delivery, other, other_prerequisite)
            .unwrap();
        let unrelated = owner
            .checkpoint
            .fixture_retry_row(&owner.source, other)
            .unwrap();
        Some((delivery, prior, other, unrelated))
    } else {
        None
    };
    let floors = owner
        .checkpoint
        .fixture_delivery_state(&owner.source)
        .unwrap();
    let observed = fixture.relay.peer_transcript().len();
    let writes = fixture.relay.publication_attempts(CARRIER);
    let event = EventId::from_byte_array(carrier);
    match case {
        "reject" => fixture.relay.recipient_reject_ack(event),
        "lost" => fixture.relay.recipient_lose_ack(event),
        _ => {}
    }
    let file = fixture.profile.join("peer/provisional-unknown.db");
    let image = fs::read(&file).unwrap();
    let offset = u64::try_from(image.len() - 1).unwrap();
    let last = *image.last().unwrap();
    let mut cuts = 0;
    let mut wake_clock = None;
    let start = Instant::now();
    let outcome = if matches!(case, "positive" | "reject" | "lost") {
        fixture
            .port
            .publish_managed_consent(
                &mut owner,
                fixture.quota,
                selector,
                fixture.readiness.clone(),
                fixture.stop.clone(),
            )
            .await
    } else {
        fixture
            .port
            .fixture_managed_consent_cut(
                &mut owner,
                fixture.quota,
                selector,
                fixture.readiness.clone(),
                fixture.stop.clone(),
                |cut| match (case, cut) {
                    ("staged", Cut::Staged) => {
                        cuts += 1;
                        change_image(&file, offset, last ^ 1);
                        Ok(())
                    }
                    ("wake-cut", Cut::WakeBeforeCommit) => {
                        cuts += 1;
                        Err(Error::Binding)
                    }
                    ("wake-cut", Cut::Settled) => {
                        let settled_wall = wall();
                        let end = Instant::now() + Duration::from_millis(1500);
                        while wall() <= settled_wall && Instant::now() < end {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        let wake_wall = wall();
                        assert!(wake_wall > settled_wall);
                        wake_clock = Some(wake_wall);
                        Ok(())
                    }
                    ("wake-stop", Cut::Settled) => {
                        cuts += 1;
                        fixture.stop.store(true, Ordering::Release);
                        Ok(())
                    }
                    ("stop", Cut::Accepted) => {
                        cuts += 1;
                        fixture.stop.store(true, Ordering::Release);
                        Ok(())
                    }
                    _ => Ok(()),
                },
            )
            .await
    };
    let elapsed = start.elapsed();
    let expected = match case {
        "positive" => Publication::Settled(Wake::Completed(1)),
        "wake-cut" | "wake-stop" => Publication::Settled(Wake::Failed),
        "staged" => Publication::NotSent(Error::StageIdentityMismatch),
        "reject" => Publication::Rejected,
        "lost" => Publication::PossiblyExposed(Error::Socket(Failure::Deadline)),
        "stop" => Publication::PossiblyExposed(Error::Clock),
        _ => panic!("unknown fixture case"),
    };
    assert_eq!(outcome, expected);
    assert_eq!(
        cuts,
        usize::from(matches!(case, "wake-cut" | "wake-stop" | "staged" | "stop"))
    );
    if case == "staged" {
        change_image(&file, offset, last);
        assert_eq!(fs::read(&file).unwrap(), image);
    }
    let transcript = fixture.relay.peer_transcript();
    let expected_writes = usize::from(case != "staged");
    assert_eq!(
        fixture.relay.publication_attempts(CARRIER),
        writes + expected_writes
    );
    assert_eq!(transcript.len(), observed + expected_writes);
    if let Some(write) = transcript.get(observed) {
        assert_eq!(write.sender, fixture.offer.flight().recipient);
        assert_eq!(write.recipient, fixture.founder.public_key());
        assert_eq!(write.event, event);
        assert_eq!(write.bytes_len, bytes.len());
        assert_eq!(write.bytes_sha256, <[u8; 32]>::from(Sha256::digest(&bytes)));
    }
    if matches!(case, "staged" | "lost" | "stop" | "wake-stop") {
        assert!(matches!(
            fixture.port.wait_live(deadline()).await,
            Err(Failure::Retired)
        ));
    }
    drop(owner);
    let mut owner = reopen();
    let stored = owner
        .checkpoint
        .read_managed_queued(
            &owner.source,
            fixture.offer.flight().recipient,
            fixture.relay.url.as_str(),
            selector,
            wall(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(stored.fixture_frame(), (carrier, bytes.as_slice()));
    let (attempt, ack) = stored.fixture_attempt();
    assert_eq!(attempt, 1);
    assert_eq!(ack.is_some(), wake);
    let retry = stored.fixture_retry_at().unwrap();
    assert!(retry > wall());
    assert_eq!(stored.is_due(retry), !wake);
    if let Some((delivery, prior, other, unrelated)) = related {
        assert_eq!(
            owner
                .checkpoint
                .fixture_retry_row(&owner.source, other)
                .unwrap(),
            unrelated
        );
        let matching = owner
            .checkpoint
            .fixture_retry_row(&owner.source, delivery)
            .unwrap();
        let state = owner
            .checkpoint
            .fixture_delivery_state(&owner.source)
            .unwrap();
        let ack = i64::try_from(ack.unwrap()).unwrap();
        assert_eq!(state[4..], floors[4..]);
        if case == "positive" {
            let (due, revision, parent) = matching;
            assert!(u64::try_from(ack).unwrap() <= due && due < prior.0);
            let due = i64::try_from(due).unwrap();
            assert_eq!(state[2..4], [due, due]);
            assert!(prior.1 < revision);
            assert_eq!(parent, prior.2);
        } else {
            assert_eq!(matching, prior);
            assert_eq!(state[..2], floors[..2]);
            assert_eq!(state[2..4], [ack, ack]);
            if let Some(wake_wall) = wake_clock {
                assert!(u64::try_from(state[3]).unwrap() < wake_wall);
            }
        }
        if case != "wake-stop" {
            assert_eq!(
                fixture
                    .port
                    .publish_managed_consent(
                        &mut owner,
                        fixture.quota,
                        selector,
                        fixture.readiness.clone(),
                        fixture.stop.clone(),
                    )
                    .await,
                Publication::Unavailable
            );
        }
        assert_eq!(
            owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap(),
            state
        );
        if case == "positive" {
            let (due, _, _) = matching;
            while wall() < due {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let saved = fixture
                .port
                .use_current_recipient(|account, origin, wall| {
                    owner.due_managed_delivery(account, origin, wall)
                })
                .unwrap()
                .unwrap()
                .unwrap();
            let prepared = fixture
                .port
                .use_saved_delivery(&saved, |checked| {
                    owner.observe_managed_saved_delivery(checked)
                })
                .unwrap()
                .unwrap();
            let ManagedSaved::Live(prepared) = prepared else {
                panic!("saved Delivery should be live");
            };
            let proof = fixture
                .port
                .use_delivery_native(prepared.challenge(), |checked| {
                    owner.measure_delivery(
                        fixture.quota,
                        checked,
                        fixture.readiness.clone(),
                        Arc::clone(&fixture.stop),
                    )
                })
                .unwrap()
                .unwrap();
            let outcome = fixture
                .port
                .use_saved_delivery(&saved, |checked| {
                    owner.finish_measured_saved_delivery(fixture.quota, checked, prepared, proof)
                })
                .unwrap()
                .unwrap();
            assert_eq!(
                outcome,
                Some(Status::Accepted(
                    crate::peer_invitation::store::TrustedOutcome::Held
                ))
            );
            assert_eq!(
                owner
                    .checkpoint
                    .fixture_retry_row(&owner.source, other)
                    .unwrap(),
                unrelated
            );
            let held = owner
                .checkpoint
                .fixture_delivery_state(&owner.source)
                .unwrap();
            assert_eq!((held[4], held[5]), (state[4] - 1, state[5] + 1));
        }
    } else {
        let state = owner
            .checkpoint
            .fixture_delivery_state(&owner.source)
            .unwrap();
        assert_eq!(state[..2], floors[..2]);
        let reserved = i64::try_from(retry.checked_sub(60).unwrap()).unwrap();
        assert_eq!(state[2..4], [reserved, reserved]);
        assert_eq!(state[4..], floors[4..]);
    }
    assert_eq!(
        fixture.relay.peer_transcript().len(),
        observed + expected_writes
    );
    assert_eq!(welcome_counts(), native);
    eprintln!(
        "managed_consent_port case={case} elapsed_ms={:.3} native_margin_ms={:.3} relay_frames={expected_writes}",
        elapsed.as_secs_f64() * 1000.0,
        Duration::from_secs(5).saturating_sub(elapsed).as_secs_f64() * 1000.0,
    );
}
