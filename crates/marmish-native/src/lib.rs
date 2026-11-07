//! Native v0.2 Marmish. Legacy Python tooling is a separate test fixture.
pub mod cli;
mod command_cli;
mod command_client;
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
mod daemon_launch;
pub mod diagnostics;
pub mod error;
pub mod foundation;
pub mod foundation_check;
pub mod identity;
pub mod ipc;
pub mod journal;
pub mod mdk_adapter;
mod rsync_cli;
mod stream_cli;
mod stream_runner;
mod stream_service;
pub mod transport;
pub use error::{Error, Result};
pub(crate) use mdk_adapter::command_local;

mod native_install;
mod native_manifest;
mod native_preflight;
mod native_setup;

mod command_srv;
mod native_layout;

/// Static release provenance, without opening identity or execution state.
pub fn release_info() -> Result<String> {
    serde_json::to_string(&native_manifest::compiled_release_metadata())
        .map_err(|_| Error::Invalid("release_metadata"))
}

/// Validate this executable's immutable installed lifetime before use.
pub fn validate_executable_layout() -> Result<()> {
    native_layout::resolve(&std::env::current_exe()?).map(|_| ())
}

mod native_release;
/// Build-only packaging entrypoint; not part of installed runtime launchers.
pub fn native_release_tool() -> u8 {
    native_release::entry()
}

/// Build-only installed acceptance tooling; absent from default product builds.
#[cfg(feature = "installed-checks")]
pub async fn installed_check_entry() -> u8 {
    mdk_adapter::installed_check::entry().await
}
/// Build-only deterministic remote byte fixture.
#[cfg(feature = "installed-checks")]
pub fn installed_fixture_entry() -> u8 {
    mdk_adapter::installed_check::fixture_entry()
}

mod copy_files;
mod copy_local_gc;

mod copy_helper;

mod copy_worker;

mod stream_session;

mod copy_cli;
mod copy_client;
mod copy_io;

#[cfg(test)]
mod copy_integration_tests;

#[cfg(test)]
mod copy_io_abort_tests;

#[cfg(test)]
mod copy_commit_hook;
