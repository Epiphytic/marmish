use super::*;
use crate::peer_invitation::{
    prepared_consent::{CustodyProgress, JoinCut, terminal_retirement::Cut},
    store::{
        TrustedOutcome,
        consent_checkpoint::{
            Mode,
            provisioning::prepared_consent::custody::{
                Approval, CustodyState, HeldEvidence, Replay,
                terminal::{TerminalReason, TerminalRecord, TerminalStep},
            },
        },
    },
};
use consent_checkpoint::wire::Offer;
use std::{path::Path, sync::atomic::Ordering};

#[test]
#[ignore = "explicit bounded loopback TLS and SQLCipher qualification"]
fn peer_private_wss_pending_join_custody_manual_and_automatic() {
    run_custody_fixture(Scenario::Standard);
}

#[test]
#[ignore = "explicit bounded loopback TLS and SQLCipher qualification"]
fn peer_private_wss_pending_join_custody_stopped_before_ingest() {
    run_custody_fixture(Scenario::BeforeIngest);
}

#[test]
#[ignore = "explicit bounded loopback TLS and SQLCipher qualification"]
fn peer_private_wss_pending_retirement_recovers_each_filesystem_cut() {
    run_custody_fixture(Scenario::Retirement);
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Scenario {
    Standard,
    BeforeIngest,
    Retirement,
}

fn run_custody_fixture(scenario: Scenario) {
    // The unoptimized native MLS futures need more than libtest's stack.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(180), async {
                        enroll(Some(Mode::Manual), scenario).await;
                        if scenario == Scenario::Standard {
                            enroll(Some(Mode::Automatic), scenario).await;
                        }
                    })
                    .await
                    .expect("bounded private native join and restart");
                });
        })
        .unwrap()
        .join()
        .unwrap();
}

pub(super) struct Round<'a> {
    pub(super) mode: Mode,
    pub(super) scenario: Scenario,
    pub(super) wall: u64,
    pub(super) fixed_wall: &'a Arc<AtomicU64>,
    pub(super) stage_path: &'a Path,
    pub(super) stage_image: &'a dyn Fn() -> [u8; 32],
    pub(super) original_image: [u8; 32],
    pub(super) selector: Selector,
    pub(super) generation: u64,
    pub(super) codec: &'a Codec,
    pub(super) signer: &'a Keys,
    pub(super) founder: &'a Arc<Keys>,
    pub(super) founder_path: &'a Path,
    pub(super) offer: &'a Offer,
    pub(super) consent_frame: &'a [u8],
    pub(super) lease: &'a ProfileLease,
    pub(super) material: &'a IdentityMaterial,
    pub(super) route: &'a Route,
}

impl Round<'_> {
    fn open_as(&self, mode: Mode) -> PreparedConsentOwner<'_> {
        self.open_policy(Policy {
            mode,
            ..Policy::default()
        })
    }

    fn open_policy(&self, policy: Policy) -> PreparedConsentOwner<'_> {
        PreparedConsentOwner::open_custody(
            self.lease,
            self.material,
            Role::Client,
            policy,
            self.route,
            Wall::Fixture(self.fixed_wall.clone()),
        )
        .unwrap()
    }

    fn assert_state(
        &self,
        owner: &mut PreparedConsentOwner<'_>,
        state: CustodyState,
    ) -> HeldEvidence {
        let row = owner
            .state
            .checkpoint
            .read_custody(
                &owner.state.source,
                self.selector,
                self.generation,
                self.fixed_wall.load(Ordering::Relaxed),
            )
            .unwrap()
            .expect("qualified durable custody");
        assert_eq!(row.state(), state);
        row
    }

    pub(super) async fn exchange(self, client: &mut Owner, server: &mut Owner) {
        let manual = matches!(self.mode, Mode::Manual);
        let mut local = self.open_as(self.mode);
        let consent = self
            .codec
            .open_consent(self.founder, self.consent_frame, self.offer, self.wall)
            .unwrap();
        let welcome = peer_enrollment_welcome(
            self.founder_path,
            self.founder.clone(),
            self.signer.public_key(),
            consent.offer().routing(),
            consent.offer().room_id(),
            consent.key_package().clone(),
        )
        .await;
        // The signed expiry stays valid while the custody deadline (five
        // minutes earlier) can be reached within the qualified clock skew.
        let expires = self.wall + 360;
        let delivery = self
            .codec
            .prepare_delivery(self.founder, &consent, &welcome, self.wall, expires)
            .unwrap();
        let publish_consent = |result| move || result;
        if !manual {
            assert!(
                local
                    .publish_consent_wss(client, self.selector, deadline(), publish_consent(Ok(())))
                    .await
                    .unwrap()
            );
        }
        assert!(
            server
                .publish(&delivery, delivery.flight.flight(), deadline())
                .await
                .unwrap()
        );
        let live = client.wait_live(deadline()).await.unwrap().unwrap();
        if manual {
            assert_eq!(status(&mut local, client, &live), Status::Deferred);
            assert_eq!(local.retry_custody_due().unwrap(), None);
            assert_eq!(
                local
                    .publish_consent_wss(
                        client,
                        self.selector,
                        deadline(),
                        publish_consent(Err(Error::Clock)),
                    )
                    .await,
                Err(Error::Clock)
            );
            drop(local);
            local = self.open_as(self.mode);
            assert_eq!(local.retry_custody_due().unwrap(), None);
            self.fixed_wall.store(self.wall + 2, Ordering::Relaxed);
            assert_eq!(
                local.retry_custody_due().unwrap(),
                Some(Status::Accepted(TrustedOutcome::Held))
            );
        } else {
            assert_eq!(
                status(&mut local, client, &live),
                Status::Accepted(TrustedOutcome::Held)
            );
        }
        let accepted = server.wait_live(deadline()).await.unwrap().unwrap();
        let observed = self
            .codec
            .open_consent(
                self.founder,
                accepted.arrival().body.get().as_bytes(),
                self.offer,
                self.wall,
            )
            .unwrap();
        assert_eq!(observed.flight().carrier_id, consent.flight().carrier_id);
        assert_eq!((self.stage_image)(), self.original_image);

        let repeat = |owner: &mut PreparedConsentOwner<'_>, client: &Owner| {
            let Received::Admitted(disposition) = owner.receive_wss(client, &live).unwrap() else {
                panic!("expected durable replay disposition");
            };
            assert_eq!(
                disposition.status,
                Status::Accepted(TrustedOutcome::AlreadyApplied)
            );
        };
        self.assert_state(&mut local, CustodyState::Held);
        repeat(&mut local, client);
        let conflict = self
            .codec
            .prepare_delivery(self.founder, &consent, &welcome, self.wall, expires - 1)
            .unwrap();
        assert!(
            server
                .publish(&conflict, conflict.flight.flight(), deadline())
                .await
                .unwrap()
        );
        let competing = client.wait_live(deadline()).await.unwrap().unwrap();
        assert_eq!(status(&mut local, client, &competing), Status::Invalid);

        if self.scenario == Scenario::BeforeIngest {
            assert_eq!(
                local
                    .advance_custody(self.selector, self.generation, |cut| {
                        assert_eq!(cut, JoinCut::Started);
                        Err(Error::Clock)
                    })
                    .await,
                Err(Error::Clock)
            );
            drop(local);
            let mut local = self.open_as(self.mode);
            self.assert_state(&mut local, CustodyState::JoinStarted);
            repeat(&mut local, client);
            assert!(matches!(
                local
                    .advance_custody(self.selector, self.generation, |_| unreachable!())
                    .await,
                Ok(CustodyProgress::Unavailable(_))
            ));
            let stopped = self.assert_state(&mut local, CustodyState::JoinStarted);
            self.fixed_wall
                .store(stopped.held_deadline(), Ordering::Relaxed);
            assert_eq!(
                local
                    .advance_custody(self.selector, self.generation, |_| unreachable!())
                    .await,
                Err(Error::Checkpoint(consent_checkpoint::Error::Expired))
            );
            assert_eq!((self.stage_image)(), self.original_image);
            return;
        }

        assert_eq!(
            local
                .advance_custody(self.selector, self.generation + 1, |_| unreachable!())
                .await,
            Ok(CustodyProgress::Missing)
        );
        let started = Instant::now();
        assert_eq!(
            local
                .advance_custody(self.selector, self.generation, |cut| match cut {
                    JoinCut::Started => Ok(()),
                    JoinCut::OriginalClosed => Err(Error::Clock),
                    _ => panic!("unexpected cut {cut:?}"),
                })
                .await,
            Err(Error::Clock)
        );
        let join_elapsed = started.elapsed();
        self.assert_state(&mut local, CustodyState::JoinStarted);
        repeat(&mut local, client);
        assert_eq!(status(&mut local, client, &competing), Status::Invalid);
        assert_ne!((self.stage_image)(), self.original_image);
        let joined_image = (self.stage_image)();
        if manual {
            let mut sidecar = self.stage_path.as_os_str().to_owned();
            sidecar.push("-journal");
            let marker = b"simulated hot journal marker";
            std::fs::write(&sidecar, marker).unwrap();
            repeat(&mut local, client);
            assert_eq!(
                local
                    .publish_consent_wss(client, self.selector, deadline(), || unreachable!())
                    .await,
                Ok(false)
            );
            assert!(matches!(
                local
                    .advance_custody(self.selector, self.generation, |_| unreachable!())
                    .await,
                Ok(CustodyProgress::Unavailable(_))
            ));
            self.assert_state(&mut local, CustodyState::JoinStarted);
            assert!(std::fs::read(&sidecar).unwrap() == marker);
            std::fs::remove_file(sidecar).unwrap();
            assert_eq!((self.stage_image)(), joined_image);
        }
        drop(local);

        let started = Instant::now();
        let mut local = self.open_as(self.mode);
        assert_eq!(
            local
                .advance_custody(self.selector, self.generation, |cut| match cut {
                    JoinCut::ReadbackVerified => Ok(()),
                    JoinCut::PendingCommitted => Err(Error::Clock),
                    _ => panic!("a cold JoinStarted must not ingest again: {cut:?}"),
                })
                .await,
            Err(Error::Clock)
        );
        let readback_elapsed = started.elapsed();
        let pending = self.assert_state(&mut local, CustodyState::Pending);
        repeat(&mut local, client);
        drop(local);
        if self.scenario == Scenario::Retirement {
            self.recover_terminal_files(pending);
            return;
        }

        let (connection, _other) = UnixStream::pair().unwrap();
        let mut blocked = self.open_policy(Policy {
            mode: Mode::Automatic,
            blocked: [self.selector.server].into(),
            ..Policy::default()
        });
        assert_eq!(
            blocked.activate_custody(
                self.selector,
                self.generation,
                Some(&connection),
                |_| unreachable!()
            ),
            Err(Error::Checkpoint(consent_checkpoint::Error::Policy))
        );
        drop(blocked);

        let mut local = self.open_as(Mode::Automatic);
        let pending = self.assert_state(&mut local, CustodyState::Pending);
        let pending_deadline = pending.pending_deadline().unwrap();
        self.fixed_wall.store(pending_deadline, Ordering::Relaxed);
        assert_eq!(
            local.activate_custody(
                self.selector,
                self.generation,
                Some(&connection),
                |_| unreachable!()
            ),
            Err(Error::Checkpoint(consent_checkpoint::Error::Expired))
        );
        if !manual {
            let joined = local
                .inspect_joined(pending.stage(), &delivery.flight, pending.package_digest())
                .unwrap();
            assert_eq!(
                local.state.checkpoint.commit_custody_activating(
                    &local.state.source,
                    &pending,
                    &joined,
                    Approval::Automatic,
                    pending_deadline,
                ),
                Err(consent_checkpoint::Error::Expired)
            );
        }
        self.fixed_wall
            .store(self.wall + if manual { 2 } else { 0 }, Ordering::Relaxed);
        assert_eq!(
            local
                .advance_custody(self.selector, self.generation, |cut| {
                    assert_eq!(cut, JoinCut::ReadbackVerified);
                    Ok(())
                })
                .await,
            Ok(CustodyProgress::Pending)
        );
        assert_eq!(
            local.activate_custody(
                self.selector,
                self.generation + 1,
                Some(&connection),
                |_| unreachable!()
            ),
            Ok(CustodyProgress::Missing)
        );
        if manual {
            assert_eq!(
                local.activate_custody(self.selector, self.generation, None, |_| unreachable!()),
                Err(Error::Checkpoint(consent_checkpoint::Error::Policy))
            );
            self.assert_state(&mut local, CustodyState::Pending);
        }
        let started = Instant::now();
        assert_eq!(
            local.activate_custody(
                self.selector,
                self.generation,
                manual.then_some(&connection),
                |cut| {
                    assert_eq!(cut, JoinCut::ActivatingCommitted);
                    Err(Error::Clock)
                },
            ),
            Err(Error::Clock)
        );
        let decision_elapsed = started.elapsed();
        self.assert_state(&mut local, CustodyState::Activating);
        repeat(&mut local, client);
        drop(local);

        let mut local = self.open_as(Mode::Automatic);
        assert_eq!(
            local
                .advance_custody(self.selector, self.generation, |cut| {
                    assert_eq!(cut, JoinCut::ReadbackVerified);
                    Ok(())
                })
                .await,
            Ok(CustodyProgress::Activating)
        );
        assert!(matches!(
            local.recover_wss(client, deadline()).await.unwrap(),
            History::Committed
        ));
        self.fixed_wall.store(pending_deadline, Ordering::Relaxed);
        assert_eq!(
            local
                .advance_custody(self.selector, self.generation, |_| unreachable!())
                .await,
            Err(Error::Checkpoint(consent_checkpoint::Error::Expired))
        );

        // The durable cursor can be tested at logical expiry without moving
        // the live owner's qualified wall outside its system-clock tolerance.
        let raw = record(live.arrival());
        let checkpoint = &mut local.state.checkpoint;
        let source = &local.state.source;
        assert!(matches!(
            checkpoint.custody_replay(source, raw, expires).unwrap(),
            Replay::AlreadyApplied(_)
        ));
        assert!(matches!(
            checkpoint
                .custody_replay(source, raw, delivery.flight.flight().replay_safe_until)
                .unwrap(),
            Replay::Invalid(_)
        ));
        self.fixed_wall.store(
            delivery.flight.flight().replay_safe_until,
            Ordering::Relaxed,
        );
        assert_eq!(
            local
                .advance_custody(self.selector, self.generation, |_| unreachable!())
                .await,
            Err(Error::Clock)
        );
        assert_eq!((self.stage_image)(), joined_image);
        let mode = if manual { "manual" } else { "automatic" };
        eprintln!(
            "custody={mode} join_and_close_ms={} restart_and_pending_ms={} verified_decision_ms={}",
            join_elapsed.as_millis(),
            readback_elapsed.as_millis(),
            decision_elapsed.as_millis()
        );
    }

    fn terminal_record(&self, owner: &mut PreparedConsentOwner<'_>) -> TerminalRecord {
        let wall = owner.sample().unwrap().as_secs();
        owner
            .state
            .checkpoint
            .terminal_lookup(&owner.state.source, self.selector, self.generation, wall)
            .unwrap()
            .expect("exact durable terminal decision")
    }

    fn recover_terminal_files(&self, pending: HeldEvidence) {
        let wall = pending.pending_deadline().unwrap();
        // The fixture chooses a short held window so this reaches its deadline
        // without exceeding the local owner's allowed clock skew.
        assert_eq!(wall, pending.held_deadline());
        self.fixed_wall.store(wall, Ordering::Relaxed);
        let mut local = self.open_as(Mode::Manual);
        let decision = local
            .state
            .checkpoint
            .expire_pending(&local.state.source, self.selector, self.generation, wall)
            .unwrap();
        assert_eq!(decision.reason(), TerminalReason::Expired);
        assert_eq!(decision.decided_at(), wall);
        assert!(matches!(decision.step(), TerminalStep::Fenced));
        let joined_digest = (self.stage_image)();
        drop(local);

        let open_closed = || {
            let owner = self.open_policy(Policy {
                mode: Mode::Off,
                blocked: [self.selector.server].into(),
                ..Policy::default()
            });
            assert!(ProfileLease::acquire(self.lease.root().parent().unwrap(), "client").is_err());
            owner
        };
        let mut local = open_closed();
        let record = self.terminal_record(&mut local);
        self.fixed_wall.store(wall - 1, Ordering::Relaxed);
        assert!(matches!(
            local.retire_terminal_at_cut(record, |_| unreachable!()),
            Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
                store::Error::ClockRegressed
            )))
        ));
        assert_eq!((self.stage_image)(), joined_digest);
        self.fixed_wall.store(wall, Ordering::Relaxed);
        drop(local);

        for (index, cut) in [Cut::Intent, Cut::Unlinked, Cut::Synced, Cut::FilesRemoved]
            .into_iter()
            .enumerate()
        {
            let mut local = open_closed();
            let record = self.terminal_record(&mut local);
            assert!(matches!(
                (index, record.step()),
                (0, TerminalStep::Fenced) | (1..=3, TerminalStep::Intent(_))
            ));
            assert!(matches!(
                local.retire_terminal_at_cut(record, |actual| {
                    if cut == actual {
                        Err(Error::Clock)
                    } else {
                        Ok(())
                    }
                }),
                Err(Error::Clock)
            ));
            assert_eq!(self.stage_path.try_exists().unwrap(), cut == Cut::Intent);
            let record = self.terminal_record(&mut local);
            assert_eq!(record.reason(), TerminalReason::Expired);
            assert!(record.stage().is_some());
            assert!(matches!(
                (cut, record.step()),
                (Cut::FilesRemoved, TerminalStep::FilesRemoved(_))
                    | (
                        Cut::Intent | Cut::Unlinked | Cut::Synced,
                        TerminalStep::Intent(_)
                    )
            ));
        }

        let mut local = open_closed();
        let record = self.terminal_record(&mut local);
        // A sidecar arriving after FilesRemoved cannot make release eligible.
        let mut sidecar = self.stage_path.as_os_str().to_owned();
        sidecar.push("-wal");
        std::fs::write(&sidecar, b"fixture late sidecar").unwrap();
        assert!(matches!(
            local.retire_terminal(record),
            Err(Error::StageIdentityMismatch)
        ));
        assert_eq!(std::fs::read(&sidecar).unwrap(), b"fixture late sidecar");
        std::fs::remove_file(sidecar).unwrap();
        let record = self.terminal_record(&mut local);
        assert!(matches!(record.step(), TerminalStep::FilesRemoved(_)));
        let released = local.retire_terminal(record).unwrap();
        assert!(matches!(released.step(), TerminalStep::Released));
        assert!(released.stage().is_none());
        drop(local);

        let mut local = open_closed();
        let record = self.terminal_record(&mut local);
        assert_eq!(record.reason(), TerminalReason::Expired);
        assert!(matches!(record.step(), TerminalStep::Released));
        assert!(
            local
                .state
                .checkpoint
                .terminal_cleanup_due(&local.state.source, wall)
                .unwrap()
                .is_none()
        );
        assert!(!self.stage_path.try_exists().unwrap());
    }
}

fn status(
    owner: &mut PreparedConsentOwner<'_>,
    client: &Owner,
    incoming: &crate::peer_invitation::socket::owner::LiveRecord,
) -> Status {
    let Received::Admitted(disposition) = owner.receive_wss(client, incoming).unwrap() else {
        panic!("expected durable custody cursor disposition");
    };
    disposition.status
}
