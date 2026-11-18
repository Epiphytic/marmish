//! Private owner for one authenticated, non-resumable Terminal enrollment.
use super::terminal_enrollment_room::{App, HeldWelcome, Room, entropy, unix_now};
use super::terminal_enrollment_transport::{Authority, GuardedAdapter, Lane};
use super::*;
use crate::terminal_bootstrap::enrollment::{
    Capability, ENROLLMENT_LIFETIME, OUTER_CAP, Package, Plan, RETENTION, Record, fresh,
};
use crate::terminal_bootstrap::state::{Exchange, Time};
use crate::terminal_bootstrap::{Prepared, RumorId};
use crate::terminal_frames as frames;
use cgka_traits::TransportDeliveryPlane;
use cgka_traits::app_event::{AppMessageRetentionDecision, MarmotAppEvent};
use serde::Deserialize;
use tokio_util::sync::{CancellationToken, DropGuard};

const RECEIPT_KIND: u64 = 24864;
const RECEIPT: Error = Error::Invalid("terminal_enrollment_receipt");
const ORDER: Error = Error::Invalid("terminal_enrollment_order");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Receipt {
    JoinProof,
    ServerReady,
    ClientHello,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptWire {
    version: u8,
    #[serde(rename = "type")]
    kind: Receipt,
    digest: String,
}

enum Stage {
    Start,
    Offered {
        id: RumorId,
        package: Package,
        held: Option<Box<HeldWelcome>>,
    },
    Joining {
        digest: [u8; 32],
    },
    Founding {
        digest: [u8; 32],
    },
    Ready {
        digest: [u8; 32],
    },
    InFlight,
}

pub(super) struct Owner {
    exchange: Arc<Exchange>,
    signer: Arc<Keys>,
    capability: Capability,
    alias: Option<Lane>,
    pub(super) alias_adapter: GuardedAdapter,
    alias_watch: Option<AliasWatch>,
    pub(super) room: Room,
    stage: Stage,
}

struct AliasWatch {
    stop: DropGuard,
    task: tokio::task::JoinHandle<Lane>,
}

impl AliasWatch {
    async fn close(self) -> Result<()> {
        drop(self.stop);
        self.task.await.map_err(|_| ORDER)?.close().await
    }
}

impl Owner {
    pub(super) async fn open(
        mut exchange: Exchange,
        signer: Arc<Keys>,
        alias: Lane,
        room_connector: RelayConnector,
    ) -> Result<Self> {
        let authority = alias.adapter.authority.clone();
        alias.adapter.check().await?;
        let confirmed = exchange.take_terminal(clock()?, authority.check().is_ok())?;
        let capability = Capability {
            ready: confirmed.ready,
            client: confirmed.client,
            server: confirmed.server,
        };
        let deadline = confirmed.confirmed_at + Duration::from_secs(ENROLLMENT_LIFETIME);
        authority.begin_enrollment(Instant::from_std(deadline))?;
        let codec = exchange.enrollment_codec()?;
        if alias.adapter.account.as_slice() != codec.inbound_alias().public_key().to_bytes() {
            authority.retire();
            return Err(ORDER);
        }
        let room = Room::open(
            room_connector,
            alias.adapter.endpoint.clone(),
            Arc::new(confirmed.local),
            capability.client,
            capability.server,
            authority,
        )
        .await?;
        Ok(Self {
            exchange: Arc::new(exchange),
            signer,
            capability,
            alias_adapter: alias.adapter.clone(),
            alias: Some(alias),
            alias_watch: None,
            room,
            stage: Stage::Start,
        })
    }

    pub(super) async fn offer(&mut self) -> Result<usize> {
        let authority = self.authority();
        authority
            .run(async {
                if !matches!(self.stage, Stage::Start)
                    || self.room.lane.adapter.account.as_slice()
                        != self.capability.client.to_bytes()
                {
                    return Err(ORDER);
                }
                let package = self.room.prepare_package().await?;
                let package_size = package.mls.len();
                let record = Record::Package(package);
                let sealed = self.seal(&record)?;
                let Record::Package(package) = record else {
                    return Err(ORDER);
                };
                self.stage = Stage::Offered {
                    id: sealed.id,
                    package,
                    held: None,
                };
                self.alias
                    .as_ref()
                    .ok_or(ORDER)?
                    .publish_alias(&sealed.frame)
                    .await?;
                Ok(package_size)
            })
            .await
    }

    pub(super) async fn hold_welcome(&mut self) -> Result<()> {
        let authority = self.authority();
        authority
            .run(async {
                let Stage::Offered { package, held, .. } = &mut self.stage else {
                    return Err(ORDER);
                };
                if held.is_some() {
                    return Err(ORDER);
                }
                *held = Some(Box::new(self.room.receive_welcome(package).await?));
                Ok(())
            })
            .await
    }

    pub(super) async fn found(
        &mut self,
        clear: impl Fn(&[u8; 32], &[u8; 32]) -> bool,
    ) -> Result<()> {
        let authority = self.authority();
        authority
            .run(async {
                if !matches!(self.stage, Stage::Start)
                    || self.room.lane.adapter.account.as_slice()
                        != self.capability.server.to_bytes()
                {
                    return Err(ORDER);
                }
                let (id, Record::Package(package)) = self.next().await? else {
                    return Err(ORDER);
                };
                let mut plan = Plan {
                    package: id,
                    group: entropy()?,
                    route: entropy()?,
                    digest: [0; 32],
                };
                if plan.group == plan.route || !clear(&plan.group, &plan.route) {
                    return Err(ORDER);
                }
                plan.digest = self.digest(&package, &plan)?;
                let prepared = self.room.prepare_founder(&package, &plan).await?;
                let digest = plan.digest;
                let sealed = self.seal(&Record::Plan(plan))?;
                self.stage = Stage::Founding { digest };
                self.alias
                    .as_ref()
                    .ok_or(ORDER)?
                    .publish_alias(&sealed.frame)
                    .await?;
                self.watch_alias(id)?;
                self.room.publish_welcome(prepared).await
            })
            .await
    }

    pub(super) async fn join(
        &mut self,
        clear: impl Fn(&[u8; 32], &[u8; 32]) -> bool,
    ) -> Result<()> {
        let authority = self.authority();
        authority
            .run(async {
                let Stage::Offered { id, package, held } =
                    std::mem::replace(&mut self.stage, Stage::InFlight)
                else {
                    return Err(ORDER);
                };
                let (plan_id, Record::Plan(plan)) = self.next().await? else {
                    return Err(ORDER);
                };
                if plan.package != id
                    || !clear(&plan.group, &plan.route)
                    || plan.digest != self.digest(&package, &plan)?
                {
                    return Err(ORDER);
                }
                self.watch_alias(plan_id)?;
                let held = match held {
                    Some(held) => *held,
                    None => self.room.receive_welcome(&package).await?,
                };
                self.room.join(held, &package, &plan).await?;
                self.stage = Stage::Joining {
                    digest: plan.digest,
                };
                Ok(())
            })
            .await
    }

    pub(super) async fn client_receipts(&mut self) -> Result<()> {
        let authority = self.authority();
        authority
            .run(async {
                let Stage::Joining { digest } = std::mem::replace(&mut self.stage, Stage::InFlight)
                else {
                    return Err(ORDER);
                };
                self.send_receipt(Receipt::JoinProof, &digest).await?;
                self.receive_receipt(Receipt::ServerReady, &digest).await?;
                self.send_receipt(Receipt::ClientHello, &digest).await?;
                self.stage = Stage::Ready { digest };
                Ok(())
            })
            .await
    }

    pub(super) async fn server_receipts(&mut self) -> Result<()> {
        let authority = self.authority();
        authority
            .run(async {
                let Stage::Founding { digest } =
                    std::mem::replace(&mut self.stage, Stage::InFlight)
                else {
                    return Err(ORDER);
                };
                self.receive_receipt(Receipt::JoinProof, &digest).await?;
                self.send_receipt(Receipt::ServerReady, &digest).await?;
                self.receive_receipt(Receipt::ClientHello, &digest).await?;
                self.stage = Stage::Ready { digest };
                Ok(())
            })
            .await
    }

    async fn send_receipt(&mut self, kind: Receipt, digest: &[u8; 32]) -> Result<()> {
        let bytes = receipt(&self.room.lane.adapter.account, unix_now()?, kind, digest)?;
        self.room.send_app(bytes).await
    }

    async fn receive_receipt(&mut self, kind: Receipt, digest: &[u8; 32]) -> Result<()> {
        let app = self.room.receive_app().await?;
        check_receipt(&app.bytes, unix_now()?, kind, digest)
    }

    pub(super) async fn ready(&self) -> Result<[u8; 32]> {
        self.authority().check()?;
        let Stage::Ready { digest } = self.stage else {
            return Err(ORDER);
        };
        self.room.check_live().await?;
        Ok(digest)
    }

    pub(super) async fn send_frame(
        &mut self,
        binding: frames::Binding,
        bytes: Vec<u8>,
    ) -> Result<()> {
        if self.ready().await? != binding.transcript
            || self.authority().generation() != binding.generation
        {
            return Err(ORDER);
        }
        if bytes.len() > frames::MAX_APP {
            return Err(ORDER);
        }
        let now = unix_now()?;
        let event = MarmotAppEvent::decode(&bytes).map_err(|_| ORDER)?;
        let local = self
            .room
            .lane
            .adapter
            .account
            .as_slice()
            .try_into()
            .map_err(|_| ORDER)?;
        let direction = if local == &self.capability.client.to_bytes() {
            frames::Direction::ToServer
        } else {
            frames::Direction::FromServer
        };
        frames::decode_app(
            &bytes,
            local,
            binding,
            direction,
            frames::Arrival {
                retention: Some(AppMessageRetentionDecision::new(
                    event.created_at,
                    RETENTION,
                )),
                wall: Some(now),
                monotonic: Some(Duration::ZERO),
            },
        )
        .ok_or(ORDER)?;
        self.room.send_app(bytes).await
    }

    pub(super) async fn receive_frame(&mut self, binding: frames::Binding) -> Result<App> {
        if self.ready().await? != binding.transcript
            || self.authority().generation() != binding.generation
        {
            return Err(ORDER);
        }
        let app = self.room.receive_app().await?;
        let (peer, direction) =
            if self.room.lane.adapter.account.as_slice() == self.capability.client.to_bytes() {
                (self.capability.server, frames::Direction::FromServer)
            } else {
                (self.capability.client, frames::Direction::ToServer)
            };
        frames::decode_app(
            &app.bytes,
            &peer.to_bytes(),
            binding,
            direction,
            frames::Arrival {
                retention: Some(app.retention),
                wall: Some(unix_now()?),
                monotonic: Some(Duration::ZERO),
            },
        )
        .ok_or_else(|| {
            self.authority().retire();
            ORDER
        })?;
        self.authority().check()?;
        Ok(app)
    }

    pub(super) async fn close(self) -> Result<()> {
        self.authority().retire();
        let alias = async {
            match (self.alias, self.alias_watch) {
                (Some(lane), None) => lane.close().await,
                (None, Some(watch)) => watch.close().await,
                _ => Err(ORDER),
            }
        };
        let (alias, room) = tokio::join!(alias, self.room.lane.close());
        alias.and(room)
    }

    fn authority(&self) -> Arc<Authority> {
        self.alias_adapter.authority.clone()
    }

    fn seal(&self, record: &Record) -> Result<Prepared> {
        self.authority().check()?;
        self.exchange.enrollment_codec()?.wrap_enrollment(
            &self.signer,
            self.capability,
            record,
            unix_now()?,
        )
    }

    fn digest(&self, package: &Package, plan: &Plan) -> Result<[u8; 32]> {
        Ok(self.exchange.enrollment_codec()?.enrollment_digest(
            self.capability,
            package,
            plan,
            &self.alias_adapter.endpoint.0,
        ))
    }

    async fn next(&mut self) -> Result<(RumorId, Record)> {
        next_alias(
            self.alias.as_mut().ok_or(ORDER)?,
            &self.exchange,
            self.capability,
        )
        .await
    }

    fn watch_alias(&mut self, accepted: RumorId) -> Result<()> {
        let mut lane = self.alias.take().ok_or(ORDER)?;
        let exchange = self.exchange.clone();
        let capability = self.capability;
        let stop = CancellationToken::new();
        let watch_stop = stop.clone();
        let task = tokio::spawn(async move {
            // Keep a placement-to-ack sequence in one task. Room I/O must
            // never cancel it between the adapter queue and alias validation.
            tokio::select! {
                biased;
                _ = watch_stop.cancelled() => {},
                _ = watch_alias(&mut lane, &exchange, capability, accepted) => {},
            }
            lane
        });
        self.alias_watch = Some(AliasWatch {
            stop: stop.drop_guard(),
            task,
        });
        Ok(())
    }
}

async fn next_alias(
    alias: &mut Lane,
    exchange: &Exchange,
    capability: Capability,
) -> Result<(RumorId, Record)> {
    let delivery = alias
        .receive(TransportDeliveryPlane::AccountInbox, None, OUTER_CAP)
        .await?;
    alias.adapter.authority.check()?;
    let record = exchange.enrollment_codec()?.open_enrollment(
        &delivery.message.payload,
        capability,
        unix_now()?,
    );
    alias.acknowledge(&delivery.message.id).await;
    record
}

async fn watch_alias(
    alias: &mut Lane,
    exchange: &Exchange,
    capability: Capability,
    accepted: RumorId,
) {
    loop {
        match next_alias(alias, exchange, capability).await {
            Ok((id, _)) if id == accepted => {}
            _ => {
                alias.adapter.authority.retire();
                return;
            }
        }
    }
}

fn receipt(issuer: &MemberId, created: u64, kind: Receipt, digest: &[u8; 32]) -> Result<Vec<u8>> {
    let content = serde_json::to_string(&ReceiptWire {
        version: 1,
        kind,
        digest: hex::encode(digest),
    })
    .map_err(|_| RECEIPT)?;
    MarmotAppEvent::new(
        hex::encode(issuer.as_slice()),
        created,
        RECEIPT_KIND,
        vec![],
        content,
    )
    .encode()
    .map_err(|_| RECEIPT)
}

fn check_receipt(bytes: &[u8], now: u64, expected: Receipt, digest: &[u8; 32]) -> Result<()> {
    if bytes.len() > 768 {
        return Err(RECEIPT);
    }
    let event = MarmotAppEvent::decode(bytes).map_err(|_| RECEIPT)?;
    if event.kind != RECEIPT_KIND
        || !event.tags.is_empty()
        || !fresh(event.created_at, now)
        || event.created_at.checked_add(RETENTION).is_none()
    {
        return Err(RECEIPT);
    }
    let decoded: ReceiptWire = serde_json::from_str(&event.content).map_err(|_| RECEIPT)?;
    if decoded.version != 1 || decoded.kind != expected || decoded.digest != hex::encode(digest) {
        return Err(RECEIPT);
    }
    Ok(())
}

fn clock() -> Result<Time> {
    Ok(Time {
        unix: unix_now()?,
        monotonic: std::time::Instant::now(),
    })
}

#[cfg(test)]
mod core_tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn receipt_kind_order_version_digest_and_signed_age() {
        let sender = MemberId::new([7; 32].to_vec());
        let digest = [0x5a; 32];
        let encoded = receipt(&sender, 100, Receipt::JoinProof, &digest).unwrap();
        let check = |time, expected, digest| check_receipt(&encoded, time, expected, digest);
        assert!(check(95, Receipt::JoinProof, &digest).is_ok());
        assert!(check(129, Receipt::JoinProof, &digest).is_ok());
        assert!(check(94, Receipt::JoinProof, &digest).is_err());
        assert!(check(130, Receipt::JoinProof, &digest).is_err());
        assert!(check(100, Receipt::ClientHello, &digest).is_err());
        assert!(check(100, Receipt::JoinProof, &[0; 32]).is_err());
        let invalid = MarmotAppEvent::new(
            hex::encode(sender.as_slice()),
            100,
            RECEIPT_KIND,
            vec![],
            format!(
                r#"{{"version":2,"type":"join_proof","digest":"{}"}}"#,
                hex::encode(digest)
            ),
        )
        .encode()
        .unwrap();
        assert!(check_receipt(&invalid, 100, Receipt::JoinProof, &digest).is_err());
    }

    #[tokio::test]
    async fn revocation_cancels_a_wait_and_cannot_reactivate_the_generation() {
        let active = Arc::new(AtomicU64::new(9));
        let current = active.clone();
        let authority = Authority::new(
            9,
            Arc::new(move |generation| current.load(Ordering::Acquire) == generation),
            Instant::now() + Duration::from_secs(1),
        );
        authority
            .begin_enrollment(Instant::now() + Duration::from_secs(2))
            .unwrap();
        let pending = authority.run(std::future::pending::<Result<()>>());
        let (_, outcome) = tokio::join!(
            async {
                tokio::time::sleep(Duration::from_millis(15)).await;
                active.store(10, Ordering::Release);
            },
            async {
                tokio::time::timeout(Duration::from_millis(300), pending)
                    .await
                    .unwrap()
            }
        );
        assert!(matches!(
            outcome,
            Err(Error::Invalid("terminal_enrollment_authority"))
        ));
        active.store(9, Ordering::Release);
        assert!(authority.check().is_err());
    }
}
