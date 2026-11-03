//! Exact local retry evidence captured from a genuine native publication.
use crate::journal::PublicationKey;
use crate::{Error, Result};
use cgka_traits::app_event::{AppMessageAuthority, AppMessageRetentionDecision};
use cgka_traits::{EpochId, GroupId, MessageId};
use marmot_session::PublishedApplicationMessage;
use serde::{Deserialize, Serialize};

const MAX_BYTES: usize = 4096;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PublishedEvidence {
    version: u8,
    source_epoch: u64,
    retention: Retention,
    authority: Option<Authority>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Retention {
    retention_seconds: u64,
    expires_at: Option<u64>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authority {
    source_context: [u8; 32],
    moderation_grant: bool,
}
impl PublishedEvidence {
    pub(crate) fn from_native(value: &PublishedApplicationMessage) -> Self {
        Self {
            version: 1,
            source_epoch: value.source_epoch.0,
            retention: Retention {
                retention_seconds: value.retention.retention_seconds,
                expires_at: value.retention.expires_at,
            },
            authority: value.authority.map(|authority| Authority {
                source_context: authority.source_context,
                moderation_grant: authority.moderation_grant,
            }),
        }
    }
    pub(crate) fn to_native(
        &self,
        key: &PublicationKey,
        native_id: &[u8],
    ) -> PublishedApplicationMessage {
        PublishedApplicationMessage {
            group_id: GroupId::new(key.group.clone()),
            app_event_id: key.app_event_id.clone(),
            message_id: MessageId::new(native_id.to_vec()),
            source_epoch: EpochId(self.source_epoch),
            retention: AppMessageRetentionDecision {
                retention_seconds: self.retention.retention_seconds,
                expires_at: self.retention.expires_at,
            },
            authority: self
                .authority
                .as_ref()
                .map(|authority| AppMessageAuthority {
                    source_context: authority.source_context,
                    moderation_grant: authority.moderation_grant,
                }),
        }
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        let encoded =
            serde_json::to_vec(self).map_err(|_| Error::Storage("command_publication_evidence"))?;
        if encoded.len() > MAX_BYTES || self.version != 1 {
            return Err(Error::Storage("command_publication_evidence"));
        }
        Ok(encoded)
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::Storage("command_publication_evidence"));
        }
        let decoded: Self = serde_json::from_slice(bytes)
            .map_err(|_| Error::Storage("command_publication_evidence"))?;
        if decoded.encode()? != bytes {
            return Err(Error::Storage("command_publication_evidence"));
        }
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn publication() -> PublishedApplicationMessage {
        PublishedApplicationMessage {
            group_id: GroupId::new(vec![1; 32]),
            app_event_id: "a".repeat(64),
            message_id: MessageId::new(vec![2; 32]),
            source_epoch: EpochId(43),
            retention: AppMessageRetentionDecision {
                retention_seconds: 86400,
                expires_at: None,
            },
            authority: Some(AppMessageAuthority {
                source_context: [7; 32],
                moderation_grant: true,
            }),
        }
    }
    #[test]
    fn native_retry_retains_exact_epoch_retention_and_authority() {
        let native = publication();
        let evidence = PublishedEvidence::from_native(&native);
        let restored = PublishedEvidence::decode(&evidence.encode().unwrap()).unwrap();
        let key = PublicationKey {
            group: native.group_id.as_slice().to_vec(),
            app_event_id: native.app_event_id.clone(),
        };
        assert_eq!(
            restored.to_native(&key, native.message_id.as_slice()),
            native
        );
    }
    #[test]
    fn noncanonical_unknown_duplicate_and_oversized_evidence_is_refused() {
        let encoded = PublishedEvidence::from_native(&publication())
            .encode()
            .unwrap();
        let text = String::from_utf8(encoded.clone()).unwrap();
        for changed in [
            format!("{text}\n"),
            text.replacen("\"version\":1", "\"version\":2", 1),
            text.replacen("\"version\":1", "\"version\":1,\"extra\":0", 1),
            text.replacen(
                "\"source_epoch\":43",
                "\"source_epoch\":43,\"source_epoch\":43",
                1,
            ),
            text.replacen("\"expires_at\":null", "\"expires_at\":null,\"extra\":0", 1),
            text.replacen(
                "\"moderation_grant\":true",
                "\"moderation_grant\":true,\"extra\":0",
                1,
            ),
            text.replacen(",\"expires_at\":null", "", 1),
        ] {
            assert!(PublishedEvidence::decode(changed.as_bytes()).is_err());
        }
        assert!(PublishedEvidence::decode(&vec![b' '; MAX_BYTES + 1]).is_err());
    }
}
