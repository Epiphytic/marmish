//! Bind a persisted activation attempt to one authenticated fixture socket.

use std::sync::{
    Arc, Mutex as Slot,
    atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use cgka_traits::{
    MemberId, Timestamp, TransportAccountActivation, TransportAdapter, TransportAdapterError,
    TransportDelivery, TransportEndpoint, TransportEndpointAckKind, TransportEndpointFailure,
    TransportEndpointFailureKind, TransportEndpointReceipt, TransportGroupSync, TransportMessage,
    TransportPublishFailure, TransportPublishReport, TransportPublishRequest,
    TransportPublishTarget,
};
use sha2::{Digest, Sha256};
use tokio::{
    sync::{Mutex, OwnedMutexGuard},
    time::{Instant, timeout_at},
};

use super::{
    postjoin_gate::{DeliveryResult, DeliverySink},
    socket::owner::Owner,
    store::consent_checkpoint::{
        Checkpoint, Source,
        provisioning::prepared_consent::custody::publication::{
            CandidateBinding, EgressAttempt, Kind, Publication, Step,
        },
    },
};

const MAX_SIGNED_MESSAGE_BYTES: usize = 65_536;

#[derive(Debug)]
pub(super) struct Refused;

pub(super) struct ReservedSocket {
    owner: OwnedMutexGuard<Owner>,
    wall: Arc<AtomicU64>,
    deadline: Instant,
    generation: u64,
    kind: Kind,
    candidate: CandidateBinding,
    number: u8,
}

impl ReservedSocket {
    pub(super) async fn reserve(
        owner: Arc<Mutex<Owner>>,
        publication: &Publication,
        wall: Arc<AtomicU64>,
        deadline: Instant,
    ) -> Result<Self, Refused> {
        let number = match publication.step() {
            Step::Captured => 1,
            Step::Uncertain { number: 1 } => 2,
            _ => return Err(Refused),
        };
        let candidate = publication.candidate().ok_or(Refused)?;
        if wall.load(Ordering::Acquire) >= publication.deadline() {
            return Err(Refused);
        }
        let mut owner = timeout_at(deadline, owner.lock_owned())
            .await
            .map_err(|_| Refused)?;
        if !owner.owns_inbox(publication.origin(), publication.recipient()) {
            return Err(Refused);
        }
        owner.ready_to_publish(deadline).map_err(|_| Refused)?;
        Ok(Self {
            owner,
            wall,
            deadline,
            generation: publication.generation(),
            kind: publication.kind(),
            candidate,
            number,
        })
    }

    pub(super) fn bind(
        self,
        checkpoint: &mut Checkpoint,
        source: &Source,
        attempt: EgressAttempt,
        request: &TransportPublishRequest,
        kind: Kind,
    ) -> Result<SocketTurn, Refused> {
        let publication = attempt.publication();
        if !matches!(publication.step(), Step::Armed { number } if number == self.number)
            || publication.kind() != kind
            || self.kind != kind
            || self.generation != publication.generation()
            || publication.candidate() != Some(self.candidate)
            || !self.is_current(publication)
            || !matches_request(publication, request)
        {
            return Err(Refused);
        }
        checkpoint
            .check_armed(source, &attempt, self.wall.load(Ordering::Acquire))
            .map_err(|_| Refused)?;
        Ok(SocketTurn {
            socket: self,
            attempt,
            request: request.clone(),
        })
    }

    fn is_current(&self, publication: &Publication) -> bool {
        Instant::now() < self.deadline
            && self.wall.load(Ordering::Acquire) < publication.deadline()
            && self
                .owner
                .owns_inbox(publication.origin(), publication.recipient())
    }
}

pub(super) struct SocketTurn {
    socket: ReservedSocket,
    attempt: EgressAttempt,
    request: TransportPublishRequest,
}

impl SocketTurn {
    pub(super) fn into_postjoin_sink(self) -> Result<DeliverySink, Refused> {
        if self.attempt.publication().kind() != Kind::PostJoin {
            return Err(Refused);
        }
        Ok(Box::new(move |message| Box::pin(self.publish(message))))
    }

    async fn publish(mut self, message: TransportMessage) -> DeliveryResult {
        let publication = self.attempt.publication();
        if message != self.request.message || !self.socket.is_current(publication) {
            return DeliveryResult::PossiblyExposed;
        }
        let Ok(event_id) = message.id.as_slice().try_into() else {
            return DeliveryResult::PossiblyExposed;
        };
        let result = self
            .socket
            .owner
            .push_group_exact(
                publication.route(),
                &message.payload,
                event_id,
                self.socket.deadline,
            )
            .await;
        if result.is_ok_and(|ack| {
            self.socket.is_current(publication) && self.socket.owner.accepted(&ack, event_id)
        }) {
            DeliveryResult::Accepted
        } else {
            DeliveryResult::PossiblyExposed
        }
    }
}

/// The generic publish method can egress only a previously verified receipt.
/// Commit publication has a separate adapter and cannot take this turn.
pub(super) struct ReceiptGate {
    turn: Slot<Option<SocketTurn>>,
    wall: Arc<AtomicU64>,
}

impl ReceiptGate {
    pub(super) fn closed(wall: Arc<AtomicU64>) -> Self {
        Self {
            turn: Slot::new(None),
            wall,
        }
    }

    pub(super) fn arm(&self, turn: SocketTurn) -> Result<(), Refused> {
        if turn.attempt.publication().kind() != Kind::AcceptReceipt {
            return Err(Refused);
        }
        let mut slot = self.turn.lock().map_err(|_| Refused)?;
        if slot.is_some() {
            return Err(Refused);
        }
        *slot = Some(turn);
        Ok(())
    }

    fn take(&self, request: &TransportPublishRequest) -> Option<SocketTurn> {
        let mut slot = self.turn.lock().ok()?;
        let turn = slot.as_ref()?;
        if !turn.socket.is_current(turn.attempt.publication()) {
            slot.take();
            return None;
        }
        if turn.request != *request {
            return None;
        }
        slot.take()
    }
}

#[async_trait]
impl TransportAdapter for ReceiptGate {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn deactivate_account(&self, _: &MemberId) -> Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn publish_commit(
        &self,
        _: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        let [endpoint] = request.target.endpoints() else {
            return Err(TransportAdapterError::Closed);
        };
        if endpoint.as_str().len() > 2048 {
            return Err(TransportAdapterError::Closed);
        }
        let Some(turn) = self.take(&request) else {
            return Err(receipt_refused(&request, endpoint, true));
        };
        match turn.publish(request.message.clone()).await {
            DeliveryResult::Accepted => Ok(TransportPublishReport {
                message_id: request.message.id,
                accepted: vec![TransportEndpointReceipt {
                    endpoint: endpoint.clone(),
                    accepted_at: Some(Timestamp(self.wall.load(Ordering::Acquire))),
                    ack_kind: Some(TransportEndpointAckKind::Affirmative),
                }],
                failed: Vec::new(),
                required_acks: 1,
            }),
            DeliveryResult::PossiblyExposed => Err(receipt_refused(&request, endpoint, false)),
        }
    }
}

fn receipt_refused(
    request: &TransportPublishRequest,
    endpoint: &TransportEndpoint,
    pre_send: bool,
) -> TransportAdapterError {
    let (kind, reason) = if pre_send {
        (
            TransportEndpointFailureKind::RetryableUnavailable,
            "peer_receipt_gate_refused",
        )
    } else {
        (
            TransportEndpointFailureKind::PossiblyExposed,
            "peer_receipt_delivery_unconfirmed",
        )
    };
    TransportAdapterError::PublishEndpoints(
        TransportPublishFailure::with_endpoint_failures(
            reason,
            vec![TransportEndpointFailure {
                endpoint: endpoint.clone(),
                reason: reason.into(),
                kind,
                rejection_category: None,
                size_class: None,
                pre_send,
            }],
        )
        .with_message_id(request.message.id.clone()),
    )
}

fn matches_request(publication: &Publication, request: &TransportPublishRequest) -> bool {
    let TransportPublishTarget::Group {
        group_id,
        transport_group_id,
        endpoints,
    } = &request.target
    else {
        return false;
    };
    let [endpoint]: &[TransportEndpoint] = endpoints else {
        return false;
    };
    let Some(candidate) = publication.candidate() else {
        return false;
    };
    let message = &request.message;
    if message.payload.is_empty() || message.payload.len() > MAX_SIGNED_MESSAGE_BYTES {
        return false;
    }
    let digest: [u8; 32] = Sha256::digest(&message.payload).into();
    request.required_acks == 1
        && request.validate_envelope_matches_target().is_ok()
        && request.account_id.as_slice() == publication.recipient().to_bytes()
        && group_id.as_slice() == publication.group()
        && transport_group_id.as_slice() == publication.route()
        && endpoint.as_str() == publication.origin()
        && message.id.as_slice() == candidate.event_id
        && digest == candidate.signed_digest
}
