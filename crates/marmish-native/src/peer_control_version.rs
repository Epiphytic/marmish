//! Version and receipt coordinates common to offline Peer control formats.

use nostr::key::PublicKey;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlVersion {
    V1 = 1,
    V2 = 2,
}

/// An equality check over enrollment coordinates grants no MLS authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReceiptCoordinates {
    pub(crate) founder: PublicKey,
    pub(crate) recipient: PublicKey,
    pub(crate) room_id: [u8; 16],
    pub(crate) offer_id: [u8; 16],
    pub(crate) offer_revision: [u8; 32],
    pub(crate) welcome_id: [u8; 32],
}
