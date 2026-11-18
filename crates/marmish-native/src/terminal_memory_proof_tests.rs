//! Opt-in in-vitro experiment against the pinned native MDK.
use super::terminal_room_fixture::{ENDPOINT, prove};
use std::time::Duration;

#[tokio::test]
#[ignore = "opt-in offline proof with the production SQLCipher memory-security setting"]
async fn unpublished_pending_package_joins_and_carries_terminal_frames() {
    tokio::time::timeout(Duration::from_secs(20), async {
        prove(0, ENDPOINT).await;
        prove(1, ENDPOINT).await;
    })
    .await
    .expect("bounded offline experiment");
}

#[tokio::test]
#[ignore = "opt-in actual MDK round trip for maximum Terminal v1 records"]
async fn maximum_terminal_records_round_trip_through_memory_rooms() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut room = prove(0, ENDPOINT).await;
        for (sender, name) in [(0, "output"), (1, "input")] {
            let started = std::time::Instant::now();
            let frame = room.prepare_maximum_frame(sender).await;
            room.accept_maximum_frame(&frame, None).await;
            println!(
                "terminal_frame_memory direction={name} payload_bytes=4096 app_bytes={} protected_content_bytes={} outer_bytes={} seal_and_open_us={}",
                frame.app_bytes,
                frame.protected_bytes,
                frame.outer_bytes,
                started.elapsed().as_micros()
            );
        }
        room.assert_no_package_publication();
    })
    .await
    .expect("bounded offline experiment");
}
