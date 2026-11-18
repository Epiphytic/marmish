//! Qualifies the local sampler before it is mounted by the daemon.
use marmish_native::{Error, Result};

#[allow(dead_code)]
#[path = "../src/command_limits.rs"]
mod command_limits;
#[path = "../src/peer_heartbeat_sampler.rs"]
mod peer_heartbeat_sampler;
#[path = "../src/peer_status_wire.rs"]
mod peer_status_wire;
