//! Per-retained-row observation and the real engine's same-fence ACK.
use super::*;
use crate::{
    command_engine::StopOutcome,
    dispatcher_fatal::Trip,
    mdk_adapter::{retained_candidate_rows, terminal_relay_fixture::Relay},
};
use std::os::unix::fs::PermissionsExt;

fn observations(owner: &MdkOwner, invocations: &[[u8; 16]; 2]) -> [u64; 2] {
    let page = owner.journal.command_evidence(None, 2).unwrap();
    assert_eq!(page.rows.len(), 2);
    invocations.map(|invocation| {
        page.rows
            .iter()
            .find(|row| row.invocation == hex::encode(invocation))
            .expect("exact retained invocation")
            .observed_seq
    })
}

async fn prove() {
    let root = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let mut relay = Relay::start().await;
    let trip = Arc::new(Trip::new().0);
    let admission = Admission::new(&trip, || true);
    let (peer, mut owner, invocations) =
        retained_candidate_rows(root.path(), &relay, admission.clone()).await;

    let idle = Instant::now() + Duration::from_secs(15);
    loop {
        let counts = owner.journal.command_cleanup_counts().unwrap();
        if counts.active == 0
            && counts.uncertain == 0
            && counts.worker_running == 0
            && counts.worker_unknown == 0
            && !counts.observation_pending
            && !owner
                .commands
                .as_ref()
                .unwrap()
                .cleanup_observation_pending()
        {
            break;
        }
        assert!(Instant::now() < idle, "bounded ordinary idle convergence");
        owner
            .tick((Instant::now() + Duration::from_millis(25)).min(idle))
            .await
            .unwrap();
    }
    let before = observations(&owner, &invocations);
    owner.commands.as_mut().unwrap().close_admission();
    let fence = IdleFence {
        admission,
        end: Clock::now() + BUDGET,
    };
    let Progress::Started(sequence) = trusted_turn(&mut owner, &Step::Begin, &fence).unwrap()
    else {
        panic!("one allocation of the final observation fence");
    };
    loop {
        match trusted_turn(&mut owner, &Step::Poll(sequence), &fence).unwrap() {
            Progress::Polled {
                state: CleanupState::Draining,
                fence: Some(actual),
                ..
            } if actual == sequence => tokio::time::sleep(WAIT).await,
            Progress::Polled {
                state: CleanupState::Quiesced,
                fence: Some(actual),
                ..
            } if actual == sequence => break,
            _ => panic!("same-fence real Command quiescence"),
        }
    }
    for (prior, after) in before.into_iter().zip(observations(&owner, &invocations)) {
        assert!(after >= sequence && after > prior);
    }
    let service = owner.commands.as_mut().unwrap();
    assert!(!service.cleanup_observation_pending());
    let mut engine = service.take_engine_for_stop().unwrap();
    assert!(!engine.observation_pending());
    let (result, joined) = tokio::task::spawn_blocking(move || {
        let result = engine.stop_outcome(Clock::now() + Duration::from_secs(3));
        (result, engine.worker_joined())
    })
    .await
    .unwrap();
    let result = result.unwrap();
    assert!(joined, "real engine worker was joined");
    assert!(matches!(result, StopOutcome::Ordinary(report) if report.complete && report.quiesced));
    peer.stop_fixture_server(owner).await;
    relay.stop().await;
    println!("retained FinalObserve rows=2 worker_joined=true");
}

#[test]
#[ignore = "uses a private WSS relay and real Command worker"]
fn retained_rows_advance_at_exact_idle_final_observation() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async { tokio::time::timeout(Duration::from_secs(90), prove()).await })
                .expect("bounded private retained Command proof");
        })
        .unwrap()
        .join()
        .unwrap();
}
