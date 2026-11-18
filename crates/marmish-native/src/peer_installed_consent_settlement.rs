//! Compare-and-set bridge from the library-only authenticated socket proof.

use crate::peer_invitation::socket::owner::recipient::CheckedAck;

use super::*;
use std::sync::Arc;

pub(in crate::peer_invitation) struct SettledConsent {
    owner: Arc<()>,
    selector: Selector,
    carrier: [u8; 32],
    wall: u64,
}

impl SettledConsent {
    pub(in crate::peer_invitation) fn consume(self) -> (Arc<()>, Selector, [u8; 32], u64) {
        (self.owner, self.selector, self.carrier, self.wall)
    }
}

impl Checkpoint {
    pub(in crate::peer_invitation) fn settle_installed_prepared_attempt(
        &mut self,
        source: &Source,
        attempt: Attempt,
        ack: CheckedAck<'_>,
    ) -> Result<SettledConsent> {
        let route = self.route(attempt.origin())?;
        let wall = ack.exact_attempt(&attempt, &route.urls[0])?;
        self.check_installed_prepared_attempt(source, &attempt, wall)?;
        let settled = SettledConsent {
            owner: Arc::clone(&source.owner),
            selector: attempt.selector(),
            carrier: attempt.carrier(),
            wall,
        };
        self.settle_attempt(source, attempt, wall)?;
        Ok(settled)
    }
}
