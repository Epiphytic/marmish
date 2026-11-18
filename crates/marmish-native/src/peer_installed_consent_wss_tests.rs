use std::{
    process::{Command, Stdio},
    thread,
};

use cgka_traits::app_components::NostrRoutingV1;
use nostr::event::EventId;
use sha2::Digest;

use crate::{
    peer_quota_vfs::dispatch::{ProfileSlots, mount_for_proof, resolve_once},
    peer_relay_trust::{ReceiverRouteAdmission, qualified_recipient_fixture},
};
use socket::owner::recipient::{PeerRecipientPort, PublicationOutcome};

use super::*;

const SCENARIO: &str = "MARMISH_INSTALLED_CONSENT_SCENARIO";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Case {
    Positive,
    BeforeFlush,
    Rejected,
    SettleReadOnly,
    LostAck,
    StaleAttempt,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::BeforeFlush => "before_flush",
            Self::Rejected => "negative_ack",
            Self::SettleReadOnly => "settlement_storage",
            Self::LostAck => "lost_ack",
            Self::StaleAttempt => "stale_attempt",
        }
    }
}

const CASES: [Case; 6] = [
    Case::Positive,
    Case::BeforeFlush,
    Case::Rejected,
    Case::SettleReadOnly,
    Case::LostAck,
    Case::StaleAttempt,
];

pub(super) fn time_limit() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

pub(super) fn wall() -> u64 {
    Wall::System.sample().unwrap().as_secs()
}

#[test]
#[ignore = "private WSS with installed SQLCipher and a real 60-second retry"]
fn peer_installed_consent_private_wss() {
    let child_selector = concat!(
        "peer_invitation::prepared_consent::installed_consent::tests::",
        "peer_installed_consent_private_wss_child"
    );
    for case in CASES {
        let started = std::time::Instant::now();
        let limit = if case == Case::LostAck { 130 } else { 65 };
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", child_selector, "--nocapture"])
            .env(SCENARIO, case.name())
            .env_remove("RUST_BACKTRACE")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let exit = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() >= Duration::from_secs(limit) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{} watchdog expired", case.name());
            }
            thread::sleep(Duration::from_millis(30));
        };
        assert!(exit.success(), "{} scenario", case.name());
        eprintln!(
            "installed_consent {} elapsed_ms={}",
            case.name(),
            started.elapsed().as_millis()
        );
    }
}

#[test]
#[ignore = "child of the private WSS suite"]
fn peer_installed_consent_private_wss_child() {
    let Ok(name) = std::env::var(SCENARIO) else {
        return;
    };
    let case = CASES.into_iter().find(|case| case.name() == name).unwrap();
    mount_for_proof().unwrap();
    thread::Builder::new()
        .name("installed-consent".into())
        .stack_size(32 << 20)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(Box::pin(run(case)));
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn run(case: Case) {
    let founder = Keys::generate();
    let second = Keys::generate();
    let (relay, _temp, lease, material) = qualified_recipient_fixture(
        founder.public_key(),
        second.public_key(),
        Duration::from_secs(180),
    )
    .await;
    let account = material.public_key();
    let url = relay.url.as_str();
    let route = Route::configured(url).unwrap();
    let routing = NostrRoutingV1::new([3; 32], vec![url.to_owned()]).unwrap();
    let paths = Paths::new(&lease, &material).unwrap();
    paths.initialize().unwrap();

    let mut fixture = Fixture::open(&lease, &material, &route).unwrap();
    let admission = || ReceiverRouteAdmission::fixture(lease.clone(), &material).unwrap();
    let mut port = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    let connector = relay.connector(lease.root(), true, None);
    let mut sender = Owner::open(
        &connector,
        url,
        founder.public_key(),
        &founder,
        time_limit(),
    )
    .await
    .unwrap();
    let offer = fixture
        .owner
        .state
        .checkpoint
        .custody_codec()
        .prepare_offer(&founder, account, [0x8d; 16], &routing, wall())
        .unwrap();
    let selector = Selector::from(&offer.flight);
    assert!(
        sender
            .publish(&offer, offer.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let live = port.wait_live(time_limit()).await.unwrap().unwrap();
    assert_eq!(
        live.arrival().id,
        offer.flight.flight().carrier_id.to_bytes()
    );
    let state = &mut fixture.owner.state;
    port.use_live_carrier(&live, |carrier| {
        state
            .checkpoint
            .admit_installed_custody_offer(&state.source, carrier)
    })
    .unwrap()
    .unwrap();
    let reservation = state
        .checkpoint
        .reserve_provisioning(&state.source, selector, wall())
        .unwrap();
    let quota = ProfileSlots::reserve(&lease, store::installed::RESERVED_BYTES).unwrap();
    resolve_once(state.prepare_provisional_with_quota(&reservation, |_| Ok(()), Some(&quota)))
        .unwrap()
        .unwrap();

    let transcript = fixture.queue(&reservation, wall()).unwrap();
    let queued = fixture.queued(selector, wall()).unwrap().unwrap();
    assert_eq!(queued.publication_state(), (0, None, None));
    assert!(matches!(
        fixture
            .owner
            .attest(queued.stage(), Duration::from_secs(wall())),
        Err(Error::Binding)
    ));
    let stage = fixture
        .owner
        .state
        .paths
        .reopen_provisional(queued.stage().0, queued.stage().1)
        .unwrap()
        .path()
        .to_owned();
    let digest: [u8; 32] = Sha256::digest(queued.frame()).into();
    let len = queued.frame().len();
    let carrier = EventId::from_byte_array(transcript.carrier);
    let writes = || {
        relay
            .peer_transcript()
            .into_iter()
            .filter(|write| write.event == carrier)
            .map(|write| {
                (
                    write.sender,
                    write.recipient,
                    write.bytes_sha256,
                    write.bytes_len,
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = (account, founder.public_key(), digest, len);

    // The actual port refuses a stale turn before recovering or reserving.
    assert_eq!(
        port.publish_prepared_consent(&mut fixture, selector, Instant::now())
            .await,
        PublicationOutcome::NotSent(Failure::Deadline.into())
    );
    assert!(matches!(
        fixture.reserve(second.public_key(), url, selector, wall()),
        Err(Error::Checkpoint(consent_checkpoint::Error::Source))
    ));
    assert!(matches!(
        fixture.reserve(account, "wss://wrong.invalid/", selector, wall()),
        Err(Error::Binding)
    ));
    assert_eq!(
        fixture
            .queued(selector, wall())
            .unwrap()
            .unwrap()
            .publication_state(),
        (0, None, None)
    );
    assert!(writes().is_empty());

    if case == Case::StaleAttempt {
        let reserved = fixture
            .reserve(account, url, selector, wall())
            .unwrap()
            .unwrap();
        assert_eq!(reserved.attempt.sequence(), 1);
        assert_eq!(
            fixture.check(&reserved, reserved.attempt.deadline()),
            Err(consent_checkpoint::Error::Expired.into())
        );
        drop(fixture);
        let mut reopened = Fixture::open(&lease, &material, &route).unwrap();
        assert_eq!(
            reopened.check(&reserved, wall()),
            Err(consent_checkpoint::Error::LocalOwner.into())
        );
        let queued = reopened.queued(selector, wall()).unwrap().unwrap();
        assert!(matches!(queued.publication_state(), (1, Some(_), None)));
        assert!(writes().is_empty());
        return;
    }

    match case {
        Case::BeforeFlush => fixture.cut = Some(Cut::ReplaceStagedImage),
        Case::Rejected => relay.recipient_reject_ack(carrier),
        Case::SettleReadOnly => fixture.cut = Some(Cut::ReadOnlySettlement),
        Case::LostAck => relay.recipient_lose_ack(carrier),
        Case::Positive | Case::StaleAttempt => {}
    }
    let first = port
        .publish_prepared_consent(&mut fixture, selector, time_limit())
        .await;
    match case {
        Case::Positive => assert_eq!(first, PublicationOutcome::Settled),
        Case::Rejected => assert_eq!(first, PublicationOutcome::Rejected),
        Case::BeforeFlush => {
            assert_eq!(
                first,
                PublicationOutcome::NotSent(Error::StageIdentityMismatch)
            );
            assert!(fixture.cut.is_none());
            assert!(
                writes().is_empty(),
                "a staged unflushed frame reached the relay"
            );
            fs::rename(stage.with_extension("saved"), &stage).unwrap();
        }
        Case::SettleReadOnly => assert_eq!(
            first,
            PublicationOutcome::PossiblyExposed(
                consent_checkpoint::Error::Cursor(store::Error::Storage).into()
            )
        ),
        Case::StaleAttempt => unreachable!(),
        Case::LostAck => assert!(matches!(
            first,
            PublicationOutcome::PossiblyExposed(Error::Socket(Failure::Deadline))
        )),
    }
    if matches!(
        case,
        Case::BeforeFlush | Case::SettleReadOnly | Case::LostAck
    ) {
        assert!(matches!(
            port.wait_live(time_limit()).await,
            Err(Failure::Retired)
        ));
    }
    if case != Case::BeforeFlush {
        assert_eq!(writes(), [expected]);
    }
    drop(port);
    drop(fixture);

    // A new fixture binds a new Source and rereads the capped ciphertext image.
    let mut fixture = Fixture::open(&lease, &material, &route).unwrap();
    let queued = fixture.queued(selector, wall()).unwrap().unwrap();
    let (sequence, retry_at, ack_at) = queued.publication_state();
    let retry_at = retry_at.unwrap();
    assert_eq!(sequence, 1);
    assert_eq!(ack_at.is_some(), case == Case::Positive);
    assert_eq!(queued.frame().len(), len);
    assert_eq!(<[u8; 32]>::from(Sha256::digest(queued.frame())), digest);

    if case == Case::LostAck {
        let mut port = PeerRecipientPort::open(admission(), time_limit())
            .await
            .unwrap();
        assert!(wall() < retry_at);
        assert_eq!(
            port.publish_prepared_consent(&mut fixture, selector, time_limit())
                .await,
            PublicationOutcome::Unavailable
        );
        assert_eq!(writes(), [expected]);
        assert_eq!(
            fixture
                .queued(selector, wall())
                .unwrap()
                .unwrap()
                .publication_state(),
            (1, Some(retry_at), None)
        );

        while wall() < retry_at {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        assert_eq!(
            port.publish_prepared_consent(&mut fixture, selector, time_limit())
                .await,
            PublicationOutcome::Settled
        );
        assert_eq!(writes(), [expected, expected]);
        assert_eq!(
            port.publish_prepared_consent(&mut fixture, selector, time_limit())
                .await,
            PublicationOutcome::Unavailable
        );
        assert_eq!(writes(), [expected, expected]);
        drop(fixture);
        let mut fixture = Fixture::open(&lease, &material, &route).unwrap();
        let settled = fixture.queued(selector, wall()).unwrap().unwrap();
        let (sequence, next_retry, ack_at) = settled.publication_state();
        assert_eq!(sequence, 2);
        assert!(next_retry.unwrap() >= retry_at + 60);
        assert!(ack_at.unwrap() >= retry_at);
    }
}
