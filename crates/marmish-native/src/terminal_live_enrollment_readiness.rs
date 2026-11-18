//! Exact NIP-42 acknowledgement and inbox EOSE on an ordered relay epoch.
use crate::{Error, Result};
use nostr::event::{Event, EventId};
use nostr::key::Keys;
use nostr::message::RelayMessage;
use nostr::types::RelayUrl;
use nostr_sdk::authenticator::{Authenticator, SignerAuthenticator};
use nostr_sdk::error::Error as SdkError;
use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

const AUTH: Error = Error::Transport("terminal_enrollment_auth");
const MAX_CHALLENGE: usize = 256;
const MAX_EVIDENCE: usize = 4;

#[derive(Default)]
pub(super) struct SignedAuth {
    events: Mutex<VecDeque<(RelayUrl, String, EventId)>>,
}

impl SignedAuth {
    fn take(&self, origin: &RelayUrl, challenge: &str, id: &EventId) -> bool {
        let Ok(mut events) = self.events.lock() else {
            return false;
        };
        let Some(index) = events
            .iter()
            .position(|entry| &entry.0 == origin && entry.1 == challenge && &entry.2 == id)
        else {
            return false;
        };
        events.remove(index);
        true
    }
}

pub(super) struct RecordAuth {
    signer: SignerAuthenticator<Keys>,
    evidence: Arc<SignedAuth>,
}

impl RecordAuth {
    pub(super) fn new(keys: Keys, evidence: Arc<SignedAuth>) -> Self {
        Self {
            signer: SignerAuthenticator::new(keys),
            evidence,
        }
    }
}

impl fmt::Debug for RecordAuth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TerminalRelayAuthenticator")
    }
}

impl Authenticator for RecordAuth {
    fn make_auth_event<'a>(
        &'a self,
        relay_url: &'a RelayUrl,
        challenge: &'a str,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<Event, SdkError>> + Send + 'a>> {
        Box::pin(async move {
            if challenge.is_empty()
                || challenge.len() > MAX_CHALLENGE
                || challenge.bytes().any(|byte| byte.is_ascii_control())
            {
                return Err(SdkError::other("terminal relay challenge"));
            }
            let event = self.signer.make_auth_event(relay_url, challenge).await?;
            let mut events = self
                .evidence
                .events
                .lock()
                .map_err(|_| SdkError::other("terminal relay authentication evidence"))?;
            if events.len() >= MAX_EVIDENCE {
                return Err(SdkError::other(
                    "terminal relay authentication evidence capacity",
                ));
            }
            events.push_back((relay_url.clone(), challenge.to_owned(), event.id));
            Ok(event)
        })
    }
}

#[derive(Default)]
pub(super) enum Readiness {
    #[default]
    Pending,
    Challenged(String),
    Acknowledged,
    Ready,
}

impl Readiness {
    pub(super) fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// The caller has already required the exact replay epoch and relay source.
    pub(super) fn observe(
        &mut self,
        relay: &RelayUrl,
        inbox: &str,
        message: &RelayMessage<'_>,
        signed: &SignedAuth,
    ) -> Result<()> {
        match message {
            RelayMessage::Auth { challenge } => {
                if challenge.is_empty()
                    || challenge.len() > MAX_CHALLENGE
                    || challenge.bytes().any(|byte| byte.is_ascii_control())
                {
                    return Err(AUTH);
                }
                match self {
                    Self::Pending => *self = Self::Challenged(challenge.to_string()),
                    Self::Challenged(previous) if previous == challenge.as_ref() => {}
                    _ => return Err(AUTH),
                }
            }
            RelayMessage::Ok {
                event_id, status, ..
            } => {
                if let Self::Challenged(challenge) = self
                    && signed.take(relay, challenge, event_id)
                {
                    if !status {
                        return Err(AUTH);
                    }
                    *self = Self::Acknowledged;
                }
            }
            RelayMessage::EndOfStoredEvents(id) if id.as_str() == inbox => {
                if matches!(self, Self::Acknowledged) {
                    *self = Self::Ready;
                }
            }
            RelayMessage::Closed {
                subscription_id,
                message,
            } if subscription_id.as_str() == inbox => {
                // An initial subscription can receive auth-required before
                // the SDK's authenticated REQ replay. A later CLOSE is final.
                if matches!(self, Self::Pending | Self::Challenged(_))
                    && message.starts_with("auth-required:")
                {
                    return Ok(());
                }
                return Err(AUTH);
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::message::SubscriptionId;
    use std::borrow::Cow;

    #[tokio::test]
    async fn requires_signed_exact_ok_then_exact_eose() {
        let evidence = Arc::new(SignedAuth::default());
        let auth = RecordAuth::new(Keys::generate(), evidence.clone());
        let relay = RelayUrl::parse("wss://localhost/relay").unwrap();
        let foreign = RelayUrl::parse("wss://localhost/other").unwrap();
        let mut state = Readiness::default();
        let observe =
            |state: &mut Readiness, from, msg| state.observe(from, "inbox", msg, &evidence);
        let exact = RelayMessage::EndOfStoredEvents(Cow::Owned(SubscriptionId::new("inbox")));
        let wrong = RelayMessage::EndOfStoredEvents(Cow::Owned(SubscriptionId::new("other")));
        let challenge = RelayMessage::Auth {
            challenge: Cow::Borrowed("test"),
        };
        let closed = RelayMessage::Closed {
            subscription_id: Cow::Owned(SubscriptionId::new("inbox")),
            message: Cow::Borrowed("auth-required: test"),
        };
        observe(&mut state, &relay, &closed).unwrap();
        observe(&mut state, &relay, &exact).unwrap();
        observe(&mut state, &relay, &challenge).unwrap();
        let auth_id = auth.make_auth_event(&relay, "test").await.unwrap().id;
        let accepted = RelayMessage::Ok {
            event_id: auth_id,
            status: true,
            message: Cow::Borrowed(""),
        };
        observe(&mut state, &foreign, &accepted).unwrap();
        observe(&mut state, &relay, &exact).unwrap();
        assert!(!state.is_ready());
        observe(&mut state, &relay, &accepted).unwrap();
        observe(&mut state, &relay, &wrong).unwrap();
        assert!(!state.is_ready());
        observe(&mut state, &relay, &exact).unwrap();
        assert!(state.is_ready());
        assert!(observe(&mut state, &relay, &closed).is_err());
    }

    #[tokio::test]
    async fn enrolled_source_cannot_reopen_from_old_auth_evidence() {
        let evidence = Arc::new(SignedAuth::default());
        let auth = RecordAuth::new(Keys::generate(), evidence.clone());
        let relay = RelayUrl::parse("wss://localhost/relay").unwrap();
        let challenge = RelayMessage::Auth {
            challenge: Cow::Borrowed("test"),
        };
        let mut state = Readiness::default();
        state
            .observe(&relay, "inbox", &challenge, &evidence)
            .unwrap();
        let previous_event = auth.make_auth_event(&relay, "test").await.unwrap();
        let accepted = RelayMessage::Ok {
            event_id: previous_event.id,
            status: true,
            message: Cow::Borrowed(""),
        };
        state
            .observe(&relay, "inbox", &accepted, &evidence)
            .unwrap();
        let eose = RelayMessage::EndOfStoredEvents(Cow::Owned(SubscriptionId::new("inbox")));
        state.observe(&relay, "inbox", &eose, &evidence).unwrap();
        assert!(state.is_ready());
        assert!(
            state
                .observe(&relay, "inbox", &challenge, &evidence)
                .is_err()
        );
        let mut new_state = Readiness::default();
        new_state
            .observe(&relay, "inbox", &challenge, &evidence)
            .unwrap();
        new_state
            .observe(&relay, "inbox", &accepted, &evidence)
            .unwrap();
        new_state
            .observe(&relay, "inbox", &eose, &evidence)
            .unwrap();
        assert!(!new_state.is_ready());
        for challenge in ["", "\n", &"x".repeat(MAX_CHALLENGE + 1)] {
            assert!(auth.make_auth_event(&relay, challenge).await.is_err());
        }
    }

    #[tokio::test]
    async fn negative_exact_auth_is_final() {
        let evidence = Arc::new(SignedAuth::default());
        let auth = RecordAuth::new(Keys::generate(), evidence.clone());
        let relay = RelayUrl::parse("wss://localhost/relay").unwrap();
        let mut state = Readiness::Challenged("test".to_owned());
        let event_id = auth.make_auth_event(&relay, "test").await.unwrap().id;
        let rejected = RelayMessage::Ok {
            event_id,
            status: false,
            message: Cow::Borrowed("restricted: test"),
        };
        assert!(
            state
                .observe(&relay, "inbox", &rejected, &evidence)
                .is_err()
        );
        assert!(!state.is_ready());
    }
}
