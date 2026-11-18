//! Serialized ownership of a raw private Peer WSS lane.

#[path = "peer_raw_socket_owner.rs"]
pub(super) mod owner;
use super::store::consent_checkpoint::wire as peer_control_wire;
#[path = "peer_raw_socket_wire.rs"]
pub(super) mod wire;

#[cfg(test)]
#[path = "peer_raw_socket_fixture.rs"]
pub(super) mod fixture;
#[cfg(test)]
#[path = "peer_raw_socket_tests.rs"]
mod tests;
