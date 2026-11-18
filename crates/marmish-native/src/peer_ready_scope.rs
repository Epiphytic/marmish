//! Read-only local startup scope for the private installed receiver candidate.

use super::{CommandConfig, HeldFile, PROFILE_CHANGED, ProfileFiles, Result};
use crate::{
    command_config::peer_intent::Receive,
    identity::{IdentityMaterial, MANIFEST_LIMIT, ProfileLease},
};
use std::sync::Arc;

/// Retains exact locally selected files without creating any relay or quota
/// authority. The installed worker still needs independently checked scope.
struct ReadyReceiverScope {
    profile: ProfileFiles,
    manifest: HeldFile,
    config: HeldFile,
}

impl ReadyReceiverScope {
    fn capture(
        lease: &Arc<ProfileLease>,
        material: &IdentityMaterial,
        daemon_config: &CommandConfig,
    ) -> Result<Option<Self>> {
        if !lease.setup_snapshot().ready || daemon_config.peer_receive() != Some(&Receive::Manual) {
            return Ok(None);
        }
        lease.check_prepared_peer_identity(material)?;
        let profile = ProfileFiles::open(lease.clone())?;
        let (actual, config) = profile.checked_command()?;
        daemon_config.bound_peer_relay(&actual)?;
        let (manifest, _) = HeldFile::open(&profile, c"profile.json", MANIFEST_LIMIT as usize)
            .map_err(|_| PROFILE_CHANGED)?;

        let scope = Self {
            profile,
            manifest,
            config,
        };
        scope.recheck(lease)?;
        Ok(Some(scope))
    }

    fn recheck(&self, lease: &Arc<ProfileLease>) -> Result<()> {
        let result = (|| {
            if !Arc::ptr_eq(&self.profile.lease, lease) {
                return Err(PROFILE_CHANGED);
            }
            self.profile.recheck()?;
            self.manifest
                .recheck(&self.profile)
                .map_err(|_| PROFILE_CHANGED)?;
            self.config
                .recheck(&self.profile)
                .map_err(|_| PROFILE_CHANGED)?;
            self.profile.recheck()
        })();
        if result.is_err() {
            self.profile.cancel.cancel();
        }
        result
    }
}

#[path = "peer_ready_scope_tests.rs"]
mod tests;
