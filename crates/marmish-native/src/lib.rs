//! Native v0.2 Marmish. Legacy Python tooling is a separate test fixture.
pub mod cli;
mod command_client;
mod command_cli;
mod command_config;
mod command_engine;
mod command_evidence;
mod command_identity;
mod command_lifecycle;
mod command_limits;
mod command_service;
mod command_target;
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
pub(crate) use mdk_adapter::command_local;

mod native_manifest;
mod native_setup;
mod native_preflight;
mod native_install;

mod command_srv;
mod native_layout;

/// Static release provenance, without opening identity or execution state.
pub fn release_info() -> Result<String> {
    serde_json::to_string(&native_manifest::compiled_release_metadata())
        .map_err(|_|Error::Invalid("release_metadata"))
}

/// Validate this executable's immutable installed lifetime before use.
pub fn validate_executable_layout() -> Result<()> {
    native_layout::resolve(&std::env::current_exe()?).map(|_|())
}

mod native_release;
/// Build-only packaging entrypoint; not part of installed runtime launchers.
pub fn native_release_tool() -> u8 { native_release::entry() }
