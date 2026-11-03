//! Native v0.2 Marmish. Legacy Python tooling is a separate test fixture.
pub mod cli;
#[cfg(test)]
mod command_config;
#[cfg(test)]
mod command_wire;
pub mod daemon;
pub mod diagnostics;
pub mod error;
pub mod foundation;
pub mod foundation_check;
pub mod identity;
pub mod ipc;
pub mod journal;
pub mod mdk_adapter;
pub mod transport;
pub use error::{Error, Result};
