//! Offline qualification of the Peer control codec before product integration.
#[allow(dead_code, unused_imports)] // Custody callers live in the checkpoint fixture.
#[path = "../src/peer_control_wire.rs"]
mod peer_control_wire;

// Standalone wire tests have no checkpoint and cannot issue Held evidence.
mod peer_invitation {
    use crate::peer_control_wire::{Offer, PackageBinding, Transcript};

    pub(crate) enum HeldEvidence {}

    macro_rules! unavailable {
        ($($name:ident: $result:ty),* $(,)?) => {
            $(
                pub(crate) fn $name(&self) -> $result {
                    match *self {}
                }
            )*
        };
    }

    impl HeldEvidence {
        unavailable!(
            offer: &Offer,
            transcript: Transcript,
            signed_consent: &[u8],
            signed_delivery: &[u8],
            delivery_carrier: [u8; 32],
            outer_stamp: u64,
            flight_created_at: u64,
            delivery_expires: u64,
            replay_until: u64,
            package: PackageBinding,
            package_bounds: (u64, u64),
            held_deadline: u64,
            held_at: u64,
            welcome_id: [u8; 32],
            welcome_digest: [u8; 32],
        );
    }
}
