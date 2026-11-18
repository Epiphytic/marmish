//! Compare-and-set bridge from the library-only authenticated socket proof.

use crate::peer_invitation::socket::owner::recipient::CheckedAck;

use super::*;

impl Checkpoint {
    pub(in crate::peer_invitation) fn settle_installed_prepared_attempt(
        &mut self,
        source: &Source,
        attempt: Attempt,
        ack: CheckedAck<'_>,
    ) -> Result<()> {
        let route = self.route(attempt.origin())?;
        let wall = ack.exact_attempt(&attempt, &route.urls[0])?;
        self.check_installed_prepared_attempt(source, &attempt, wall)?;
        self.settle_attempt(source, attempt, wall)
    }
}
