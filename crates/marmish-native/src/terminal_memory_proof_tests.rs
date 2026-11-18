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
