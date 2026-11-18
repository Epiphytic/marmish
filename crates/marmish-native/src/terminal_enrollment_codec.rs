//! Signed enrollment records carried on the confirmed Terminal alias lanes.
use super::{Codec, GiftLimits, Hex, Prepared, Role, RumorId};
use crate::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use nostr::event::{EventId, Kind, UnsignedEvent};
use nostr::key::{Keys, PublicKey};
use nostr::types::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

pub(crate) const OUTER_CAP: usize = 64 * 1024;
pub(crate) const RETENTION: u64 = 60;
pub(crate) const ENROLLMENT_LIFETIME: u64 = 30;
const KEY_PACKAGE_CAP: usize = 12 * 1024;
const RUMOR_CONTENT_CAP: usize = 20 * 1024;
const KIND: Kind = Kind::Custom(24863);
const LIMITS: GiftLimits = GiftLimits {
    outer: OUTER_CAP,
    outer_content: 63 * 1024,
    seal: 46 * 1024,
    seal_content: 46 * 1024,
    rumor: 24 * 1024,
};
const RECORD: Error = Error::Invalid("terminal_enrollment_record");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Capability {
    pub(crate) ready: RumorId,
    pub(crate) client: PublicKey,
    pub(crate) server: PublicKey,
}

pub(crate) struct Package {
    pub(crate) event: [u8; 32],
    pub(crate) reference: [u8; 32],
    pub(crate) mls: Vec<u8>,
}

pub(crate) struct Plan {
    pub(crate) package: RumorId,
    pub(crate) group: [u8; 32],
    pub(crate) route: [u8; 32],
    pub(crate) digest: [u8; 32],
}

pub(crate) enum Record {
    Package(Package),
    Plan(Plan),
}

impl Record {
    fn sender(&self) -> Role {
        match self {
            Self::Package(_) => Role::Client,
            Self::Plan(_) => Role::Server,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Wire {
    Package {
        version: u8,
        ready: Hex<32>,
        client_key: Hex<32>,
        server_key: Hex<32>,
        event: Hex<32>,
        reference: Hex<32>,
        mls: String,
    },
    Plan {
        version: u8,
        ready: Hex<32>,
        package: Hex<32>,
        group: Hex<32>,
        route: Hex<32>,
        digest: Hex<32>,
    },
}

impl Wire {
    fn from_record(capability: Capability, record: &Record) -> Result<Self> {
        let ready = Hex(capability.ready);
        match record {
            Record::Package(package) if (1..=KEY_PACKAGE_CAP).contains(&package.mls.len()) => {
                Ok(Self::Package {
                    version: 1,
                    ready,
                    client_key: Hex(capability.client.to_bytes()),
                    server_key: Hex(capability.server.to_bytes()),
                    event: Hex(package.event),
                    reference: Hex(package.reference),
                    mls: STANDARD.encode(&package.mls),
                })
            }
            Record::Plan(plan) if plan.group != plan.route => Ok(Self::Plan {
                version: 1,
                ready,
                package: Hex(plan.package),
                group: Hex(plan.group),
                route: Hex(plan.route),
                digest: Hex(plan.digest),
            }),
            _ => Err(RECORD),
        }
    }

    fn decode(self, expected: Capability) -> Result<Record> {
        match self {
            Self::Package {
                version: 1,
                ready: Hex(ready),
                client_key: Hex(client),
                server_key: Hex(server),
                event: Hex(event),
                reference: Hex(reference),
                mls,
            } if ready == expected.ready
                && client == expected.client.to_bytes()
                && server == expected.server.to_bytes() =>
            {
                let bytes = STANDARD.decode(mls.as_bytes()).map_err(|_| RECORD)?;
                if !(1..=KEY_PACKAGE_CAP).contains(&bytes.len()) || STANDARD.encode(&bytes) != mls {
                    return Err(RECORD);
                }
                Ok(Record::Package(Package {
                    event,
                    reference,
                    mls: bytes,
                }))
            }
            Self::Plan {
                version: 1,
                ready: Hex(ready),
                package: Hex(package),
                group: Hex(group),
                route: Hex(route),
                digest: Hex(digest),
            } if ready == expected.ready && group != route => Ok(Record::Plan(Plan {
                package,
                group,
                route,
                digest,
            })),
            _ => Err(RECORD),
        }
    }
}

impl Codec {
    pub(crate) fn wrap_enrollment(
        &self,
        signer: &Keys,
        capability: Capability,
        record: &Record,
        created_at: u64,
    ) -> Result<Prepared> {
        let (local, _) = self.binding.accounts();
        if signer.public_key() != local || record.sender() != self.binding.role {
            return Err(RECORD);
        }
        let content =
            serde_json::to_string(&Wire::from_record(capability, record)?).map_err(|_| RECORD)?;
        if content.len() > RUMOR_CONTENT_CAP {
            return Err(RECORD);
        }
        let rumor = UnsignedEvent::new(local, Timestamp::from_secs(created_at), KIND, [], content);
        self.wrap_gift(signer, rumor, &LIMITS)
    }

    pub(crate) fn open_enrollment(
        &self,
        frame: &[u8],
        capability: Capability,
        now: u64,
    ) -> Result<(RumorId, Record)> {
        let mut rumor = self.open_gift(frame, &LIMITS)?;
        let content = Zeroizing::new(std::mem::take(&mut rumor.content));
        if rumor.kind != KIND
            || !rumor.tags.is_empty()
            || content.len() > RUMOR_CONTENT_CAP
            || !fresh(rumor.created_at.as_secs(), now)
        {
            return Err(RECORD);
        }
        let id = EventId::compute(
            &rumor.pubkey,
            &rumor.created_at,
            &rumor.kind,
            &rumor.tags,
            &content,
        );
        if rumor.id.is_some_and(|supplied| supplied != id) {
            return Err(RECORD);
        }
        let wire: Wire = serde_json::from_str(&content).map_err(|_| RECORD)?;
        let record = wire.decode(capability)?;
        if record.sender() == self.binding.role {
            return Err(RECORD);
        }
        Ok((id.to_bytes(), record))
    }

    pub(crate) fn enrollment_digest(
        &self,
        capability: Capability,
        package: &Package,
        plan: &Plan,
        relay: &str,
    ) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"marmish/terminal/enrollment/1\0");
        hash.update((self.binding.digest.len() as u32).to_be_bytes());
        hash.update(self.binding.digest);
        for key in [self.binding.client, self.binding.server] {
            hash.update(key.as_bytes());
        }
        hash.update(capability.ready);
        for key in [capability.client, capability.server] {
            hash.update(key.as_bytes());
        }
        hash.update(plan.package);
        hash.update(package.event);
        hash.update(package.reference);
        hash.update(Sha256::digest(&package.mls));
        hash.update(plan.group);
        hash.update(plan.route);
        hash.update((relay.len() as u32).to_be_bytes());
        hash.update(relay.as_bytes());
        // The only accepted native profile in this version is Current.
        hash.update([1]);
        hash.update(RETENTION.to_be_bytes());
        hash.finalize().into()
    }
}

pub(crate) fn fresh(created_at: u64, now: u64) -> bool {
    created_at.saturating_sub(now) <= 5 && now.saturating_sub(created_at) < ENROLLMENT_LIFETIME
}

#[cfg(test)]
mod core_tests {
    use super::*;
    use crate::mdk_adapter::{AuthenticatedCommandContext, CommandDirection};

    fn keys(n: u8) -> Keys {
        Keys::parse(&format!("{n:064x}")).unwrap()
    }

    fn fixture() -> (Keys, Keys, Codec, Codec, Capability) {
        let client = keys(1);
        let server = keys(2);
        let left = client.public_key().to_bytes();
        let right = server.public_key().to_bytes();
        let context = |local, peer, direction| {
            AuthenticatedCommandContext::test_fixture(
                local,
                peer,
                vec![0x3a; 32],
                [3; 16],
                direction,
            )
        };
        let sender = Codec::new(
            &context(left, right, CommandDirection::Client),
            &client,
            &[],
        )
        .unwrap();
        let receiver = Codec::new(
            &context(right, left, CommandDirection::Server),
            &server,
            &[],
        )
        .unwrap();
        let capability = Capability {
            ready: [4; 32],
            client: keys(5).public_key(),
            server: keys(6).public_key(),
        };
        (client, server, sender, receiver, capability)
    }

    #[test]
    fn exact_role_and_capability_with_timed_closed_schema() {
        let (client, server, sender, receiver, capability) = fixture();
        let created = 100;
        let package = Package {
            event: [7; 32],
            reference: [8; 32],
            mls: vec![0x1b, 0x02, 0xff, 0],
        };
        let record = Record::Package(package);
        assert!(
            receiver
                .wrap_enrollment(&server, capability, &record, created)
                .is_err()
        );
        let wrapped = sender
            .wrap_enrollment(&client, capability, &record, created)
            .unwrap();
        let input = |cap, now| receiver.open_enrollment(&wrapped.frame, cap, now);
        assert!(
            matches!(input(capability, 95), Ok((id, Record::Package(value))) if id == wrapped.id && value.event == [7;32])
        );
        assert!(input(capability, 94).is_err());
        assert!(input(capability, 129).is_ok());
        assert!(input(capability, 130).is_err());
        assert!(
            input(
                Capability {
                    ready: [0; 32],
                    ..capability
                },
                created
            )
            .is_err()
        );
        assert!(
            input(
                Capability {
                    client: capability.server,
                    ..capability
                },
                created
            )
            .is_err()
        );
        assert!(
            sender
                .open_enrollment(&wrapped.frame, capability, created)
                .is_err()
        );
        assert!(
            receiver
                .open_enrollment(&vec![0; OUTER_CAP + 1], capability, created)
                .is_err()
        );

        let mut malformed =
            serde_json::to_value(Wire::from_record(capability, &record).unwrap()).unwrap();
        malformed["extra"] = serde_json::json!(true);
        assert!(serde_json::from_str::<Wire>(&malformed.to_string()).is_err());
        malformed.as_object_mut().unwrap().remove("extra");
        malformed["mls"] = serde_json::json!("GwL/AA");
        assert!(
            serde_json::from_str::<Wire>(&malformed.to_string())
                .unwrap()
                .decode(capability)
                .is_err()
        );
    }

    #[test]
    fn transcript_digest_is_role_invariant_and_binds_plan_fields() {
        let (_, _, sender, receiver, capability) = fixture();
        let package = Package {
            event: [7; 32],
            reference: [8; 32],
            mls: vec![0x1b, 0x02, 0xff, 0],
        };
        let mut plan = Plan {
            package: [9; 32],
            group: [10; 32],
            route: [11; 32],
            digest: [0; 32],
        };
        let relay = "wss://terminal.invalid/";
        let digest = sender.enrollment_digest(capability, &package, &plan, relay);
        assert_eq!(
            digest,
            receiver.enrollment_digest(capability, &package, &plan, relay)
        );
        assert_eq!(
            hex::encode(digest),
            "3f950abc968b9cc2298aceffe4ebdcc70b65f90a60a900702bdf27f6e1a4c992"
        );
        assert_ne!(
            digest,
            sender.enrollment_digest(capability, &package, &plan, "wss://other.invalid/")
        );
        plan.route[0] ^= 1;
        assert_ne!(
            digest,
            sender.enrollment_digest(capability, &package, &plan, relay)
        );
    }
}
