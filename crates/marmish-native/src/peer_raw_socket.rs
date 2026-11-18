//! Test-only ownership of a raw private Peer WSS lane.

#[path = "peer_raw_socket_owner.rs"]
mod owner;
#[path = "peer_control_wire.rs"]
mod peer_control_wire;
#[path = "peer_raw_socket_wire.rs"]
mod wire;

#[path = "peer_raw_socket_fixture.rs"]
mod fixture;
#[path = "peer_raw_socket_tests.rs"]
mod tests;
