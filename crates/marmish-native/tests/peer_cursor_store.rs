//! Offline SQLCipher scan and retry qualification before carrier integration.
#[allow(dead_code)] // This target also compiles the private production methods.
#[path = "../src/peer_cursor_store.rs"]
mod peer_cursor_store;

// This storage-only target includes the cursor source directly. Native artifact
// and local ACK authority exist in the library fixture, so there are no values
// of either capability in this target.
#[path = "../src/peer_accept_receipt_wire.rs"]
pub(crate) mod receipt_wire;

mod peer_invitation {
    pub(crate) use crate::receipt_wire as accept_receipt_wire;
    pub(crate) use crate::peer_cursor_store::consent_checkpoint::provisioning::
        prepared_consent::custody::HeldEvidence;

    // Standalone storage tests cannot mint native proofs. These uninhabited
    // stand-ins permit checking the same storage source without linking MDK.
    pub(crate) mod postjoin_verify {
        use cgka_traits::engine::EpochPin;
        use cgka_traits::{GroupId, MemberId, MessageId, TransportPublishRequest};

        macro_rules! uninhabited {
            ($name:ident { $($method:ident -> $return:ty),+ $(,)? }) => {
                pub(crate) enum $name {}

                impl $name {
                    $(pub(crate) fn $method(&self) -> $return { match *self {} })+
                }
            };
        }

        uninhabited!(Baseline {
            obligation_id -> &MessageId,
            group -> &GroupId,
            recipient -> &MemberId,
            founder -> &MemberId,
            route -> [u8; 32],
            origin -> &str,
            inode -> (u64, u64),
            generation -> u64,
            deadline -> u64,
            welcome -> &MessageId,
            source -> &EpochPin,
            recipient_leaf -> u32,
        });

        uninhabited!(Capture {
            request -> &TransportPublishRequest,
            obligation_id -> &MessageId,
            event_id -> [u8; 32],
            signed_digest -> [u8; 32],
            target -> &EpochPin,
            generation -> u64,
            deadline -> u64,
        });

        uninhabited!(RecoveredPostJoin {
            baseline -> &Baseline,
            capture -> &Capture,
        });

        enum Never {}

        pub(crate) struct ConfirmedRotation<'a>(Never, std::marker::PhantomData<&'a ()>);

        impl<'a> ConfirmedRotation<'a> {
            pub(crate) fn baseline(&self) -> &'a Baseline {
                match self.0 {}
            }

            pub(crate) fn capture(&self) -> &'a Capture {
                match self.0 {}
            }
        }
    }

    pub(crate) mod accept_receipt_verify {
        use cgka_traits::app_components::NostrRoutingV1;
        use cgka_traits::{GroupId, MessageId};

        use super::accept_receipt_wire::Receipt;

        pub(crate) enum VerifiedReceiptSource {}

        impl VerifiedReceiptSource {
            pub(crate) fn receipt(&self) -> &Receipt {
                match *self {}
            }

            pub(crate) fn group(&self) -> &GroupId {
                match *self {}
            }

            pub(crate) fn route(&self) -> &NostrRoutingV1 {
                match *self {}
            }

            pub(crate) fn native_id(&self) -> &MessageId {
                match *self {}
            }

            pub(crate) fn app_id(&self) -> [u8; 32] {
                match *self {}
            }

            pub(crate) fn leaf(&self) -> u32 {
                match *self {}
            }

            pub(crate) fn created_at(&self) -> u64 {
                match *self {}
            }

            pub(crate) fn deadline(&self) -> u64 {
                match *self {}
            }
        }
    }

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

        pub(crate) enum JoinedReadback {}

        impl JoinedReadback {
            pub(crate) fn stage(&self) -> (Binding, FileWitness) {
                match *self {}
            }

            pub(crate) fn carrier(&self) -> [u8; 32] {
                match *self {}
            }

            pub(crate) fn coordinates(&self) -> (&[u8], u64, [u8; 32], u32) {
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
