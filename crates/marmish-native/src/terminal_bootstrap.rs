//! Private authenticated bootstrap for the volatile Terminal owner.
use crate::mdk_adapter::{AuthenticatedCommandContext, CommandDirection};
use crate::{Error, Result};
use hkdf::Hkdf;
use nostr::event::{Event, EventId, FinalizeEvent, Kind, Tag, UnsignedEvent};
use nostr::key::{Keys, PublicKey, SecretKey};
use nostr::nips::nip44::{self, v2::ConversationKey};
use nostr::nips::nip59::GiftWrapBuilder;
use nostr::types::Timestamp;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use std::sync::Arc;
use zeroize::Zeroizing;

#[cfg(test)]
#[path = "terminal_bootstrap_core_tests.rs"]
mod core_tests;
#[path = "terminal_enrollment_codec.rs"]
pub(crate) mod enrollment;
#[path = "terminal_bootstrap_state.rs"]
pub(crate) mod state;

const VERSION: u8 = 1;
const KIND: Kind = Kind::Custom(24861);
const BINDING_LABEL: &[u8] = b"marmish/terminal/bootstrap/1\0binding\0";
const ALIAS_LABEL: &[u8] = b"marmish/terminal/bootstrap/1\0alias\0";
const GROUP_CAP: usize = 1024;
const OUTER_CAP: usize = 32 * 1024;
const OUTER_CONTENT_CAP: usize = 31 * 1024;
const SEAL_CONTENT_CAP: usize = 8 * 1024;
const RUMOR_CONTENT_CAP: usize = 2 * 1024;
const LIFETIME: u64 = 30;
const BOOTSTRAP_GIFT: GiftLimits = GiftLimits {
    outer: OUTER_CAP,
    outer_content: OUTER_CONTENT_CAP,
    seal: OUTER_CONTENT_CAP,
    seal_content: SEAL_CONTENT_CAP,
    rumor: SEAL_CONTENT_CAP,
};

struct GiftLimits {
    outer: usize,
    outer_content: usize,
    seal: usize,
    seal_content: usize,
    rumor: usize,
}

const BINDING: Error = Error::Invalid("terminal_bootstrap_binding");
const ALIAS: Error = Error::Invalid("terminal_bootstrap_alias");
const WRAP: Error = Error::Invalid("terminal_bootstrap_wrap");
const SEAL: Error = Error::Invalid("terminal_bootstrap_seal");
const RUMOR: Error = Error::Invalid("terminal_bootstrap_rumor");
const RECORD: Error = Error::Invalid("terminal_bootstrap_record");
const CLOCK: Error = Error::Invalid("terminal_bootstrap_clock");

type Challenge = [u8; 16];
pub(crate) type RumorId = [u8; 32];

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    Client = 1,
    Server = 2,
}

struct Binding {
    digest: [u8; 32],
    client: PublicKey,
    server: PublicKey,
    role: Role,
}

fn read_key(bytes: [u8; 32]) -> Result<PublicKey> {
    let key = PublicKey::from_byte_array(bytes);
    key.xonly().map_err(|_| RECORD)?;
    Ok(key)
}

impl Binding {
    // Callers supply an already revalidated Command context. Its local UID
    // and authority digest are deliberately outside this shared transcript.
    fn new(context: &AuthenticatedCommandContext, signer: &Keys) -> Result<Self> {
        let local = read_key(context.local_account()).map_err(|_| BINDING)?;
        let peer = read_key(context.peer_account()).map_err(|_| BINDING)?;
        if local == peer || local != signer.public_key() {
            return Err(BINDING);
        }
        let (client, server, role) = match context.direction() {
            CommandDirection::Client => (local, peer, Role::Client),
            CommandDirection::Server => (peer, local, Role::Server),
        };
        let mut hash = Sha256::new();
        hash.update(BINDING_LABEL);
        hash.update([VERSION]);
        hash.update(context.session());
        for group in [context.mls_group(), context.transport_group()] {
            if !(1..=GROUP_CAP).contains(&group.len()) {
                return Err(BINDING);
            }
            hash.update((group.len() as u16).to_be_bytes());
            hash.update(group);
        }
        hash.update([Role::Client as u8]);
        hash.update(client.as_bytes());
        hash.update([Role::Server as u8]);
        hash.update(server.as_bytes());
        Ok(Self {
            digest: hash.finalize().into(),
            client,
            server,
            role,
        })
    }

    fn accounts(&self) -> (PublicKey, PublicKey) {
        match self.role {
            Role::Client => (self.client, self.server),
            Role::Server => (self.server, self.client),
        }
    }
}

/// Alias secrets are scoped to the in-memory codec and are never serialized.
struct Alias {
    secret: SecretKey,
    public: PublicKey,
}

pub(crate) struct Codec {
    binding: Binding,
    inbound: Alias,
    outbound: PublicKey,
}

/// Binding-scoped canonical operations. The admission owner can neither
/// export the key nor use this capability as a general event signer.
pub(crate) struct BoundSigning {
    signer: Arc<Keys>,
    binding: [u8; 32],
    role: Role,
    context: Option<AuthenticatedCommandContext>,
}

impl BoundSigning {
    pub(crate) fn bind(
        context: &AuthenticatedCommandContext,
        signer: Arc<Keys>,
    ) -> Result<(Self, Codec)> {
        let codec = Codec::new(context, &signer, &[])?;
        let mut bound = Self::from_codec(&codec, signer)?;
        bound.context = Some(context.clone());
        Ok((bound, codec))
    }

    pub(crate) fn from_codec(codec: &Codec, signer: Arc<Keys>) -> Result<Self> {
        if signer.public_key() != codec.binding.accounts().0 {
            return Err(BINDING);
        }
        Ok(Self {
            signer,
            binding: codec.binding_digest(),
            role: codec.binding.role,
            context: None,
        })
    }

    pub(crate) fn check_context(
        &self,
        context: &AuthenticatedCommandContext,
        codec: &Codec,
    ) -> Result<()> {
        self.for_codec(codec)?;
        if self.context.as_ref() != Some(context) {
            return Err(BINDING);
        }
        Ok(())
    }

    fn for_codec(&self, codec: &Codec) -> Result<&Keys> {
        if self.binding != codec.binding_digest() || self.role != codec.binding.role {
            return Err(BINDING);
        }
        Ok(&self.signer)
    }

    pub(crate) fn wrap_enrollment(
        &self,
        codec: &Codec,
        capability: enrollment::Capability,
        record: &enrollment::Record,
        now: u64,
    ) -> Result<Prepared> {
        codec.wrap_enrollment(self.for_codec(codec)?, capability, record, now)
    }
}

impl Codec {
    pub(crate) fn new(
        context: &AuthenticatedCommandContext,
        signer: &Keys,
        active_local_aliases: &[PublicKey],
    ) -> Result<Self> {
        let binding = Binding::new(context, signer)?;
        let (_, remote) = binding.accounts();
        let conversation =
            ConversationKey::derive(signer.secret_key(), &remote).map_err(|_| ALIAS)?;
        let hkdf = Hkdf::<Sha256>::new(Some(ALIAS_LABEL), conversation.as_bytes());
        let client =
            binding.derive_alias(&hkdf, Role::Client, &[binding.client, binding.server])?;
        let server = binding.derive_alias(
            &hkdf,
            Role::Server,
            &[binding.client, binding.server, client.public],
        )?;
        let (inbound, outbound) = match binding.role {
            Role::Client => (client, server.public),
            Role::Server => (server, client.public),
        };
        if active_local_aliases.contains(&inbound.public) {
            return Err(ALIAS);
        }
        Ok(Self {
            binding,
            inbound,
            outbound,
        })
    }

    pub(crate) fn binding_digest(&self) -> [u8; 32] {
        self.binding.digest
    }

    pub(crate) fn inbound_alias(&self) -> Keys {
        Keys::new(self.inbound.secret.clone())
    }

    #[cfg(test)]
    pub(crate) fn outbound_alias(&self) -> PublicKey {
        self.outbound
    }

    fn wrap(&self, signer: &Keys, body: Body, created_at: u64) -> Result<Prepared> {
        let (local, _) = self.binding.accounts();
        if signer.public_key() != local || body.step().sender() != self.binding.role {
            return Err(BINDING);
        }
        let wire = Wire::from_body(self.binding.digest, body, created_at)?;
        let content = serde_json::to_string(&wire).map_err(|_| RECORD)?;
        let rumor = UnsignedEvent::new(
            local,
            Timestamp::from_secs(created_at),
            KIND,
            [Tag::public_key(self.outbound)],
            content,
        );
        self.wrap_gift(signer, rumor, &BOOTSTRAP_GIFT)
    }

    fn wrap_gift(
        &self,
        signer: &Keys,
        rumor: UnsignedEvent,
        limits: &GiftLimits,
    ) -> Result<Prepared> {
        let id = rumor.compute_id().to_bytes();
        let event = GiftWrapBuilder::new(self.outbound, rumor)
            .finalize(signer)
            .map_err(|_| WRAP)?;
        let frame = serde_json::to_vec(&event).map_err(|_| WRAP)?;
        if frame.len() > limits.outer || event.content.len() > limits.outer_content {
            return Err(WRAP);
        }
        Ok(Prepared { id, frame })
    }

    fn open_gift(&self, frame: &[u8], limits: &GiftLimits) -> Result<UnsignedEvent> {
        if frame.len() > limits.outer {
            return Err(WRAP);
        }
        let outer: Event = serde_json::from_slice(frame).map_err(|_| WRAP)?;
        if outer.kind != Kind::GiftWrap
            || outer.content.len() > limits.outer_content
            || !only_recipient(&outer.tags, self.inbound.public)
            || outer.verify().is_err()
        {
            return Err(WRAP);
        }
        let seal_bytes = Zeroizing::new(
            nip44::decrypt_to_bytes(&self.inbound.secret, &outer.pubkey, &outer.content)
                .map_err(|_| SEAL)?,
        );
        if seal_bytes.len() > limits.seal {
            return Err(SEAL);
        }
        let seal: Event = serde_json::from_slice(&seal_bytes).map_err(|_| SEAL)?;
        let (_, remote) = self.binding.accounts();
        if seal.kind != Kind::Seal
            || seal.pubkey != remote
            || seal.content.len() > limits.seal_content
            || !seal.tags.is_empty()
            || seal.verify().is_err()
        {
            return Err(SEAL);
        }
        let rumor_bytes = Zeroizing::new(
            nip44::decrypt_to_bytes(&self.inbound.secret, &seal.pubkey, &seal.content)
                .map_err(|_| RUMOR)?,
        );
        if rumor_bytes.len() > limits.rumor {
            return Err(RUMOR);
        }
        let rumor: UnsignedEvent = serde_json::from_slice(&rumor_bytes).map_err(|_| RUMOR)?;
        if rumor.pubkey != seal.pubkey {
            return Err(RUMOR);
        }
        Ok(rumor)
    }

    fn open(&self, frame: &[u8], now: u64) -> Result<Verified> {
        let mut rumor = self.open_gift(frame, &BOOTSTRAP_GIFT)?;
        let content = Zeroizing::new(std::mem::take(&mut rumor.content));
        if rumor.kind != KIND
            || content.len() > RUMOR_CONTENT_CAP
            || !only_recipient(&rumor.tags, self.inbound.public)
        {
            return Err(RUMOR);
        }
        let id = EventId::compute(
            &rumor.pubkey,
            &rumor.created_at,
            &rumor.kind,
            &rumor.tags,
            &content,
        );
        if rumor.id.is_some_and(|supplied| supplied != id) {
            return Err(RUMOR);
        }
        let created_at = rumor.created_at.as_secs();
        let wire: Wire = serde_json::from_str(&content).map_err(|_| RECORD)?;
        if wire.binding.0 != self.binding.digest || wire.sender_role == self.binding.role {
            return Err(BINDING);
        }
        let body = wire.into_body(created_at, now)?;
        Ok(Verified {
            id: id.to_bytes(),
            body,
        })
    }
}

impl Binding {
    fn derive_alias(
        &self,
        hkdf: &Hkdf<Sha256>,
        receiver: Role,
        forbidden: &[PublicKey],
    ) -> Result<Alias> {
        let mut info = [0; 34];
        info[..32].copy_from_slice(&self.digest);
        info[32] = receiver as u8;
        for counter in 0..4 {
            info[33] = counter;
            let mut scalar = Zeroizing::new([0; 32]);
            hkdf.expand(&info, &mut *scalar).map_err(|_| ALIAS)?;
            if let Ok(secret) = SecretKey::from_slice(&*scalar) {
                let public = Keys::new(secret.clone()).public_key();
                if !forbidden.contains(&public) {
                    return Ok(Alias { secret, public });
                }
            }
        }
        Err(ALIAS)
    }
}

fn only_recipient(tags: &nostr::event::Tags, recipient: PublicKey) -> bool {
    tags.len() == 1
        && tags
            .iter()
            .next()
            .is_some_and(|tag| tag.as_slice() == ["p", &recipient.to_hex()])
}

pub(crate) struct Prepared {
    pub(crate) id: RumorId,
    pub(crate) frame: Vec<u8>,
}

struct Verified {
    id: RumorId,
    body: Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Step {
    Probe,
    Challenge,
    Confirm,
    Ready,
}

impl Step {
    pub(crate) fn sender(self) -> Role {
        match self {
            Self::Probe | Self::Confirm => Role::Client,
            Self::Challenge | Self::Ready => Role::Server,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Pair {
    client: Challenge,
    server: Challenge,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Body {
    Probe {
        client: Challenge,
    },
    Challenge {
        pair: Pair,
        previous: RumorId,
    },
    Confirm {
        pair: Pair,
        previous: RumorId,
        client_key: PublicKey,
    },
    Ready {
        pair: Pair,
        previous: RumorId,
        client_key: PublicKey,
        server_key: PublicKey,
    },
}

impl Body {
    fn step(self) -> Step {
        match self {
            Self::Probe { .. } => Step::Probe,
            Self::Challenge { .. } => Step::Challenge,
            Self::Confirm { .. } => Step::Confirm,
            Self::Ready { .. } => Step::Ready,
        }
    }

    fn previous(self) -> Option<RumorId> {
        match self {
            Self::Probe { .. } => None,
            Self::Challenge { previous, .. }
            | Self::Confirm { previous, .. }
            | Self::Ready { previous, .. } => Some(previous),
        }
    }
}

#[derive(Serialize, Deserialize)]
enum Protocol {
    #[serde(rename = "marmish/terminal/bootstrap/1")]
    TerminalBootstrap,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Lossless,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    version: u8,
    protocol: Protocol,
    #[serde(rename = "type")]
    step: Step,
    sender_role: Role,
    mode: Mode,
    binding: Hex<32>,
    expires_at: u64,
    client_challenge: Hex<16>,
    #[serde(
        default,
        deserialize_with = "optional_hex",
        skip_serializing_if = "Option::is_none"
    )]
    server_challenge: Option<Hex<16>>,
    #[serde(
        default,
        deserialize_with = "optional_hex",
        skip_serializing_if = "Option::is_none"
    )]
    previous: Option<Hex<32>>,
    #[serde(
        default,
        deserialize_with = "optional_hex",
        skip_serializing_if = "Option::is_none"
    )]
    client_key: Option<Hex<32>>,
    #[serde(
        default,
        deserialize_with = "optional_hex",
        skip_serializing_if = "Option::is_none"
    )]
    server_key: Option<Hex<32>>,
}

impl Wire {
    fn from_body(binding: [u8; 32], body: Body, created_at: u64) -> Result<Self> {
        let (pair, client_key, server_key) = match body {
            Body::Probe { client } => (
                Pair {
                    client,
                    server: [0; 16],
                },
                None,
                None,
            ),
            Body::Challenge { pair, .. } => (pair, None, None),
            Body::Confirm {
                pair, client_key, ..
            } => (pair, Some(client_key), None),
            Body::Ready {
                pair,
                client_key,
                server_key,
                ..
            } => (pair, Some(client_key), Some(server_key)),
        };
        let step = body.step();
        Ok(Self {
            version: VERSION,
            protocol: Protocol::TerminalBootstrap,
            step,
            sender_role: step.sender(),
            mode: Mode::Lossless,
            binding: Hex(binding),
            expires_at: created_at.checked_add(LIFETIME).ok_or(CLOCK)?,
            client_challenge: Hex(pair.client),
            server_challenge: (step != Step::Probe).then_some(Hex(pair.server)),
            previous: body.previous().map(Hex),
            client_key: client_key.map(|key| Hex(key.to_bytes())),
            server_key: server_key.map(|key| Hex(key.to_bytes())),
        })
    }

    fn into_body(self, created_at: u64, now: u64) -> Result<Body> {
        if self.version != VERSION || self.step.sender() != self.sender_role {
            return Err(RECORD);
        }
        if created_at.checked_add(LIFETIME) != Some(self.expires_at)
            || now.saturating_sub(created_at) > 35
            || created_at.saturating_sub(now) > 5
        {
            return Err(CLOCK);
        }
        let client = self.client_challenge.0;
        let pair = |server| Pair { client, server };
        match (
            self.step,
            self.server_challenge,
            self.previous,
            self.client_key,
            self.server_key,
        ) {
            (Step::Probe, None, None, None, None) => Ok(Body::Probe { client }),
            (Step::Challenge, Some(Hex(server)), Some(Hex(previous)), None, None) => {
                Ok(Body::Challenge {
                    pair: pair(server),
                    previous,
                })
            }
            (
                Step::Confirm,
                Some(Hex(server)),
                Some(Hex(previous)),
                Some(Hex(client_key)),
                None,
            ) => Ok(Body::Confirm {
                pair: pair(server),
                previous,
                client_key: read_key(client_key)?,
            }),
            (
                Step::Ready,
                Some(Hex(server)),
                Some(Hex(previous)),
                Some(Hex(client_key)),
                Some(Hex(server_key)),
            ) if client_key != server_key => Ok(Body::Ready {
                pair: pair(server),
                previous,
                client_key: read_key(client_key)?,
                server_key: read_key(server_key)?,
            }),
            _ => Err(RECORD),
        }
    }
}

struct Hex<const N: usize>([u8; N]);

impl<const N: usize> Serialize for Hex<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de, const N: usize> Deserialize<'de> for Hex<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let hex = <&str>::deserialize(deserializer)?;
        if hex.len() != 2 * N
            || !hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(serde::de::Error::custom("invalid bootstrap hex"));
        }
        let mut bytes = [0; N];
        hex::decode_to_slice(hex, &mut bytes).map_err(serde::de::Error::custom)?;
        Ok(Self(bytes))
    }
}

fn optional_hex<'de, D: Deserializer<'de>, const N: usize>(
    deserializer: D,
) -> std::result::Result<Option<Hex<N>>, D::Error> {
    Hex::deserialize(deserializer).map(Some)
}
