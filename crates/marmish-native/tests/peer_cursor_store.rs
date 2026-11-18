//! Offline SQLCipher scan and retry qualification before carrier integration.
#[allow(dead_code)] // This target also compiles the private production methods.
#[path = "../src/peer_cursor_store.rs"]
mod peer_cursor_store;

// This storage-only target includes the cursor source directly. Native artifact
// and local ACK authority exist in the library fixture, so there are no values
// of either capability in this target.
mod peer_invitation {
    pub(crate) mod prepared_consent {
        use crate::peer_cursor_store::consent_checkpoint::{
            Result,
            provisioning::{
                Binding,
                prepared_consent::Attempt,
                stage_journal::{Artifact, FileWitness},
            },
        };

        pub(crate) enum NativeAttestation {}

        impl NativeAttestation {
            pub(crate) fn require(
                &self,
                _: (Binding, FileWitness),
                _: Artifact,
                _: u64,
            ) -> Result<&[u8]> {
                match *self {}
            }
        }

        pub(crate) enum MatchingAck {}

        impl MatchingAck {
            pub(crate) fn into_attempt(self) -> Attempt {
                match self {}
            }
        }
    }
}
