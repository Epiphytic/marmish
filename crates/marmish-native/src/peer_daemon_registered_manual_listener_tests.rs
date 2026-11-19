//! Check the actual recipient/listener pairing and independent Reader custody.
use super::*;
use crate::{
    mdk_adapter::terminal_relay_fixture::SocketEnd,
    peer_invitation::daemon_wss_tests::{
        WAIT, private,
        registered_qualification::passive_recipient::{eventually, recipient_live},
        run_native,
    },
    peer_relay_trust::qualified_manual_recipient_fixture,
};

const LIMIT: Duration = Duration::from_secs(420);

#[test]
fn recipient_rejects_a_foreign_fatal_observer() {
    let (trip, fatal) = crate::dispatcher_fatal::Trip::new();
    let (_, foreign) = crate::dispatcher_fatal::Trip::new();
    assert!(check_recipient_fatal(&fatal, &trip.observe()).is_ok());
    assert!(matches!(
        check_recipient_fatal(&fatal, &foreign),
        Err(crate::Error::Identity("candidate_startup_custody"))
    ));
}

async fn check_listener_reader(intent: QualificationIntent) {
    use crate::peer_relay_trust::candidate_registration::ReceiverPreflight;
    use nostr::key::Keys;

    let root = private();
    let (mut relay, _temp, lease, material) = qualified_manual_recipient_fixture(
        Keys::generate().public_key(),
        Keys::generate().public_key(),
        root.path(),
        None,
    )
    .await;
    let signer = material.public_key();
    let config = CommandConfig::load(lease.root()).unwrap();
    let mut receiver = ReceiverPreflight::prepare(&lease, &material, &config)
        .unwrap()
        .unwrap()
        .register(&lease, &material)
        .unwrap();
    let (_fence, _, _custody) = receiver.take_command_fence().unwrap().into_parts();
    let fatal = receiver.forwarding_verifier().fatal();
    let mut recipient = Recipient::default();
    let registration = recipient
        .start(&lease, (intent, material), receiver, &fatal)
        .unwrap();
    let reader = registration.view_reader_for_test();
    assert_eq!(
        reader.is_some(),
        intent == QualificationIntent::ManualRecipient
    );
    let mut listener = Listener::default();
    let (_requests, _status) = listener.start(&lease, registration).unwrap();
    recipient.startup.as_ref().unwrap().ordinary_ready();
    let physical = recipient_live(&relay, signer, 1).await;
    if let Some(reader) = &reader {
        assert!(reader.admit(NonZeroU64::MIN).is_some());
    }
    recipient.actor.as_ref().unwrap().stop();
    recipient.wait_until(Instant::now() + WAIT).await;
    assert!(matches!(
        recipient.completion,
        Some(Completion {
            exit: Exit::Stopped,
            physical: PhysicalEnd::Drained
        })
    ));
    assert!(
        listener
            .task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    );
    if let Some(reader) = &reader {
        assert!(reader.admit(NonZeroU64::MIN).is_none());
    }
    eventually("exact reader-test socket closed", || {
        relay.runtime_socket_end(physical) == Some(SocketEnd::Raw)
    })
    .await;
    listener.start_drain();
    listener.wait_until(Instant::now() + WAIT).await;
    assert!(matches!(listener.outcome, Some(Ok(Ok(())))));
    lease.cleanup_runtime().unwrap();
    relay.stop().await;
}

#[test]
#[ignore = "isolated native registration and private WSS"]
fn manual_listener_keeps_the_read_half_of_its_actor_view() {
    run_native(LIMIT, || {
        Box::pin(check_listener_reader(QualificationIntent::ManualRecipient))
    });
}

#[test]
#[ignore = "isolated native registration and private WSS"]
fn passive_listener_has_no_actor_view() {
    run_native(LIMIT, || {
        Box::pin(check_listener_reader(QualificationIntent::PassiveRecipient))
    });
}
