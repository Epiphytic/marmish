//! Canonical selector shared by candidate reads and test-only custody IPC.
use nostr::{
    key::PublicKey,
    nips::nip19::{FromBech32, ToBech32},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Coordinates", into = "Coordinates")]
pub(crate) struct Selection {
    pub(crate) server: PublicKey,
    pub(crate) offer_id: [u8; 16],
    pub(crate) revision: [u8; 32],
    pub(crate) generation: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Coordinates {
    server: String,
    offer_id: String,
    revision: String,
    generation: u64,
}

impl TryFrom<Coordinates> for Selection {
    type Error = &'static str;

    fn try_from(raw: Coordinates) -> std::result::Result<Self, Self::Error> {
        let server = PublicKey::from_bech32(&raw.server).map_err(|_| "peer_server_npub")?;
        if server.to_bech32().ok().as_deref() != Some(&raw.server) || raw.generation == 0 {
            return Err("peer_selection");
        }
        Ok(Self {
            server,
            offer_id: lowercase_hex(&raw.offer_id).ok_or("peer_offer_id")?,
            revision: lowercase_hex(&raw.revision).ok_or("peer_revision")?,
            generation: raw.generation,
        })
    }
}

impl From<Selection> for Coordinates {
    fn from(selection: Selection) -> Self {
        Self {
            server: selection.server.to_bech32().expect("npub encoding"),
            offer_id: hex::encode(selection.offer_id),
            revision: hex::encode(selection.revision),
            generation: selection.generation,
        }
    }
}

fn lowercase_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    if value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        hex::decode_to_slice(value, &mut bytes).ok()?;
        return Some(bytes);
    }
    None
}
